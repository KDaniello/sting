//! Taking the hot wallets out before anything is clustered.
//!
//! This is the control against false positives, and it has to happen before components are found
//! rather than inside the search. One exchange funds hundreds of thousands of addresses; leave it
//! in and every one of them lands in a single component that says nothing about anybody.

use std::collections::{HashMap, HashSet};

use petgraph::Direction;
use petgraph::graph::{DiGraph, NodeIndex};
use petgraph::visit::EdgeRef;
use sting_sybil_types::{AccountKind, FundingEdge, OwnerId};

/// Who is taken out, and above which out-degree.
#[derive(Default)]
pub struct ExclusionCfg {
    /// Exchange addresses, named rather than guessed.
    pub cex_allowlist: HashSet<OwnerId>,
    /// Bridge addresses, likewise.
    pub bridge_allowlist: HashSet<OwnerId>,
    /// Above this many outgoing edges, an address is a hub whoever it belongs to.
    pub hub_degree_threshold: usize,
}

/// Whether this owner is one clustering must not see.
///
/// Anything that is not a plain wallet is out: a contract funds addresses because that is its
/// job, and a shared relation to it is not a shared owner.
#[must_use]
pub fn is_excluded(
    owner: OwnerId,
    kind: AccountKind,
    out_degree: usize,
    cfg: &ExclusionCfg,
) -> bool {
    cfg.cex_allowlist.contains(&owner)
        || cfg.bridge_allowlist.contains(&owner)
        || !matches!(kind, AccountKind::Wallet)
        || out_degree > cfg.hub_degree_threshold
}

/// A new graph with the excluded owners and their edges gone.
///
/// `account_kind` is a closure rather than a dependency because what an address is comes from a
/// catalogue of code hashes that this crate has no business holding — and would have to hold, in
/// full, for a single predicate.
#[must_use]
pub fn apply_exclusion(
    graph: &DiGraph<OwnerId, FundingEdge>,
    cfg: &ExclusionCfg,
    account_kind: impl Fn(OwnerId) -> AccountKind,
) -> DiGraph<OwnerId, FundingEdge> {
    let mut filtered = DiGraph::new();
    let mut kept: HashMap<NodeIndex, NodeIndex> = HashMap::new();

    for index in graph.node_indices() {
        let owner = graph[index];
        let out_degree = graph.edges_directed(index, Direction::Outgoing).count();
        if !is_excluded(owner, account_kind(owner), out_degree, cfg) {
            kept.insert(index, filtered.add_node(owner));
        }
    }
    for edge in graph.edge_references() {
        if let (Some(&src), Some(&dst)) = (kept.get(&edge.source()), kept.get(&edge.target())) {
            filtered.add_edge(src, dst, *edge.weight());
        }
    }
    filtered
}
