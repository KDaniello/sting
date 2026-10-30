# Documentation

This directory is the technical source of truth for the platform. It is written for whoever builds
`sting` and for anyone who wants to understand how it works before trusting it with a contract. These
documents answer one question only: how is it built, and why exactly this way.

## 1. Read in this order

| Document | What it answers |
|---|---|
| [`CAPABILITIES.md`](./CAPABILITIES.md) | what the platform can do: one block per capability, eight fields each, every command run by the gate |
| [`ARCHITECTURE.md`](./ARCHITECTURE.md) | the ten invariants everything follows from, the five contours, the crate graph, the development rules |
| [`API.md`](./API.md) | every contract that crosses a boundary: the types, the three command lines, the HTTP protocol, the error codes |
| [`TESTING.md`](./TESTING.md) | the five sources of evidence, what each proves and what it cannot, and the gates |
| [`DATA.md`](./DATA.md) | the corpora, the vendored binaries, the external APIs and the rules for touching chain data |
| [`GLOSSARY.md`](./GLOSSARY.md) | the terms of the chain, of static analysis and of this project |
| [`POSITION.md`](./POSITION.md) | three businesses, who buys which, and what to strike at in which order |
| [`PLAN.md`](./PLAN.md) | the five gates and the product register, each with a failure condition |
| [`ROADMAP.md`](./ROADMAP.md) | the milestones, and the threshold each one is judged by |
| [`DECISIONS.md`](./DECISIONS.md) | what is settled, why, and what is still the owner's to decide |
| [`OPEN-QUESTIONS.md`](./OPEN-QUESTIONS.md) | questions that would change a decision, each with how it is checked |
| [`domains/`](./domains/README.md) | the deep dives: detectors, dataflow, the emulator, the schema, the extractors, the dictionary, reading a message, the service, the risk signals, the landscape |
| [`audit/`](./audit/README.md) | how an external audit of this repository is run |
| [`repo/`](./repo/ci.md) | what CI runs, and how a release is cut |

Two names in these documents point outside the tree. `sybil/` is the Python side of the clustering
contour, and `tools/ton-res` is the liteserver tool; neither is published here, and the Rust crates
`sting-sybil-*` do not need them to build. A link to either is a description, not a path to follow.

Working in the repository: [`../AGENTS.md`](../AGENTS.md) · [`../CONTRIBUTING.md`](../CONTRIBUTING.md).

## 2. What `sting` is, in three sentences

`sting` grew from one static analyser for **Tolk** into a platform of four contours, and 29 crates in
the workspace carry it.

| Contour | What it does |
|---|---|
| security | finds defects in Tolk source and confirms them in a real TVM |
| abi-layer | extracts a normalised contract schema from Tolk, Tact and FunC and serves it over HTTP |
| scamcheck | produces risk signals for jettons and gift collections |
| foundation | the Tolk compiler, the emulator, chain access — shared by the other three |

All four build with one command, run under one gate and share one engineering discipline.

None of them is published: no remote is configured, the version is `0.0.0`, and publishing is
disabled.

## 3. Building, and the gate

```
cargo build --workspace --all-targets
make pr
```

`make pr` is the whole gate: formatting, clippy with warnings denied, licences and advisories,
spelling, rustdoc, the mechanical documentation rules, the derived numbers, the tests and the
doctests. CI runs the same targets rather than its own copy of them.

Some tests need an external artefact and **skip rather than fail** when it is absent
([`TESTING.md`](./TESTING.md) §6):

```
$env:STING_LIBEMULATOR_PATH = "vendor\libemulator\libemulator.dll"   # PowerShell
export STING_LIBEMULATOR_PATH=vendor/libemulator/libemulator.so      # bash
```

Chain access runs on the anonymous tier and needs nothing; `.env.example` says what the two
toncenter keys would buy and why no client sends them yet.

## 4. One entry point per contour

Static analysis of Tolk source:

```
cargo run -p sting-cli -- lint corpus/clean/acton-contracts/counter/
cargo run -p sting-cli -- explain AC
cargo run -p sting-cli -- bench --update-baseline
```

`lint` prints findings the way a compiler does — the source line, a caret under the exact column
range, and a `= help:` line with a concrete remediation. `explain` prints a detector's calibrated
profile including its measured accuracy and its known limits, the inconvenient ones included. `bench`
recomputes precision and recall over the whole corpus and compares against the committed baseline.

Contract schema extraction:

```
cargo run -p sting-abi-cli -- extract corpus/clean/acton-contracts/counter/Counter.tolk
cargo run -p sting-abi-cli -- dictionary-probe <address>
cargo run -p sting-abi-cli -- coverage-report --db coverage.sqlite3
```

`extract` produces the same normalised schema whichever of the three languages the source was in.
`dictionary-probe` reads no source at all: it pulls real code and data by address and calls every
known method id through a real virtual machine, which is how a contract nobody has the source for is
recognised.

Risk signals for a jetton or a gift collection:

```
cargo run -p sting-scamcheck -- <jetton-address>
cargo run -p sting-scamcheck --bin gift_trust_check -- <collection-address> --domain <domain>
```

The domain check is not hypothetical: a hyphenated imitation of the official gift host is recognised
as a typosquat at edit distance one, on a real collection.

## 5. The rules this documentation follows

**A claim without a number does not enter.** Every "it works" carries a measurement and where it came
from: a baseline file, a command, a commit.

**A negative result is documented beside a positive one.** Two detectors sit near 33% precision; one
Tier 2 field is unmeasurable on the current ground truth; one language serves about one request a
second over HTTP. These are measured limits of the product, and they sit in the same tables as the
good numbers.

**Refusing to answer is a result rather than an error.** Unsupported bytecode, an inconclusive
verification, an unknown collection — all of them are types rather than strings in a log. A tool that
says "I do not know" where it does not know is worth more than one that guesses.

**Documentation and code have to agree, and that is checked.** A full review once found a document
telling a prospective partner that a branch had not been started, a day after its first working
version was finished. `make lint` now checks what a machine can check, and the same rules hold the
comments ([`TESTING.md`](./TESTING.md) §10).

**A gate is a checkable condition.** "The detectors work" is not a gate; an aggregate precision
threshold against a committed baseline, or a documented deviation from it, is.

**English is canonical and [`ru/`](./ru/) is an identical mirror.** Both change in the same commit,
and `make docs-check` compares the heading trees: a translation that has silently fallen behind is
worse than no translation, because a reader trusts it.

## 6. Related directories

| Directory | What is in it |
|---|---|
| `crates/` | the four contours — the map is in [`ARCHITECTURE.md`](./ARCHITECTURE.md) §3 |
| `corpus/` | the security corpus: clean projects, golden fixtures, the baseline |
| `corpus-abi/` | the extraction corpus: Tact and FunC sources, the method dictionary, its baseline |
| `vendor/` | external binaries — see [`DATA.md`](./DATA.md) §2 |
| `scripts/` | the mechanical checks and the derived numbers |
| `.github/workflows/` | what CI runs — see [`repo/ci.md`](./repo/ci.md) |

## 7. What this index does not cover

It does not summarise the documents it lists: a summary is a second copy that goes stale on its own
schedule. It does not describe work in progress — that is [`ROADMAP.md`](./ROADMAP.md), which is the
one document here allowed to name a milestone.
