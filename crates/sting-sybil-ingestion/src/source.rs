//! Where blocks come from: the boundary between the writer and whatever is answering.

use std::collections::HashMap;
use std::future::Future;
use std::sync::{Mutex, PoisonError};

use crate::BlockBatch;
use crate::error::{IngestError, Result};

/// Where the chain says its tip is.
pub struct MasterchainInfo {
    /// The highest masterchain sequence number the endpoint will answer for.
    pub last_seqno: u32,
}

/// A source of masterchain blocks.
///
/// The methods return `impl Future<..> + Send` rather than being written as `async fn`: a native
/// `async fn` in a trait does not promise the returned future is `Send`, and the follower is
/// spawned onto a runtime that requires it. Implementations still write plain `async fn`.
pub trait BlockSource: Send + Sync {
    /// Where the chain's tip is right now.
    fn masterchain_info(&self) -> impl Future<Output = Result<MasterchainInfo>> + Send;

    /// One masterchain block with its shard transactions already assembled.
    fn fetch_masterchain_block(
        &self,
        seqno: u32,
    ) -> impl Future<Output = Result<BlockBatch>> + Send;
}

/// A source with no network behind it, for tests that must be deterministic.
pub struct FakeBlockSource {
    blocks: HashMap<u32, BlockBatch>,
    tip: u32,
    /// How many more times a given sequence number must fail before it succeeds.
    fail_budget: Mutex<HashMap<u32, u32>>,
}

impl FakeBlockSource {
    /// Holds `blocks`, and reports the highest sequence number among them as the tip.
    #[must_use]
    pub fn new(blocks: Vec<BlockBatch>) -> Self {
        let tip = blocks.iter().map(|b| b.seqno).max().unwrap_or(0);
        Self {
            blocks: blocks.into_iter().map(|b| (b.seqno, b)).collect(),
            tip,
            fail_budget: Mutex::new(HashMap::new()),
        }
    }

    /// The fetch itself, which needs no runtime: a block is either held or it is not.
    fn fetch_now(&self, seqno: u32) -> Result<BlockBatch> {
        {
            let mut budget = self
                .fail_budget
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            if let Some(remaining) = budget.get_mut(&seqno)
                && *remaining > 0
            {
                *remaining -= 1;
                return Err(IngestError::Transient { seqno });
            }
        }
        self.blocks
            .get(&seqno)
            .cloned()
            .ok_or(IngestError::NoSuchBlock { seqno })
    }

    /// Makes the next `times` fetches of `seqno` fail the way a flaky endpoint does.
    pub fn set_fail_budget(&self, seqno: u32, times: u32) {
        self.fail_budget
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(seqno, times);
    }
}

// The bodies wait for nothing — a fake reaches nothing — but the work still happens when the
// future is polled: spending a failure budget at the moment a future is built would charge a
// caller for a fetch it then decided not to make.
#[allow(
    clippy::unused_async_trait_impl,
    reason = "the trait states its methods as futures so callers may spawn them; a body that waits for nothing still has to do its work when polled, and `std::future::ready` would do it when the future is built"
)]
impl BlockSource for FakeBlockSource {
    async fn masterchain_info(&self) -> Result<MasterchainInfo> {
        Ok(MasterchainInfo {
            last_seqno: self.tip,
        })
    }

    async fn fetch_masterchain_block(&self, seqno: u32) -> Result<BlockBatch> {
        self.fetch_now(seqno)
    }
}
