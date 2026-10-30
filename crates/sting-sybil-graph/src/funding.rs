//! The two funding graphs: what moved between owners, and who activated whom.
//!
//! A node is an owner, with no wrapper type of its own — what kind of account it is already has a
//! name in [`sting_sybil_types::AccountKind`]. An edge is a [`FundingEdge`] in both graphs: the
//! two differ by which graph an edge lands in, not by carrying different types.

use std::collections::HashMap;

use petgraph::Direction;
use petgraph::graph::{DiGraph, NodeIndex};
use sting_sybil_types::{FundingEdge, JettonAmount, OwnerId};

/// Both graphs, built together from the same blocks.
#[derive(Default)]
pub struct FundingGraphs {
    /// Every transfer of value between owners.
    pub value: DiGraph<OwnerId, FundingEdge>,
    /// The first funding of each address, which is who put it on the chain.
    pub gas: DiGraph<OwnerId, FundingEdge>,
    value_index: HashMap<OwnerId, NodeIndex>,
    gas_index: HashMap<OwnerId, NodeIndex>,
    /// Transfers under this amount create neither an edge nor a node.
    ///
    /// It is the control against address poisoning: anyone can send a thousand addresses one
    /// unit of a jetton and, without a floor, that alone would draw a thousand edges out of one
    /// wallet and make a cluster out of strangers.
    dust_threshold: JettonAmount,
}

impl FundingGraphs {
    /// Empty graphs that ignore anything under `dust_threshold`.
    #[must_use]
    pub fn with_dust_threshold(dust_threshold: JettonAmount) -> Self {
        Self {
            dust_threshold,
            ..Self::default()
        }
    }

    // One index per graph. Sharing node indices between two independent graphs would mean keeping
    // them in step for no gain: nothing asks whether an owner has the same index in both.
    fn value_node(&mut self, owner: OwnerId) -> NodeIndex {
        if let Some(&index) = self.value_index.get(&owner) {
            return index;
        }
        let index = self.value.add_node(owner);
        self.value_index.insert(owner, index);
        index
    }

    fn gas_node(&mut self, owner: OwnerId) -> NodeIndex {
        if let Some(&index) = self.gas_index.get(&owner) {
            return index;
        }
        let index = self.gas.add_node(owner);
        self.gas_index.insert(owner, index);
        index
    }

    /// Adds a transfer of value — native or jetton — between two owners.
    pub fn add_value_edge(&mut self, edge: FundingEdge) {
        if edge.amount < self.dust_threshold {
            return;
        }
        let (src, dst) = (self.value_node(edge.src), self.value_node(edge.dst));
        self.value.add_edge(src, dst, edge);
    }

    /// Adds a first funding, which is an edge only for an address nothing has funded yet.
    ///
    /// First is global to the destination rather than per `(funder, destination)` pair: what the
    /// gas graph claims is who activated an address, and an address has one activation. Keyed by
    /// the pair, a second and different funder of the same address would also be recorded as its
    /// first, which is the opposite of the claim.
    ///
    /// The dust floor applies here too: a dust payment does not activate an address.
    pub fn add_first_funding(&mut self, edge: FundingEdge) {
        if edge.amount < self.dust_threshold {
            return;
        }
        let dst = self.gas_node(edge.dst);
        let already_funded = self
            .gas
            .edges_directed(dst, Direction::Incoming)
            .next()
            .is_some();
        if already_funded {
            return;
        }
        let src = self.gas_node(edge.src);
        self.gas.add_edge(src, dst, edge);
    }

    /// How many owners the value graph holds.
    #[must_use]
    pub fn value_node_count(&self) -> usize {
        self.value_index.len()
    }

    /// How many owners the gas graph holds.
    #[must_use]
    pub fn gas_node_count(&self) -> usize {
        self.gas_index.len()
    }
}
