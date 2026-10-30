# Overview — the machine end to end

Karman answers one question about a crypto-economic design: **what does this mechanism do under
the worst plausible pressure?** This document walks the machine once, in plain language. The
rules and boundaries it mentions are stated precisely in [`ARCHITECTURE.md`](./ARCHITECTURE.md);
what makes the answers trustworthy is [`VERIFICATION.md`](./VERIFICATION.md).

## 1. The parts

A simulation is four things:

- **A mechanism** — the rules of the venue: a lending market with liquidation thresholds, a
  perpetual-futures book with margin and funding, an AMM curve. In code it is a deterministic
  state machine: state in, action in, events out. No randomness, no clock, no floats.
- **Agents** — the population acting on the mechanism: liquidators, arbitrageurs, noise flow,
  whales with vesting schedules. Agents are where randomness lives, and every draw comes from a
  seeded stream.
- **The engine** — the loop that gives each agent its turn, applies actions to the mechanism,
  and advances logical time. It is generic over the mechanism: the same engine runs every
  family, and that is what "universal core" means concretely.
- **Data** — the real state a run starts from and the real paths it replays: reserves pulled
  from a protocol's API, an oracle price series, a recorded liquidation episode. Everything
  enters through one crate, gets written to disk as a fixture, and the run reads the fixture —
  a run never fetches.

## 2. One run

A run is a pure function. The inputs are a scenario file — which mechanism, which parameters,
which agents — and a 64-bit seed. The engine ticks: on each tick every agent looks at the state
and may act, the mechanism applies each action or rejects it, then time-driven rules fire —
interest accrues, funding settles, tokens unlock. After every transition the mechanism's own
invariants are checked; a violation aborts the run loudly, because it means the mechanism
implementation is wrong, and that must never look like a market outcome.

Same scenario, same seed: bit-identical result, on any machine. That is not a nicety — it is
what makes a finding reproducible, a bug reportable by its seed, and a sweep resumable.

## 3. Sweeps

One run is an anecdote. The engine's modes turn runs into evidence:

- **Monte Carlo** — thousands of independent runs from one scenario seed, each individually
  reproducible by its run number. The output is a distribution, not a story.
- **Replay** — recorded reality driven through the mechanism, for validation
  ([`VERIFICATION.md`](./VERIFICATION.md) level 1).
- **Adversarial search** — the mode the project exists for: a search over scenario space for
  the price path, the agent behaviour, the parameter corner that breaks the design. The output
  is the worst plausible case, not the average one.

## 4. The three families

The core is proven universal by implementing three mechanism families that price assets in
three unrelated ways:

| Family | Price comes from | The hard part |
|---|---|---|
| lending (credit) | outside — an oracle | liquidation cascades, bad debt |
| perpetual futures | inside — an order book | margin, funding, forced deleveraging |
| curves (AMM, bonding) | a formula over reserves | drainage, path dependence |

Anything the engine needs that is specific to one family is a design failure; the families are
peers behind one port. Most other crypto mechanisms are combinations of these three — a
stablecoin is credit plus a curve, a launchpad is a curve plus vesting — which is why these
three are first.

## 5. Trust

A simulator is easy to build and easy to disbelieve. The project's spine is the four-level
verification ladder — exact arithmetic against the reference implementation, replay of real
episodes, statistical realism, preregistered prediction — and a mechanism's current level is
part of its public API. The ladder, with pass criteria, is
[`VERIFICATION.md`](./VERIFICATION.md).

## 6. Surfaces

The engine is a library first. `sting-karman-cli` is the reference surface: it loads a scenario,
wires concrete mechanisms and agents to the core's ports — the only place wiring happens — and
prints measurable output. The SDK under `sdk/` exposes the same scenario contract to Python; a
scenario file means the same thing on every surface.
