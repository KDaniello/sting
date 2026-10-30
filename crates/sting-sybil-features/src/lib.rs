//! What a wallet looks like, as numbers a model can be trained on.
//!
//! Five groups of signal, each in its own module: how a wallet is spaced in time, how much it
//! moved and to how many people, how similar two wallets' counterparties are, where it sits in the
//! funding graph, and which shape the graph draws around it. Assembling them into one row and
//! writing that row out is the last step, and the only one that knows the canonical column order.
//!
//! The strongest of them is the temporal fingerprint, and the trap in it is worth naming: what
//! separates automation from a person is that the spacing is *anomalous*, not that it is regular.
//! A bot that fires every sixty seconds and a bot that fires in bursts are both bots, and a
//! feature that only looked for one of them would find half of them.
//!
//! # What this crate does not do
//!
//! It decides no exclusion policy: the graphs it is handed must already have had the hot wallets
//! taken out, exactly as the graph crate requires of its own callers. It scores nothing — a
//! feature is evidence, and what a risk is comes later. It computes no cluster agreement, so the
//! one flag that depends on it stays unset rather than being guessed at.

mod aggregate;
pub mod fixtures;
mod funding_position;
mod jaccard;
pub mod persist;
pub mod pipeline;
pub mod schema;
mod temporal;
mod topology;

pub use aggregate::{AggregateFeatures, WalletEvent, extract_aggregate};
pub use funding_position::{FundingPosition, extract_funding_position_batch};
pub use jaccard::{jaccard, pairwise_jaccard_within_groups};
pub use pipeline::{FeatureExtractionConfig, extract_features_for_range};
pub use schema::{WalletFeatureRow, build_record_batch, wallet_features_schema};
pub use temporal::{
    BurstFeatures, TemporalFeatures, extract_active_hours_entropy, extract_burst, extract_temporal,
};
pub use topology::{
    TopologyThresholds, detect_star_convergence, detect_star_divergence, find_chain_paths,
    is_chain, is_tree_root,
};

/// What feature extraction answers with when a step of it fails.
#[derive(Debug, thiserror::Error)]
pub enum FeatureError {
    /// A batch could not be built or written in the columnar format.
    #[error("columnar layout refused: {0}")]
    Arrow(#[from] arrow::error::ArrowError),
    /// The partition could not be written.
    #[error("{0}")]
    Store(#[from] sting_sybil_store::StoreError),
    /// A range was asked for backwards.
    #[error("range {from}..={to} runs backwards")]
    BackwardsRange {
        /// The lower bound as given.
        from: u32,
        /// The upper bound as given.
        to: u32,
    },
    /// Reading the graph or the rows behind it failed.
    #[error("{0}")]
    Graph(#[from] sting_sybil_graph::GraphError),
}

/// What a caller gets back from every fallible step of feature extraction.
pub type Result<T> = std::result::Result<T, FeatureError>;
