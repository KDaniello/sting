# sting

sting is not a linter for TON and not an ABI service. It is one core over TON's contract code —
parsing, dataflow, an emulator, a method dictionary — and three surfaces that read that core.

| Surface | What it answers | Entry point |
|---|---|---|
| security | which defect classes this Tolk source contains, and whether a deployment confirms one | `sting lint`, `sting scan`, `sting verify` |
| abi-layer | what this contract's storage, messages, get methods and error codes are | `abi-layer extract`, `sting-abi-service` |
| scamcheck | which rug and scam signals a jetton or an NFT collection carries | `sting-scamcheck`, `gift_trust_check` |

---

## 1. What it stands on

The platform parses Tolk source directly through `tree-sitter-tolk` and never invokes the Tolk
compiler, so static analysis needs no Node.js and no toolchain beyond Rust. Dynamic analysis runs real
bytecode through TON's own emulator, loaded as a shared library.

60 crates in the workspace, 1585 tests green, 14 detectors in sting, 1388 method ids in the dictionary,
297 Tolk files in the corpus. Each of those numbers is derived from the tree by
`scripts/doc-numbers.py` and re-checked by `make numbers-check`; a document that states a different one
fails the check ([`docs/TESTING.md`](docs/TESTING.md) §8).

## 2. Build

```sh
git clone <this-repository>
cd sting
cargo build --release
```

A stable Rust toolchain is enough; the pinned version is in `rust-toolchain.toml`. The binary lands at
`target/release/sting`.

Dynamic analysis additionally needs `libemulator`, TON's emulator shared library. Without it
`sting lint` and `sting bench` work unchanged, while `sting scan` and `sting verify` refuse to start
and say what is missing rather than crashing ([`docs/domains/emulator.md`](docs/domains/emulator.md) §3).

## 3. The security surface

```sh
# Static analysis of one file or a directory
./target/release/sting lint corpus/fixtures/bridge/UbmMintPattern.tolk

# What a detector looks for, and where it is known to be wrong
./target/release/sting explain UBM

# Precision and recall over the labelled corpus
./target/release/sting bench --report bench-report.md

# Dynamic analysis of a deployed contract
./target/release/sting scan 0:000011a50b2cb855dd55972ea27e8fbb76c3eaf135918723849b0e5bdb69eae6

# Static finding confirmed against a real deployment
./target/release/sting verify corpus/fixtures/bridge/IdSwappedPattern.tolk <address> \
    --hypotheses corpus/fixtures/bridge/hyp.example.toml
```

Eight commands exist: `lint`, `scan`, `verify`, `bench`, `fetch`, `explain`, `fuzz`, `config dump`.
`--help` on each carries real examples, so this table is a map rather than a substitute.

| Command | What it does |
|---|---|
| `lint <path>` | runs the detectors over `.tolk` source, recursively over a directory |
| `scan <address>` | probes a deployed contract's bytecode: structural validity and named get methods |
| `verify <path> <address>` | lints the source, then drives the supplied hypotheses against the live bytecode |
| `bench` | measures every detector against the labelled corpus |
| `fetch <address>` | downloads code and data by address, through toncenter with a tonapi fallback, cached on disk |
| `explain <id>` | prints one detector's contract, its calibrated failure profile and its severity |
| `fuzz <target>` | a wrapper over `cargo fuzz run`, needing nightly and `cargo-fuzz` |
| `config dump` | prints the effective `Sting.toml` |

Common flags: `--format {human,sarif,json}`, `--config <path>` (nearest `Sting.toml` upward by
default), `--min-severity`, `-v`/`-vv`. Add `--testnet` to the address commands for testnet.

### 3.1 Source and address are not interchangeable

This is the single confusion worth stating before it is discovered by trial:

- `sting lint` needs Tolk **source**. That is the only input a static detector can read.
- `sting fetch` and `sting scan` retrieve and probe **bytecode**, a code and data cell pair. Almost no
  deployed contract exposes its source through the chain.
- sting never compiles source into bytecode and never decompiles bytecode into source. A contract
  known only by address reaches the emulator and never reaches the detectors, because there is no tree
  to run them on.
- Both together — your own source and your own deployment of it — is what a proof looks like: a static
  finding confirmed against the real deployed bytes. `sting verify` is that loop, and the hypothesis
  file is supplied rather than guessed.

`fetch --help` and `scan --help` repeat this at the point someone is about to hit it.

## 4. The abi-layer surface

TON has no protocol-level ABI standard. abi-layer extracts a normalized `ContractSchema` — storage,
messages, get methods, error codes — from Tolk and Tact through their compilers, and from FunC through
a calibrated heuristic extractor ([`docs/domains/abi-ir.md`](docs/domains/abi-ir.md)).

```sh
# One file, offline
cargo run -p sting-abi-cli -- extract corpus/clean/acton-contracts/counter/Counter.tolk
cargo run -p sting-abi-cli -- extract path/to/MyContract.tact --lang tact

# Accuracy and throughput over the corpus
cargo run -p sting-abi-cli -- bench --report abi-bench-report.md

# The HTTP service
cargo run -p sting-abi-service --bin sting-abi-service
curl -X POST localhost:8080/v1/extract -H 'content-type: application/json' \
  -d '{"source": "...", "lang": "tolk"}'
curl localhost:8080/v1/schema/EQ...
```

Tolk and Tact extraction need those compilers vendored locally under `vendor/`; FunC needs nothing
beyond the tree. A missing compiler degrades that one language to a per-request error and never to a
startup crash ([`docs/domains/service.md`](docs/domains/service.md)).

## 5. The scamcheck surface

scamcheck scores jettons and NFT collections against rug and scam signals. Four run against a jetton
master — mint rights, holder concentration, LP concentration, honeypot behaviour — and three against a
Gift or an NFT: collection authenticity, domain reputation, wallet reputation. Two binaries live in
the one crate,
because each wraps exactly one library and a separate crate per binary would be a split that buys
nothing.

```sh
# One jetton master, all four signals, a composite report
cargo run -p sting-scamcheck -- <jetton-master-address> --ston-fi-pool <pool-address>

# One Gift or NFT collection, the trust signals
cargo run -p sting-scamcheck --bin gift_trust_check -- <collection-address>
```

The signals and their calibration are in
[`docs/domains/scamcheck.md`](docs/domains/scamcheck.md).

## 6. Which Tolk this understands

`sting --version` prints this table, and `crates/sting-ast/tests/tolk_version_matrix.rs` verifies it.
The test is symmetric: it fails when support breaks and equally when support silently improves, so the
table cannot drift away from the code.

| Tolk | Status |
|---|---|
| ≤ 1.4.x | full |

Parsing is `vendor/tree-sitter-tolk`, forked from the published 0.0.5 because that grammar knows Tolk
1.2 and the compiler is at 1.4.2 ([`vendor/tree-sitter-tolk/README.md`](vendor/tree-sitter-tolk/README.md)
lists every change and the file count each unblocked).

**297 of 297 corpus files parse with no `ERROR` node**, and the compiler reads nothing the grammar
refuses. That second half is what makes the first trustworthy: the grammar's own report can only say
what the grammar knows, so the vendored `tolk` is asked the same question and the two answers are
compared.

## 7. The numbers

| Claim | Value | Where it comes from |
|---|---|---|
| aggregate precision over the default set | 96.4% | `corpus/baseline.json` |
| aggregate precision over every measured detector | 82.0% | `corpus/baseline.json` |
| recall over injected defects | 100% | `corpus/baseline.json` |
| mainnet code-hash recognition | 86.2%, 337 of 391 unique codes | [`docs/domains/method-dictionary.md`](docs/domains/method-dictionary.md) §2 |
| overlap with the foundation's catalogue | 47.2% of theirs, 20.1% of ours | [`docs/domains/method-dictionary.md`](docs/domains/method-dictionary.md) §11 |

The two precision figures are two detector sets, not a contradiction. `AC` is experimental and off
unless enabled, and `LEP` reports at `Info`; both sat well below the rest and were moved out of the
default set. A document that names one figure without naming its set fails `make numbers-check`
([`docs/domains/detectors.md`](docs/domains/detectors.md) §1).

## 8. Documentation

English is canonical and [`docs/ru/`](docs/ru/README.md) mirrors it heading for heading.

- [`docs/README.md`](docs/README.md) — the map: which document answers which question.
- [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md) — layers, the ten invariants, the crate graph.
- [`docs/API.md`](docs/API.md) — the public surface of every crate and of the service.
- [`docs/DATA.md`](docs/DATA.md) — the corpora, the baselines, the caches and where they live.
- [`docs/TESTING.md`](docs/TESTING.md) — what is tested, how, and which gate refuses what.
- [`docs/domains/`](docs/domains/README.md) — one file per domain, each with its measurements.
- [`docs/PLAN.md`](docs/PLAN.md) and [`docs/ROADMAP.md`](docs/ROADMAP.md) — the five gates and the
  order of work.
- [`docs/DECISIONS.md`](docs/DECISIONS.md) — decisions taken, each with what it costs.
- [`CONTRIBUTING.md`](CONTRIBUTING.md) and [`AGENTS.md`](AGENTS.md) — what gets a change rejected.
