//! Who funded whom: resolving a jetton wallet to its owner, correlating a transfer into one
//! trace, and building the two graphs that carry the answer.
//!
//! Raw rows say that a message went from an address to an address, and half those addresses are
//! jetton wallets rather than the people behind them. Turning that into a funding relation takes
//! four steps that only mean something together: decode the body, verify the wallet really belongs
//! to the owner it claims, correlate the two-to-four messages of one transfer by their query id,
//! and only then add an edge.
//!
//! Two graphs come out of it, and they differ by which relation they hold rather than by type: the
//! value graph carries every transfer between owners, and the gas graph carries only the first
//! funding of an address — who activated it.
//!
//! # What this crate does not do
//!
//! It reads no message body of its own: opcodes and cell layout belong to the standard, so they
//! live in [`sting_tep`] and are re-exported here under the names callers already use. It reaches
//! no chain — [`JettonRpc`] is the boundary a caller implements. It clusters nothing beyond weakly
//! connected components: what is a cluster and what is a coincidence is a question for the crate
//! above.

mod exclusion;
mod funding;
mod jetton;
mod kcore;
mod persist;
mod pipeline;
mod resolve;
mod wcc;

pub use exclusion::{ExclusionCfg, apply_exclusion, is_excluded};
pub use funding::FundingGraphs;
pub use jetton::{JettonEvent, TransferTrace, correlate_traces};
pub use kcore::k_core_prune;
pub use persist::{FundingEdgeStore, GraphKind};
pub use pipeline::{
    events_from_block, ingest_block, ingest_block_into_gas_graph, ingest_block_into_value_graph,
    load_gas_graph_from_range,
};
pub use resolve::{JettonRpc, MasterAllowlist, resolve_and_verify_owner};
pub use sting_tep::{
    JettonOp, OP_EXCESSES, OP_INTERNAL_TRANSFER, OP_TRANSFER, OP_TRANSFER_NOTIFICATION,
    decode_jetton_body,
};
pub use wcc::{component_sizes, largest_component_size, weakly_connected_components};

/// What the graph answers with when a step of it fails.
#[derive(Debug, thiserror::Error)]
pub enum GraphError {
    /// The storage backend refused.
    #[error("storage refused: {0}")]
    Storage(#[from] clickhouse::error::Error),
    /// Reading raw rows back failed.
    #[error("reading raw rows failed: {0}")]
    Ingest(#[from] sting_sybil_ingestion::IngestError),
}

/// What a caller gets back from every fallible step of graph building.
pub type Result<T> = std::result::Result<T, GraphError>;

#[cfg(test)]
mod tests {
    use petgraph::graph::DiGraph;

    use crate::weakly_connected_components;

    #[test]
    fn two_nodes_joined_by_an_edge_are_one_component() {
        let mut graph: DiGraph<(), ()> = DiGraph::new();
        let a = graph.add_node(());
        let b = graph.add_node(());
        graph.add_edge(a, b, ());

        let uf = weakly_connected_components(&graph);
        assert_eq!(uf.find(a.index()), uf.find(b.index()));
    }
}
