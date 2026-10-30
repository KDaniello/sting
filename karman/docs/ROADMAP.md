# Roadmap

Progress is measured by gates, not by commit count: a stage is closed when its gate — a
measurable, preregistered criterion — has been met on this repository, and the measurement is
in [`benches/baseline.toml`](../benches/baseline.toml) or `findings/`. Statuses: **closed** ·
**partially** · **open**.

Stage 0 builds the machine; stages 1–3 build the mechanism families through it, ordered by the
verification ladder ([`VERIFICATION.md`](./VERIFICATION.md)): lending first because its level 0
has an on-chain reference to diff against, perps second because their levels 1–2 have public L4
data, curves last because their remaining work is product-shaped, not core-shaped.

## Stage 0 — the machine

| # | Home | Contents | Gate | Status |
|---|---|---|---|---|
| 0.1 | `sting-karman-types` | unit-typed integer money, signed changes, `mul_div` with an explicit rounding direction, logical time, ids | the contract of [`ARITHMETIC.md`](./ARITHMETIC.md): units cannot mix at the type level; no lossy operation can be called without a direction; `mul_div` agrees with the reference on the preregistered vector corpus, including operands whose product exceeds 128 bits; serde round-trips the full width | **closed** |
| 0.2 | `sting-karman-core/ports` | `Mechanism` with an engine-supplied actor and a fallible tick, `Agent`, typed `PathSource`, `Observer` over events and refusals | two unrelated mechanism families compile against the ports with zero core changes | **closed** — a curve and a credit market implement the same four ports, and the second arrived without a line of the core moving |
| 0.3 | `sting-karman-core/engine` | seeded streams, tick loop, Monte Carlo mode | one seed → bit-identical result, 1000 runs; sweep order-invariant under rayon | **closed** |
| 0.4 | `sting-karman-core/conformance` | determinism + invariant harness, golden runs vs `baseline.toml` | suite runs from a mechanism crate's tests and fails on a seeded injected bug | **closed** |
| 0.5 | `sting-karman-core/lib.rs` | the public API surface and its rustdoc | `cargo doc` clean; every public item documented, enforced by `missing_docs` rather than by review; the surface presents every port `ARCHITECTURE.md` §4 declares | **closed** |
| 0.6 | `sting-karman-agents` | behaviour library; parameters as serialisable structs | two behaviours beyond noise, each with a stated calibration story | **closed** — informed flow against a reference, and technical flow following the pool's own move |
| 0.7 | `sting-karman-verify` | replay harness; differential and stylized-fact harnesses | each harness proven on a real mechanism, not a toy: see gates 1.3, 1.5, 2.5 | **closed** — the differential harness against deployed bytecode, replay against one real episode, and the stylized-fact one against a series the curve and its population produced |
| 0.8 | `sting-karman-cli` | the composition root: `run`, `sweep` across one parameter, `verify` against the ladder, `profile` | runs a scenario file to a JSON summary; any run reproducible from `(scenario, seed)` printed in the output | **closed** |
| 0.9 | `sdk/python` | PyO3 bindings over the scenario contract | a notebook drives the same scenario the CLI runs and gets the same numbers | **closed** — one composition under both surfaces, and the equivalence is a test rather than a claim |
| 0.10 | `sting-karman-scenario` | the scenario schema and fail-fast validation | invalid scenarios rejected at load with the field named; JSON round-trip green | **closed** (first gate) |
| 0.11 | `python/` | notebooks, the cadCAD reference model, the benchmark | equivalence, not speed: the cadCAD reference and the engine produce statistically indistinguishable distributions on the shared scenario; timings are recorded in `baseline.toml` as measurements, never as gates | **closed** |
| 0.12 | `sting-karman-report` | run reports: identity, initial conditions, coverage, distributions with drift and corridors, extreme-run pointers, gaps | every sweep emits a report satisfying REPORTING.md §2, Markdown and JSON, written crash-safe; a sweep without one is a failed sweep | **closed** |
| 0.13 | `karman profile` | phases via the `Probe` port, single- vs all-thread throughput, counting-allocator memory, measured allocation discipline | every optimisation commit quotes a before/after profile (PROFILER.md §4); the discipline check runs green on every mechanism's profiling scenario | **closed** |

## Stage 1 — lending (`sting-karman-mech-lending`)

The first family with an on-chain reference, so the first to climb level 0 of the ladder.

| # | Contents | Gate | Status |
|---|---|---|---|
| 1.1 | market state, interest accrual, share accounting — Morpho Blue semantics, integer arithmetic with explicit rounding | property tests: conservation, monotone accrual, rejection safety | **closed** — each of the three fails on its own injected defect, and so does every cell of the rounding table |
| 1.2 | LLTV liquidation: the incentive, and bad debt written off against the supply side. No close factor, because the reference has none — what bounds a seizure is the position itself | seeded cascade scenario produces bad debt if and only if the threshold arithmetic says so | **closed** — both outcomes occur in one seeded run, and the episode balances to an accounting identity |
| 1.3 | **level 0**: differential harness — same inputs through reference Solidity on `revm` and through this crate | zero divergences on the preregistered input corpus | **closed** — seventeen transitions through the deployed bytecode and through this crate, agreeing to the unit, and the same corpus refuses a mechanism the reference does not describe |
| 1.4 | `sting-karman-data` adapter: market state and positions pulled to fixtures | a fixture pulled today rebuilds a runnable initial state, offline | **closed** — one live Morpho Blue market at a named block, nineteen holders and the remainder, opening as a state the mechanism accepts and a run carries |
| 1.5 | **level 1**: replay of one real liquidation episode | divergence within the episode's preregistered tolerance | **closed** — no divergence at all: five actions of a real WETH/USDC market, the liquidation among them, reproducing every event the chain reported and leaving the market where the chain left it |
| 1.6 | first public case: adversarial search over a real market's parameters | a dated finding in `findings/`, reproducible from `(scenario, seed)` by a third party | **closed** — two claims registered before their sweeps and graded by them: a position becomes insolvent at the liquidation incentive times where it opened, which the sweep localises to between 8230 and 8260 basis points of the opening quote; and a liquidator one basis point short of the whole position realises none of the loss, in nine hundred runs of a market it took over every time |

## Stage 2 — perpetual futures (`sting-karman-mech-perp`)

The deepest family: matching, margin and liquidation interact. Public L4 data provides the
replay material; the engine's semantics follow the published reference reconstruction.

| # | Contents | Gate |
|---|---|---|
| 2.1 | CLOB: price-time priority matching, order lifecycle including rejection | matching invariants under property tests; book digest stable across platforms |
| 2.2 | margin and funding: cross-margin equity, funding transfers | conservation: the venue's books balance to zero every tick |
| 2.3 | liquidation and forced deleveraging per the public risk-engine specification | seeded cascade reproduces the specified ordering exactly |
| 2.4 | **level 1**: replay against a public L4 slice | divergence within preregistered tolerance on the chosen window |
| 2.5 | **level 2**: stylized facts of the synthetic market vs the real venue's | the preregistered statistic list, each within its preregistered band |

## Stage 3 — curves (`sting-karman-mech-curve`)

Started first because it was the cheapest proof of the ports; finishes last because what
remains is breadth, not core risk.

| # | Contents | Gate | Status |
|---|---|---|---|
| 3.1 | constant-product AMM, integer arithmetic rounding for the pool | conformance green; golden run pinned in `baseline.toml` | **closed** |
| 3.2 | swap fees and LP accounting | product monotone under fees; LP shares conserve value | open |
| 3.3 | bonding curves and vesting schedules | launchpad-style scenario runs end to end under conformance | open |
| 3.4 | **level 3 (retrodiction)**: a real historical launch, engine given only pre-launch data | the search finds the weakness before the recorded collapse does | open |
