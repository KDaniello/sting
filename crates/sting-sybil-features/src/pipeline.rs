//! Raw rows and the funding graph in, feature rows out.
//!
//! The groups of features are computed by their own modules and know nothing of each other. This
//! is where they meet: one pass to group a range's rows by owner, then one row per owner built
//! from every group at once.
//!
//! The graphs must already have had the hot wallets taken out — the exclusion policy is the
//! caller's, exactly as it is for the graph crate. Who gets a row is the union of the two graphs'
//! nodes: an owner in neither was either excluded or never funded anybody, and inventing a row of
//! zeros for them would be inventing a wallet.
//!
//! Topology and funding position are read off the gas graph rather than the value graph. The gas
//! graph is real on live data today; the value graph needs a chain client that does not exist
//! outside test doubles, and features computed from an empty graph would be zeros that look like
//! measurements.
//!
//! Similarity between wallets is not here. It is a measure over a *pair*, and a feature row is a
//! scalar per wallet — it stays a library function rather than becoming a column that could only
//! ever hold one arbitrary half of a comparison.

use std::collections::{HashMap, HashSet};

use petgraph::graph::NodeIndex;
use sting_sybil_graph::FundingGraphs;
use sting_sybil_ingestion::{Direction, RawMessage, RawTx};
use sting_sybil_types::{OwnerId, TransferTrace};

use crate::schema::{BULK_FLAG_NOT_COMPUTED, WalletFeatureRow};
use crate::topology::TopologyThresholds;
use crate::{
    WalletEvent, detect_star_convergence, detect_star_divergence, extract_active_hours_entropy,
    extract_aggregate, extract_burst, extract_funding_position_batch, extract_temporal,
    find_chain_paths, is_tree_root,
};

/// One owner's raw material, before any feature is computed from it.
#[derive(Default)]
pub struct OwnerRawData {
    /// The owner's own transaction times, in order.
    pub timestamps: Vec<u32>,
    /// What moved, to and from whom.
    pub events: Vec<WalletEvent>,
}

/// What the extraction is parameterised by.
pub struct FeatureExtractionConfig {
    /// Which version of the vocabulary this run produces.
    pub feature_version: String,
    /// The point in time the rows describe.
    pub snapshot_ts: u32,
    /// How far below the mean gap counts as a burst.
    pub burst_factor: f64,
    /// Where a fan-out becomes a star or a tree.
    pub topology: TopologyThresholds,
}

/// Everything a range holds, grouped by owner in one pass.
///
/// One pass rather than a query per owner: at the size of a chain the second shape is not slower
/// but impossible.
///
/// The native channel comes from the messages. One logical transfer appears twice — outgoing on
/// the sender's transaction, incoming on the recipient's — and both are kept: they are one event
/// for the sender and one for the recipient, which is two rows about two wallets rather than one
/// transfer counted twice.
///
/// The jetton channel comes from correlated traces, and each completed one is an event for both
/// sides.
#[must_use]
pub fn group_raw_data_by_owner(
    txs: &[RawTx],
    messages: &[RawMessage],
    jetton_traces: &[TransferTrace],
) -> HashMap<OwnerId, OwnerRawData> {
    let mut data: HashMap<OwnerId, OwnerRawData> = HashMap::new();

    for tx in txs {
        data.entry(OwnerId(tx.account))
            .or_default()
            .timestamps
            .push(tx.now);
    }

    for msg in messages {
        if msg.op_code != 0 || msg.value_ton == 0 {
            // Not a native transfer; jettons arrive through the other channel below.
            continue;
        }
        let (owner, counterparty, sent) = match msg.direction {
            Direction::Out => (OwnerId(msg.src), OwnerId(msg.dst), true),
            Direction::In => (OwnerId(msg.dst), OwnerId(msg.src), false),
        };
        data.entry(owner)
            .or_default()
            .events
            .push(WalletEvent::native(
                counterparty,
                u128::from(msg.value_ton),
                sent,
            ));
    }

    for trace in jetton_traces {
        data.entry(trace.from_owner)
            .or_default()
            .events
            .push(WalletEvent::jetton(
                trace.to_owner,
                trace.amount,
                true,
                trace.jetton_master,
            ));
        data.entry(trace.to_owner)
            .or_default()
            .events
            .push(WalletEvent::jetton(
                trace.from_owner,
                trace.amount,
                false,
                trace.jetton_master,
            ));
    }

    for owner_data in data.values_mut() {
        owner_data.timestamps.sort_unstable();
    }
    data
}

/// One feature row per owner in the range, sorted by owner.
///
/// Sorted here as well as in the batch builder because this output is read directly — a test or a
/// snapshot that consumed it unsorted would depend on hash-map order.
#[must_use]
pub fn extract_features_for_range(
    txs: &[RawTx],
    messages: &[RawMessage],
    jetton_traces: &[TransferTrace],
    graphs: &FundingGraphs,
    cfg: &FeatureExtractionConfig,
) -> Vec<WalletFeatureRow> {
    let raw_by_owner = group_raw_data_by_owner(txs, messages, jetton_traces);
    let positions = extract_funding_position_batch(&graphs.gas);
    let gas_index: HashMap<OwnerId, NodeIndex> = graphs
        .gas
        .node_indices()
        .map(|index| (graphs.gas[index], index))
        .collect();

    let mut chain_owners: HashSet<OwnerId> = HashSet::new();
    for path in find_chain_paths(&graphs.gas) {
        chain_owners.extend(path.into_iter().map(|index| graphs.gas[index]));
    }

    // Who gets a row: the union of both graphs' nodes, deduplicated — most owners are in both.
    let mut owners: HashSet<OwnerId> = HashSet::new();
    owners.extend(graphs.value.node_indices().map(|index| graphs.value[index]));
    owners.extend(graphs.gas.node_indices().map(|index| graphs.gas[index]));

    let default_raw = OwnerRawData::default();
    let default_position = crate::FundingPosition::default();

    let mut rows: Vec<WalletFeatureRow> = owners
        .into_iter()
        .map(|owner| {
            let raw = raw_by_owner.get(&owner).unwrap_or(&default_raw);
            let temporal = extract_temporal(&raw.timestamps);
            let burst = extract_burst(&raw.timestamps, cfg.burst_factor);
            let active_hours_entropy = extract_active_hours_entropy(&raw.timestamps);
            let aggregate = extract_aggregate(&raw.events);
            let position = positions.get(&owner).unwrap_or(&default_position);

            let (star_divergence, star_convergence, tree_flag) = match gas_index.get(&owner) {
                Some(&index) => (
                    detect_star_divergence(&graphs.gas, index, cfg.topology.star_fan_threshold),
                    detect_star_convergence(&graphs.gas, index, cfg.topology.star_fan_threshold),
                    is_tree_root(&graphs.gas, index, cfg.topology.tree_fan_threshold),
                ),
                None => (false, false, false),
            };

            WalletFeatureRow {
                owner,
                snapshot_ts: cfg.snapshot_ts,
                feature_version: cfg.feature_version.clone(),
                iet_mean: temporal.iet_mean,
                iet_std: temporal.iet_std,
                iet_cv: temporal.iet_cv,
                iet_regularity: temporal.iet_regularity,
                burst_count: burst.burst_count,
                longest_burst: burst.longest_burst,
                active_duration: temporal.active_duration,
                active_hours_entropy,
                funding_sync_delta: position.funding_sync_delta,
                tx_count: aggregate.tx_count,
                in_deg: aggregate.in_deg,
                out_deg: aggregate.out_deg,
                deg_ratio: aggregate.deg_ratio,
                n_unique_in: aggregate.n_unique_in,
                n_unique_out: aggregate.n_unique_out,
                amount_mean_sent: aggregate.amount_mean_sent,
                amount_var_sent: aggregate.amount_var_sent,
                amount_mean_recv: aggregate.amount_mean_recv,
                amount_var_recv: aggregate.amount_var_recv,
                n_jetton_types: aggregate.n_jetton_types,
                novelty_ratio: aggregate.novelty_ratio,
                first_funder: position.first_funder,
                shared_funder_count: position.shared_funder_count,
                star_divergence,
                star_convergence,
                chain_flag: chain_owners.contains(&owner),
                tree_flag,
                bulk_flag: BULK_FLAG_NOT_COMPUTED,
            }
        })
        .collect();

    rows.sort_by_key(|row| row.owner.0);
    rows
}
