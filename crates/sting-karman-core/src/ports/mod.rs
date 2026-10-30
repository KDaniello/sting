//! The ports of the core: the traits the rest of the workspace implements.
//!
//! A port earns its place by admitting implementations that differ in kind — a mechanism that
//! is a curve and one that is a book, an exogenous path that is a recording and one that is
//! synthetic — and by being reached from inside a run. A trait the edge calls once while
//! building a world is a function, not a port; a trait with a single conceivable implementation
//! is an internal type. The core stays free of family-specific cases only because these stay
//! narrow.

mod agent;
mod data;
mod executor;
mod mechanism;
mod observer;

pub use agent::Agent;
pub use data::{PathSource, SourceError};
pub use executor::{ExecutorError, TvmExecutor};
pub use mechanism::{Abort, InvariantViolation, MechError, Mechanism};
pub use observer::{NullObserver, Observer};
