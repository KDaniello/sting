use rayon::prelude::*;

use crate::{
    World,
    engine::{
        rng::RunRng,
        world::{RunAborted, RunStats},
    },
    ports::{Mechanism, NullObserver, Observer},
};

/// A Monte Carlo sweep: `n_runs` independent worlds, one scenario seed.
///
/// Run `i` draws from stream `i` of the scenario seed, so the sweep is reproducible as a whole
/// and any single run is reproducible alone — a failing run's number is its reproduction. The
/// result vector is indexed by run number; parallel execution is invisible in the output.
/// `summarize` receives the run's activity counters alongside its final state, so reports can
/// account for what happened, not only where it ended.
pub fn monte_carlo<M, T, B, S>(
    n_runs: u64,
    n_ticks: u64,
    seed: u64,
    build: B,
    summarize: S,
) -> Vec<Result<T, RunAborted>>
where
    M: Mechanism,
    T: Send,
    B: Fn() -> World<M> + Sync,
    S: Fn(&M::State, RunStats) -> T + Sync,
{
    monte_carlo_watched(
        n_runs,
        n_ticks,
        seed,
        build,
        || NullObserver,
        |state, stats, _| summarize(state, stats),
    )
}

/// The same sweep, with an observer per run and its answer folded into the summary.
///
/// A state at the end of a run is not everything a run did. What a mechanism reports while it is
/// happening — a liquidation, a write-off, a refusal — is gone by the time the last tick lands,
/// and a sweep that could only read states could never count it. So the observer is built per
/// run, watches it, and is handed to the summary alongside the state.
///
/// One observer per run and never one shared, for the reason the whole sweep is order-invariant:
/// a shared one would be written to in whatever order the runs finished.
pub fn monte_carlo_watched<M, T, B, O, W, S>(
    n_runs: u64,
    n_ticks: u64,
    seed: u64,
    build: B,
    watch: W,
    summarize: S,
) -> Vec<Result<T, RunAborted>>
where
    M: Mechanism,
    T: Send,
    O: Observer<M>,
    B: Fn() -> World<M> + Sync,
    W: Fn() -> O + Sync,
    S: Fn(&M::State, RunStats, O) -> T + Sync,
{
    (0..n_runs)
        .into_par_iter()
        .map(|run| {
            let mut rng = RunRng::new(seed, run);
            let mut world = build();
            let mut watcher = watch();
            world.run(n_ticks, &mut rng, &mut watcher)?;
            Ok(summarize(world.state(), world.stats(), watcher))
        })
        .collect()
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
    use crate::{
        engine::world::RunStats,
        testing::{Counter, CounterState, RandomAdder},
    };

    const SEED: u64 = 4242;
    const TICKS: u64 = 40;

    fn build() -> World<Counter> {
        World::new(
            Counter { cap: i64::MAX },
            CounterState::default(),
            vec![
                Box::new(RandomAdder { bound: 16 }),
                Box::new(RandomAdder { bound: 16 }),
            ],
        )
    }

    /// The whole state and the run's counters, not a projection of them: two runs that agree on
    /// one number can still have reached it differently, and that is the divergence a sweep is
    /// least likely to notice on its own.
    fn sweep(n_runs: u64, threads: usize) -> Vec<Option<(CounterState, RunStats)>> {
        let pool = rayon::ThreadPoolBuilder::new()
            .num_threads(threads)
            .build()
            .unwrap();
        let swept: Vec<_> = pool.install(|| {
            monte_carlo(n_runs, TICKS, SEED, build, |s: &CounterState, stats| {
                (s.clone(), stats)
            })
            .into_iter()
            .map(Result::ok)
            .collect()
        });
        assert!(
            swept.iter().all(Option::is_some),
            "a run aborted: this sweep proves nothing"
        );
        swept
    }

    #[test]
    fn one_thread_and_many_threads_produce_the_same_vector() {
        assert_eq!(
            sweep(64, 1),
            sweep(64, 8),
            "the sweep depends on how it was scheduled"
        );
    }

    /// The load-bearing half of order invariance: each swept run is compared against one
    /// computed outside `monte_carlo` altogether, so agreement cannot come from both sides
    /// sharing the same scheduling.
    #[test]
    fn a_run_index_is_its_rng_stream() {
        let swept = sweep(16, 4);
        for (index, result) in swept.iter().enumerate() {
            let mut alone = build();
            let mut rng = RunRng::new(SEED, index as u64);
            alone.run(TICKS, &mut rng, &mut NullObserver).unwrap();
            assert_eq!(
                *result,
                Some((alone.state().clone(), alone.stats())),
                "run {index} is not reproducible alone"
            );
        }
    }

    #[test]
    fn a_sweep_of_a_thousand_runs_reproduces_byte_for_byte() {
        assert_eq!(sweep(1_000, 8), sweep(1_000, 8));
    }

    #[test]
    fn an_aborted_run_stays_in_its_own_slot() {
        // A cap near the population's mean total: some streams cross it and some do not, so the
        // result vector has to carry both outcomes in stable positions.
        let capped = || {
            World::new(
                Counter { cap: 300 },
                CounterState::default(),
                vec![Box::new(RandomAdder { bound: 16 })],
            )
        };
        let run = || {
            monte_carlo(32, TICKS, SEED, capped, |s: &CounterState, _| s.total)
                .into_iter()
                .map(|r| r.map_err(|abort| abort.at))
                .collect::<Vec<_>>()
        };
        let first = run();
        assert!(
            first.iter().any(Result::is_err),
            "no run aborted: the cap proves nothing"
        );
        assert!(
            first.iter().any(Result::is_ok),
            "every run aborted: the cap proves nothing"
        );
        assert_eq!(first, run());
    }
}
