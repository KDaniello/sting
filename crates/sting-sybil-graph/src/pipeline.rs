//! The whole way through: raw rows in, verified events, correlated traces, edges out.
//!
//! Resolution, decoding, correlation and the graphs are four steps that each mean nothing alone.
//! This module is what joins them, and it is the only place that knows their order.

use std::collections::HashMap;

use sting_sybil_ingestion::{
    BlockBatch, ClickHouseStore, Direction as MsgDirection, RawMessage, RawTx,
};
use sting_sybil_types::{AccountKind, FundingEdge, JettonWalletAddr, OwnerId, TonAddress, TxHash};
use sting_tep::{
    JettonOp, OP_EXCESSES, OP_INTERNAL_TRANSFER, OP_TRANSFER, OP_TRANSFER_NOTIFICATION,
};

use crate::exclusion::{ExclusionCfg, apply_exclusion};
use crate::jetton::JettonEvent;
use crate::resolve::resolve_and_verify_owner_with_master;
use crate::{
    FundingGraphs, JettonRpc, MasterAllowlist, Result, correlate_traces, decode_jetton_body,
};

fn is_known_jetton_op_code(op_code: u32) -> bool {
    matches!(
        op_code,
        OP_TRANSFER | OP_INTERNAL_TRANSFER | OP_TRANSFER_NOTIFICATION | OP_EXCESSES
    )
}

/// Which wallet contract this particular message belongs to.
///
/// Only that side is resolved and verified: the counterparty is already named in the body, and
/// asking the chain about it would be answering a question the message has already answered.
fn local_wallet(msg: &RawMessage, op: &JettonOp) -> JettonWalletAddr {
    match op {
        JettonOp::Transfer { .. } => JettonWalletAddr(msg.dst),
        JettonOp::InternalTransfer { .. }
        | JettonOp::TransferNotification { .. }
        | JettonOp::Excesses { .. } => JettonWalletAddr(msg.src),
    }
}

/// Decodes and verifies the jetton messages of one block.
///
/// A message is skipped when its opcode is not one of the standard's, when its body does not
/// read, or when its wallet does not verify against an allowed master. Missing an event is the
/// cheaper mistake: a spoofed wallet that was believed would draw an edge between two people who
/// have never met.
///
/// Resolution is cached for the length of one call. A cache that survives blocks is worth having
/// and is infrastructure — a store or an eviction policy — rather than something this function
/// should be deciding.
///
/// `bounced` is always false in what comes out of here, and that is not an omission: a real
/// bounce rewrites the message's opcode to the bounce marker followed by an echo of the body, so
/// a bounced `internal_transfer` never reaches the decode above. It is filtered by its opcode
/// rather than carried as a flag.
#[must_use]
pub fn events_from_block(
    batch: &BlockBatch,
    rpc: &impl JettonRpc,
    allowlist: &MasterAllowlist,
) -> Vec<JettonEvent> {
    let tx_now: HashMap<TxHash, u32> = batch.txs.iter().map(|tx| (tx.tx_hash, tx.now)).collect();
    let mut wallet_cache: HashMap<JettonWalletAddr, Option<(OwnerId, TonAddress)>> = HashMap::new();

    let mut events = Vec::new();
    for msg in &batch.messages {
        if !is_known_jetton_op_code(msg.op_code) {
            continue;
        }
        let Ok(op) = decode_jetton_body(msg.op_code, &msg.body_boc) else {
            continue;
        };
        let wallet = local_wallet(msg, &op);
        let verified = *wallet_cache
            .entry(wallet)
            .or_insert_with(|| resolve_and_verify_owner_with_master(rpc, wallet, allowlist));
        let Some((_owner, jetton_master)) = verified else {
            continue;
        };

        events.push(JettonEvent {
            jetton_wallet: wallet,
            jetton_master,
            op,
            lt: msg.created_lt,
            bounced: false,
            tx_hash: msg.tx_hash,
            now: tx_now.get(&msg.tx_hash).copied().unwrap_or(0),
        });
    }
    events
}

fn trace_to_value_edge(trace: &sting_sybil_types::TransferTrace) -> FundingEdge {
    FundingEdge {
        src: trace.from_owner,
        dst: trace.to_owner,
        amount: trace.amount,
        jetton_master: Some(trace.jetton_master),
        tx_hash: trace.tx_hash,
        first_ts: trace.completed_ts,
    }
}

/// Verified events of one block, correlated into transfers, added to the value graph.
pub fn ingest_block_into_value_graph(
    batch: &BlockBatch,
    rpc: &impl JettonRpc,
    allowlist: &MasterAllowlist,
    graphs: &mut FundingGraphs,
) {
    let events = events_from_block(batch, rpc, allowlist);
    for trace in correlate_traces(&events) {
        graphs.add_value_edge(trace_to_value_edge(&trace));
    }
}

/// The first native transfer into each address of one block, added to the gas graph.
///
/// Separate from the jetton path and much simpler, because a native transfer already runs between
/// owner addresses: there is no wallet contract in the middle and nothing to resolve.
///
/// Only incoming messages are counted. One logical message appears twice in a block — outgoing on
/// the sender's transaction and incoming on the recipient's — so counting both would count every
/// transfer twice.
///
/// First means first within what has been processed. A streaming pipeline that wants first ever
/// keeps one `graphs` across blocks, at which point the deduplication by destination holds across
/// block boundaries too; that is the caller's decision and not this function's.
pub fn ingest_block_into_gas_graph(batch: &BlockBatch, graphs: &mut FundingGraphs) {
    let tx_now: HashMap<TxHash, u32> = batch.txs.iter().map(|tx| (tx.tx_hash, tx.now)).collect();

    let mut plain_transfers: Vec<&RawMessage> = batch
        .messages
        .iter()
        .filter(|msg| msg.op_code == 0 && msg.value_ton > 0 && msg.direction == MsgDirection::In)
        .collect();
    plain_transfers.sort_by_key(|msg| msg.created_lt);

    for msg in plain_transfers {
        graphs.add_first_funding(FundingEdge {
            src: OwnerId(msg.src),
            dst: OwnerId(msg.dst),
            amount: u128::from(msg.value_ton),
            jetton_master: None,
            tx_hash: msg.tx_hash,
            first_ts: tx_now.get(&msg.tx_hash).copied().unwrap_or(0),
        });
    }
}

/// Both graphs from one block.
pub fn ingest_block(
    batch: &BlockBatch,
    rpc: &impl JettonRpc,
    allowlist: &MasterAllowlist,
    graphs: &mut FundingGraphs,
) {
    ingest_block_into_value_graph(batch, rpc, allowlist, graphs);
    ingest_block_into_gas_graph(batch, graphs);
}

/// Rows from storage, a gas graph, and the exclusion pass — the sequence two callers both need
/// whole.
///
/// Feature extraction and clustering each want the same post-exclusion gas graph over the same
/// range of blocks. Written out in one of them, the other would grow a second copy that drifts,
/// which is the reason the column mapping lives in one place too.
///
/// The value graph is deliberately left empty here. There is no live implementation of
/// [`JettonRpc`] anywhere in the tree — only test doubles — so a value graph built on this path
/// would be a graph of nothing, and pretending otherwise would be worse than the documented gap.
///
/// # Errors
/// [`crate::GraphError`] when the rows cannot be read.
pub async fn load_gas_graph_from_range(
    store: &ClickHouseStore,
    from: u32,
    to: u32,
    exclusion_cfg: &ExclusionCfg,
    account_kind: impl Fn(OwnerId) -> AccountKind,
) -> Result<(FundingGraphs, Vec<RawTx>, Vec<RawMessage>)> {
    let txs = store.fetch_txs_in_seqno_range(from, to).await?;
    let messages = store.fetch_messages_in_seqno_range(from, to).await?;

    let batch = BlockBatch {
        seqno: to,
        finality: sting_sybil_types::Finality::Finalized,
        txs,
        messages,
    };
    let mut graphs = FundingGraphs::default();
    ingest_block_into_gas_graph(&batch, &mut graphs);
    graphs.gas = apply_exclusion(&graphs.gas, exclusion_cfg, &account_kind);
    graphs.value = apply_exclusion(&graphs.value, exclusion_cfg, &account_kind);

    Ok((graphs, batch.txs, batch.messages))
}
