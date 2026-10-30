//! The deterministic engine: seeded randomness, the tick loop, and the sweep modes.

/// Ways to run many worlds at once.
pub mod modes;
pub mod probe;
/// The seeded randomness a run is allowed to draw from.
pub mod rng;
/// One world and the loop that advances it.
pub mod world;
