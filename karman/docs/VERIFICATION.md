# Verification — the ladder

A simulator is easy to build and easy to disbelieve, and it earns the disbelief by being
plausible instead of correct. This document is the project's answer: four levels, each with its
own question, its own method and its own pass criterion. A mechanism's current level is part of
its public API, and no claim outranks the level backing it.

The levels are ordered by strength. Each level presumes the ones below it: statistical realism
of a mechanism whose arithmetic is wrong is theatre.

## 1. Level 0 — the mechanics are exact

**Question.** Does our implementation of the mechanism compute what the reference computes?

**Method.** Differential testing. Where a reference implementation exists on-chain, the same
inputs go through the reference bytecode (executed on `revm` against a fixture state) and
through our Rust mechanism, and the outputs must agree **bit for bit** — the transition, the
rounding, the rejection behaviour. Property tests and the conformance suite cover what the
differential cannot reach: conservation invariants, rejection-leaves-state-unchanged,
determinism across platforms.

**Pass criterion.** Zero divergences over the preregistered input corpus. There is no
tolerance at this level; "almost" is a failure.

## 2. Level 1 — reality replays

**Question.** Fed the real recorded inputs of a real episode, does the simulator produce the
real outcomes?

**Method.** Replay (`sting-karman-verify::replay`). A recorded episode — the actions, the oracle
path, the venue's actual events — drives the mechanism from the episode's initial state. An
action the venue accepted must not be rejected; the events must match. The first divergence
ends the replay with its position, because everything after it happens in a counterfactual
world.

**Pass criterion.** Divergence within the tolerance preregistered for the episode — and the
tolerance is written down before the replay runs, never after.

## 3. Level 2 — the synthetic market is credible

**Question.** When agents act on their own — no replay — does the simulated market behave like
the real one?

**Method.** Two halves. *Stylized facts*: the distributions the real market exhibits (heavy
tails, clustering of liquidations, utilisation response to rate) measured on simulated output
and compared within preregistered tolerances. *Out-of-sample discipline*: agent parameters are
calibrated on one data window and judged on another the calibration never saw.

**Pass criterion.** The preregistered list of statistics, each within its preregistered band.
The list and the bands are fixed before the run; a statistic added after looking at results is
not evidence, whatever it shows.

## 4. Level 3 — predictions, registered before reality answers

**Question.** Does the engine say true things about the future?

**Method.** This level is a process, not a program, which is why `sting-karman-verify` has no module
for it. Two moves:

- **Retrodiction.** Take a known blow-up. Give the engine only data available *before* it.
  Show the adversarial search finds the weak point ahead of the event.
- **Preregistered prediction.** Publish the claim, the thresholds and the verdict zones before
  the outcome exists; let reality grade it. Predictions and their grades are public artifacts
  in `findings/`, one file per claim, immutable once registered.

**Pass criterion.** The public track record. Nothing else at this level counts as passing.

## 5. The conservatism contract

Every approximation in the engine errs in a stated direction, and the direction is chosen so
that the error understates safety, never overstates it: risk estimates round up, capacity
estimates round down, mechanism rounding favours the venue. When a number is uncertain, the
report says so and names the side the uncertainty falls on. A tool that flatters a design under
test is worse than no tool — the flattery is what ships.

## 6. Where each level lives

| Level | Executable home | Evidence artifact |
|---|---|---|
| 0 | mechanism crates' tests + `sting-karman-core::conformance` + differential harness | green suite; golden values in `benches/baseline.toml` |
| 1 | `sting-karman-verify::replay` | replay verdicts against recorded episodes in `testdata/` |
| 2 | `sting-karman-verify` statistical harnesses | preregistered tolerance files + measured results |
| 3 | none — a process | dated, immutable entries in `findings/` |
