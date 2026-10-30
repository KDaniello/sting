//! The shapes a funding graph draws: a hub fanning out, a collector sweeping in, a chain passing
//! money along, and a tree doing the first of those twice over.
//!
//! One shape from the standard taxonomy is missing, deliberately. Whether a group behaves *as a
//! batch* — synchronised in time and near-identical in every feature — needs the cluster's full
//! feature set and an agreement measure over it, and neither exists yet. A flag computed without
//! them would be a guess with a column of its own.

use std::collections::HashSet;

use petgraph::Direction as PgDirection;
use petgraph::graph::{DiGraph, NodeIndex};
use petgraph::visit::EdgeRef;
use sting_sybil_types::{FundingEdge, OwnerId};

/// Above how much fan-out a node is a hub, and above how much a tree root.
///
/// The caller's, not this module's: a threshold written into an algorithm is a threshold fitted
/// to whichever data it was written against, and a sensible default belongs to whatever surface
/// the operator configures.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TopologyThresholds {
    /// How many first-fundings make a node a star.
    pub star_fan_threshold: usize,
    /// How many make it a tree root.
    pub tree_fan_threshold: usize,
}

/// Whether this node funded at least `threshold` addresses: a distributor.
#[must_use]
pub fn detect_star_divergence(
    graph: &DiGraph<OwnerId, FundingEdge>,
    node: NodeIndex,
    threshold: usize,
) -> bool {
    graph.edges_directed(node, PgDirection::Outgoing).count() >= threshold
}

/// Whether at least `threshold` addresses funded this node: a collector.
#[must_use]
pub fn detect_star_convergence(
    graph: &DiGraph<OwnerId, FundingEdge>,
    node: NodeIndex,
    threshold: usize,
) -> bool {
    graph.edges_directed(node, PgDirection::Incoming).count() >= threshold
}

/// Whether `path` is a chain: money passed along through nodes that do nothing else.
///
/// Every node in the middle has exactly one way in and one way out. A node with a second edge is
/// a fork or a hub, and a path through it is a route rather than a chain.
#[must_use]
pub fn is_chain(graph: &DiGraph<OwnerId, FundingEdge>, path: &[NodeIndex]) -> bool {
    if path.len() < 3 {
        return false;
    }
    let edges_exist = path
        .windows(2)
        .all(|pair| graph.find_edge(pair[0], pair[1]).is_some());
    let middle_is_linear = path[1..path.len() - 1].iter().all(|&node| {
        graph.edges_directed(node, PgDirection::Incoming).count() == 1
            && graph.edges_directed(node, PgDirection::Outgoing).count() == 1
    });
    edges_exist && middle_is_linear
}

/// Whether this node is the root of a hierarchy: it fans out, and at least one of the addresses
/// it funded fans out too.
#[must_use]
pub fn is_tree_root(
    graph: &DiGraph<OwnerId, FundingEdge>,
    node: NodeIndex,
    threshold: usize,
) -> bool {
    if !detect_star_divergence(graph, node, threshold) {
        return false;
    }
    graph
        .edges_directed(node, PgDirection::Outgoing)
        .any(|edge| detect_star_divergence(graph, edge.target(), threshold))
}

/// Every maximal chain in the graph.
///
/// Finding candidates rather than only checking one: a caller who had to supply the path already
/// knew the answer. The walk starts at each node whose single predecessor is not itself
/// pass-through — that is where a run begins — and follows single-in, single-out nodes until one
/// of them is not. Each pass-through node is visited once, so the cost is linear in the edges
/// rather than quadratic in the nodes.
#[must_use]
pub fn find_chain_paths(graph: &DiGraph<OwnerId, FundingEdge>) -> Vec<Vec<NodeIndex>> {
    let is_pass_through = |node: NodeIndex| {
        graph.edges_directed(node, PgDirection::Incoming).count() == 1
            && graph.edges_directed(node, PgDirection::Outgoing).count() == 1
    };
    let pred_of = |node: NodeIndex| -> Option<NodeIndex> {
        graph
            .edges_directed(node, PgDirection::Incoming)
            .next()
            .map(|edge| edge.source())
    };
    let succ_of = |node: NodeIndex| -> Option<NodeIndex> {
        graph
            .edges_directed(node, PgDirection::Outgoing)
            .next()
            .map(|edge| edge.target())
    };

    let mut visited: HashSet<NodeIndex> = HashSet::new();
    let mut chains = Vec::new();

    for node in graph.node_indices() {
        if !is_pass_through(node) || visited.contains(&node) {
            continue;
        }
        // A pass-through node has exactly one predecessor by definition, so this cannot be absent.
        let Some(before) = pred_of(node) else {
            continue;
        };
        if is_pass_through(before) {
            // The run starts further back and will be found from there.
            continue;
        }

        let mut middle = vec![node];
        visited.insert(node);
        let mut cursor = node;
        while let Some(next) = succ_of(cursor) {
            if !is_pass_through(next) || visited.contains(&next) {
                break;
            }
            middle.push(next);
            visited.insert(next);
            cursor = next;
        }

        let Some(&last) = middle.last() else {
            continue;
        };
        let Some(after) = succ_of(last) else {
            continue;
        };

        let mut path = vec![before];
        path.extend(middle);
        path.push(after);
        chains.push(path);
    }

    // The walk should only ever produce chains; the filter makes that true by construction rather
    // than by argument.
    chains.retain(|path| is_chain(graph, path));
    chains
}

#[cfg(test)]
mod chain_path_tests {
    use super::*;

    fn owner(byte: u8) -> OwnerId {
        OwnerId(sting_sybil_types::TonAddress::new(0, [byte; 32]))
    }

    fn edge() -> FundingEdge {
        FundingEdge {
            src: owner(0),
            dst: owner(0),
            amount: 1,
            jetton_master: None,
            tx_hash: [0; 32],
            first_ts: 0,
        }
    }

    #[test]
    fn two_chains_are_found_and_a_hub_produces_none() {
        let mut graph: DiGraph<OwnerId, FundingEdge> = DiGraph::new();

        let chain1: Vec<_> = (0u8..5).map(|i| graph.add_node(owner(i))).collect();
        for pair in chain1.windows(2) {
            graph.add_edge(pair[0], pair[1], edge());
        }

        let chain2: Vec<_> = (10u8..14).map(|i| graph.add_node(owner(i))).collect();
        for pair in chain2.windows(2) {
            graph.add_edge(pair[0], pair[1], edge());
        }

        // A hub funding twenty-five addresses: neither it nor its children are pass-through, so
        // no chain can come out of it however long the walk runs.
        let hub = graph.add_node(owner(20));
        for i in 21u8..=45 {
            let child = graph.add_node(owner(i));
            graph.add_edge(hub, child, edge());
        }

        let mut found = find_chain_paths(&graph);
        found.sort_by_key(|path| path[0]);

        assert_eq!(found.len(), 2);
        assert_eq!(found[0], chain1);
        assert_eq!(found[1], chain2);
    }

    #[test]
    fn an_empty_graph_holds_no_chains() {
        let graph: DiGraph<OwnerId, FundingEdge> = DiGraph::new();
        assert!(find_chain_paths(&graph).is_empty());
    }

    #[test]
    fn every_path_returned_is_a_chain() {
        let mut graph: DiGraph<OwnerId, FundingEdge> = DiGraph::new();
        let nodes: Vec<_> = (0u8..6).map(|i| graph.add_node(owner(i))).collect();
        for pair in nodes.windows(2) {
            graph.add_edge(pair[0], pair[1], edge());
        }
        let chains = find_chain_paths(&graph);
        assert!(!chains.is_empty());
        for path in &chains {
            assert!(is_chain(&graph, path));
        }
    }
}
