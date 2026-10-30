//! Where blocks go: an idempotent write, a checkpoint, and a way to take back what was
//! provisional.
//!
//! [`InMemoryStore`] is not a mock. Its upsert keys are the storage backend's `ORDER BY` keys,
//! so the deduplication a test proves against memory is the deduplication the real backend
//! performs — which is what lets the acceptance suite run without a database and still mean
//! something.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::future::Future;
use std::sync::{Mutex, PoisonError};

use sting_sybil_types::{Finality, Lt, TonAddress, TxHash};

use crate::error::Result;
use crate::{BlockBatch, Direction, RawMessage, RawTx};

/// Somewhere raw rows are kept.
///
/// The futures are spelled out rather than written as `async fn` for the reason given on
/// [`crate::BlockSource`].
pub trait Store: Send + Sync {
    /// Writes a block, idempotently under `(account, lt)` for transactions and
    /// `(tx_hash, direction, created_lt)` for messages.
    ///
    /// Writing the same key again with a firmer finality is how a confirmed row becomes a
    /// finalized one: no separate promotion call exists, because the real backend performs
    /// exactly that replacement on merge.
    fn write_batch(&self, batch: BlockBatch) -> impl Future<Output = Result<()>> + Send;

    /// The highest masterchain sequence number written so far, if any.
    fn checkpoint(&self) -> impl Future<Output = Result<Option<u32>>> + Send;

    /// Takes back rows belonging to transactions that turned out not to be on the chain.
    ///
    /// Only rows that were never finalized are removed. A finalized row is not provisional and
    /// is not something an invalidation may reach.
    fn invalidate(&self, tx_hashes: &[TxHash]) -> impl Future<Output = Result<()>> + Send;

    /// What the store currently holds.
    ///
    /// It returns a `Result` even where an implementation cannot fail: a network-backed store
    /// can, and a trait that could not express it would force every caller of the real store to
    /// pretend otherwise.
    fn summary(&self) -> impl Future<Output = Result<StoreSummary>> + Send;
}

/// A snapshot of a store, in the form a test compares byte for byte.
///
/// `op_code_counts` is a `BTreeMap` because a snapshot that reordered itself between runs would
/// fail for a reason that has nothing to do with ingestion.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct StoreSummary {
    /// How many transactions are held.
    pub tx_count: usize,
    /// How many messages are held.
    pub message_count: usize,
    /// The highest masterchain sequence number written.
    pub checkpoint: Option<u32>,
    /// How many messages carry each opcode.
    pub op_code_counts: BTreeMap<u32, usize>,
}

/// The reference store: the same keys as the real one, held in memory.
#[derive(Default)]
pub struct InMemoryStore {
    inner: Mutex<Inner>,
}

#[derive(Default)]
struct Inner {
    txs: HashMap<(TonAddress, Lt), RawTx>,
    messages: HashMap<(TxHash, Direction, Lt), RawMessage>,
    checkpoint: Option<u32>,
}

impl InMemoryStore {
    /// Every transaction it holds, in whatever order the map returns them.
    ///
    /// Unordered on purpose: what order they belong in is a question about what is being written,
    /// and [`crate::snapshot::Snapshot`] answers it rather than this method deciding for it.
    #[must_use]
    pub fn txs(&self) -> Vec<RawTx> {
        self.inner
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .txs
            .values()
            .cloned()
            .collect()
    }

    /// Every message it holds, in whatever order the map returns them.
    #[must_use]
    pub fn messages(&self) -> Vec<RawMessage> {
        self.inner
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .messages
            .values()
            .cloned()
            .collect()
    }
}

#[allow(
    clippy::unused_async_trait_impl,
    reason = "the trait states its methods as futures so callers may spawn them; a body that waits for nothing still has to do its work when polled, and `std::future::ready` would do it when the future is built"
)]
impl Store for InMemoryStore {
    async fn write_batch(&self, batch: BlockBatch) -> Result<()> {
        let mut inner = self.inner.lock().unwrap_or_else(PoisonError::into_inner);
        for tx in batch.txs {
            inner.txs.insert((tx.account, tx.lt), tx);
        }
        for msg in batch.messages {
            inner
                .messages
                .insert((msg.tx_hash, msg.direction, msg.created_lt), msg);
        }
        inner.checkpoint = Some(inner.checkpoint.map_or(batch.seqno, |c| c.max(batch.seqno)));
        Ok(())
    }

    async fn checkpoint(&self) -> Result<Option<u32>> {
        Ok(self
            .inner
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .checkpoint)
    }

    async fn invalidate(&self, tx_hashes: &[TxHash]) -> Result<()> {
        let mut inner = self.inner.lock().unwrap_or_else(PoisonError::into_inner);
        let set: HashSet<&TxHash> = tx_hashes.iter().collect();
        inner
            .txs
            .retain(|_, tx| !(set.contains(&tx.tx_hash) && tx.finality != Finality::Finalized));
        inner
            .messages
            .retain(|_, msg| !(set.contains(&msg.tx_hash) && msg.finality != Finality::Finalized));
        Ok(())
    }

    async fn summary(&self) -> Result<StoreSummary> {
        let inner = self.inner.lock().unwrap_or_else(PoisonError::into_inner);
        let mut op_code_counts = BTreeMap::new();
        for msg in inner.messages.values() {
            *op_code_counts.entry(msg.op_code).or_insert(0) += 1;
        }
        Ok(StoreSummary {
            tx_count: inner.txs.len(),
            message_count: inner.messages.len(),
            checkpoint: inner.checkpoint,
            op_code_counts,
        })
    }
}
