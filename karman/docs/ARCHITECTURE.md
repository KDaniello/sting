# Architecture

A living document. It describes what the workspace is made of, the boundaries between the
parts, and the rules the code is written by. When a decision changes, it changes here — not in
someone's head.

## 1. Languages and their roles

| Language | Role | Why |
|---|---|---|
| **Rust** | Everything that simulates: the core, the mechanisms, the agents, the harnesses | Sweeps are millions of runs of an inner loop; determinism demands control over every source of ordering and arithmetic |
| **Python** | The SDK surface under `sdk/python` | The users this engine serves — quants, risk teams, token engineers — live in Python notebooks; the SDK is bindings over the same scenario contract, never a second implementation |

## 2. The layers

```
4 · surfaces        sting-karman-cli, sdk/python    argument grammars and bindings; no wiring of their own
3ᐟ· composition     sting-karman-app                where a scenario becomes a world; the only concrete wiring
3 · verification    sting-karman-verify             harnesses over public ports; no mechanism knowledge
2 · implementations karman-mech-*, sting-karman-agents, sting-karman-data
── ports boundary ─ sting-karman-core               traits + engine + conformance; no I/O, no clocks
1 · foundation      sting-karman-types, sting-karman-scenario
```

`sting-karman-data` implements the ports it supplies, so it sits with the other implementations. Being
the I/O boundary is a property of that crate — nothing else opens a file or a socket — and not a
level of its own.

Wiring lives in `sting-karman-app` and in no surface. Two surfaces that each resolved a scenario into a
mechanism would be two answers waiting to disagree, and the Python bindings promise the same
numbers as the terminal precisely because there is one implementation between them. What a
surface keeps is what only it can own: an argument grammar, an exit code, a counting allocator.

Dependencies point down and only down. `sting-karman-core` never names a concrete mechanism;
mechanisms never depend on each other; agents depend on mechanisms, never the reverse. Wiring —
which mechanism, which agents, which data adapter — happens once, at a composition root.

## 3. Crates

| Crate | Responsibility | May depend on |
|---|---|---|
| `sting-karman-types` | unit-typed integer money, widening checked arithmetic, logical time, identifiers | — |
| `sting-karman-core` | the ports, the deterministic engine, the conformance suite | types |
| `sting-karman-scenario` | the serialisable description of a run, fail-fast validation | types |
| `sting-karman-mech-curve` | inventory-curve markets: CPAMM, bonding curves | core, types |
| `sting-karman-mech-lending` | credit markets with Morpho Blue semantics | core, types |
| `sting-karman-mech-perp` | order-driven perp markets with Hyperliquid semantics | core, types |
| `sting-karman-mech-showcase` | assets held under a declared price, a standing offer and a charge | core, types |
| `sting-karman-mech-vouch` | credit standing on somebody else's stake, and what a default takes | core, types |
| `sting-karman-agents` | reusable behaviours, parameterised and serialisable | core, types, mech-* |
| `sting-karman-data` | fixtures on disk; recorded exogenous paths; every network adapter | core, types |
| `sting-karman-report` | run reports: identity, coverage, distributions, gaps | types |
| `sting-karman-verify` | the executable half of the verification ladder | core, types |
| `karman-reference` | the on-chain implementation this workspace models, executed, and the corpus it produces | verify, mech-lending, types |
| `sting-karman-app` | the composition: a scenario becomes a world, and the operations over it | everything below |
| `sting-karman-cli` | the `karman` binary: the terminal surface | app, core, curve, scenario |
| `sdk/python` | the PyO3 surface | app, scenario |

## 4. The ports

Four ports, all in `sting-karman-core/src/ports/`. A port earns its place by admitting
implementations that differ in kind **and** by being reached from inside a run; a trait the edge
calls once while building a world is a function, and a trait with one conceivable implementation
is an internal type. Adding one is a decision this document has to justify.

- **`Mechanism`** — the central port. A mechanism owns its `State`, `Action` and `Event` types;
  the engine is generic over all three, which is what keeps the core free of family-specific
  cases. `apply` is a deterministic transition; `Err` is a rejection and leaves state
  untouched, and the acting `AgentId` comes from the engine rather than from the action, so no
  agent can move another's position. `tick` fires the actor-less rules — interest, funding,
  vesting — and may abort, because there is nobody there to reject and skipping a transition
  would leave a state that looks valid and is wrong. `check_invariants` names what a reachable
  state must satisfy after every applied action, and `audit` names what also holds but costs a
  walk of the state to establish — the engine calls the first once per action and the second once
  per tick. The contract is the same either way and never weaker; what differs is how often a
  mechanism can afford to be asked. One holding a million items cannot answer a walk seventy
  thousand times a run, and a simulator nobody can afford to run is a simulator that checks nothing.
- **`Agent<M>`** — one decision per tick, taking the seeded run stream as the only randomness
  in the system and the agent's own `AgentId`, which is both its handle on the state it owns
  and its place in the acting order.
- **`PathSource<T>`** — how reality enters mid-run: the values a run replays but does not
  generate. A replayed recording and a synthetic stress path are indistinguishable to the
  mechanism, by design, and the swap happens at a composition root. Typed by the value it
  carries, so an oracle path and a funding path cannot be handed to each other's consumer.

What a path cannot express is an input that reads the state it is being asked about, and one
such input exists: a borrow rate rises with utilisation. A seam like that belongs to the family
that speaks it rather than here — `BorrowRate` lives in `sting-karman-mech-lending`, shaped after the
reference's own pluggable interface — because invariant 4 below leaves no room for the core to
learn the word.
- **`Observer<M>`** — the engine's only voice, for what happened and for what was refused. The
  reason an action was rejected exists nowhere else, so the port carries it out or it is lost.
  The core never prints and never writes.

The class-hierarchy question does not arise: what inheritance would do is done by generics and
associated types, and an incompatible `Action` fails at compile time, not at runtime.

## 5. Architecture invariants

The boundaries above are commitments. Violating one is not a style problem — it is the bug
class this structure exists to prevent.

1. A run is a pure function of `(scenario, seed)`; two runs of equal inputs are bit-identical
   on any platform.
2. `sting-karman-core` performs no I/O: no files, no sockets, no environment, no wall clock.
3. Every byte from the outside world enters through `sting-karman-data` and is written to a fixture
   before a simulation reads it. A run never fetches.
4. The core names no concrete mechanism; adding a mechanism family adds a crate and touches
   nothing in layer 1.
5. A rejected action leaves state unchanged. A run aborts for exactly two reasons — a state
   the mechanism declares impossible, or an input it needed and could not be given. There is no
   third path: no silent defaults, no partial writes, no skipped accrual.
6. Mechanism arithmetic is integer, checked, with an explicit rounding direction, and the
   rounding direction is the conservative one ([`VERIFICATION.md`](./VERIFICATION.md) §5). Where
   the mechanism models an existing venue that means favouring the venue, because agreeing with
   the reference is what level 0 demands and disagreeing with it is the error that matters. Where
   this workspace is designing a mechanism rather than modelling one there is no reference to
   agree with, and the conservative direction is instead the one that **understates the design's
   own effect**, so that a result is a lower bound rather than an artefact of rounding. Which of
   the two applies is a property of the family and is stated in its own documentation.
7. Every mechanism passes the conformance suite, and its golden run is pinned in
   `benches/baseline.toml`.
8. Agent randomness comes from the run's seeded stream and nowhere else.

## 6. The scenario contract

A scenario file plus a seed is the entire input of a run — the CLI, the SDK and a stored
finding speak this one schema, owned by `sting-karman-scenario`. The scenario *describes* mechanisms
and behaviours by name and parameters; a composition root resolves names to concrete types.
Validation is fail-fast at load: a scenario that would produce an empty or meaningless run is
an error before the first tick, because a number computed from a silently empty world still
looks like a number.

On the wire, money is a decimal string: JSON numbers cannot carry 128-bit integers, and a
JavaScript consumer would corrupt them past 2^53 without noticing.

## 7. Flow of one sweep

```
ScenarioConfig ──▶ validate ──▶ composition root builds World  (mechanism + agents)
     seed ──▶ RunRng stream per run ──▶ N independent runs (rayon, order-invariant)
each run:  agents decide ▶ mechanism applies ▶ invariants checked ▶ observer records
     results indexed by run number ──▶ aggregation ──▶ summary / findings
```

## 8. Rules for writing code

The full working guide, including the commit format, lives in [`AGENTS.md`](../AGENTS.md);
these are the load-bearing rules.

**Determinism.** No `Instant::now`, `SystemTime::now`, `thread_rng`, `rand::random`; no
`HashMap`/`HashSet` iteration where order can affect a result. `clippy.toml` bans the
mechanical half at compile time. Time is the scheduler's tick. What one tick means in seconds
is scenario configuration, never engine knowledge.

**Money.** Integer base units in a newtype, checked arithmetic, rounding direction stated at
the operation. A float may appear in analysis and reporting, never in a mechanism transition.
The widths, the scales and the direction each operation must round are
[`ARITHMETIC.md`](./ARITHMETIC.md), and every one of them is traceable to a reference.

**Errors.** Rejections are values (`MechError`); implementation bugs are aborts
(`InvariantViolation`); invalid input fails at the boundary with the field named. `unwrap` in
library code is a bug outside of tests and provably-unreachable arms.

**Interfaces.** Newtypes for identifiers; the unit in the type wherever the type can carry one
(`Amount<Collateral>`), and in the name wherever it cannot (`cliff_days`); named structs on any
boundary — no positional tuples longer than two.

**Comments.** As many as it takes for the code to be understandable, and no more. A comment
explains a block, an algorithm or a contract — never a line; *why*, never *what*; and never
project meta — no milestone names, no "for now", no "to be added later". A comment describes
the code as it is; describing the plan dates it instantly and turns it into a lie.

**Authorship.** Every commit, tag, issue and release note is authored by a human. No
`Co-Authored-By` naming an assistant, no "Generated with …" footers, no mention of development
tooling anywhere in the git history. The tool used to type the text has nothing to do with
authorship.

**Language.** English everywhere in code and history. `docs/ru/` is a mirror, not a source,
and both languages change in the same commit.

## 9. File map

Every file in the workspace, its single responsibility, and the quality gate it is held to.
Gates are qualitative — correctness, determinism, boundary integrity — never speed: a number
that varies by host cannot gate a merge. Performance is pursued by design and recorded as
measurements in `benches/baseline.toml`; the design rules that stand in for speed gates are:

- **Share-nothing runs.** Runs of a sweep share no mutable state; parallel and sequential
  execution of the same sweep are byte-identical. Checked by the order-invariance property,
  not by a benchmark.
- **No locks in the simulation path.** A lock, an atomic or a channel inside layers 1–2 is a
  rejected change; parallelism lives at the sweep level, where runs are independent.
- **Allocation discipline.** A run's memory is proportional to state and population, never to
  tick count: history accumulates only where the observer asks for it.

The SOLID mapping is structural rather than decorative: **S** is this map itself — one file,
one answer; **O** is §3's mechanism column — a new family adds a crate and changes nothing;
**L** is the conformance suite — substitutability is enforced, not assumed; **I** is the four
narrow ports of §4; **D** is the composition roots — the core depends on nothing concrete.

### 9.1 Repository root

| File | Responsibility | Gate |
|---|---|---|
| `Cargo.toml` | workspace membership, shared dependencies, lints, profiles | internal deps carry explicit versions; wildcards banned; lints deny nondeterminism and an undocumented public item |
| `rust-toolchain.toml` | pins the toolchain | a clean machine reproduces the build; the version pinned is a current one, because nothing here is a library anyone depends on and an old floor buys nothing while cutting the workspace off from the ecosystem it has to be diffed against |
| `rustfmt.toml` | the formatting contract (nightly) | `fmt --check` clean in CI |
| `clippy.toml` | the mechanical half of §8: banned methods and types | every entry names the rule it enforces |
| `deny.toml` | dependency law: licenses, sources, bans | no TLS or network stack resolvable outside `sting-karman-data` (§5, invariant 3) |
| `typos.toml` | spell-check scope | `docs/ru/` excluded; domain words listed |
| `.editorconfig`, `.gitattributes` | byte-level hygiene | LF, UTF-8, final newline, everywhere |
| `.gitignore` | what never enters history | no build output, no local run artifacts |
| `Makefile` | the developer verbs; `make pr` mirrors CI | Makefile and CI never diverge — same commands |
| `README.md` | the front door: what, why, status | every number it states is measured and lives in `baseline.toml` |
| `AGENTS.md` | the condensed working guide | indexes `docs/`, never contradicts or duplicates it |
| `CLAUDE.md` | the two unmissable rules | points to `AGENTS.md`; nothing else |
| `.github/workflows/ci.yml` | the gatekeeper | tests green on two platforms — the determinism contract is cross-platform |
| `benches/baseline.toml` | every measured number, next to its method | a value moves only with the change that moved it, same commit |
| `findings/README.md` | the claim registry contract and the shape of a registered claim | entries dated and immutable once registered; a claim states the basis of its thresholds, or it is a number someone liked |
| `docs/ARITHMETIC.md` | the numeric contract: widths, scales, rounding | every rule names the reference it is read from; a rule with no source is not a rule |
| `testdata/scenarios/*.json` | committed scenario fixtures | pass `validate()`; reproducible from `(file, seed)`; one per family, so a change to a schema is caught by a run rather than by a reader |
| `testdata/fixtures/*` | what an adapter pulled off a chain, committed so a run reads bytes rather than a network | every one carries the origin that would ask the same question again; a fixture that does not reconcile fails where it is pulled and never lands here |
| `sdk/python/README.md` | the SDK surface contract | same scenario, same numbers as the CLI |
| `python/reference/` | the same model on a second engine, and the test that compares them | written from the description rather than from this workspace's code; the equivalence test rejects a sample neither engine would have produced, so passing it means something |
| `sdk/python/pyproject.toml` | how the wheel is built | one build backend; the wheel is abi3, so one artifact serves every supported Python |

### 9.2 `sting-karman-types`

| File | Responsibility | Gate |
|---|---|---|
| `src/lib.rs` | the crate surface: logical time (`Tick`) and the `AgentId` identifier | no floats anywhere in the crate; every public item documented |
| `src/money.rs` | `Unit`, `Amount`, `Delta` — quantities that carry what they count | two units cannot be added, and the proof is a `compile_fail` doctest; a quantity cannot be negative; every operation checked |
| `src/arith.rs` | `Rounding`, `ArithError`, `Product`, and the widening `mul_div` behind `Amount` | agrees with the reference on the committed vector corpus, products wider than 128 bits included; no lossy call compiles without a direction |
| `src/wire.rs` | decimal-string encoding for 128-bit integers | the full range survives a JSON round trip; a negative number is not a quantity |

### 9.3 `sting-karman-core` — `ports/` (the **I** of SOLID: four narrow interfaces)

| File | Responsibility | Gate |
|---|---|---|
| `ports/mod.rs` | the port index | a port exists only with two unrelated implementations in sight; no god-trait |
| `ports/mechanism.rs` | `Mechanism`, `MechError`, `InvariantViolation`, `Abort` | rejection leaves state unchanged; the actor comes from the engine, never from the action; transitions deterministic and integer-only; a tick that cannot proceed aborts, loudly |
| `ports/agent.rs` | `Agent<M>` — one decision per tick, given the agent's own `AgentId` | the `RunRng` parameter is the only randomness an agent may touch |
| `ports/data.rs` | `PathSource`, `SourceError` | a replayed and a synthetic path are indistinguishable to the mechanism; partial data is an error, never a truncation |
| `ports/observer.rs` | `Observer`, `NullObserver` — the core's only voice, for events and for refusals | all accumulation lives behind it; a rejection reason leaves through it or is lost; the core itself never prints or stores history |

### 9.4 `sting-karman-core` — `engine/`

| File | Responsibility | Gate |
|---|---|---|
| `engine/mod.rs` | the module seam | — |
| `engine/probe.rs` | the `Probe` port: phase announcements for profilers, `NoProbe` default | probes listen, never influence — profiled and unprofiled runs are the same run; `NoProbe` monomorphises to nothing |
| `engine/rng.rs` | `RunRng`: spec-defined ChaCha streams | same bytes on every platform; `(seed, stream)` pairs independent; draws unbiased by rejection sampling |
| `engine/world.rs` | the tick loop, the invariant guard, and the run's activity counters (`RunStats`) | agent order fixed by construction, and that order is the identity handed to the mechanism; a run is a pure function of its inputs; aborts carry the tick and the cause; rejections are counted and announced, never silently dropped |
| `engine/modes.rs` | the Monte Carlo sweep | results indexed by run number; parallel and sequential execution byte-identical; any single run reproducible alone |

### 9.5 `sting-karman-core` — `conformance/` and the crate surface (the **L**: substitutability enforced)

| File | Responsibility | Gate |
|---|---|---|
| `conformance/mod.rs` | the suite every mechanism must pass | called from each mechanism crate's own tests; a mechanism without it does not merge |
| `conformance/harness.rs` | `assert_deterministic` | fails on a seeded, deliberately injected nondeterminism bug |
| `conformance/golden.rs` | golden values against `baseline.toml` | a mismatch is information: the value moves only with its cause, in the same commit |
| `src/lib.rs` | the core's public API surface: every port, the run types and the sweep, reachable without naming a module | `cargo doc` clean under `-D warnings`; `missing_docs` denies an undocumented public item at build time; the surface presents as many ports as §4 declares |
| `src/testing.rs` | the reference mechanisms behind the `testing` feature, for crates that must exercise port-generic code without a real family | trivial enough that a failure indicts the code under test, never the fixture; one mechanism per port feature — accounts for the actor, an exogenous path for the abort; never reachable from a simulation |

### 9.6 Mechanism crates (the **O**: a new family adds a crate)

| File | Responsibility | Gate |
|---|---|---|
| `sting-karman-mech-curve/src/lib.rs` | the CPAMM: state, actions, swap events, invariants, conformance tests | rounding favours the pool; product non-decreasing; conformance and golden green |
| `sting-karman-mech-lending/src/lib.rs` | the credit family: its units, and the two scales it speaks | no mechanism claim before level 0 shows zero divergence against the reference |
| `sting-karman-mech-lending/src/shares.rs` | assets against shares, and the direction each conversion resolves its remainder in | every conversion rounds against the party asking; the four cases are one rule stated once, and a flipped one fails a property rather than a review |
| `sting-karman-mech-lending/src/accrual.rs` | interest, and the share of it a market mints for its fee recipient | interest lands on both totals, so idle liquidity is untouched by time; the series is truncated downward, never to nearest |
| `sting-karman-mech-lending/tests/analytic.rs` | the boundary this mechanism has, against the boundary algebra says it has | a closed form needs no data, so the error is measurable to the last representable price: the price a market's loss becomes possible at is `LIF·λ` of the price its positions became liquidatable at, and nothing else — not the size of a position, not the market, not the population |
| `sting-karman-mech-lending/src/liquidation.rs` | what an unhealthy position costs to take over, and what is left over when its collateral runs out before its debt does | both ways of naming a trade round against the liquidator; a loss is realised exactly when the last of the collateral goes, and it lands on the supply side |
| `sting-karman-mech-lending/src/rate.rs` | `BorrowRate`, the family's own port | takes the market it prices, because a rate that could not read utilisation would remove the feedback loop the family exists to show |
| `sting-karman-mech-lending/src/mechanism.rs` | the transitions, the health check and the invariants | every transition resolves against copies and commits once, so a refusal cannot half-write; health is not an invariant, because an unhealthy position is exactly what liquidation is for |
| `sting-karman-mech-perp/src/lib.rs` | the order-driven family: Hyperliquid semantics | the venue's books balance to zero every tick; liquidation order matches the public specification exactly |
| `sting-karman-mech-showcase/src/lib.rs` | the showcase: a declared price, a standing offer to sell at it, and a charge on both | the charge is assessed when an item is touched and never on a tick, so a tick cannot abort and the floor of applicability is a property of the design rather than of the gas price; it rounds down, against the venue, because the venue here is the design under test and no reference exists to agree with; the venue holds a purse like anybody, so conservation is checked rather than asserted; a buyout takes the debt before the fee and never touches the seller's purse, so the one transition the family exists to produce cannot fail for want of money |
| `sting-karman-mech-vouch/src/lib.rs` | the guarantee layer: credit drawn against what other people locked up over you | the collateral a draw must have behind it falls with settled history and never below a stated floor, so the leverage the design extends is one posted number rather than an emergent one; a draw rounds that requirement down and a premium rounds down too, both extending more credit and collecting less than the terms say, because the design is what is under test and no reference exists to agree with; a withdrawal of a guarantee always waits out a lag, or a guarantor steps aside from a due date it can see coming; defaults are swept before withdrawals mature within one tick, for the same reason |

### 9.7 `sting-karman-agents`

| File | Responsibility | Gate |
|---|---|---|
| `src/lib.rs` | the library contract: behaviours with serialisable parameters, and the basis-point share they are all quoted in | a behaviour states its calibration story, or names itself a baseline; a share is held to what a share can be, wherever it is built from |
| `src/noise.rs` | `NoiseTrader`, the zero-intelligence baseline | claims no realism; every draw from the run stream |
| `src/informed.rs` | `Arbitrageur`, the informed half of every account of how a pool earns and loses | trades toward its reference and never away; the band is the round trip, measured per venue rather than fitted |
| `src/borrower.rs` | `Borrower`, a leveraged position and what it does after part of it is taken | decides from quantities it can see and never from a price: a borrower with the venue's own oracle is not a borrower |
| `src/liquidator.rs` | `Liquidator`, and the two parameters a market's losses turn on | judges nothing — a covered position refuses it, which is what a real bot discovers by being refused; a reaction time and a capacity, both measurable rather than fitted |
| `src/depositor.rs` | `Depositor`, the side that bears what nobody reached in time | the baseline of the family and says so: it claims the direction of deposit flow and nothing about its shape |
| `src/trend.rs` | `MomentumTrader`, the chartist of the agent-based canon | reads the pool and never the run stream; carries one remembered price rather than a history |
| `src/holder.rs` | `Holder`, somebody who owns things and decides what to say they are worth | what it declares comes from Weyl and Zhang rather than from a taste of ours, so the markup falls to nothing where the charge equals the chance of selling and turns negative past it — a curve that could only rise would have no best rate on it to find; a collector is the same rule with a stronger attachment and not a second type, because two types differing only in constants drift apart; it looks at a few items and never at the market, because a participant who could read all of it would be one with the venue's own view |

### 9.8 `sting-karman-scenario`

| File | Responsibility | Gate |
|---|---|---|
| `src/lib.rs` | the scenario schema and fail-fast validation | invalid input names the broken field; money is strings on the wire; JSON round-trip stable |
| `src/knob.rs` | the parameters a sweep is allowed to vary | a closed set rather than a path into the document; a turned knob produces a scenario that goes through the same validation as one written by hand |

### 9.9 `sting-karman-data` (the I/O boundary)

| File | Responsibility | Gate |
|---|---|---|
| `src/lib.rs` | the boundary statement | network dependencies resolve nowhere else; a run never fetches |
| `src/fixtures.rs` | typed JSON fixtures and text artifacts | writes are temp-then-rename — no half-written file survives a crash; errors carry the path |
| `src/paths.rs` | `RecordedPath`, the replayed half of `PathSource` | values hold forward between observations and are never extrapolated past the recording; a recording out of order is refused, not sorted |
| `src/lending.rs` | a live lending market read out of a node's answers, with its origin attached | a word too wide for the width money is carried in is refused rather than truncated; a pull whose named holders hold more than the market issued came from two blocks and is refused; how a market is listed and what its oracle answered are read here too, so nothing downstream can restate them; the holders a pull could not name are the exact remainder, and never invent collateral nobody measured |
| `src/bin/pull-market.rs` | the tool that turns those answers into a fixture | the reaching is the shell's and the reading is this crate's; a pull that does not reconcile fails here rather than in whatever opens it later |
| `src/ton.rs` | a market of assets that are held rather than traded, counted out of two indexers' answers | one sale through an escrow is two transfers and one platform handout is a third, so turnover is counted on distinct items and both numbers are reported; an item listed for sale counts to its seller and never to the escrow holding it; a page read twice contributes one item and says how often that happened; rows outside the declared window are dropped and counted rather than absorbed; serving a platform's description is not being that platform's collection, so what an issuer walked away from and a copy of it are told apart by the chain and never netted together; an asking price in another token is not added to prices in this one |
| `src/bin/pull-ton-census.rs` | the tool that folds one sweep into a census fixture | the same division as the pull above: the reaching is the shell's, the counting is this crate's; a sweep with no header measured nothing and is refused rather than closed |
| `census_sweep`, run as `sweep-ton-census` | the sweep itself: which collections a census is about, and every page it takes to answer for them | paced and retried against a public tier that refuses, because a page silently lost is a collection that looks smaller than it is; each indexer is asked only what it can answer, and which one answered travels with the census; the long listings are walked by cursor rather than by offset, because an offset into a listing that grows under the walk skips rows; a sweep of this market runs for the better part of an hour, so every stage lands through a temporary neighbour and a rerun continues where it stopped. The pacing, the retries and the key are `sting-chain`'s, which is where every part of the platform reaches a chain |

### 9.10 `sting-karman-report`

| File | Responsibility | Gate |
|---|---|---|
| `src/lib.rs` | `RunReport` and its Markdown rendering: identity, coverage, distributions, verdict, gaps (docs/REPORTING.md) | every mandatory section renders even when empty-handed; percentiles nearest-rank over integers — exact, platform-independent; no I/O |
| `src/sweep.rs` | the comparative report of a parameter sweep | every point carries its own coverage, so a point that aborted is never drawn as a point that behaved |
| `src/rank.rs` | how closely two columns agree about the order of things | rank rather than value, because a market that priced everything at a tenth of its worth ordered it perfectly and a coefficient punishing it for the tenth would measure the wrong thing; ranks are carried doubled so a tie is a whole number, the sums are bounded by widths rather than by hope, and the square root is an integer one — two machines cannot disagree about a coefficient, which is what lets one appear in a report |
| `src/claim.rs` | claims registered in `findings/`, and the grading of a sweep against one | a claim grades the sweep it names and no other; a zone that holds everything or nothing is refused at load; the registration is quoted into the report beside its outcome |

### 9.11 `sting-karman-verify`

| File | Responsibility | Gate |
|---|---|---|
| `src/lib.rs` | `Verdict` and the ladder's executable half | owns no mechanism knowledge; drives public ports only |
| `src/differential.rs` | level 0: a corpus of single transitions against the implementation being modelled | a mechanism more permissive than its reference diverges exactly as loudly as one that computes a different number; the first disagreement is the one reported, by the name of the case that probes it |
| `src/facts.rs` | level 2: the statistical signature of a synthetic return series | returns are simple rather than logarithmic, so a verdict does not depend on which machine rendered it; a statistic with nothing to measure is absent, never a stand-in number |
| `src/replay.rs` | level 1: recorded reality through the mechanism | a recorded step names its actor, so an episode settled per account replays as one; the first divergence ends the replay with its position; an action the venue accepted is never silently rejected |

### 9.12 `karman-reference` (level 0, where the ladder meets what it models)

| File | Responsibility | Gate |
|---|---|---|
| `src/lib.rs` | the corpus as committed, and the reference's provenance | the vendored code is the code its own hash names, checked before anything is asked of it; a corpus that agreed with any mechanism would prove nothing, so one the reference does not describe has to diverge |
| `src/evm.rs` | the reference's bytecode under an EVM, and the reads that turn its storage back into a state | vendored rather than compiled, because the code that runs is a stronger reference than a local build of the source that claims to produce it; the storage slot the owner is written into is asked for rather than assumed |
| `src/script.rs` | the program the corpus is generated from | chosen for the edges, not for realism: every case is a transition where two implementations of this arithmetic could first part company |
| `src/episode.rs` | level 1: one real episode of a real market, recorded and replayed | tick zero is where the market's own clock stood rather than where the window opened; the rates are the ones the episode was charged, and an episode replayed against different ones has to diverge |
| `src/main.rs` | prints the corpus the reference produces | a tool, not a run — it says what the reference did and the shell decides where that goes |

### 9.13 `sting-karman-app` (the **D**: wiring in one place)

| File | Responsibility | Gate |
|---|---|---|
| `src/lib.rs` | the operations every surface offers, and the one place a scenario becomes a world | concrete wiring exists here and in no surface; every sweep writes a report satisfying docs/REPORTING.md §2, gaps filled by this crate; a library returns its summary and never prints one |
| `src/measure.rs` | what a sweep of one scenario measures | one implementation, so a run and a point of a parameter sweep can never report different numbers about the same world |
| `src/verify.rs` | the ladder, including the levels a mechanism cannot reach | every level is listed; a level not reached says why and names the gate that reaches it |
| `src/lending.rs` | a pulled market as the state a run starts from | the only place the boundary crate and a mechanism are both in scope; every share the market issued is held by somebody, so a market read in part opens as a whole one; the pair, the threshold and the price come from the fixture, so a scenario states only what it chose and a market pulled without its listing is refused rather than filled in |
| `src/showcase.rs` | a measured market as the world a run starts from, and the worths nothing in the run may read | what a collection is and how thinly it is held come out of the census and are never fitted; three parameters are, and they cannot fit the four numbers the gate checks; a world is a pure function of its spec and its seed because no float touches its generation — the exponential of a fitted shape is two tables of integers and a shift, since two libraries' `exp` agree to a hair and not to a bit; the population a stock needs is stated and refused rather than quietly crowded |

### 9.14 `sting-karman-cli` and `sdk/python` (the surfaces)

| File | Responsibility | Gate |
|---|---|---|
| `sting-karman-cli/src/main.rs` | the argument grammar, the exit code, and nothing else | resolves no scenario of its own; every output names `(scenario, seed)`, so any printed number reproduces |
| `sdk/python/src/lib.rs` | the PyO3 bindings | serialisation at the boundary and no business logic across it; every operation calls the same function the terminal calls |
| `sdk/python/tests/` | that both surfaces answer alike | all four operations compared field for field, at one seed |
| `sting-karman-cli/src/profile.rs` | the `karman profile` command: the stopwatch behind the `Probe` port, throughput and memory sections (docs/PROFILER.md) | wall clock lives only here, behind a scoped justified allow; timings are artifacts in `out/` and never gates, while the allocation-discipline ratio is a property of the code rather than of the host and fails the command |
| `sting-karman-cli/src/alloc.rs` | the counting global allocator | delegates to `System` unchanged; relaxed counters only — a profiled binary allocates exactly like an unprofiled one |
