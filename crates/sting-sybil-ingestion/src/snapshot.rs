//! What a range of blocks ingested to, written down so that a second run can be held against it.
//!
//! A summary of counts is not a snapshot. Two ingestions of one range can agree on how many
//! transactions they hold and disagree about which, and a check that compares only the counts
//! passes on the day that matters. So what is written here is every row, in one order, with the
//! bodies as they arrived.
//!
//! **The order is the point.** Rows live in a hash map keyed by their identity, and a hash map hands
//! them back in whatever order it happens to iterate. Sorted by that same identity — `(mc_seqno,
//! account, lt)` for a transaction, `(tx_hash, direction, created_lt)` for a message — one range
//! ingests to one file, and a difference between two runs is a difference in what the chain
//! answered rather than in how a map felt about it.
//!
//! **What this does not do.** It does not snapshot the column store: what is written is what the
//! ingestion produced, read out of the in-memory store, and a store that transformed a row on the
//! way in would not be caught by it. It carries no timestamp of its own, because a file that
//! records when it was written cannot be compared with one written at another time.

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64;

use crate::store::{InMemoryStore, StoreSummary};
use crate::{Direction, RawMessage, RawTx};

/// This file format's own version.
pub const SNAPSHOT_SCHEMA_VERSION: &str = "1.0";

/// Everything one replay of a fixed range produced.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct Snapshot {
    /// Which version of this file's shape it is in.
    pub schema_version: String,
    /// Where the blocks came from: the chain, or the synthetic generator.
    pub source: String,
    /// First masterchain sequence number.
    pub from: u32,
    /// Last masterchain sequence number, inclusive.
    pub to: u32,
    /// Every transaction, in identity order.
    pub txs: Vec<TxRow>,
    /// Every message, in identity order.
    pub messages: Vec<MessageRow>,
    /// The counts, so that a reader sees the shape before reading the rows.
    pub summary: StoreSummary,
}

/// One transaction as a snapshot records it.
///
/// A view rather than the row type itself: a row is what this crate passes around, and giving it a
/// wire format by derive is how a format starts changing whenever a field is added for another
/// reason.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct TxRow {
    /// The masterchain block it was read from.
    pub mc_seqno: u32,
    /// Whose transaction it is, raw form.
    pub account: String,
    /// Logical time within that account.
    pub lt: u64,
    /// The transaction hash, in hex.
    pub tx_hash: String,
    /// The account's clock at that transaction, in epoch seconds.
    pub now: u32,
    /// How firmly the block it came from stands.
    pub finality: String,
}

/// One message as a snapshot records it.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct MessageRow {
    /// The transaction this message belongs to, in hex.
    pub tx_hash: String,
    /// Which way it ran.
    pub direction: String,
    /// The address it came from, unresolved, raw form.
    pub src: String,
    /// The address it went to, unresolved, raw form.
    pub dst: String,
    /// Native value carried, in nanotons.
    pub value_ton: u64,
    /// The leading opcode of the body, or zero where the body carries none.
    pub op_code: u32,
    /// The body itself, as base64 of the bag of cells it arrived as.
    pub body: String,
    /// Logical time the message was created at.
    pub created_lt: u64,
    /// The masterchain block it was read from.
    pub mc_seqno: u32,
    /// How firmly the block it was read from stands.
    pub finality: String,
}

impl Snapshot {
    /// Reads everything `store` holds into a snapshot of `from..=to` from `source`.
    #[must_use]
    pub fn of(
        store: &InMemoryStore,
        source: &str,
        from: u32,
        to: u32,
        summary: StoreSummary,
    ) -> Self {
        let mut txs: Vec<TxRow> = store.txs().iter().map(TxRow::of).collect();
        txs.sort_by(|left, right| {
            (left.mc_seqno, &left.account, left.lt).cmp(&(right.mc_seqno, &right.account, right.lt))
        });
        let mut messages: Vec<MessageRow> = store.messages().iter().map(MessageRow::of).collect();
        messages.sort_by(|left, right| {
            (&left.tx_hash, &left.direction, left.created_lt).cmp(&(
                &right.tx_hash,
                &right.direction,
                right.created_lt,
            ))
        });
        Self {
            schema_version: SNAPSHOT_SCHEMA_VERSION.to_string(),
            source: source.to_string(),
            from,
            to,
            txs,
            messages,
            summary,
        }
    }

    /// The snapshot as the bytes a file holds.
    ///
    /// # Errors
    /// When the snapshot cannot be serialised, which needs a row this crate could not have built.
    pub fn to_bytes(&self) -> Result<Vec<u8>, serde_json::Error> {
        serde_json::to_vec_pretty(self)
    }
}

impl TxRow {
    /// One row, as a snapshot records it.
    fn of(tx: &RawTx) -> Self {
        Self {
            mc_seqno: tx.mc_seqno,
            account: tx.account.to_raw_string(),
            lt: tx.lt,
            tx_hash: hex::encode(tx.tx_hash),
            now: tx.now,
            finality: format!("{:?}", tx.finality).to_lowercase(),
        }
    }
}

impl MessageRow {
    /// One row, as a snapshot records it.
    fn of(message: &RawMessage) -> Self {
        Self {
            tx_hash: hex::encode(message.tx_hash),
            direction: match message.direction {
                Direction::In => "in".to_string(),
                Direction::Out => "out".to_string(),
            },
            src: message.src.to_raw_string(),
            dst: message.dst.to_raw_string(),
            value_ton: message.value_ton,
            op_code: message.op_code,
            body: BASE64.encode(&message.body_boc),
            created_lt: message.created_lt,
            mc_seqno: message.mc_seqno,
            finality: format!("{:?}", message.finality).to_lowercase(),
        }
    }
}
