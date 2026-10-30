<div align="center">

# karman

**Agent-based wind tunnel for crypto-economic mechanisms.
Test the mechanism before the market does.**

[Overview](./docs/OVERVIEW.md) ·
[Architecture](./docs/ARCHITECTURE.md) ·
[Verification](./docs/VERIFICATION.md) ·
[Русская документация](./docs/ru/)

</div>

---

## What is Karman

Karman simulates a crypto-economic mechanism — a lending market, a perpetual-futures venue, an
AMM — under a population of interacting agents, and answers the question every parameter change
raises: *what does this design do under the worst plausible pressure?*

A mechanism is a deterministic state machine behind a trait; the engine is generic over it, so
lending, perps and curves are peer implementations of one port rather than three simulators. A
run is a pure function of a scenario file and a 64-bit seed: same inputs, same result, on any
machine. On top of single runs sit sweeps — Monte Carlo populations and adversarial search over
the scenarios that break the design.

Simulators earn distrust by being plausible instead of correct, so verification is the spine of
the project rather than a chapter: every mechanism climbs an explicit four-level ladder — exact
agreement with the reference implementation, replay of real recorded episodes, statistical
realism of the synthetic market, and preregistered public predictions. The ladder is
[docs/VERIFICATION.md](./docs/VERIFICATION.md); a mechanism's level is part of its API.

## Why

Protocols ship mechanisms holding billions with less simulation than a footbridge gets. The
teams that do simulate — the commercial risk firms — sell closed engagements and deliver PDFs;
the open frameworks leave calibration and validation entirely to the user. Karman is the missing
middle: an open engine a protocol team, a vault curator or a trader runs themselves, with the
verification story built in.

## Goals

1. **Reproducibility.** One `(scenario, seed)` pair defines one run, bit-for-bit, cross-platform.
2. **A universal core.** Three mechanism families — exogenous-price credit, order-driven perps,
   inventory curves — behind one port, with no family-specific cases in the engine.
3. **Verification as a contract.** Every mechanism passes the conformance suite; every level a
   mechanism claims on the ladder is backed by a committed, runnable check.
4. **Integer money.** Mechanism state transitions never touch a float; level 0 of the ladder is
   bit-exact agreement with on-chain arithmetic.
5. **Self-serve scale.** The same scenario runs on a laptop and fans out to millions of runs;
   the SDK is the product surface, not a demo.

## Status

**Pre-alpha. Not yet released.** The core ports, the deterministic engine, the conformance
suite and the first mechanism family (constant-product curves) are in place and green under
`make pr`. The lending family, the level-0 differential harness against reference Solidity, and
the Python SDK are the current front. Numbers this repository states about itself are measured,
and the measurements live in [`benches/baseline.toml`](./benches/baseline.toml).

## For users

```bash
# Run a scenario as a Monte Carlo sweep and print a JSON summary.
karman run --scenario testdata/scenarios/cpamm-smoke.json --runs 1000

# Same seed, same result — reproduce any run of the sweep exactly.
karman run --scenario testdata/scenarios/cpamm-smoke.json --runs 1 --seed 42

# Grade the sweep against a claim registered before it ran. Without one, no verdict.
karman run --scenario testdata/scenarios/cpamm-smoke.json --runs 1000 \
  --claim findings/2026-08-19-cpamm-smoke-symmetric-flow-leaves-no-drift.json

# Vary one parameter and put the designs side by side.
karman sweep --scenario testdata/scenarios/cpamm-smoke.json --vary group-count:0 --values 1,5,20

# Ask what this mechanism can claim on the verification ladder, and what it cannot.
karman verify --scenario testdata/scenarios/cpamm-mixed.json
```

## For developers

```bash
git clone https://github.com/karman-dev/karman && cd karman
make test    # unit, property, conformance and doc tests
make lint    # fmt, clippy -D warnings, deny, typos
make pr      # exactly what CI runs; must be green before a pull request
```

[OVERVIEW.md](./docs/OVERVIEW.md) describes the machine end to end;
[ARCHITECTURE.md](./docs/ARCHITECTURE.md) states the rules the code is governed by.

## License

Apache-2.0. See [LICENSE-APACHE](./LICENSE-APACHE).
