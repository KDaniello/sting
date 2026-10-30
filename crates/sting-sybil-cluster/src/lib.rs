//! Who belongs together, by the half of the answer that is structural.
//!
//! Clustering has two halves and they answer different questions. The hard half is topology: two
//! addresses are in the same component because money actually ran between them, and union-find
//! over the funding graph settles it. The soft half is behaviour: two addresses look alike across
//! their features, which is a density question and belongs to the machine-learning side of the
//! stack, where a parameter can be varied and an experiment repeated.
//!
//! This crate is the hard half and nothing else. What comes out of it is one row per owner:
//! which component, and how large that component is.
//!
//! # What this crate does not do
//!
//! It computes no soft cluster and no agreement between the two halves — the columns for those
//! stay empty here and are filled in by the side that owns them. It decides no exclusion policy:
//! a graph that still contains an exchange would produce one component the size of a country, and
//! taking those out happens before anything reaches this crate.

mod hard;
pub mod persist;
pub mod retro;
pub mod schema;

pub use hard::{HardClusterInfo, materialize_hard_clusters};
pub use retro::{RETRO_SCHEMA_VERSION, RegisteredThreshold, RetroResult, compare};
pub use schema::{ClusterMembershipRow, build_record_batch, cluster_membership_schema};

/// What clustering answers with when a step of it fails.
#[derive(Debug, thiserror::Error)]
pub enum ClusterError {
    /// A retrospective comparison could not be carried out.
    ///
    /// One variant for reading a registration, asking git when it was registered, and reading the
    /// two lists: from a caller's side they are one failure, that the measurement did not happen.
    #[error("{0}")]
    Retro(String),

    /// A batch could not be built in the columnar layout.
    #[error("columnar layout refused: {0}")]
    Arrow(#[from] arrow::error::ArrowError),
    /// The partition could not be written.
    #[error("{0}")]
    Store(#[from] sting_sybil_store::StoreError),
    /// Reading the graph or the rows behind it failed.
    #[error("{0}")]
    Graph(#[from] sting_sybil_graph::GraphError),
    /// A range was asked for backwards.
    #[error("range {from}..={to} runs backwards")]
    BackwardsRange {
        /// The lower bound as given.
        from: u32,
        /// The upper bound as given.
        to: u32,
    },
}

/// What a caller gets back from every fallible step of clustering.
pub type Result<T> = std::result::Result<T, ClusterError>;
