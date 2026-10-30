# Capabilities

What this platform can do, one block per capability, eight fields each. What it answers:
[`README.md`](./README.md). How it is built: [`ARCHITECTURE.md`](./ARCHITECTURE.md). What proves
it: [`TESTING.md`](./TESTING.md). What is built next: [`ROADMAP.md`](./ROADMAP.md).

A capability is something a caller can ask for and get an answer to. That makes the command a
field rather than a footnote: a capability with no command is a claim, and the field nobody fills
in without being made to is **Does not** — precisely the field a reader fills in generously on its
author's behalf when it is absent.

Every field is checked by `scripts/check-capabilities.py`, which fails `make pr` on a block missing
one, on a mode it does not know, on a layer the architecture does not declare, and on a command
naming a binary no manifest builds. What the commands themselves do when run is
`scripts/run-capabilities.py`, which reads these same blocks.

## The modes

| Mode | Where it runs | Why |
|---|---|---|
| `offline` | `make pr`, from a clean clone | it needs nothing this repository does not carry |
| `offline, costly` | `make capabilities-full` | it needs minutes, and minutes in the gate is a gate people stop running |
| `offline, needs an artefact` | `make capabilities-full` | its input is what another capability produces |
| `offline, needs a toolchain` | by hand | it needs a toolchain this tree deliberately does not vendor |
| `live` | `make test-live` | it needs a network, a container or a key from `.env` |

Every **Cost** below was measured on one developer machine, in a debug build, and is an order of
magnitude rather than a promise: a release build is faster and somebody else's machine is
different. Where a number is a measurement of the platform rather than of the host — a precision,
a count of detectors — it is in **Evidence** instead, and the two are not mixed.

---

## 1. Security: defects in Tolk source, confirmed against real bytecode

### `security.lint`

| | |
|---|---|
| Input | a Tolk or `FunC` file, or a directory of them |
| Output | SARIF 2.1.0 (`$schema` pinned, `version` `2.1.0`), or human-readable text |
| Command | `sting lint corpus/clean/acton-contracts/counter/Counter.tolk` |
| Mode | offline |
| Evidence | 14 detectors, one of which reads both languages; 96.4% precision on the default set and 82.4% with every detector on, over 297 Tolk corpus files and 6 `FunC` fixtures |
| Cost | 41 ms for one file |
| Does not | prove a finding — a static finding is a hypothesis, and confirming it against deployed bytecode is `security.verify`; nor read a deployed contract, which has no source to read; nor run a detector against a language it has no rule for, which it says rather than leaves to silence |
| Layer | 5 · products |

### `security.explain`

| | |
|---|---|
| Input | a detector identifier |
| Output | text: what it looks for, its measured false-positive profile, why it carries its severity |
| Command | `sting explain UBM` |
| Mode | offline |
| Evidence | one entry per detector, each carrying its own measured precision |
| Cost | 28 ms |
| Does not | analyse anything — it explains a detector, and says nothing about any particular contract |
| Layer | 5 · products |

### `security.doctor`

| | |
|---|---|
| Input | nothing |
| Output | four versions and where each was read from, and a non-zero exit when any is unknown |
| Command | `sting doctor` |
| Mode | offline |
| Evidence | four versions named: emulator library, Tolk compiler, grammar, chain global version |
| Cost | 50 ms |
| Does not | install or repair anything: it reports what a run's answer depends on, and refuses to guess at a version it cannot read |
| Layer | 5 · products |

### `security.config`

| | |
|---|---|
| Input | `Sting.toml`, or nothing at all |
| Output | the effective configuration, including which detectors it activates |
| Command | `sting config dump` |
| Mode | offline |
| Evidence | every detector the configuration activates is listed by identifier |
| Cost | 32 ms |
| Does not | validate a project — it prints what sting would use, not what a project should |
| Layer | 5 · products |

### `security.bench`

| | |
|---|---|
| Input | the labelled corpus |
| Output | precision, recall and false-positive rate per detector: Markdown to read, and a baseline JSON (`schema_version` `1.0`) to compare |
| Command | `sting bench --report /tmp/bench.md` |
| Mode | offline, costly |
| Evidence | 297 corpus files; the figures it produces are the ones `security.lint` states |
| Cost | 77 s |
| Does not | measure anything about contracts outside the corpus, and a corpus is a sample rather than a population |
| Layer | 5 · products |

### `security.scan`

| | |
|---|---|
| Input | a deployed contract's address |
| Output | whether its state loads as structurally valid, and what named get-methods answer |
| Command | `sting scan EQD8TJ8xEWB1SpnRE4d89YO3jl0W0EiBnNS4IBaHaUmdfizE` |
| Mode | live |
| Evidence | the same emulator path the detector confirmation uses, against the pinned library commit |
| Does not | run the static detectors — there is no source at an address, and deriving a finding from bytecode alone is an open problem rather than a shortcut this skipped |
| Cost | one chain request plus a few milliseconds of emulation |
| Layer | 5 · products |

### `security.fetch`

| | |
|---|---|
| Input | a deployed contract's address |
| Output | its compiled code and data, as bags of cells |
| Command | `sting fetch EQD8TJ8xEWB1SpnRE4d89YO3jl0W0EiBnNS4IBaHaUmdfizE` |
| Mode | live |
| Evidence | the one chain client the whole tree reaches a service through, at 10 requests a second |
| Cost | one chain request |
| Does not | recover source: what it fetches is input for an emulator and a fuzzer, never for the static detectors |
| Layer | 5 · products |

### `security.verify`

| | |
|---|---|
| Input | a Tolk path and the address of that contract's deployment |
| Output | the static findings, each scored against the committed benchmark baseline after a dynamic attempt |
| Command | `sting verify --hypotheses corpus/fixtures/hypotheses/ubm.toml <path> <address>` |
| Mode | live |
| Evidence | the exploit strategies the end-to-end bridge tests already prove, driven against real bytecode |
| Cost | one chain request plus one emulation per hypothesis |
| Does not | confirm a class it has no strategy for — a finding whose detector has no matching hypothesis section is reported unconfirmed rather than quietly dropped |
| Layer | 5 · products |

### `security.fuzz`

| | |
|---|---|
| Input | a fuzz target name and a time budget |
| Output | whatever the target's corpus and crashes directory hold afterwards |
| Command | `sting fuzz --target boc_roundtrip --time 60` |
| Mode | offline, needs a toolchain |
| Evidence | the targets drive the same emulator binding the detectors confirm through |
| Cost | as long as it is given |
| Does not | ship with the toolchain it needs: `cargo-fuzz` and a nightly compiler are the caller's, and a release binary alone cannot run it because the targets' source is part of the checkout |
| Layer | 5 · products |

---

## 2. ABI layer: a normalised schema for a contract, and the dictionary behind it

### `security.benchmark-export`

| | |
|---|---|
| Input | the labelled corpus |
| Output | 508 unit directories, each a project a participant can analyse, with its own manifest |
| Command | `sting-benchmark export --dir /tmp/bench-run` |
| Mode | offline, costly |
| Evidence | 43 real projects, 368 injected defects, 97 hand-written fixtures |
| Cost | 90 s |
| Does not | run any analyser: a participant is a separate process, and this only lays out what it reads |
| Layer | 5 · products |

### `security.benchmark-score`

| | |
|---|---|
| Input | the exported units with a participant's output beside each |
| Output | Markdown: precision and recall per class per participant, and the aggregate over the classes both cover |
| Command | `sting-benchmark score --dir /tmp/bench-run --configuration "acton 1.1.0 as shipped"` |
| Mode | offline, needs an artefact |
| Evidence | 415 of 508 units comparable; sting 81.5% precision at 100% recall, acton 87.1% at 76.4% |
| Cost | 40 s |
| Does not | rank the participants, and does not count a class a participant has no rule for as a zero |
| Layer | 5 · products |

### `security.hunt`

| | |
|---|---|
| Input | the published bug bounty scopes of the TON ecosystem, as [`sting-hunt`'s committed scope table](../crates/sting-hunt/src/scope.rs) |
| Output | how much of each scope this analyser can read, then what fired where it could: Markdown to read, and JSON (`schema_version` `1.0`) to compare with a later run |
| Command | `sting-hunt --cache target/hunt --output corpus/hunt/first-run.md` |
| Mode | live |
| Evidence | 12 of 18 scope entries carry readable code, 170 of 2121 files; all 170 parse with no `ERROR` node; 20.0% precision on the Tolk target against 82.4% on the fixture corpus |
| Cost | 25 s with an empty cache and 45 MB of clones, about a second with them already there |
| Does not | report anything outside a published scope, claim a target is safe, or copy a target into this repository |
| Layer | 5 · products |

### `abi.extract`

| | |
|---|---|
| Input | one contract's entrypoint file, in Tolk, Tact or FunC |
| Output | `ContractSchema` (`ir_schema_version` `0.2`) as JSON or text |
| Command | `abi-layer extract corpus/clean/acton-contracts/counter/Counter.tolk --format json` |
| Mode | offline |
| Evidence | golden files per language, and a Tier 1 completeness figure the benchmark states |
| Cost | 77 ms for a Tolk contract, most of it the compiler |
| Does not | extract from an address: a deployed contract has no source, and the dictionary probe is what answers there |
| Layer | 5 · products |

### `abi.bench`

| | |
|---|---|
| Input | the Tolk, Tact and FunC corpora |
| Output | Tier 1 structural completeness and Tier 2 precision and recall, as Markdown and JSON (`schema_version` `1.0`) |
| Command | `abi-layer bench` |
| Mode | offline, costly |
| Evidence | measured against a committed baseline, and a regression against it fails the run |
| Cost | 27 s |
| Does not | measure a language whose compiler is not reachable — it says which one was missing rather than reporting a lower number |
| Layer | 5 · products |

### `abi.dictionary-build`

| | |
|---|---|
| Input | the Tolk, Tact and FunC corpora |
| Output | a method-id dictionary as JSON (`schema_version` `1.1`), its corpora, licence and build command named in the file, collisions included |
| Command | `abi-layer dictionary-build --output /tmp/method-dictionary.json` |
| Mode | offline, costly |
| Evidence | 1 388 method ids in the committed dictionary |
| Cost | 214 s |
| Does not | invent a name: an id whose name no corpus contract carries stays unknown, which is what makes the probe's silence meaningful |
| Layer | 5 · products |

### `abi.dictionary-probe`

| | |
|---|---|
| Input | a deployed contract's address and a saved dictionary |
| Output | text: which of the known methods that bytecode answers, and how many were inconclusive |
| Command | `abi-layer dictionary-probe <address> --dictionary corpus-abi/method-dictionary.json` |
| Mode | live |
| Evidence | 11 µs per probed entry, measured by the profiler against real mainnet code |
| Cost | one chain request plus about 9 ms per full dictionary pass |
| Does not | read source, and does not name a method the dictionary does not know |
| Layer | 5 · products |

### `abi.dictionary-verify`

| | |
|---|---|
| Input | the committed dictionary and the corpora it was built from |
| Output | text: the file reproduces byte for byte, or the first offset at which it does not |
| Command | `abi-layer dictionary-verify` |
| Mode | offline, costly |
| Evidence | 706 060 bytes reproduced, twice, from one corpus |
| Cost | 215 s — the rebuild, plus the comparison |
| Does not | check that the dictionary is right: it checks that the file a consumer holds is the file this corpus produces, which is a different question, answered by `abi.dictionary-audit` |
| Layer | 5 · products |

### `abi.dictionary-audit`

| | |
|---|---|
| Input | the committed dictionary and a Tolk compiler |
| Output | text: how far the sample diverges from the compiler, each divergence named, non-zero above 1% |
| Command | `abi-layer dictionary-audit --sample 100` |
| Mode | offline, costly |
| Evidence | 100 of 115 compiler-extracted claims across 40 contracts, divergence 0.0% |
| Cost | 2.0 s |
| Does not | say anything about the Tact and FunC entries, which are the larger part of the dataset and have no compiler to be checked against |
| Layer | 5 · products |

### `abi.catalogue-fetch`

| | |
|---|---|
| Input | a checkout of `ton-blockchain/abis` |
| Output | their two lists as JSON (`schema_version` `1.0`), with the commit read out of the checkout |
| Command | `abi-layer catalogue-fetch --checkout <clone of ton-blockchain/abis> --read-at 2026-08-23` |
| Mode | offline, needs an artefact |
| Evidence | 600 get-method names and 1 296 opcodes from 285 type files, the same bytes on a second read |
| Cost | 4.4 s |
| Does not | clone anything, and does not read the date from the clock: the same checkout has to produce the same file whenever it is read |
| Layer | 5 · products |

### `abi.catalogue-diff`

| | |
|---|---|
| Input | the committed dictionary and the recorded catalogue lists |
| Output | JSON (`schema_version` `1.0`) and text: the overlap and both differences, including the unflattering one |
| Command | `abi-layer catalogue-diff` |
| Mode | offline |
| Evidence | 283 names in both, 1 122 only here, 317 only there — 47.2% of their catalogue, 20.1% of this dataset |
| Cost | 30 ms |
| Does not | compare opcodes: their catalogue declares 1 296 of them, and the opcodes this tree knows are the standard ones in `sting-tep` rather than a learned dataset |
| Layer | 5 · products |

### `abi.decode`

| | |
|---|---|
| Input | a message body as base64, or a mainnet transaction hash |
| Output | JSON (`schema_version` `1.0`) and text: what operation the body is, with its fields read in order |
| Command | `abi-layer decode --body te6cckEBAgEAPgABZHNi0Jzv5G1UHv2tpUEeGjAIAAxyuxvBmtyIDdYsj3iafEOsMHxyCuREQYSBefhi2qTlAQAOAAAAADMwMD/iE3A=` |
| Mode | offline |
| Evidence | 1 319 opcodes across two datasets; a captured mainnet body reads to the same values an independent decoder gave it |
| Cost | 50 ms |
| Does not | choose between two contracts that declare one opcode, and does not read a field whose shape the dataset states no reading for — it stops there and says which field |
| Layer | 5 · products |

### `abi.decode-sample`

| | |
|---|---|
| Input | a count of mainnet transactions |
| Output | JSON (`schema_version` `1.0`) and text: three unknown shares, and the twenty most frequent opcodes nothing names |
| Command | `abi-layer decode-sample --transactions 10000` |
| Mode | live |
| Evidence | 10 000 transactions, 6 967 carrying an operation: 37.8% of the operations unnamed, 26.4% of the transactions, 94.2% of the vocabulary |
| Cost | about 100 requests against a public service per 10 000 transactions |
| Does not | say why an opcode is unknown, and does not grow the dataset: it prints the working list a growth pass would start from |
| Layer | 5 · products |

### `abi.catalogue-operations`

| | |
|---|---|
| Input | a checkout of `ton-blockchain/abis` |
| Output | an operation dictionary as JSON (`schema_version` `1.0`) learned from their type files, under their MIT terms |
| Command | `abi-layer catalogue-operations --checkout <clone of ton-blockchain/abis>` |
| Mode | offline, needs an artefact |
| Evidence | 285 of 285 type files compile; 945 opcodes, 269 of them claimed by more than one operation |
| Cost | 40 s |
| Does not | merge what it learns into this repository's own dataset: the two carry different terms, and a file whose licence depends on the entry is a file nobody can use |
| Layer | 5 · products |

### `abi.sweep`

| | |
|---|---|
| Input | a mainnet transaction range and a coverage database |
| Output | the database, extended with every newly discovered code hash and its probe result |
| Command | `abi-layer sweep --db /tmp/coverage.sqlite --pilot-size 5` |
| Mode | live |
| Evidence | deduplicated by code hash, which is what keeps a sweep proportional to the code rather than to the traffic |
| Cost | sustained traffic against a public third-party service; read the domain document before running one at size |
| Does not | pace itself beyond the shared client's 10 requests a second, and does not ask permission of the service it reads |
| Layer | 5 · products |

### `abi.tact-share`

| | |
|---|---|
| Input | a coverage database |
| Output | how many of its unique on-chain codes Tact emitted |
| Command | `abi-layer tact-share --db /tmp/coverage.sqlite` |
| Mode | offline, needs an artefact |
| Evidence | 58 of 389 codes were not Tact when the perimeter was last measured, each with its own reason |
| Cost | seconds over a database the sweep already built |
| Does not | build the database it reads, and says nothing about code no sweep has seen |
| Layer | 5 · products |

### `abi.coverage-report`

| | |
|---|---|
| Input | a coverage database |
| Output | text: aggregate coverage, and the top candidates for extending what is known |
| Command | `abi-layer coverage-report --db /tmp/coverage.sqlite` |
| Mode | offline, needs an artefact |
| Evidence | candidates ranked by how many distinct addresses carry the unknown id |
| Cost | seconds over a database the sweep already built |
| Does not | extend the dictionary — it says what is worth extending it with |
| Layer | 5 · products |

### `abi.serve`

| | |
|---|---|
| Input | HTTP requests carrying contract source or an address |
| Output | the same `ContractSchema` (`ir_schema_version` `0.2`) `abi.extract` produces, as a JSON response |
| Command | `sting-abi-service serve --port 8080` |
| Mode | offline |
| Evidence | a request body limit, a request timeout and a rate limit, each with its own test |
| Cost | it holds a port; a request costs what `abi.extract` costs |
| Does not | serve a language whose compiler is unreachable: it starts anyway and refuses that language per request, rather than refusing to start |
| Layer | 5 · products |

### `abi.profile`

| | |
|---|---|
| Input | nothing |
| Output | a table of where the extraction time goes, per stage and per call |
| Command | `sting-abi-profiler` |
| Mode | offline, costly |
| Evidence | every row is a measured total and a derived per-call figure, with the derivation named |
| Cost | 35 s, and one chain request for the probe row |
| Does not | profile a caller's own contract — the stages are fixed, and it measures this workspace's corpora |
| Layer | 5 · products |

---

## 3. Scamcheck: risk signals for jettons and for gift collections

### `scamcheck.jetton`

| | |
|---|---|
| Input | a jetton master's address, and optionally a pool address for the liquidity signal |
| Output | every applicable signal with its own verdict, and a composite score |
| Command | `sting-scamcheck EQCxE6mUtQJKFnGfaROTKOt1lZbDiiX1kCixRv7Nw2Id_sDs` |
| Mode | live |
| Evidence | eleven signals, each proven against a captured mainnet contract with its own recorded ground truth |
| Cost | a handful of chain requests plus milliseconds of emulation per signal |
| Does not | return a bare yes or no, and does not read a signal it has no data for: a signal that does not apply is absent rather than neutral |
| Layer | 4 · subject |

### `scamcheck.gift`

| | |
|---|---|
| Input | an NFT item or collection address, and optionally a marketplace domain and a wallet |
| Output | the applicable trust signals, structured, across authenticity, domain and wallet reputation |
| Command | `gift_trust_check 0:46FA0E9A864014196A5E7D66F1F83FFDB10F2859BBF2EA9BAEABBF14D9CE0D50` |
| Mode | live |
| Evidence | every registry entry was cross-checked live against three independent facts |
| Cost | a handful of chain requests |
| Does not | call an unlisted collection fake: absence from a nine-entry seed registry is absence of evidence, and the answer says so |
| Layer | 4 · subject |

### `scamcheck.namesakes`

| | |
|---|---|
| Input | a catalogue of collections: address, name, and how much of its own supply each issuer holds |
| Output | which names are contested, which contract is the series, and which are copies of it |
| Command | `cargo test -p sting-scamcheck --test gift_authenticity_over_the_census` |
| Mode | offline |
| Evidence | over the committed census of 167 collections: 8 contested names across 53 rows, and 40 copies behind 7 of them |
| Cost | 0.4 s over the committed census, most of it building the test |
| Does not | separate a group where nobody holds back supply — six contracts claiming one name and none holding any of it is a group this rule declines rather than guesses at |
| Layer | 4 · subject |

---

## 4. Sybil: who funded whom, and what that makes a wallet look like

### `sybil.replay`

| | |
|---|---|
| Input | a range of masterchain block numbers |
| Output | raw transactions and messages, and a summary of what was written |
| Command | `sting-sybil-ingestion replay --from 1 --to 200` |
| Mode | offline |
| Evidence | the same range replays to the same rows, and a restart from the checkpoint lands where an uninterrupted run did |
| Cost | 55 ms for 200 synthetic blocks |
| Does not | resolve ownership or decode a body beyond its opcode — an ingested row must be re-derivable from the chain alone |
| Layer | 5 · products |

### `sybil.migrate`

| | |
|---|---|
| Input | a column-store URL |
| Output | the raw-store tables, created where they are absent |
| Command | `sting-sybil-ingestion migrate --clickhouse-url http://127.0.0.1:8123` |
| Mode | live |
| Evidence | running it twice changes nothing, which its own acceptance test asserts |
| Cost | one round trip per table |
| Does not | migrate data: it creates tables, and a schema that has already drifted is not repaired by it |
| Layer | 5 · products |

### `sybil.ingest`

| | |
|---|---|
| Input | a range of real masterchain blocks |
| Output | their transactions and messages in the raw store, and a summary of what was written |
| Command | `sting-sybil-ingestion replay --from 87210000 --to 87210003 --live --to-clickhouse` |
| Mode | live |
| Evidence | the same range ingested twice leaves one copy, which the store's own acceptance suite asserts against the real backend |
| Cost | one chain request per block, and one insert per block |
| Does not | follow the tip: this replays a finite range, and following is what the library's follower does under a caller that keeps it running |
| Layer | 5 · products |

### `sybil.features`

| | |
|---|---|
| Input | a range of masterchain blocks already in the raw store |
| Output | one partition of the 31-column feature vocabulary, as columnar files under `feature_version` (`v1` today) |
| Command | `sting-sybil-features extract --from 87210000 --to 87210003 --feature-version v1` |
| Mode | live |
| Evidence | the canonical schema is asserted field for field, and a recorded snapshot pins every column of every row |
| Cost | one pass over the range, plus one file write |
| Does not | build the value graph: resolving a jetton wallet needs a chain client that exists only as a test double, so the jetton figures come out zero and the document says so |
| Layer | 5 · products |

### `sybil.cluster`

| | |
|---|---|
| Input | a range of masterchain blocks already in the raw store |
| Output | one partition of cluster membership, structural half only |
| Command | `sting-sybil-cluster extract --from 87210000 --to 87210003` |
| Mode | live |
| Evidence | a giant component breaks into its dense cores, and the same graph clusters the same way twice |
| Cost | one pass over the range, plus one file write |
| Does not | compute the behavioural half or the agreement between the two: those columns are written empty rather than filled with a default |
| Layer | 5 · products |

### `sybil.snapshot`

| | |
|---|---|
| Input | a fixed range of masterchain blocks |
| Output | JSON (`schema_version` `1.0`): every row the range ingested to, in identity order, or the offset where a replay differs |
| Command | `sting-sybil-ingestion snapshot --from 87210000 --to 87210003 --live --verify corpus/sybil/snapshots/mc-87210000-87210003.json` |
| Mode | live |
| Evidence | 46 transactions and 60 messages, 50 543 bytes, reproduced three times |
| Cost | four chain requests per block range, about 4 s |
| Does not | snapshot the column store: what it writes is what ingestion produced, so a store that transformed a row on the way in would not be caught by it |
| Layer | 5 · products |

### `sybil.retro`

| | |
|---|---|
| Input | scores by owner, a campaign's exclusion list, and the threshold registered before the run |
| Output | JSON (`schema_version` `1.0`) and text: precision, recall, the three counts, and the commit the threshold was registered in |
| Command | `sting-sybil-cluster retro --scores <owner to risk> --excluded <the campaign's list>` |
| Mode | offline, needs an artefact |
| Evidence | the threshold is registered in commit `3cf08dc`, read out of git rather than typed |
| Cost | milliseconds; the inputs are two files |
| Does not | score anything: the pipeline that computes scores is the one being judged, and a measurement that produced its own inputs would be marking its own work |
| Layer | 5 · products |

### `sybil.serve`

| | |
|---|---|
| Input | HTTP requests carrying an address, and a key from the tier file |
| Output | a risk score, its band, the features that moved it, and whether the answer was partial |
| Command | `sting-sybil-api serve --linear-scorer-path corpus/sybil/fixtures/linear_scorer_v1.toml --feature-version v1 --api-keys-path corpus/sybil/fixtures/api-keys.toml` |
| Mode | offline |
| Evidence | a cache hit and a cache miss both answer inside 200 ms, and every answer carries at least one reason |
| Cost | it holds a port; a request costs microseconds against a warm cache |
| Does not | resolve an unknown address: a miss is answered as partial rather than sent to look, which is what makes the latency promisable |
| Layer | 5 · products |

---

## 5. Karman: mechanisms as deterministic state machines

### `karman.run`

| | |
|---|---|
| Input | a scenario file |
| Output | a Monte Carlo run report as Markdown, and a JSON summary (`schema_version` `1.0`) on standard output |
| Command | `karman run --scenario karman/testdata/scenarios/cpamm-smoke.json` |
| Mode | offline |
| Evidence | the same seed produces the same reserves value for value, over a thousand streams |
| Cost | 49 ms for 100 runs of 500 ticks |
| Does not | claim a mechanism matches a real venue — that is what the verification ladder is for, and a run alone says only what this model did |
| Layer | 5 · products |

### `karman.sweep`

| | |
|---|---|
| Input | a scenario, one parameter, and the values to turn it to |
| Output | the designs side by side: a report to read, and a JSON summary (`schema_version` `1.0`) |
| Command | `karman sweep --scenario karman/testdata/scenarios/cpamm-smoke.json --vary n-ticks --values 100,200 --runs 20` |
| Mode | offline |
| Evidence | every point draws from the same seed, so two points differ by the knob alone |
| Cost | 54 ms for two points of 20 runs |
| Does not | search: it reports the points it was given, and finding the interesting one is the caller's |
| Layer | 5 · products |

### `karman.verify`

| | |
|---|---|
| Input | a scenario file |
| Output | which level of the verification ladder the run reached and the statistic at it, as a JSON summary (`schema_version` `1.0`) |
| Command | `karman verify --scenario karman/testdata/scenarios/cpamm-smoke.json` |
| Mode | offline |
| Evidence | level 2 measured rather than judged: the report states the figure and refuses to call it a pass |
| Cost | 32 ms |
| Does not | reach level 0 without a reference implementation, and says which level it stopped at rather than reporting the last one it managed as the answer |
| Layer | 5 · products |

### `karman.profile`

| | |
|---|---|
| Input | a scenario file |
| Output | phase timings, single- against multi-thread throughput, and live peak memory |
| Command | `karman profile --scenario karman/testdata/scenarios/cpamm-smoke.json` |
| Mode | offline |
| Evidence | peak memory holds at the same figure from 500 to 5 000 ticks, which is the claim that it is a function of state |
| Cost | 3.3 s |
| Does not | profile against a wall clock a caller controls: the figures are this host's, and the ratio is the part that travels |
| Layer | 5 · products |

### `karman.tvm`

| | |
|---|---|
| Input | a contract's code and data, and a get-method name |
| Output | one unsigned integer off that method's result stack |
| Command | `cargo test -p sting-karman-tvm --test against_a_real_contract` |
| Mode | offline |
| Evidence | against a captured STON.fi pool the executor reads the quantity the endpoint's own decoder read, and the run consulting it reproduces |
| Cost | 20 ms, including building the emulator instance |
| Does not | send a message or apply a transaction: a run that could change the state it reads from would not reproduce |
| Layer | 2 · parsing |

### `karman.census-sweep`

| | |
|---|---|
| Input | a description host, a time window, and the issuers whose holdings are stock rather than ownership |
| Output | the indexer's answers, one JSON object per line (`schema_version` `1.0` in the header), one file per stage |
| Command | `cargo run -p sting-karman-data --bin sweep-ton-census -- --into /tmp/census --days 1 --served-by nft.fragment.com` |
| Mode | live |
| Evidence | a run stops at the first stage it cannot finish, and a rerun continues from there rather than starting again |
| Cost | thousands of paged requests against two public services; several gigabytes for a full window |
| Does not | count anything: reaching and counting are separate on purpose, because the counting is the half where a mistake is silent |
| Layer | 5 · products |

### `karman.census-fold`

| | |
|---|---|
| Input | the files a sweep wrote |
| Output | the census fixture: float, turnover, holders and prices, per collection |
| Command | `cargo run -p sting-karman-data --bin pull-ton-census -- /tmp/census.json /tmp/census` |
| Mode | offline, needs an artefact |
| Evidence | 167 collections over a 90-day window, with every total given both ways rather than netted |
| Cost | seconds over the files a sweep already wrote |
| Does not | reach anything, and does not judge a collection: it counts what the chain said, and the authenticity rule is a separate capability over its output |
| Layer | 5 · products |

---

## 6. Foundation: what every contour above is built on

### `foundation.surface-load`

| | |
|---|---|
| Input | any of this platform's surfaces, already running |
| Output | JSON (`schema_version` `1.0`) and text: throughput and percentiles per concurrency step, refusals counted apart, and where throughput stops rising |
| Command | `surface-load --url <a surface already running> --requests 600` |
| Mode | offline, needs an artefact |
| Evidence | the decode surface at 8 467 req/s and p95 2.4 ms at concurrency 16; the scoring endpoint at 9 999 req/s and p95 1.0 ms at concurrency 8 |
| Cost | 600 requests per step, eight steps, never fewer than eight per worker |
| Does not | run the service it measures, and does not vary its question: one request repeated says what one shape of call costs, not what a day's traffic does |
| Layer | 5 · products |

### `foundation.emulate`

| | |
|---|---|
| Input | a contract's code, data and context, and a get-method to run |
| Output | the method's exit code and its result stack |
| Command | `cargo test -p sting-emulator` |
| Mode | offline |
| Evidence | the library is pinned by commit, and a build against a different one refuses to start |
| Cost | 9 µs to build an instance, 11 µs per get-method call |
| Does not | apply a transaction in the general case: what is proven here is the read path, and the write path has its own binding and its own tests |
| Layer | 1 · foundation |

### `foundation.chain`

| | |
|---|---|
| Input | a provider, a network and a request |
| Output | the service's answer, retried and paced |
| Command | `cargo test -p sting-chain` |
| Mode | offline |
| Evidence | one limiter per host, an attempt is an attempt, and a stub server proves both without a network |
| Cost | 10 requests a second per host, by policy rather than by accident |
| Does not | retry a refusal that is the caller's fault: a 4xx is returned rather than repeated |
| Layer | 1 · foundation |

### `foundation.corpus`

| | |
|---|---|
| Input | an address, or a corpus path |
| Output | a contract's source or its fetched bytecode, cached on disk |
| Command | `cargo test -p sting-corpus` |
| Mode | offline |
| Evidence | 297 Tolk files parse without an error node, which is what the parse gate measures |
| Cost | a cache hit is a file read; a miss is one chain request |
| Does not | keep a cache coherent with the chain: a cached account is what it was when it was fetched |
| Layer | 1 · foundation |

---

## 7. What this inventory does not do

It does not say which capability is worth money. Everything above is something the platform can be
asked to do; who would pay for it is a different question and a different document.

It does not promise a number will hold. Every cost is one host's measurement in a debug build, and
every piece of evidence is a measurement of a corpus, a census or a captured contract — a sample,
never a population.

It does not describe an interface. What a command's output looks like field by field belongs to
[`API.md`](./API.md) and to the schemas themselves; what is here is what the capability answers and
what it refuses to answer.

And it does not stand still: a command that stops running fails `make pr`, because the whole point
of writing the commands down was to stop the document from outliving the thing it describes.
