//! The Karman core: ports, the deterministic engine, and the conformance suite.
//!
//! This crate is the hub of the workspace and depends on nothing but `sting-karman-types`. It does no
//! I/O, opens no sockets and never reads a clock: everything it needs arrives through its ports,
//! and every run is a pure function of `(scenario, seed)`.
//!
//! The single idea everything follows from: **a mechanism is a state machine behind a trait, and
//! the engine is generic over it.** Lending, perps and curves are peer implementations of the
//! same [`ports::Mechanism`] port, and the conformance suite in [`conformance`] is the
//! substitutability contract each of them must pass.

pub mod conformance;
pub mod engine;
pub mod ports;
#[cfg(any(test, feature = "testing"))]
pub mod testing;

// The crate root is the surface: everything needed to implement a port, build a world or drive
// a sweep is reachable without naming a module. The modules stay public for what is organisation
// rather than vocabulary — the conformance suite, the profiler's phases.
pub use engine::{
    modes::{monte_carlo, monte_carlo_watched},
    rng::RunRng,
    world::{RunAborted, RunStats, World},
};
pub use ports::{
    Abort, Agent, InvariantViolation, MechError, Mechanism, NullObserver, Observer, PathSource,
    SourceError,
};
