//! The probe port: how a profiler observes the engine's phases without the engine owning a
//! clock.
//!
//! The core is forbidden wall time (karman/docs/ARCHITECTURE.md §8), yet a profiler must know where
//! ticks go. The resolution is the same as for every other boundary: the loop announces its
//! phases through a port, and the implementation that actually reads a clock lives at the
//! edge, where clocks are legal. [`NoProbe`] is the default and monomorphises to nothing — an
//! unprofiled run pays zero for the port's existence.

/// The phases of one tick, in the order the loop enters them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    /// Agents observing state and choosing actions.
    Decide,
    /// The mechanism applying an action.
    Apply,
    /// The invariant guard after a transition.
    Invariants,
    /// The mechanism's time-driven rules at tick end.
    Tick,
}

impl Phase {
    /// Every phase, in the order one tick enters them.
    pub const ALL: [Phase; 4] = [Phase::Decide, Phase::Apply, Phase::Invariants, Phase::Tick];

    /// Position in [`Phase::ALL`], so a probe can accumulate into a fixed array without a map
    /// and without a fallible lookup.
    #[must_use]
    pub fn index(self) -> usize {
        match self {
            Phase::Decide => 0,
            Phase::Apply => 1,
            Phase::Invariants => 2,
            Phase::Tick => 3,
        }
    }

    /// The phase's name as a profile reports it.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Phase::Decide => "decide",
            Phase::Apply => "apply",
            Phase::Invariants => "invariants",
            Phase::Tick => "tick",
        }
    }
}

/// A listener for phase boundaries. Implementations measure; they never influence — a probe
/// that changes behaviour breaks the reproducibility contract, because a profiled and an
/// unprofiled run must be the same run.
///
/// Phases do not nest: the loop enters one phase, exits it, and only then enters the next, so
/// an implementation may hold a single open phase rather than a stack.
pub trait Probe {
    /// The loop is about to do the work the phase names.
    fn enter(&mut self, phase: Phase);
    /// The loop has finished that work. Always paired with the [`Probe::enter`] before it.
    fn exit(&mut self, phase: Phase);
}

/// The default probe: does nothing, costs nothing.
#[derive(Debug, Default, Clone, Copy)]
pub struct NoProbe;

impl Probe for NoProbe {
    fn enter(&mut self, _phase: Phase) {}

    fn exit(&mut self, _phase: Phase) {}
}

#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "karman/docs/ARCHITECTURE.md §3.2 scopes the no-unwrap/expect/panic policy to library code outside tests"
)]
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn index_agrees_with_the_declared_order() {
        for (position, phase) in Phase::ALL.into_iter().enumerate() {
            assert_eq!(phase.index(), position);
        }
    }
}
