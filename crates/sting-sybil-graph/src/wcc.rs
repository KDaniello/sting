//! Weakly connected components, which is the hard pass of clustering.
//!
//! It lives beside the funding graph because it operates on that graph directly: a component here
//! is a set of owners joined by funding, and direction is deliberately ignored — who paid whom is
//! already carried by the edge, and the question a component answers is who is connected at all.

use std::collections::HashMap;

use petgraph::graph::DiGraph;
use petgraph::unionfind::UnionFind;
use petgraph::visit::{EdgeRef, NodeIndexable};

/// The components of `graph`, as a union-find over node indices.
#[must_use]
pub fn weakly_connected_components<N, E>(graph: &DiGraph<N, E>) -> UnionFind<usize> {
    let mut uf = UnionFind::new(graph.node_bound());
    for edge in graph.edge_references() {
        uf.union(graph.to_index(edge.source()), graph.to_index(edge.target()));
    }
    uf
}

/// How many nodes each component holds, keyed by its root.
pub fn component_sizes(uf: &mut UnionFind<usize>, node_count: usize) -> HashMap<usize, usize> {
    let mut sizes: HashMap<usize, usize> = HashMap::new();
    for i in 0..node_count {
        *sizes.entry(uf.find(i)).or_insert(0) += 1;
    }
    sizes
}

/// The size of the largest component, which is what says whether an exclusion pass did anything.
pub fn largest_component_size(uf: &mut UnionFind<usize>, node_count: usize) -> usize {
    component_sizes(uf, node_count)
        .values()
        .copied()
        .max()
        .unwrap_or(0)
}
