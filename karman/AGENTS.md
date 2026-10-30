# Working in this repository

Guide for automated tooling and for anyone arriving without context. It is a condensed index
into `docs/`, not a replacement for it.

---

## Rule zero: authorship

**Every action on this repository is performed as the developer. The only author is a human.**

- **Never** add a `Co-Authored-By` trailer naming an assistant, model or tool to a commit message.
- **Never** add a "Generated with …", "Created by …" or similar footer to a commit, pull
  request, issue or release note.
- **Never** mention development tooling in the git history, tags, changelog or release notes,
  in any form.

Full statement: [`docs/ARCHITECTURE.md`](./docs/ARCHITECTURE.md) §8.

---

## What this project is

Karman simulates crypto-economic mechanisms — lending markets, perpetual-futures venues,
AMM curves — under populations of interacting agents, deterministically: one `(scenario, seed)`
pair defines one run, bit-for-bit. On top of that sit Monte Carlo sweeps and adversarial search
over the scenarios that break a design, and a four-level verification ladder that separates a
plausible simulator from a correct one.

The single idea everything follows from: **a mechanism is a deterministic state machine behind
one port, and the engine is generic over it.** Read
[`docs/ARCHITECTURE.md`](./docs/ARCHITECTURE.md) §4 before touching anything.

## Layers

| Layer | Crates | Rule |
|---|---|---|
| 4 · products | `sting-karman-cli`, `sdk/`, `karman-reference` | the only composition roots: concrete wiring lives here and nowhere else |
| 3 · verification | `sting-karman-verify` | drives mechanisms through public ports; owns no mechanism knowledge |
| 2 · mechanisms and agents | `karman-mech-*`, `sting-karman-agents` | implement the core's ports; never depend on each other's internals |
| 1 · core | `sting-karman-core`, `sting-karman-types`, `sting-karman-scenario` | no I/O, no network, no clocks; depends on nothing above it |
| — **I/O boundary** — | `sting-karman-data` | the only crate that touches the filesystem for fixtures or the network |

`sting-karman-core` never names a concrete mechanism. A new mechanism family is a new crate; a change
to the core to accommodate one is a design failure to escalate, not a patch to merge.

## Commands

```bash
make test    # unit, property, conformance and doc tests
make lint    # fmt, clippy -D warnings, deny, typos
make pr      # what CI runs; must be green before opening a pull request
```

Formatting is nightly rustfmt: `cargo +nightly fmt --all`.

## Rules that will get a change rejected

The full set is [`docs/ARCHITECTURE.md`](./docs/ARCHITECTURE.md) §8; these are the ones broken
most often.

1. **No nondeterminism.** No `Instant::now`, `SystemTime::now`, `thread_rng`, `rand::random`,
   or `HashMap`/`HashSet` iteration anywhere a simulation result can be affected. Time is the
   scheduler's tick; randomness is drawn from the seeded run stream (`RunRng`); ordered maps
   only. `clippy.toml` enforces the mechanical half.
2. **Money is integer.** Mechanism state and transitions use integer base units, checked
   arithmetic, explicit rounding direction. A float in a mechanism transition fails level 0 of
   the ladder and does not merge.
3. **No I/O outside `sting-karman-data`.** The core and the mechanisms read no files, open no
   sockets, print nothing; results leave through the `Observer` port. A run never fetches —
   adapters pull to fixtures first.
4. **Rejection is not corruption.** A rejected action returns `Err` and leaves state untouched;
   an invariant violation aborts the run loudly. Never a silent default, never a partial write.
5. **Every mechanism passes conformance.** Determinism and invariant preservation via
   `sting_karman_core::conformance`, plus a golden run pinned in `benches/baseline.toml`. A golden
   value changes only with the change that legitimately moved it, in the same commit.
6. **Newtypes for identifiers, units in the type where the type can carry one.** `AgentId`,
   `Tick`, `Amount<Collateral>`. A quantity that carries its unit needs no unit in its name;
   everything else states the unit there — `cliff_days`, never a bare integer with an implied
   meaning.
7. **English everywhere** — code, comments, doc comments, error text, commits, issues.
   `docs/ru/` is a mirror, not a source.

## Comments

**As many as it takes for the code to be understandable, and no more.** A comment on every line
is harmful: it doubles the reading volume, diverges from the code at the first edit, and lies
from then on. A lying comment is worse than a missing one, because it is believed.

**A comment explains a block, an algorithm or a contract — never a line. Explain *why*, not
*what*.** If the "what" is unclear, fix the names and the structure, not the prose.

Mandatory: `///` on public items in `sting-karman-types`, `sting-karman-core` and `sting-karman-scenario` · a line
or two on any non-trivial algorithm · invariants not expressible in the type system · `// SAFETY:`
on every `unsafe` · a deliberately non-obvious decision · **anything that exists to protect
determinism or rounding direction**, because otherwise it looks redundant and gets deleted in a
cleanup.

Forbidden: restating the code · commented-out code · a `TODO` with no owner and no issue link ·
decorative separator bars · doc comments on obvious accessors · copying text out of `docs/`
instead of linking to it · **project meta of any kind** — milestone names, roadmap references,
schedules, "for now", "temporary", "to be added later". A comment describes the code as it is;
describing the plan dates it instantly and turns it into a lie. State a real limitation and its
consequence, never when it is expected to end.

## Commits

Small, and the message is about the important part.

```
<type>(<scope>): <short description>
```

Types: `feat`, `fix`, `perf`, `refactor`, `docs`, `test`, `chore`.
Scope: `types`, `core`, `curve`, `lending`, `perp`, `agents`, `scenario`, `data`, `report`,
`verify`, `cli`, `sdk`, `ci`, `docs`.

Subject: imperative, lower case, no trailing period, ≤ 72 characters. Body optional, one to
three sentences, answering *why*. Never a list of changed files. Never "comprehensive",
"robust", "enhance", "leverage".

A change that moves a golden value states which and why:
`BASELINE: cpamm golden reserves move; rounding direction fixed to favour the pool`.

## Tests

- Every bug fix carries its regression test **in the same commit**, and the message names the
  seed or scenario that found it.
- Verify a new test fails for the **right reason** before making it pass. A determinism test
  that compared two empty states is worse than no test.
- A mechanism change reruns conformance locally before the push: `make test-conformance`.
- **No retry annotations. Ever.** A flaky test in a determinism project is a P1 finding, not a
  nuisance.

## Documentation

`docs/` is English and canonical. `docs/ru/` is an identical Russian mirror with the same
heading structure. **Both change in the same commit.**

Cross-references use relative links with a section number: ``[`ARCHITECTURE.md`](./docs/ARCHITECTURE.md) §4``.
Renumbering a section means fixing every reference to it in the same change.

## Where to look first

| Question | Document |
|---|---|
| what is this machine, end to end | [`docs/OVERVIEW.md`](./docs/OVERVIEW.md) |
| why is it built this way, and the rules | [`docs/ARCHITECTURE.md`](./docs/ARCHITECTURE.md) |
| what makes a mechanism trustworthy | [`docs/VERIFICATION.md`](./docs/VERIFICATION.md) |
| what a run must report about itself | [`docs/REPORTING.md`](./docs/REPORTING.md) |
| where the time and memory go | [`docs/PROFILER.md`](./docs/PROFILER.md) |
