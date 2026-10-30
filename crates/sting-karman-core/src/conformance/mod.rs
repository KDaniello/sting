//! The substitutability contract: what every mechanism implementation must pass.
//!
//! A mechanism crate does not decide for itself whether it is a valid implementation of the
//! port — this suite does. Each mechanism crate calls these harnesses from its own tests, and a
//! mechanism that cannot pass them does not merge, whatever else it can do.

// A test harness that lives outside `#[cfg(test)]` because other crates' tests call it. Its
// contract is to fail loudly: a conformance check that returned an error for its caller to ignore
// would be a check nobody runs. `karman/docs/ARCHITECTURE.md` §3.2 scopes the no-panic policy to library
// code, and this is test code that happens to be compiled into the library.
#![allow(
    clippy::panic,
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "the conformance harness is test code exported for other crates' tests"
)]

mod golden;
mod harness;

pub use golden::{GoldenError, assert_golden, expected};
pub use harness::assert_deterministic;
