# Profiler — where the time and memory go

Optimisation without a profile is guessing with confidence. This document fixes how Karman
measures itself: what `karman profile` reports, the design constraints the profiler lives
under, and how to read its output. One rule frames everything:
**measurements are never gates** — a number that varies by host cannot gate a merge
([`ARCHITECTURE.md`](./ARCHITECTURE.md) §9) — and its converse: **no optimisation lands
without a profile taken before and after.** A speedup nobody measured is a rumour.

## 1. What `karman profile` measures

One command, four sections, written to `out/profile-<scenario>-seed<seed>.md`:

```bash
cargo build --release
./target/release/karman profile --scenario testdata/scenarios/cpamm-smoke.json --runs 2000
```

1. **Phases.** One instrumented run; wall time per engine phase — `decide`, `apply`,
   `invariants`, `tick` — with share bars, entry counts and mean cost per entry. This is the
   "what do we optimise" table: the biggest share is the only defensible first target.
2. **Throughput.** The same sweep on one thread and on all logical CPUs: runs/s, ticks/s,
   speedup and parallel efficiency, plus an extrapolation to a million runs. Efficiency far
   below 100% on a healthy share-nothing sweep means the work units are too small for the
   pool's overhead or something is contended — both findings, not noise.
3. **Memory.** From the counting allocator: bytes allocated during the sweep, allocation
   count and allocations per run, live peak above the starting level, and live before/after —
   a "live after" that grows across repeated sweeps is a leak, not a pool.
4. **Allocation discipline, measured.** The §9 design rule — a run's memory is O(state), not
   O(ticks) — checked empirically: live peak at N ticks versus 10N ticks. A ratio near 1 means
   the rule holds; a growing ratio names the accumulation the observer did not ask for.

**This ratio is the one number here that gates.** Everything else on this page is a
measurement of a host and cannot decide a merge. Whether a run's memory grows with its length
is not a property of the machine that measured it — it is a property of the code, identical
everywhere — so `karman profile` exits non-zero when it fails, and `make profile` runs it over
every committed scenario.

## 2. Design constraints

- **The core never reads a clock.** Wall time is banned in simulation crates by `clippy.toml`,
  profiler included. The engine announces its phases through the `Probe` port
  (`sting-karman-core/engine/probe.rs`); the stopwatch implementation lives in the CLI
  (`sting-karman-cli/src/profile.rs`), the sanctioned edge, behind a scoped, justified allow. The
  default `NoProbe` monomorphises to nothing: an unprofiled run pays zero.
- **A probe listens, never influences.** A profiled and an unprofiled run of the same inputs
  produce the same state, stats and events; the conformance determinism suite is what holds
  that claim to account.
- **The counting allocator is always on** in the `karman` binary (a few relaxed atomics per
  allocation, delegating to the system allocator unchanged). Its cost is part of every number
  the profiler prints — stated here once so no reader mistakes instrumented throughput for a
  marketing figure.
- **Profiles are artifacts, not baselines.** They land in `out/`, dated by their content and
  host-dependent by nature. `benches/baseline.toml` holds only host-independent numbers
  (golden values); a profile figure enters a commit message or a finding as context, never as
  a gate.

## 3. Reading a profile

| Symptom | Likely meaning | First move |
|---|---|---|
| one phase dominates the share table | that phase is the optimisation target — nothing else is | profile that phase's mechanism/agent code before touching it |
| parallel efficiency far below 100% | work units too small for pool overhead, or contention — with share-nothing runs, suspect allocation | raise ticks-per-run in the profiling scenario to separate the two |
| high allocations per run | per-event `Vec` churn in the hot loop | count which events allocate; buffer reuse is a design change, propose it with numbers |
| live after grows across sweeps | a leak, not a pool | rerun with `--runs` doubled: linear growth confirms |
| discipline ratio well above 1 | history accumulating outside the observer | find what stores per-tick data and shouldn't |

## 4. The workflow

1. Profile in `--release`; debug numbers mislead in both directions.
2. Change one thing.
3. Profile again, same scenario, same seed, same host.
4. The commit that lands the optimisation quotes both profiles in its message; a claimed
   speedup with no before/after quoted does not merge.
