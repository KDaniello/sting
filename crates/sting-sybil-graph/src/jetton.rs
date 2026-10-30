//! Correlating decoded jetton events into the transfers they belong to.
//!
//! Reading the bodies themselves is not here: opcodes and cell layout belong to the standard and
//! live in [`sting_tep`]. What is here is what happens after the read — grouping the messages of
//! one transfer by the query id that ties them together, and refusing to guess when the group is
//! incomplete.

use std::collections::HashMap;

use sting_sybil_types::{JettonWalletAddr, Lt, OwnerId, TonAddress, TxHash};
use sting_tep::JettonOp;

/// One decoded message of a transfer, with the envelope context the body does not carry.
///
/// `jetton_wallet` is a contract address and never an owner. It is typed as
/// [`JettonWalletAddr`] rather than a bare address so that the distinction is held by the
/// compiler here, in the code that could confuse them, and not only by a test that says it must
/// not be confused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct JettonEvent {
    /// The wallet contract that emitted or received this message.
    pub jetton_wallet: JettonWalletAddr,
    /// The jetton the wallet belongs to.
    pub jetton_master: TonAddress,
    /// What the body said.
    pub op: JettonOp,
    /// Logical time of the message.
    pub lt: Lt,
    /// Whether the message came back.
    pub bounced: bool,
    /// The transaction it was read from, which is where an edge gets its hash.
    pub tx_hash: TxHash,
    /// That transaction's clock, which is where an edge gets its time.
    pub now: u32,
}

/// A transfer correlated by query id, in the shape the rest of the contour reads.
pub type TransferTrace = sting_sybil_types::TransferTrace;

/// Groups events by `(jetton_master, query_id)` and returns the transfers that completed.
///
/// A trace becomes a transfer only when two things are visible: an `internal_transfer` that did
/// not bounce — the one message every transfer carries — and the originating `transfer`, which
/// names the recipient as an owner. Without the second, the only way to name the recipient would
/// be to treat a jetton-wallet address as its owner, and an edge drawn that way joins two people
/// who never funded each other. Missing an edge is the cheaper mistake, so the trace is dropped.
#[must_use]
pub fn correlate_traces(events: &[JettonEvent]) -> Vec<TransferTrace> {
    let mut groups: HashMap<(TonAddress, u64), Vec<&JettonEvent>> = HashMap::new();
    for event in events {
        groups
            .entry((event.jetton_master, event.op.query_id()))
            .or_default()
            .push(event);
    }

    let mut traces = Vec::new();
    for ((jetton_master, query_id), group) in groups {
        let Some(internal) = group
            .iter()
            .find(|event| matches!(event.op, JettonOp::InternalTransfer { .. }) && !event.bounced)
        else {
            continue;
        };
        let JettonOp::InternalTransfer { amount, from, .. } = internal.op else {
            continue;
        };

        let to_owner = group.iter().find_map(|event| match event.op {
            JettonOp::Transfer { destination, .. } => Some(OwnerId(destination)),
            _ => None,
        });
        let Some(to_owner) = to_owner else {
            continue;
        };

        traces.push(TransferTrace {
            query_id,
            from_owner: OwnerId(from),
            to_owner,
            amount,
            jetton_master,
            initiated_lt: internal.lt,
            completed: true,
            tx_hash: internal.tx_hash,
            completed_ts: internal.now,
        });
    }
    traces
}
