//! The workspace's single I/O boundary.
//!
//! Every byte from the outside world — a protocol API, an exchange feed, a file a user hands
//! us — enters the workspace through this crate, and nowhere else. The rule is mechanical:
//! network dependencies are banned outside this crate at the `cargo deny` level, and the core
//! takes data only through its ports.
//!
//! The discipline that follows: **a run never fetches.** An adapter pulls from a source, writes
//! a fixture to disk, and the simulation reads the fixture. A fixture is a plain serialisable
//! value with a recorded origin, so any result can name the exact bytes it was computed from —
//! which is what makes a finding reproducible on another machine (karman/docs/VERIFICATION.md).
//!
//! A recorded exogenous path is the same discipline applied to a series: it is read once, and
//! the mechanism that consumes it through `PathSource` cannot tell it from a synthetic one.

mod census_sweep;
mod fixtures;
mod lending;
mod paths;
mod ton;

pub use census_sweep::{
    Indexers, SWEEP_SCHEMA_VERSION, SweepError, SweepPlan, run as sweep_census,
    run_against as sweep_census_against,
};
pub use fixtures::{FixtureError, read_json_fixture, write_json_fixture, write_text_artifact};
pub use lending::{Listing, Origin, PullError, PulledMarket, PulledPosition, Terms};
pub use paths::{InvalidPath, PathSample, PathSpan, RecordedPath};
pub use ton::{
    AccountId, Census, CensusBuilder, CensusError, CollectionCensus, HolderCount, ItemsPage,
    Listed, NotAnAccount, Prices, PurchasesPage, Sweep, TransfersPage,
};
