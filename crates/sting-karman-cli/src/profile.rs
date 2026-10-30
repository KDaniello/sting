//! The `karman profile` command: where the time goes, how the sweep scales, what memory it
//! takes — measured on this host, recorded as measurements, never as gates
//! (karman/docs/PROFILER.md).
//!
//! Wall time is banned in the core, so this file is where the clock lives: the engine
//! announces its phases through the `Probe` port and this module holds the stopwatch. The
//! probe only listens — a profiled run produces the same state as an unprofiled one, and the
//! determinism suite is what proves that claim.

use std::fmt::Write as _;

use std::path::Path;
use std::time::Duration;

use sting_karman_core::{
    World,
    engine::{modes::monte_carlo, probe::Phase, rng::RunRng},
    ports::{Mechanism, NullObserver},
};
use sting_karman_data::write_text_artifact;
use sting_karman_scenario::ScenarioConfig;

use crate::alloc::CountingAlloc;

/// Accumulated wall time per engine phase.
// Wall-clock reads are banned by clippy workspace-wide so they cannot reach the core; this
// module is the sanctioned edge — the readings measure phases and never influence a decision.
#[allow(clippy::disallowed_methods)]
mod stopwatch {
    use std::time::{Duration, Instant};

    use sting_karman_core::engine::probe::{Phase, Probe};

    #[derive(Debug, Default)]
    pub(crate) struct TimingProbe {
        started: Option<(Phase, Instant)>,
        pub(crate) totals: [Duration; Phase::ALL.len()],
        pub(crate) entries: [u64; Phase::ALL.len()],
    }

    impl Probe for TimingProbe {
        fn enter(&mut self, phase: Phase) {
            self.started = Some((phase, Instant::now()));
        }

        fn exit(&mut self, phase: Phase) {
            if let Some((entered, at)) = self.started.take()
                && entered == phase
            {
                self.totals[phase.index()] += at.elapsed();
                self.entries[phase.index()] += 1;
            }
        }
    }

    pub(crate) fn now() -> Instant {
        Instant::now()
    }
}

use stopwatch::TimingProbe;

pub(crate) fn profile<M: Mechanism + Sync>(
    scenario: &ScenarioConfig,
    build: impl Fn() -> World<M> + Sync,
    runs: u64,
    report_dir: &Path,
) -> Result<(), sting_karman_app::AppError> {
    let mut md = String::new();
    let _ = write!(md, "# Profile — {}\n\n", scenario.name);
    let _ = write!(
        md,
        "> Measurements from one host, never gates (karman/docs/PROFILER.md). seed `{}` · {} runs · \
         {} ticks · {} logical CPUs.\n\n",
        scenario.seed,
        runs,
        scenario.n_ticks,
        max_threads()
    );

    phase_profile(scenario, &build, &mut md)?;
    throughput_profile(scenario, &build, runs, &mut md)?;
    memory_profile(scenario, &build, runs, &mut md);
    let discipline = allocation_discipline(scenario, &build, &mut md)?;

    let path = report_dir.join(format!(
        "profile-{}-seed{}.md",
        scenario.name, scenario.seed
    ));
    write_text_artifact(&path, &md).map_err(sting_karman_app::AppError::Report)?;
    println!("{md}");
    println!("profile written to {}", path.display());

    // Timings are host measurements and gate nothing. This ratio is not a measurement of the
    // host: whether a run's memory grows with its length is a structural fact about the code,
    // identical on every machine, and it is the one number in this profile allowed to fail.
    if !discipline.holds {
        return Err(sting_karman_app::AppError::AllocationDiscipline {
            ratio: discipline.ratio,
        });
    }
    Ok(())
}

/// Whether a run's peak memory stayed put as the run got ten times longer.
#[derive(Debug, Clone, Copy)]
struct Discipline {
    holds: bool,
    ratio: f64,
}

/// Phase timings for one instrumented run: where a tick's time actually goes.
fn phase_profile<M: Mechanism>(
    scenario: &ScenarioConfig,
    build: &impl Fn() -> World<M>,
    md: &mut String,
) -> Result<(), sting_karman_app::AppError> {
    let mut probe = TimingProbe::default();
    let mut world = build();
    let mut rng = RunRng::new(scenario.seed, 0);
    // A profile of a run that stopped is a profile of nothing, so the run's own refusal is what
    // this returns rather than a message about it.
    world.run_with_probe(scenario.n_ticks, &mut rng, &mut NullObserver, &mut probe)?;

    let total: Duration = probe.totals.iter().sum();
    md.push_str("## Phases — one run, instrumented\n\n");
    md.push_str("| phase | total | share | entries | mean |\n|---|---|---|---|---|\n");
    for phase in Phase::ALL {
        let i = phase.index();
        let t = probe.totals[i];
        let share = if total.is_zero() {
            0.0
        } else {
            t.as_secs_f64() / total.as_secs_f64()
        };
        // A share is between zero and one, so thirty times it is a bar length rather than a
        // quantity. Counted in whole thirtieths rather than converted from a double, so the bar
        // is a rendering of the share and never a cast that could answer something else.
        let width = (0..30)
            .filter(|step| f64::from(*step) / 30.0 < share)
            .count();
        let bar = "#".repeat(width);
        let mean_ns = t.as_nanos() / u128::from(probe.entries[i].max(1));
        let _ = writeln!(
            md,
            "| {} | {:.2} ms | {:>5.1}% {} | {} | {} ns |",
            phase.name(),
            t.as_secs_f64() * 1e3,
            share * 100.0,
            bar,
            probe.entries[i],
            mean_ns
        );
    }
    let _ = write!(
        md,
        "\nInstrumented total: {:.2} ms (probe overhead included in every figure).\n\n",
        total.as_secs_f64() * 1e3
    );
    Ok(())
}

/// The sweep across thread counts: throughput, speedup, parallel efficiency, and where it stops.
///
/// A curve rather than two points, because two points cannot tell a workload that saturates the
/// memory system at four threads from one that would keep scaling if it were given more. The
/// difference decides whether a sweep should be given the whole machine or whether the rest of
/// the machine is better spent running a second sweep beside the first.
#[allow(clippy::disallowed_methods)]
#[allow(
    clippy::cast_precision_loss,
    reason = "a run count and a tick count at the sizes a profile runs are exact in a double, and every figure here is a rate"
)]
fn throughput_profile<M: Mechanism + Sync>(
    scenario: &ScenarioConfig,
    build: &(impl Fn() -> World<M> + Sync),
    runs: u64,
    md: &mut String,
) -> Result<(), sting_karman_app::AppError> {
    md.push_str("## Throughput — how far it scales\n\n");
    md.push_str("| threads | wall | runs/s | ticks/s | speedup | efficiency |\n");
    md.push_str("|---|---|---|---|---|---|\n");

    let mut single_thread_secs = None;
    let mut best = (1usize, 1.0f64);
    let mut fastest = 0.0f64;
    for threads in thread_ladder() {
        let pool = rayon::ThreadPoolBuilder::new()
            .num_threads(threads)
            .build()
            .map_err(|error| {
                sting_karman_app::AppError::Ungenerable(format!(
                    "a thread pool of {threads} could not be built: {error}"
                ))
            })?;
        let started = stopwatch::now();
        let results = pool.install(|| {
            monte_carlo(
                runs,
                scenario.n_ticks,
                scenario.seed,
                build,
                |_s: &M::State, stats| stats.actions_applied,
            )
        });
        let wall = started.elapsed().as_secs_f64();
        assert!(results.iter().all(Result::is_ok), "profiling sweep aborted");

        let speedup = single_thread_secs.map_or(1.0, |s: f64| s / wall);
        if single_thread_secs.is_none() {
            single_thread_secs = Some(wall);
        }
        let _ = writeln!(
            md,
            "| {} | {:.2} s | {:.0} | {:.0} | {:.2}x | {:.0}% |",
            threads,
            wall,
            runs as f64 / wall,
            (runs * scenario.n_ticks) as f64 / wall,
            speedup,
            speedup / threads as f64 * 100.0
        );

        if speedup > best.1 {
            best = (threads, speedup);
            fastest = runs as f64 / wall;
        }
    }

    let million = 1_000_000.0 / fastest.max(f64::MIN_POSITIVE);
    let _ = write!(
        md,
        "\nFastest at {} of the {} logical processors this machine reports, at {:.2}x. What the \
         curve does between them is the measurement worth having: a run that spends its time \
         waiting for memory stops scaling well before the cores run out, and the threads past \
         that point buy a second sweep rather than a faster one. A million runs at the best rate \
         measured here is about {:.0} s ({:.1} min).\n\n",
        best.0,
        max_threads(),
        best.1,
        million,
        million / 60.0
    );
    Ok(())
}

/// Thread counts worth timing: every power of two up to what the machine has, and the machine.
///
/// Powers of two because what matters about the curve is its shape, and a shape is read off a
/// handful of points spread across the range rather than off every count inside it.
fn thread_ladder() -> Vec<usize> {
    let most = max_threads();
    let mut ladder = Vec::new();
    let mut threads = 1;
    while threads < most {
        ladder.push(threads);
        threads *= 2;
    }
    ladder.push(most);
    ladder
}

/// Allocation pressure of the sweep, from the counting allocator.
fn memory_profile<M: Mechanism + Sync>(
    scenario: &ScenarioConfig,
    build: &(impl Fn() -> World<M> + Sync),
    runs: u64,
    md: &mut String,
) {
    CountingAlloc::reset_peak();
    let before = CountingAlloc::snapshot();
    let results = monte_carlo(
        runs,
        scenario.n_ticks,
        scenario.seed,
        build,
        |_s: &M::State, stats| stats.actions_applied,
    );
    let after = CountingAlloc::snapshot();
    assert!(results.iter().all(Result::is_ok), "profiling sweep aborted");

    let allocated = after.total_allocated_bytes - before.total_allocated_bytes;
    let count = after.allocation_count - before.allocation_count;
    let peak_growth = after.live_peak_bytes.saturating_sub(before.live_bytes);
    md.push_str("## Memory — counting allocator, whole process\n\n");
    let _ = write!(
        md,
        "- allocated during sweep: {} ({} allocations, {} per run)\n- live peak above start: \
         {}\n- live before/after: {} / {} — a growing \"after\" across repeated sweeps is a \
         leak, not a pool\n\n",
        human_bytes(allocated),
        count,
        count / runs.max(1),
        human_bytes(peak_growth),
        human_bytes(before.live_bytes),
        human_bytes(after.live_bytes),
    );
}

/// The allocation-discipline rule (karman/docs/ARCHITECTURE.md §9), measured: a run's memory must
/// scale with state and population, not with tick count.
#[allow(
    clippy::cast_precision_loss,
    reason = "two peak-memory readings compared as a ratio: what is asked is whether one is half again the other"
)]
fn allocation_discipline<M: Mechanism>(
    scenario: &ScenarioConfig,
    build: &impl Fn() -> World<M>,
    md: &mut String,
) -> Result<Discipline, sting_karman_app::AppError> {
    let peak_for = |ticks: u64| -> Result<u64, sting_karman_app::AppError> {
        CountingAlloc::reset_peak();
        let base = CountingAlloc::snapshot().live_bytes;
        let mut world = build();
        let mut rng = RunRng::new(scenario.seed, 0);
        world.run(ticks, &mut rng, &mut NullObserver)?;
        Ok(CountingAlloc::snapshot()
            .live_peak_bytes
            .saturating_sub(base))
    };

    let short = peak_for(scenario.n_ticks)?;
    let long = peak_for(scenario.n_ticks * 10)?;
    let ratio = long as f64 / short.max(1) as f64;
    let holds = ratio < 1.5;
    let verdict = if holds {
        "holds: peak is O(state), not O(ticks)"
    } else {
        "FAILS: peak grows with tick count — something accumulates outside the observer"
    };
    md.push_str("## Allocation discipline — measured\n\n");
    let _ = write!(
        md,
        "- live peak, {} ticks: {}\n- live peak, {} ticks: {}\n- ratio: {:.2} — {}\n\n",
        scenario.n_ticks,
        human_bytes(short),
        scenario.n_ticks * 10,
        human_bytes(long),
        ratio,
        verdict
    );
    Ok(Discipline { holds, ratio })
}

fn max_threads() -> usize {
    std::thread::available_parallelism().map_or(1, std::num::NonZeroUsize::get)
}

#[allow(
    clippy::cast_precision_loss,
    reason = "a byte count rendered to two decimals: the last unit of a gigabyte is not what a profile reports"
)]
fn human_bytes(bytes: u64) -> String {
    match bytes {
        b if b >= 1 << 30 => format!("{:.2} GiB", b as f64 / (1u64 << 30) as f64),
        b if b >= 1 << 20 => format!("{:.2} MiB", b as f64 / (1u64 << 20) as f64),
        b if b >= 1 << 10 => format!("{:.2} KiB", b as f64 / (1u64 << 10) as f64),
        b => format!("{b} B"),
    }
}
