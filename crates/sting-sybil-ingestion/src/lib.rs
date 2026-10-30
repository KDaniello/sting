//! Following the chain tip, writing what it carries, and replaying a range to the same rows.
//!
//! Ingestion is the only layer that talks to a chain endpoint and the only one that writes raw
//! rows. What it produces is deliberately unresolved: a message's `src` and `dst` are the
//! addresses the chain gave, which may be jetton wallets rather than their owners. Resolving one
//! into the other is the funding graph's job, and doing it here would mean an ingested row could
//! not be re-derived from the chain alone.
//!
//! Two properties hold the layer together. A write is idempotent under `(account, lt)` for a
//! transaction and `(tx_hash, direction, created_lt)` for a message, so the same range ingested
//! twice leaves one copy. A replay of a fixed range produces the same rows in the same order, so
//! a snapshot taken from it is a fact rather than a coincidence.
//!
//! # What this crate does not do
//!
//! It resolves no ownership, decodes no message body beyond its opcode, and joins nothing. It
//! does not follow shards on its own: a masterchain block is fetched with the shard transactions
//! already assembled by the endpoint. It stores no provisional row it cannot later take back.

mod clickhouse_store;
mod error;
mod follower;
mod snapshot;
mod source;
mod store;
mod toncenter;

pub use clickhouse_store::ClickHouseStore;
pub use error::{IngestError, Result};
pub use follower::{ingest_range, run_follower};
pub use snapshot::{MessageRow, SNAPSHOT_SCHEMA_VERSION, Snapshot, TxRow};
pub use source::{BlockSource, FakeBlockSource, MasterchainInfo};
pub use store::{InMemoryStore, Store, StoreSummary};
pub use toncenter::TonCenterV3Source;

use sting_sybil_types::{Finality, Lt, Nanoton, TonAddress, TxHash};

/// Which way a message ran relative to the transaction that carries it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Direction {
    /// Into the transaction's account.
    In,
    /// Out of it.
    Out,
}

/// A raw transaction as the chain reported it. `(account, lt)` is its identity.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RawTx {
    /// Whose transaction it is.
    pub account: TonAddress,
    /// Logical time within that account.
    pub lt: Lt,
    /// The transaction hash.
    pub tx_hash: TxHash,
    /// The masterchain block it was read from.
    pub mc_seqno: u32,
    /// The account's clock at that transaction, in epoch seconds.
    pub now: u32,
    /// How firmly the block it came from stands.
    pub finality: Finality,
}

/// A raw message, in or out.
///
/// `src` and `dst` are the addresses as given: either may be a jetton wallet rather than an
/// owner, and nothing here pretends otherwise.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RawMessage {
    /// The transaction this message belongs to.
    pub tx_hash: TxHash,
    /// Which way it ran.
    pub direction: Direction,
    /// The address it came from, unresolved.
    pub src: TonAddress,
    /// The address it went to, unresolved.
    pub dst: TonAddress,
    /// Native value carried, in nanotons.
    pub value_ton: Nanoton,
    /// The leading opcode of the body, or zero where the body carries none.
    pub op_code: u32,
    /// The body itself, still a bag of cells.
    pub body_boc: Vec<u8>,
    /// Logical time the message was created at.
    pub created_lt: Lt,
    /// The masterchain block it was read from.
    pub mc_seqno: u32,
    /// How firmly the block it was read from stands.
    pub finality: Finality,
}

/// One masterchain block with everything read out of it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BlockBatch {
    /// The masterchain sequence number.
    pub seqno: u32,
    /// How firmly the block stands.
    pub finality: Finality,
    /// The transactions across all its shards.
    pub txs: Vec<RawTx>,
    /// The messages those transactions carry.
    pub messages: Vec<RawMessage>,
}
