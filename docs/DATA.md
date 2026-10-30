# Data

The corpora, the vendored binaries, the external APIs and the rules for touching chain data. How all
of it is used to prove something: [`TESTING.md`](./TESTING.md).

One principle governs the rest: **every file in a corpus has a declared origin.** A directory without
a `SOURCE.md` or a `MANIFEST.json` is a defect, not a temporary state of affairs. For the twelve
hundred files that arrived through a mechanical registry walk, hand-written prose is replaced by a
machine-readable manifest — replaced, not omitted.

---

## 1. Two corpora, two jobs

### 1.1 `corpus/` — security

| Directory | What is in it | Its role |
|---|---|---|
| `clean/acton-contracts/` | sixteen real, independently deployed contracts: jetton, NFT, DNS, multisig, elector, config, wallets, vesting, telemint | the main source of false positives |
| `clean/tolk-bench/` | seven projects from the foundation's reference set | a second independent source |
| `clean/converted/` | FunC originals converted to Tolk | pairs for the extractor's Tier 2, and extra material for the detectors |
| `clean/production/` | deployed protocol code taken as found | real shapes nobody wrote for us |
| `clean/synthetic/` | hand-written projects for situations with no real example | used where reality had no sample |
| `fixtures/detectors/` | golden fixtures, positive and negative, three to five per detector | prove a detector fires on the pattern and stays silent without it |
| `fixtures/cross-file/` | multi-file projects that **really compile** under the vendored compiler | confirm cross-file resolution independently of the benchmark |
| `fixtures/emulator/`, `fixtures/bridge/`, `fixtures/fuzz/` | code and data cells, bridge scenarios, fuzzer input | the dynamic contour |
| `baseline.json` | a committed snapshot of precision, recall and false positives per detector | the regression gate |

297 Tolk files in the corpus: 174 under `clean/` and 123 under `fixtures/`, of which 104 are the
golden fixtures. Multi-file projects collapse to forty clean projects.

**Two of the Tolk projects do not compile at all.** They use the toolchain's own generated path
mapping, which neither the JavaScript nor the native compiler resolves alone. That is an accepted
limitation documented in the directory's own `SOURCE.md`, and the projects are **transparently
excluded** from the corpus rather than silently skipped.

**A fixture written for one detector touches the others, and that is accounted for.** The golden
fixtures for `AC` and `UTA` are taken from real low-level code whose entry-point signature by
construction never calls the end-of-parse assertion. `LEP`'s false positives rose by ten because of
it — not a regression in `LEP` but a known side effect of the corpus composition. The lesson was
applied immediately: the `UDA` fixtures deliberately added the missing assertion so as not to repeat
it, and the `LEP` and `AC` rows after `UDA` arrived are byte-identical — verified by diffing
`baseline.json` rather than by observing that the aggregate did not fall.

### 1.2 `corpus-abi/` — the extraction layer

| Directory | Files | Origin |
|---|---|---|
| `clean/tact/` | 19 | the Tact compiler's own test set, `SOURCE.md` |
| `clean/tact-registry/` | 129 | a walk of the public ABI registry, `MANIFEST.json` |
| `clean/func/` | 1197 | the same walk plus a hand-assembled growth set, `MANIFEST.json` and `SOURCE.md` |
| `clean/tolk/` | — | Tolk reuses `corpus/clean/` directly; no second fork is kept |
| `method-dictionary.json` | 1388 method ids in the dictionary | the product of `dictionary-build` over all three corpora |
| `baseline.json` | Tier 1, Tier 2 and throughput | the regression gate |

**A real mistake caught before it was committed, and worth recording.** A hundred and thirty-seven
Tact files from the registry walk first landed straight in `clean/tact/` — the directory the
benchmark uses as its Tier 1 accuracy baseline. Contaminating a calibration corpus with material
extracted by the same tool would have made the measurement meaningless. They were moved into a
separate registry root and wired in as a second independent source; a live Tier 1 run after the fix
confirmed full structural completeness and no regression.

---

## 2. Vendored external binaries

| Directory | What | What for | Limitation |
|---|---|---|---|
| `vendor/tolk/` | the native Tolk compiler | `abi.json` and `symbolTypes.json` for the detectors and the Tolk extractor | **a Windows build only** |
| `vendor/tact/` | the Tact compiler and its packages | the Tact extractor | needs Node.js; a cold start costs seconds |
| `vendor/libemulator/` | the C++ TVM emulator | all of the dynamics: the bridge, the honeypot, the dictionary probe, fuzzing | published on Windows without an import library |
| `vendor/fift/` | Fift | bytecode reassembly in the registry crate | |
| `vendor/tree-sitter-func/` | the FunC grammar | the Tier 2 extractor, parsing in memory | |
| `vendor/tree-sitter-tolk/` | the Tolk grammar | every parse of Tolk source | forked from 0.0.5, which knows Tolk 1.2 |

**The missing Linux build of the Tolk compiler is a blocking product gap rather than an
inconvenience**: the platform's main language is unreachable through the platform's main delivery
channel, the service container. It is recorded as a gap, and closing it is scheduled work rather
than a known annoyance.

`libemulator` is loaded **dynamically** rather than linked at build time, precisely because the
Windows release ships no import library. The constraint bought something: the path comes from
`STING_LIBEMULATOR_PATH`, and a test without it **skips rather than fails**.

---

## 3. External APIs

| Service | Who calls it | For what |
|---|---|---|
| toncenter v3 | `sting-corpus`, `sting-abi-coverage`, `sting-scamcheck` | code and data by address, transaction flow, jetton holders, NFT collections |
| tonapi | `sting-corpus` as a fallback, `sting-scamcheck` | the same class of data from a second provider |
| raw file hosting | the registry walk | several thousand source files in one pass, on a host that does not share the API rate limit |
| a DEX API | `sting-scamcheck` | liquidity-pool supply |

The project **does not run a TON node**. The decision was re-checked against a measurement: the
bottleneck in collection is deduplication by code hash, sixty to ninety times over in any window, and
a node does not change that.

---

## 4. Rules for touching an external API

**A sweep puts real sustained load on a public third-party service.** It is not a test endpoint and
not our server.

- The pilot size defaults to five thousand. Start with the pilot rather than the full stream.
- Deduplicate by code hash **before** fetching: an address whose code hash is already known is not
  downloaded at all. That is where the sixty-to-ninety-fold saving comes from, and it is the real
  limiter.
- `Retry-After` is honoured — a module of its own rather than a line in a client.
- The disk cache is mandatory and split by network: mainnet and testnet are separate subtrees, so
  switching networks for the same address **never** returns the other network's bytecode.
- `--force-refresh` bypasses the cache by an explicit decision of the caller, never by default.

**A defect of exactly this class, found and fixed.** Resolving library-reference cells had two real
faults: it went around a stale cache and it did not retry. The consequence was not cosmetic —
**38.6% of "unrecognised" contracts turned out not to be a dictionary gap** but unresolved library
cells. One fix moved the coverage picture further than a month of growing the dictionary.

---

## 5. Data that never enters the repository

- **A user's private code.** The tool reads files where it is run and copies them nowhere.
- **Results of runs against external targets.** Runs over ten GitHub projects, a live protocol and a
  CTF range produced real fixes, but the targets themselves are **not vendored**: that is other
  people's code, and the mode was explicitly internal — nothing was reported to or disclosed with any
  author. The honest consequence: those checks are **not reproducible from the repository**, and it
  is said wherever a number depends on them.
- **Keys and tokens.** Chain access needs a key; it lives in `.env`, which is ignored, and
  `.env.example` documents the two names without the values.

---

## 6. How to reproduce the measurements

```
cargo run -p sting-cli     -- bench --report bench-report.md          # security precision and recall
cargo run -p sting-abi-cli -- bench --report abi-bench-report.md      # Tier 1, Tier 2, throughput
cargo run -p sting-abi-cli -- dictionary-build                        # rebuild the method-id dictionary
cargo run -p sting-abi-cli -- coverage-report --db coverage.sqlite3   # mainnet coverage
make numbers                                                          # the facts the documents may state
```

The first two compare against the committed baselines and fail on a regression. `--update-baseline`
rewrites a snapshot **per tier**, and does not erase the tier that did not run in that invocation —
that was a real defect, fixed, with three tests behind it.

---

## 7. What this document does not cover

It does not say what any corpus proves — that is [`TESTING.md`](./TESTING.md), which also states what
each source cannot prove. It does not list the corpus file by file: the manifests do that, and they
are the artefact a check reads. And it holds no accuracy number: those live in the two baseline
files, which is where the gates read them from.
