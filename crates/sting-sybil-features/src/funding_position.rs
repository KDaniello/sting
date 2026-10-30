//! Where a wallet sits in the gas graph: who funded it, how many others that same funder funded,
//! and how close in time their funding was to its own.
//!
//! Computed over the graph after exclusion, for the same reason topology is: one exchange funds
//! hundreds of thousands of addresses, and counting them as a shared funder would give every one
//! of its customers a feature that says they belong together.
//!
//! Component size is not here. How large a cluster is belongs to the cluster's own record, not to
//! a wallet's feature row, and putting it in both would put it in two places at once.

use std::collections::HashMap;

use petgraph::graph::{DiGraph, NodeIndex};
use petgraph::visit::EdgeRef;
use sting_sybil_types::{FundingEdge, OwnerId};

/// A wallet's place in the funding graph.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct FundingPosition {
    /// Who activated the address, if anybody in this range did.
    pub first_funder: Option<OwnerId>,
    /// How many other addresses that funder also activated.
    pub shared_funder_count: u32,
    /// How far this address's funding was from the middle of theirs, in seconds.
    pub funding_sync_delta: f64,
}

/// The position of every owner in the graph, from one pass over its edges.
///
/// One pass rather than a query per owner. The first funder is a constant-time lookup because the
/// gas graph gives each node at most one incoming edge by construction — there is never a second
/// candidate to choose between.
///
/// Siblings are the other addresses with the *same first funder*, rather than the whole connected
/// component: a component can be enormous and only loosely related in time, and a delta measured
/// against it would say more about the component's size than about whether two addresses were
/// funded together.
#[must_use]
pub fn extract_funding_position_batch(
    gas: &DiGraph<OwnerId, FundingEdge>,
) -> HashMap<OwnerId, FundingPosition> {
    let mut by_funder: HashMap<OwnerId, Vec<(OwnerId, f64)>> = HashMap::new();
    let mut first_funder_of: HashMap<NodeIndex, OwnerId> = HashMap::new();
    let mut first_ts_of: HashMap<NodeIndex, u32> = HashMap::new();

    for edge in gas.edge_references() {
        let funder = gas[edge.source()];
        let child = gas[edge.target()];
        let ts = edge.weight().first_ts;
        by_funder
            .entry(funder)
            .or_default()
            .push((child, f64::from(ts)));
        first_funder_of.insert(edge.target(), funder);
        first_ts_of.insert(edge.target(), ts);
    }

    let mut result = HashMap::new();
    for index in gas.node_indices() {
        let owner = gas[index];
        let position = match first_funder_of.get(&index) {
            None => FundingPosition::default(),
            Some(funder) => {
                let siblings = by_funder.get(funder).map_or(&[][..], Vec::as_slice);
                let own_ts = first_ts_of.get(&index).copied().map_or(0.0, f64::from);
                let neighbour_ts: Vec<f64> = siblings
                    .iter()
                    .filter(|(child, _)| *child != owner)
                    .map(|(_, ts)| *ts)
                    .collect();
                FundingPosition {
                    first_funder: Some(*funder),
                    shared_funder_count: u32::try_from(neighbour_ts.len()).unwrap_or(u32::MAX),
                    funding_sync_delta: if neighbour_ts.is_empty() {
                        0.0
                    } else {
                        (own_ts - median(&neighbour_ts)).abs()
                    },
                }
            }
        };
        result.insert(owner, position);
    }
    result
}

/// The median, over values that came from timestamps and therefore cannot be NaN.
fn median(xs: &[f64]) -> f64 {
    let mut sorted = xs.to_vec();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let mid = sorted.len() / 2;
    if sorted.len().is_multiple_of(2) {
        f64::midpoint(sorted[mid - 1], sorted[mid])
    } else {
        sorted[mid]
    }
}

#[cfg(test)]
#[allow(
    clippy::float_cmp,
    reason = "these are exact values a feature computes from small integers, not the result of accumulated arithmetic"
)]
mod tests {
    use super::*;
    use sting_sybil_types::TonAddress;

    fn addr(byte: u8) -> TonAddress {
        TonAddress::new(0, [byte; 32])
    }

    fn owner(byte: u8) -> OwnerId {
        OwnerId(addr(byte))
    }

    fn edge(src: u8, dst: u8, first_ts: u32, tx: u8) -> FundingEdge {
        FundingEdge {
            src: owner(src),
            dst: owner(dst),
            amount: 1,
            jetton_master: None,
            tx_hash: [tx; 32],
            first_ts,
        }
    }

    #[test]
    fn five_addresses_from_one_funder_each_see_the_other_four() {
        let mut graph: DiGraph<OwnerId, FundingEdge> = DiGraph::new();
        let funder = graph.add_node(owner(0));
        for i in 1..=5u8 {
            let child = graph.add_node(owner(i));
            graph.add_edge(funder, child, edge(0, i, 1000 + u32::from(i) * 10, i));
        }

        let positions = extract_funding_position_batch(&graph);

        for i in 1..=5u8 {
            let position = &positions[&owner(i)];
            assert_eq!(position.first_funder, Some(owner(0)));
            assert_eq!(position.shared_funder_count, 4);
        }

        // Funded at 1010, against siblings at 1020, 1030, 1040 and 1050: the middle of those is
        // 1035, so the gap is 25.
        assert!((positions[&owner(1)].funding_sync_delta - 25.0).abs() < 1e-9);
    }

    #[test]
    fn a_funder_has_no_funder_of_its_own_here() {
        let mut graph: DiGraph<OwnerId, FundingEdge> = DiGraph::new();
        let funder = graph.add_node(owner(0));
        let child = graph.add_node(owner(1));
        graph.add_edge(funder, child, edge(0, 1, 1000, 1));

        let positions = extract_funding_position_batch(&graph);
        assert_eq!(positions[&owner(0)].first_funder, None);
        assert_eq!(positions[&owner(0)].shared_funder_count, 0);
        assert_eq!(positions[&owner(0)].funding_sync_delta, 0.0);
    }

    #[test]
    fn an_only_child_is_in_sync_with_nobody() {
        let mut graph: DiGraph<OwnerId, FundingEdge> = DiGraph::new();
        let funder = graph.add_node(owner(0));
        let child = graph.add_node(owner(1));
        graph.add_edge(funder, child, edge(0, 1, 1000, 1));

        let positions = extract_funding_position_batch(&graph);
        let position = &positions[&owner(1)];
        assert_eq!(position.shared_funder_count, 0);
        assert_eq!(position.funding_sync_delta, 0.0);
    }
}
