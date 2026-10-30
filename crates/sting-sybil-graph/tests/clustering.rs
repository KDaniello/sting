//! What the hard clustering pass promises: correct components, a giant one that breaks down, a
//! result that does not depend on the run, and a membership a feature can be built from.
//!
//! That exclusion is what stops a mega-component forming in the first place is proven where
//! exclusion lives, and is not restated here. The soft pass belongs to the Python side.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "AGENTS.md §5 scopes the no-unwrap/expect/panic rule to library code outside tests"
)]

use std::collections::HashMap;

use petgraph::graph::{DiGraph, NodeIndex};
use sting_sybil_graph::{
    component_sizes, k_core_prune, largest_component_size, weakly_connected_components,
};
use sting_sybil_types::{FundingEdge, OwnerId, TonAddress};

fn addr(byte: u8) -> TonAddress {
    TonAddress::new(0, [byte; 32])
}

fn owner(byte: u8) -> OwnerId {
    OwnerId(addr(byte))
}

#[test]
fn components_come_out_as_the_graph_was_built() {
    // Three components that share nothing: {0,1,2}, {3,4}, and 5 on its own.
    let mut graph: DiGraph<(), ()> = DiGraph::new();
    let nodes: Vec<_> = (0..6).map(|_| graph.add_node(())).collect();
    graph.add_edge(nodes[0], nodes[1], ());
    graph.add_edge(nodes[1], nodes[2], ());
    graph.add_edge(nodes[3], nodes[4], ());

    let uf = weakly_connected_components(&graph);
    assert_eq!(uf.find(nodes[0].index()), uf.find(nodes[1].index()));
    assert_eq!(uf.find(nodes[1].index()), uf.find(nodes[2].index()));
    assert_eq!(uf.find(nodes[3].index()), uf.find(nodes[4].index()));
    assert_ne!(
        uf.find(nodes[0].index()),
        uf.find(nodes[3].index()),
        "two components that share no edge must not merge"
    );
    assert_ne!(uf.find(nodes[0].index()), uf.find(nodes[5].index()));
    assert_ne!(uf.find(nodes[3].index()), uf.find(nodes[5].index()));
}

#[test]
fn pruning_keeps_the_dense_middle_and_drops_the_periphery() {
    // A clique of five, each with degree four, and ten leaves hanging off one of them.
    let mut graph: DiGraph<u32, ()> = DiGraph::new();
    let core: Vec<_> = (0..5).map(|i| graph.add_node(i)).collect();
    for i in 0..core.len() {
        for j in (i + 1)..core.len() {
            graph.add_edge(core[i], core[j], ());
        }
    }
    for i in 5..15 {
        let leaf = graph.add_node(i);
        graph.add_edge(core[0], leaf, ());
    }

    let pruned = k_core_prune(&graph, 3);
    assert_eq!(pruned.node_count(), 5, "only the clique has degree three");
    assert_eq!(pruned.edge_count(), 10, "a clique of five has ten edges");
}

/// A stand-in for scale, at a size that runs in a suite.
///
/// It says nothing about the pace on a real network — a chain of 50 000 nodes is the easiest
/// shape there is. What it catches is the accident that matters: an implementation whose cost is
/// quadratic looks fine on a hundred nodes and never finishes on a million.
#[test]
fn components_of_fifty_thousand_nodes_are_found_in_seconds() {
    let n = 50_000usize;
    let mut graph: DiGraph<u32, ()> = DiGraph::new();
    let nodes: Vec<_> = (0..u32::try_from(n).unwrap())
        .map(|i| graph.add_node(i))
        .collect();
    for pair in nodes.windows(2) {
        graph.add_edge(pair[0], pair[1], ());
    }

    let start = std::time::Instant::now();
    let mut uf = weakly_connected_components(&graph);
    let largest = largest_component_size(&mut uf, n);
    let elapsed = start.elapsed();

    assert_eq!(largest, n, "a chain is one component");
    assert!(
        elapsed < std::time::Duration::from_secs(5),
        "elapsed={elapsed:?}, which is long enough to suspect the cost is not linear"
    );
}

#[test]
fn the_same_graph_gives_the_same_components_twice() {
    let mut graph: DiGraph<u32, ()> = DiGraph::new();
    let nodes: Vec<_> = (0..20).map(|i| graph.add_node(i)).collect();
    for pair in nodes.windows(2) {
        graph.add_edge(pair[0], pair[1], ());
    }

    let first = weakly_connected_components(&graph);
    let second = weakly_connected_components(&graph);
    for i in 0..nodes.len() {
        assert_eq!(first.find(i), second.find(i));
    }
}

#[test]
fn every_owner_gets_a_component_and_a_size() {
    let mut graph: DiGraph<OwnerId, FundingEdge> = DiGraph::new();
    let a = graph.add_node(owner(1));
    let b = graph.add_node(owner(2));
    let _alone = graph.add_node(owner(3));
    graph.add_edge(
        a,
        b,
        FundingEdge {
            src: owner(1),
            dst: owner(2),
            amount: 1,
            jetton_master: None,
            tx_hash: [0; 32],
            first_ts: 0,
        },
    );

    let mut uf = weakly_connected_components(&graph);
    let sizes = component_sizes(&mut uf, graph.node_count());

    // Owner to component and size, which is the shape scoring consumes as a feature.
    let membership: HashMap<OwnerId, (usize, usize)> = (0..graph.node_count())
        .map(|i| {
            let root = uf.find(i);
            (graph[NodeIndex::new(i)], (root, sizes[&root]))
        })
        .collect();

    assert_eq!(membership[&owner(1)].1, 2);
    assert_eq!(membership[&owner(2)].1, 2);
    assert_eq!(
        membership[&owner(3)].1,
        1,
        "an owner nothing funded is a component of one, not a missing row"
    );
}
