//! One component per owner, and a way to break the giant one down.
//!
//! Union-find over the funding graph settles which owners are connected. What it cannot settle on
//! its own is the component that swallows everything: on a real chain one of them ends up holding
//! a large fraction of the graph through a handful of busy addresses, and a cluster that size is a
//! statement about the network rather than about anybody in it.
//!
//! So a component past a threshold is pruned down to its dense cores and the components are found
//! again inside the result. Pruning rather than a community-detection algorithm: it needs no
//! external library, no export to another runtime, and its parameter — how many neighbours a node
//! must keep — is a number an operator can reason about.

use std::collections::{HashMap, HashSet};

use petgraph::graph::{DiGraph, NodeIndex};
use petgraph::visit::EdgeRef;
use sting_sybil_graph::{component_sizes, k_core_prune, weakly_connected_components};
use sting_sybil_types::{FundingEdge, OwnerId};

/// Which component an owner is in, and how large it is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HardClusterInfo {
    /// The component's identifier.
    pub wcc_id: i64,
    /// How many owners are in it.
    pub wcc_size: u32,
}

/// Every owner's component, with anything past `giant_component_threshold` broken down.
///
/// A component over the threshold is pruned inside a graph induced by its own members, and the
/// survivors are re-clustered within that: pruning can separate what was one component only
/// through a low-degree bridge, and separating those is the whole point.
///
/// An owner the pruning removed keeps the original component's identifier. It is not lost — it is
/// simply in none of the dense cores that came out — and its size becomes what is left of the
/// original component once every core has been taken out of it, which is usually far smaller than
/// the giant it started as.
///
/// Identifiers of split subcomponents come from a range that starts past the node count, so a new
/// identifier can never collide with an original one. The whole thing is deterministic for a fixed
/// graph: nothing here is random or parallel, and the order of nodes and edges is fixed by how the
/// graph was built.
#[must_use]
pub fn materialize_hard_clusters(
    gas: &DiGraph<OwnerId, FundingEdge>,
    giant_component_threshold: usize,
    k_core_k: usize,
) -> HashMap<OwnerId, HardClusterInfo> {
    let mut uf = weakly_connected_components(gas);
    let sizes = component_sizes(&mut uf, gas.node_count());

    let mut final_id: HashMap<OwnerId, i64> = gas
        .node_indices()
        .map(|index| {
            (
                gas[index],
                i64::try_from(uf.find(index.index())).unwrap_or(i64::MAX),
            )
        })
        .collect();

    let mut next_id = i64::try_from(gas.node_count()).unwrap_or(i64::MAX);

    for (&root, &size) in &sizes {
        if size <= giant_component_threshold {
            continue;
        }
        let members: HashSet<OwnerId> = gas
            .node_indices()
            .filter(|&index| uf.find(index.index()) == root)
            .map(|index| gas[index])
            .collect();
        let sub = induced_subgraph(gas, &members);
        let pruned = k_core_prune(&sub, k_core_k);

        let sub_uf = weakly_connected_components(&pruned);
        for index in pruned.node_indices() {
            let within = i64::try_from(sub_uf.find(index.index())).unwrap_or(0);
            final_id.insert(pruned[index], next_id + within);
        }
        next_id += i64::try_from(pruned.node_count()).unwrap_or(0) + 1;
    }

    let mut size_of: HashMap<i64, u32> = HashMap::new();
    for &id in final_id.values() {
        *size_of.entry(id).or_insert(0) += 1;
    }

    final_id
        .into_iter()
        .map(|(owner, id)| {
            let wcc_size = size_of.get(&id).copied().unwrap_or(0);
            (
                owner,
                HardClusterInfo {
                    wcc_id: id,
                    wcc_size,
                },
            )
        })
        .collect()
}

/// The graph of `members` and the edges between them, and nothing else.
fn induced_subgraph(
    graph: &DiGraph<OwnerId, FundingEdge>,
    members: &HashSet<OwnerId>,
) -> DiGraph<OwnerId, FundingEdge> {
    let mut sub = DiGraph::new();
    let mut kept: HashMap<NodeIndex, NodeIndex> = HashMap::new();
    for index in graph.node_indices() {
        if members.contains(&graph[index]) {
            kept.insert(index, sub.add_node(graph[index]));
        }
    }
    for edge in graph.edge_references() {
        if let (Some(&src), Some(&dst)) = (kept.get(&edge.source()), kept.get(&edge.target())) {
            sub.add_edge(src, dst, *edge.weight());
        }
    }
    sub
}

#[cfg(test)]
mod tests {
    use super::*;
    use sting_sybil_types::TonAddress;

    fn addr(byte: u16) -> TonAddress {
        TonAddress::new(0, [u8::try_from(byte % 256).unwrap_or(0); 32])
    }

    fn owner(byte: u16) -> OwnerId {
        OwnerId(addr(byte))
    }

    fn edge(src: u16, dst: u16) -> FundingEdge {
        FundingEdge {
            src: owner(src),
            dst: owner(dst),
            amount: 1,
            jetton_master: None,
            tx_hash: [0; 32],
            first_ts: 0,
        }
    }

    #[test]
    fn a_small_component_is_left_as_it_is() {
        let mut g: DiGraph<OwnerId, FundingEdge> = DiGraph::new();
        let a = g.add_node(owner(1));
        let b = g.add_node(owner(2));
        let _alone = g.add_node(owner(3));
        g.add_edge(a, b, edge(1, 2));

        let info = materialize_hard_clusters(&g, 10_000, 2);

        assert_eq!(info[&owner(1)].wcc_id, info[&owner(2)].wcc_id);
        assert_eq!(info[&owner(1)].wcc_size, 2);
        assert_eq!(info[&owner(2)].wcc_size, 2);
        assert_ne!(info[&owner(1)].wcc_id, info[&owner(3)].wcc_id);
        assert_eq!(info[&owner(3)].wcc_size, 1);
    }

    /// A giant component of two cliques joined by one low-degree bridge breaks into the two
    /// cliques, with the bridge left over.
    #[test]
    fn a_giant_component_breaks_into_its_dense_cores() {
        let mut g: DiGraph<OwnerId, FundingEdge> = DiGraph::new();

        let clique_a: Vec<u16> = (0..6).collect();
        let clique_b: Vec<u16> = (10..16).collect();
        let bridge: u16 = 99;

        let mut nodes: HashMap<u16, NodeIndex> = HashMap::new();
        for &id in clique_a
            .iter()
            .chain(clique_b.iter())
            .chain([bridge].iter())
        {
            nodes.insert(id, g.add_node(owner(id)));
        }
        for clique in [&clique_a, &clique_b] {
            for i in 0..clique.len() {
                for j in (i + 1)..clique.len() {
                    g.add_edge(
                        nodes[&clique[i]],
                        nodes[&clique[j]],
                        edge(clique[i], clique[j]),
                    );
                }
            }
        }
        // The bridge has one edge to each clique, so at three neighbours it does not survive.
        g.add_edge(
            nodes[&bridge],
            nodes[&clique_a[0]],
            edge(bridge, clique_a[0]),
        );
        g.add_edge(
            nodes[&bridge],
            nodes[&clique_b[0]],
            edge(bridge, clique_b[0]),
        );

        // Thirteen nodes in one component, against a threshold of ten.
        let info = materialize_hard_clusters(&g, 10, 3);

        let id_a = info[&owner(clique_a[0])].wcc_id;
        let id_b = info[&owner(clique_b[0])].wcc_id;
        assert_ne!(id_a, id_b, "the two cliques are separate clusters");

        for &node in &clique_a {
            assert_eq!(info[&owner(node)].wcc_id, id_a);
            assert_eq!(info[&owner(node)].wcc_size, 6);
        }
        for &node in &clique_b {
            assert_eq!(info[&owner(node)].wcc_id, id_b);
            assert_eq!(info[&owner(node)].wcc_size, 6);
        }

        // The bridge did not survive the pruning, so it keeps the original component's
        // identifier: it is in neither core, and what is left of that component is itself.
        let bridge_id = info[&owner(bridge)].wcc_id;
        assert_ne!(bridge_id, id_a);
        assert_ne!(bridge_id, id_b);
        assert_eq!(info[&owner(bridge)].wcc_size, 1);
    }

    #[test]
    fn the_same_graph_clusters_the_same_way_twice() {
        let mut g: DiGraph<OwnerId, FundingEdge> = DiGraph::new();
        let nodes: Vec<_> = (0..20u16).map(|i| g.add_node(owner(i))).collect();
        for pair in nodes.windows(2) {
            g.add_edge(pair[0], pair[1], edge(0, 0));
        }

        let a = materialize_hard_clusters(&g, 5, 2);
        let b = materialize_hard_clusters(&g, 5, 2);
        assert_eq!(a, b);
    }
}
