//! Breaking a giant component down by peeling its periphery.
//!
//! A weakly connected component can swallow a whole subnetwork through one busy address, and what
//! is wanted from it is the dense middle rather than the chain of addresses hanging off it. The
//! k-core does that with no external library and no export to another runtime: strip every node
//! with fewer than `k` neighbours, and repeat until nothing more falls off.

use std::collections::{HashMap, HashSet};

use petgraph::Direction as PgDirection;
use petgraph::graph::{DiGraph, NodeIndex};
use petgraph::visit::EdgeRef;

/// The subgraph in which every node has at least `k` neighbours, direction ignored.
///
/// Iterative rather than one pass: removing a node lowers its neighbours' degree, so a single
/// sweep would leave nodes that no longer qualify.
#[must_use]
pub fn k_core_prune<N: Clone, E: Clone>(graph: &DiGraph<N, E>, k: usize) -> DiGraph<N, E> {
    let mut alive: HashSet<NodeIndex> = graph.node_indices().collect();
    loop {
        let to_remove: Vec<NodeIndex> = alive
            .iter()
            .copied()
            .filter(|&node| undirected_degree(graph, node, &alive) < k)
            .collect();
        if to_remove.is_empty() {
            break;
        }
        for node in to_remove {
            alive.remove(&node);
        }
    }

    let mut result = DiGraph::new();
    let mut mapping = HashMap::new();
    for &node in &alive {
        mapping.insert(node, result.add_node(graph[node].clone()));
    }
    for edge in graph.edge_references() {
        if alive.contains(&edge.source()) && alive.contains(&edge.target()) {
            result.add_edge(
                mapping[&edge.source()],
                mapping[&edge.target()],
                edge.weight().clone(),
            );
        }
    }
    result
}

fn undirected_degree<N, E>(
    graph: &DiGraph<N, E>,
    node: NodeIndex,
    alive: &HashSet<NodeIndex>,
) -> usize {
    graph
        .edges_directed(node, PgDirection::Outgoing)
        .filter(|edge| alive.contains(&edge.target()))
        .count()
        + graph
            .edges_directed(node, PgDirection::Incoming)
            .filter(|edge| alive.contains(&edge.source()))
            .count()
}
