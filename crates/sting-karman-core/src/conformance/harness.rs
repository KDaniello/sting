use std::fmt::Debug;

use crate::{World, engine::rng::RunRng, ports::Mechanism};

/// Runs the same world twice from the same seed and demands the same run twice over.
///
/// This is the cheapest and most load-bearing conformance check: it catches wall-clock reads,
/// unseeded randomness and iteration-order dependence in one assertion. Invariant checks stay
/// on for both runs, so a reachable invariant violation also fails here.
///
/// The whole state is compared, and the run's activity counters alongside it. Letting a caller
/// choose what to compare would let it weaken the one check it came here for — a projection that
/// happens to agree says nothing about the state behind it.
///
/// # Panics
///
/// Panics when the two runs diverge or when either run aborts — divergence means the mechanism
/// or an agent broke the determinism contract, and an abort means the conformance scenario
/// reaches a state the mechanism declares impossible.
pub fn assert_deterministic<M, B>(n_ticks: u64, seed: u64, build: B)
where
    M: Mechanism,
    M::State: PartialEq + Debug,
    B: Fn() -> World<M>,
{
    // Both worlds are kept alive rather than summarised or cloned: a state large enough to be
    // worth checking is large enough not to want a copy of.
    let run = |stream_build: &B| {
        let mut rng = RunRng::new(seed, 0);
        let mut world = stream_build();
        world
            .run(n_ticks, &mut rng, &mut crate::ports::NullObserver)
            .expect("conformance run aborted before it could be compared");
        world
    };
    let first = run(&build);
    let second = run(&build);
    assert_eq!(
        first.state(),
        second.state(),
        "two runs of one seed reached different states"
    );
    assert_eq!(
        first.stats(),
        second.stats(),
        "two runs of one seed did different work"
    );
}

#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "karman/docs/ARCHITECTURE.md §3.2 scopes the no-unwrap/expect/panic policy to library code outside tests"
)]
#[cfg(test)]
mod tests {
    use std::panic::{AssertUnwindSafe, catch_unwind};

    use super::*;
    use crate::testing::{Adder, Counter, CounterState, DriftingAdder, RandomAdder};

    fn world(cap: i64, agent: Box<dyn crate::ports::Agent<Counter>>) -> World<Counter> {
        World::new(Counter { cap }, CounterState::default(), vec![agent])
    }

    #[test]
    fn a_seeded_population_passes() {
        assert_deterministic(50, 3, || {
            world(i64::MAX, Box::new(RandomAdder { bound: 32 }))
        });
    }

    /// The harness earns its place only by failing on a real defect, so the defect is injected
    /// here: an agent whose amounts come from a process-global counter rather than the run
    /// stream, which is what an accidental `thread_rng` or clock read looks like from outside.
    #[test]
    fn an_agent_drawing_from_outside_the_run_stream_fails() {
        let outcome = catch_unwind(AssertUnwindSafe(|| {
            assert_deterministic(50, 3, || world(i64::MAX, Box::new(DriftingAdder)));
        }));
        assert!(
            outcome.is_err(),
            "the harness accepted a run that drifts between repetitions"
        );
    }

    /// The other half of the suite, injected the same way: a mechanism whose own scenario walks
    /// it into a state it declares impossible. Determinism alone would call these two runs equal,
    /// because they abort identically.
    #[test]
    fn a_scenario_that_reaches_an_impossible_state_fails() {
        let outcome = catch_unwind(AssertUnwindSafe(|| {
            // One per tick against a cap of ten: the eleventh tick is unreachable by the
            // mechanism's own declaration, and the harness must refuse to call this conformant.
            assert_deterministic(50, 3, || world(10, Box::new(Adder(1))));
        }));
        assert!(
            outcome.is_err(),
            "the harness accepted a run that broke the mechanism's invariant"
        );
    }
}
