# Contributing

The rules a change is judged against live in [`AGENTS.md`](./AGENTS.md); this page is how to get
from a clean clone to a green gate.

## Build

```bash
cargo build --workspace
```

A stable toolchain, pinned in `rust-toolchain.toml`. `make test` needs `cargo nextest`; `make lint`
needs `typos`, `cargo deny` and Python 3.11 or newer, since three of its rules are scripts.

Static analysis needs nothing else. Dynamic analysis needs `libemulator`, and the ABI extractors
need the real `tolk` and Tact compilers — all three are fetched into `vendor/` rather than
committed, and [`docs/DATA.md`](./docs/DATA.md) says which release each one comes from.

Chain access runs on the anonymous tier and needs nothing. `.env.example` carries the two toncenter
keys and what they would buy; no client sends them yet, and `.env` is ignored either way.

## The gate

```bash
make pr
```

That is the whole gate, and CI calls the same targets. What it runs and what each part refuses is a
table in [`docs/TESTING.md`](./docs/TESTING.md) §10.

`make docs-check` alone prints one line per documentation rule with its count and its threshold, so
its output is a work list rather than a verdict. `--list` under it prints the offenders behind every
rule rather than only behind the ones that grew.

## What gets a change rejected

The full set is [`AGENTS.md`](./AGENTS.md). The three broken most often:

- **A file that is not parsed never looks like a file without findings.** Silence means "read and
  clean", and it may not mean anything else.
- **A claim carries its number, and the number comes from a command.** "Faster" and "more precise"
  are not statements, and a figure typed into prose drifts — `sting explain` printed five detector
  precisions that no longer matched the baseline, so `make lint` compares them now.
- **A threshold is registered before the run it judges**, and a negative result is published in the
  same table as a positive one.

## Commits

`<type>(<scope>): <subject>`, imperative, lower case, no trailing period, at most 72 characters. The
body answers *why* and never lists changed files. A change that invalidates a stored artefact says
so: `BREAKING: invalidates dictionaries built before the schema change`.

Every bug fix carries its regression test in the same commit, and the message names the fixture or
corpus file that found it.

## Documentation

`docs/` is English and canonical, `docs/ru/` is an identical mirror, and both change in the same
commit. `make docs-check` compares the heading trees and fails when they diverge: a translation that
has silently fallen behind is worse than no translation, because a reader trusts it.
