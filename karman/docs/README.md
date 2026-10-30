# Karman developer documentation

This directory is the project's technical source of truth. It is written for the people building
Karman and for anyone who wants to understand how it works before trusting a number it produces.
These documents answer only the question "how is this built, and why this way".

## Reading order

| Document | What it answers |
|---|---|
| [`OVERVIEW.md`](./OVERVIEW.md) | **start here** — what the machine is, what sits at its centre, and how the pieces fit. Plain language, one sitting |
| [`ARCHITECTURE.md`](./ARCHITECTURE.md) | the crates and their boundaries, the ports, the architecture invariants, and the rules the code is written by |
| [`VERIFICATION.md`](./VERIFICATION.md) | the four-level ladder that separates a plausible simulator from a correct one, and what each level demands |
| [`ARITHMETIC.md`](./ARITHMETIC.md) | the numeric contract: widths, scales and rounding, and the reference each rule is traceable to |
| [`REPORTING.md`](./REPORTING.md) | what every run must say about itself: the mandatory report sections and their rules |
| [`PROFILER.md`](./PROFILER.md) | where time and memory go: what `karman profile` measures and how to read it |
| [`ROADMAP.md`](./ROADMAP.md) | the stages, their gates, and the honest status of each |

Repository root: [`../README.md`](../README.md) · [`../AGENTS.md`](../AGENTS.md).

## The dead-capital work

A study built on this engine rather than a part of it: a pricing machine for assets that are
owned and never sold. It is listed here because its specification is held to the same rules as
everything above, and because the engine gains a mechanism family from it.

| Document | What it answers |
|---|---|
| [`DEAD-CAPITAL.md`](./DEAD-CAPITAL.md) | why an asset with no price is an asset nobody can value, what sixty years of mechanism design says about it, and what this market measures |
| [`DEAD-CAPITAL-SPEC.md`](./DEAD-CAPITAL-SPEC.md) | the first layer as a state machine: state, transitions, rounding, invariants — what the mechanism crate is written from |
| [`VOUCH-SPEC.md`](./VOUCH-SPEC.md) | the second layer the same way — credit standing on somebody else's stake — and what a sweep of its collateral floor found out about a ring of a hundred |
| [`DEAD-CAPITAL-ROADMAP.md`](./DEAD-CAPITAL-ROADMAP.md) | the gates, each with the result that would close the project rather than advance it |

## Language

**English is canonical.** [`ru/`](./ru/) is an identical Russian mirror with the same heading
structure, because a translation that silently falls behind the original is worse than none —
the reader trusts it. When a document changes, both versions change in the same commit.

## Conventions

- Cross-references are relative links with a section number: ``[`ARCHITECTURE.md`](./ARCHITECTURE.md) §4``.
  Section numbers are stable; renumbering a section means fixing every reference to it in the
  same change.
- Every number a document states about the project is measured, and the measurement lives in
  [`../benches/baseline.toml`](../benches/baseline.toml) next to the method that produced it.
