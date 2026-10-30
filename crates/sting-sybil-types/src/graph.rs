//! The two records the funding graph is built out of: a completed transfer, and the edge it
//! becomes.

use serde::Serialize;

use crate::chain::{JettonAmount, Lt, TxHash};
use sting_address::{OwnerId, TonAddress};

/// One edge of the funding graph, by value or by gas.
///
/// `src` and `dst` are always owners and never jetton wallets. The type system holds the
/// invariant rather than a comment: there is no conversion between [`OwnerId`] and
/// [`sting_address::JettonWalletAddr`], so a wallet address cannot be passed here at all.
///
/// `Serialize` is on it because an edge is written out as a row, not only held in memory.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize)]
pub struct FundingEdge {
    /// Who sent.
    pub src: OwnerId,
    /// Who received.
    pub dst: OwnerId,
    /// How much moved.
    pub amount: JettonAmount,
    /// Which jetton, or none for native TON.
    pub jetton_master: Option<TonAddress>,
    /// The transaction the edge is claimed from.
    pub tx_hash: TxHash,
    /// When it settled, in epoch seconds.
    pub first_ts: u32,
}

/// A jetton transfer correlated by `query_id` across the two to four transactions it takes.
///
/// An edge is made only from a trace that completed: a transfer that bounced moved nothing, and
/// counting it would join two owners who never funded each other.
///
/// `tx_hash` and `completed_ts` come from the closing `internal_transfer` — the one message every
/// trace carries — because the trace itself has no hash and no time of its own.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize)]
pub struct TransferTrace {
    /// What ties the messages of one transfer together.
    pub query_id: u64,
    /// Who sent.
    pub from_owner: OwnerId,
    /// Who received.
    pub to_owner: OwnerId,
    /// How much moved.
    pub amount: JettonAmount,
    /// Which jetton.
    pub jetton_master: TonAddress,
    /// Logical time of the message that started it.
    pub initiated_lt: Lt,
    /// Whether the closing `internal_transfer` was seen.
    pub completed: bool,
    /// The closing transaction's hash.
    pub tx_hash: TxHash,
    /// When it closed, in epoch seconds.
    pub completed_ts: u32,
}
