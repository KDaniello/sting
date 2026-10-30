# Plan

A plan is not a wish list and not a session tracker. It is a sequence of work, each piece closed by a
gate, and every gate carries a failure condition. Work without a failure condition is not planned.

The order is fixed once and is not re-argued here: **the documentation takes on its discipline → the
core catches up with the chain → the code merges into one tree → the capabilities are inventoried →
the products are derived from the inventory.**

The discipline comes first because the three gates after it write their numbers into these same
documents. Reversing the rest would mean
an inventory that describes four repositories instead of one platform, and a product chosen from the
author's memory rather than from what the platform can do.

---

## 0. State

### 0.1. Measured

| What | Number | How it is confirmed |
|---|---|---|
| Crates in the root workspace | 29 | the `members` list |
| Crates outside it | 23 | `karman` 15 plus a Python SDK, `sybil` 8 |
| Tests green | 902 + 439 + 136 | three different commands in three directories, with 18 `sybil` tests skipped without ClickHouse |
| Lines of Rust | 59 279 + 25 212 + 9 481 | a walk of `crates`, `karman`, `sybil` |
| Implementations of chain access | 2 live, 2 held | `sting-chain` and `tools/ton-res` on a liteserver; two more in the closed project, counted by `make lint` until it is taken off the index ([`domains/chain.md`](./domains/chain.md) §7) |
| Tolk files in the corpus | 297 | `corpus/**/*.tolk` |
| Of those, parsed without an error | 297 — 100% | a corpus walk through the parser and its error-node scan |
| Detectors | 14 | `sting explain` on every identifier |
| Method-id dictionary | 1 388 | `corpus-abi/method-dictionary.json` |
| External users | 0 | no remote is configured, and publishing is disabled |

Crate count, test count and line count are not progress. They stand here as the basis for gates
rather than as an achievement.

### 0.2. What changed in the chain

Measured on 2026-08-21 against official sources.

| Event | Date | What it means for the core |
|---|---|---|
| Global version 12: the bounce format carries extra flags; address-loading instructions | 2025-11-13 | `UBM` reasons about a format that no longer exists |
| Global version 13: `NaN` and out-of-range arguments, end status, forward fees | 2026-04-03 | not reflected |
| The official toolchain arrives, its linter carrying 39 rules | 2026-05-08 | three of them are `UBM`, `AC` and `EPS` |
| Tolk 1.4: a native ABI, TypeScript wrappers, source maps, a debugger | 2026-05-11 | the Tolk extractor already consumes the compiler's own ABI |
| TVM v14: `SENDMSG` stopped taking a user-supplied forward fee as a lower bound | 2026-06-05 | not reflected |
| Tolk 1.4.2, the token rename | 2026-06-21 | the vendored compiler matches; the documentation does not |
| Global version 15: libraries restricted to governance, action fines collected, a message size limit | 2026-07-23 | not reflected, and the version is pinned nowhere |
| Node release v2026.08 | 2026-08-17 | the vendored emulator is built from v2026.06 |

Separately, the language perimeter. Tact is deprecated and its documentation page was removed in
mid-April 2026. FunC is legacy and its compiler is unmaintained. Tolk is the only living language of
the ecosystem.

### 0.3. Three divergences the plan is derived from

**The core read January's language while the language was on July's.** The grammar was published
2026-01-17 and the vendored compiler was built 2026-07-15, and 54 files of 297 carried an error node
because of it. Closed at M1: the grammar is vendored and patched, all 297 parse, and the compiler
reads nothing it refuses.

**A failed parse was silent.** The linter on a file with error nodes returned an empty result and
exit code 0 — exactly what it returns on a clean file. An analyser that answers the same way to "no
defects" and "I did not understand the file" is not an analyser. Closed at M1: the position is named
and the run exits non-zero.

**The projects are lying beside each other rather than merged.** Four workspaces, four names in the
exclusion list, five paths to the same chain data, three incompatible lint regimes. The previous
edition of this plan put publishing a dataset ahead of the merge — that is, it proposed releasing an
artefact before saying how it differs from the foundation's catalogue.

---

## 1. Gate O — the core sees the living TON

### 1.1. What the core does not see

| Construct | Language version | Parses |
|---|---|---|
| dynamic arrays | 1.3 | no |
| null coalescing | 1.3 | no |
| ABI annotations | 1.4 | no |
| a `contract` declaration | 1.0 onwards | no — the whole block is one error node |
| a lambda | 1.2 | no |
| strings and the string builder | 1.3 | yes |
| enumerations | 1.1 | yes |
| maps | 1.1 | yes |
| lisp-style lists | 1.3 | yes |
| the address type | 1.2 | yes |
| lazy loading | 1.0 | yes |
| a union with null | 1.2 | yes |

Five of twelve. Worse than the proportion is which one: the contract declaration is the construct the
compiler builds an ABI from, so the blindness falls on the semantic centre of the language rather
than on its edge.

### 1.2. The work

**O1. A failed parse stops being silence.** On a tree with error or missing nodes the linter returns
a diagnostic with a position and a non-zero exit code. Stated as a prohibition: a file that was not
parsed may not look like a file without findings.

**O2. The blindness gets a number.** The share of corpus files free of error nodes — 100% now, 81.8% when this was written — is
computed by a command and enters the gate. A fall in the share fails it.

**O3. The compiler becomes the oracle.** The vendored compiler is run over the corpus; a file the
compiler accepts and the grammar refuses is a divergence, and every divergence is named. The list is
either empty or explained line by line; there is no third state.

**O4. The constructs close, and the oracle names them.** The order is set by the cost of the
blindness. This document named five from a changelog and was wrong about three of them, which is what
a guess costs against an oracle: `??` already parsed, and neither dynamic arrays nor the lambda form
named here is Tolk syntax at all. The method is a change to the grammar offered upstream rather than a
second textual workaround: the text-scan fallback exists as a patch and is deleted in the same commit
that gives the grammar its rule.

**O5. The global version is named.** Emulation pins the global version explicitly, the emulator's
version is read at load and compared against what is expected. A run against an unknown version
refuses rather than proceeding quietly.

**O6. The semantics of versions 12 through 15 are ruled against the detectors.** For each of the 14
it is stated whether the chain's change moves it. Three verdicts are named in advance: `UBM` against
the new bounce format, the send-shaped detectors against the fee change, and anything reading
libraries against the restriction.

**O7. The language perimeter is declared.** Tolk is alive and gets the depth. FunC is legacy as a
language but not as a chain: mainnet is full of it, so the extractor and the grammar stay for the
dictionary and for decoding. Tact freezes at compiler 1.6.13: tests run, development stops. The
measurement came first and the freeze follows it — **between 1.5% and 3.3% of 389 unique mainnet
code hashes declare themselves Tact**, a floor rather than a ceiling since the markers are optional ([`domains/method-dictionary.md`](./domains/method-dictionary.md) §8).

### 1.3. Gate O

The core has caught up when all of the following hold. **All six hold, and the gate is passed.**

| Condition | Today |
|---|---|
| no file parses with an error silently: a diagnostic and a non-zero exit code | `sting lint` names the position and refuses |
| the share of the corpus that parses is computed by a command and checked by the gate | 297 of 297, floor at 100% in `make pr` |
| no divergence between compiler and grammar, or every one named | the oracle runs both directions, 0 either way |
| every construct the oracle names parses, and the text-scan patch is deleted | `extract_contract_headers` is gone with its tests |
| the global version and the emulator version pinned and verified at start | version 14 and commit `3f598aa2`, refused on every config path |
| for each of the 14 detectors, what versions 12 through 15 do to it | 14 rows, `UBM` moved twice |

**Failure condition:** one file of the corpus parses with an error and the user does not learn of it.
Then the gate has not passed, however many constructs the grammar has gained.

### 1.4. What gate O does not do

It adds no detector and no user. It returns to detectors already written the right to speak about a
file they read whole. Accuracy numbers measured before this gate are recomputed after it: they were
obtained on a corpus 18.2% of which the analyser never saw.

---

## 2. Gate M — code merges, not folders

### 2.1. The principle

A merge is not moving a directory into a shared tree but reducing shared code to one copy. A project
sitting in a subdirectory and building as its own workspace lies beside the tree rather than in it.
The reverse test: if two implementations of the same thing remain after a merge, the merge did not
happen.

### 2.2. Five paths to the same data

| Where | With what | What it can do |
|---|---|---|
| `sting-corpus`, the extraction crates | a synchronous HTTP client plus the emulator | code and data by address, calling get methods |
| `sting-karman-data` | two public APIs | a census of collections, pagination, retries |
| `sybil/ingestion` | an API, ClickHouse and an async runtime | a transaction stream, a follower |
| `tools/ton-res` | a light client | proven state at a known logical time |

Five paths, five sets of retries, five address models. That is the subject of the merge; everything
else is secondary.

### 2.3. Why the merge costs more than it looks

The previous edition counted the merge as moving crates. Moving is the smaller half.

**The pins diverge, and the divergence is substantive.**

| Dependency | `sting` | `karman` | `sybil` |
|---|---|---|---|
| error derivation | 2 | **1** | 2 |
| configuration parsing | 1.1 | **0.8** | — |
| HTTP | one client | through that client | through that client |
| generator | — | one generation | — |
| Python bindings | — | present | present |
| foreign | — | **an EVM executor and its ABI types** | columnar storage and a database client |

The two generations of the random generator are now one: the second belonged to the project that is
closed, and what remains is `karman`'s. That mattered because different generations mean different
seed semantics, which means different determinism — and for a project whose discipline includes
reproducibility by seed, that is not a build detail.

**The lint regimes are incompatible.** The root forbids `unsafe` and denies panics; `karman` holds
five `unsafe` blocks in a counting allocator; `sybil` has no such prohibitions at all. `forbid`
cannot be lifted by a local allow, so either a crate does not inherit the shared lints and that is
recorded by name, or the code changes.

**The perimeter widens silently.** `karman` pulls an EVM executor into a TON workspace. Either it is
declared part of the platform or it stays outside the perimeter with its crate.

### 2.4. The work

| Step | What | Gate |
|---|---|---|
| M1 | `sting-chain`: one chain client, a synchronous core with an async adapter above it | the ingestion snapshot and the market census both reproduce byte for byte |
| M2 | `sting-address`: one address model extracted from two places | the compile-fail test forbidding an owner where a jetton wallet belongs passes from the shared crate |
| M3 | manual jetton decoding is replaced by a call into the extraction layer | the graph over the same input yields the same edge set |
| M4 | pins reconciled by the table in §2.3, generator generation included | runs by seed give the same verdict before and after, or the difference is explained |
| M5 | one lint regime; every inherited crate takes the shared lints and exceptions are named | clippy is green with no allow added during the merge |
| M6 | the mechanism engine gets a TVM executor through the emulator crate | a mechanism against a real contract runs twice on one seed with the same result |
| M7 | the gift authenticity rule moves into the risk-signal crate | the copies reproduce from the new location, offline: 40 behind 7 contested names across 167 collections |
| M8 | the exclusion list is emptied and the duplicate project is closed | one command builds and runs the whole suite |

The duplicate project was started earlier and has stopped; the living one is elsewhere. Its lending
milestone and its diagnostics document — where the engine is wrong and why — move first, then the
directory leaves the index and its history stays in git. Failure condition for that step: if not one
line was taken, there was nothing to take and it should have been retired at once.

The measuring instrument is not rewritten in Rust and does not become a crate. It answers whether a
data source diverges from the chain, and it has already answered "no" with 0.00% over 160 objects. It
runs a few times a year, and its value is proven state through a light client rather than throughput.
It is the one of the five that does not merge, and the reason is recorded rather than implied.

### 2.5. The target graph

After the merge the tree is five layers, and dependency runs downward only.

```
Layer 5  products      the service, the realtime API, the two CLIs
Layer 4  subject       detectors, risk signals, clustering, mechanisms
Layer 3  analysis      semantics, dataflow, features, the engine
Layer 2  parsing       the schema core, the AST, the graph
Layer 1  foundation    core, address, chain, emulator
```

**The invariant.** A crate of layer N depends only on layers below N. An upward dependency is an
architectural error and does not pass review.

### 2.6. Gate M

The merge has happened when all of the following hold:

- chain access exists once and the other four are deleted;
- the address model is one, and the compile-fail test lives in the shared crate;
- the pins are reconciled, the generator generation is one, and no verdict by seed changed
  unexplained;
- the lint regime is one and every exception is named with its reason;
- the exclusion list is empty and one command runs the whole suite.

**Failure condition:** two implementations of chain access remain. Then the gate has not passed,
however many crates moved.

### 2.7. What gate M does not do

It does not merge the run discipline. The register of claims in the mechanism project stays its own
directory with its own format: it is a register of claims, not code. It also does not extract a
shared core from the neighbouring projects — what they share is a discipline rather than code, and a
library of that discipline would be a thin layer over a seeded generator.

---

## 3. Gate I — the inventory of capabilities, APIs and outputs

### 3.1. What counts as a capability

A capability is not a function, not a crate and not a line in a README. It is a pair of input and
output that reproduces from a clean clone with one command and is confirmed by a number. A capability
without a command is an intention; a capability without a number is a promise.

The inventory answers a question no document answers today: **what the platform can do in full.**
`API.md` describes three binaries of one contour; two of the merged projects do not appear in it at
all.

### 3.2. The form

Every capability gets a row in `CAPABILITIES.md` with eight fields:

| Field | What is in it |
|---|---|
| Identifier | stable; references to it are not rewritten |
| Input | its type and origin: source, address, hash, a list of addresses, configuration |
| Output | schema and version: SARIF, a contract schema, a dataset, a run report |
| Command | one line, reproducible from a clean clone |
| Number | what is measured and on what: coverage, precision, latency, share |
| Cost | network or local, order of time, whether it needs the emulator, a container, a key |
| Limitation | what the capability does not do — a field of its own rather than an aside |
| Layer | from §2.5, so the invariant can be checked |

### 3.3. What is inventoried

| Contour | Capabilities expected | What confirms them today |
|---|---|---|
| security | static parsing, 14 detectors, dynamic checking through the emulator, the benchmark, fuzzing | `API.md` §2, the committed baseline |
| abi-layer | schema extraction from three languages, the dictionary, recognition without source, the HTTP surface, coverage | `API.md` §3–4 |
| scamcheck | eleven signals, composite scoring, collection authenticity | `API.md` §5 |
| clustering | ingestion, the funding graph, features, clustering, scoring, realtime | no inventory |
| mechanisms | the engine, agents, five mechanisms, scenarios, reports, the market census | no inventory |
| instruments | whether a data source diverges from the chain | no inventory |

### 3.4. Gate I

The inventory is ready when all of the following hold:

- every capability has a row with all eight fields;
- every command runs from a clean clone and produces the stated output;
- every output has a schema with a version;
- every capability has its limitation field filled;
- no capability violates the layer invariant.

**Failure condition:** a capability is described and its command does not reproduce. Then the gate has
not passed, however many rows the table holds.

### 3.5. What the inventory does not do

It does not make a capability wanted. The inventory answers "what we can do", a product answers "who
pays for it", and the second does not follow from the first. The inventory exists precisely so that
the choice of product rests on a list rather than on memory.

---

## 4. Products

### 4.1. What counts as a product here

A product is not a contour, not a crate and not a capability. It is a pair of "who pays" and "for
what", with its readiness, what is missing, and the measure of success all named.

The ability to recognise a contract by its code hash at 86.2% of mainnet is not a product. What makes
it one is a buyer who needs the answer "what contract is this" and a price they pay for it.

### 4.2. The register

| # | Product | Buyer | Tier | Rests on which capabilities | Readiness |
|---|---|---|---|---|---|
| P1 | Sybil and anti-farm API | airdrops, reward mini-apps, gift marketplaces | A | ingestion, graph, features, clustering | code exists, calibration does not |
| P2 | The method-id dictionary and a decode API | indexers, explorers, wallets, analytics | A | the dictionary, recognition without source, three extractors | dataset exists, surface does not |
| P3 | Scam scoring for wallets | wallets and marketplaces, sold to two or three | B | eleven signals, collection authenticity, P2 | signals exist, labels do not |
| P4 | Valuation and an oracle for the gift market | marketplaces, lenders against gifts, bots | B | the census, the authenticity rule | census exists, trade aggregation does not |
| P5 | Compliance and AML | exchanges, payment processors, custodians | B | P1, P2 and P3 | three inputs exist, the legal side does not |
| P6 | A public security benchmark | not revenue — leverage on trust | C | the corpus, 14 detectors, the baseline | numbers exist, unpublished |
| P7 | A wind tunnel for economic mechanisms | protocols, funds, auditors | C | the engine, agents, the claim discipline | engine exists, buyers do not |

### 4.3. The products

#### P1. Sybil and anti-farm API

**What.** In: a list of addresses. Out: which of them belong to one person, with a confidence and an
explanation — through which funding node they are linked.

**Why a buyer exists.** The ecosystem grew on tap-to-earn, every distribution is eaten by farmers, and
whoever distributes has no way to tell a hundred people from one person with a hundred wallets. A
tailwind that is not a buyer: a non-custodial wallet in every client of a messenger with hundreds of
millions of users means wallets per person grow faster than people do.

**What is built.** Eight crates, 9 481 lines of Rust and 36 Python files, and 136 of its own tests green: ingestion,
the funding graph, features, clustering, scoring, a realtime endpoint, ground truth from a sandbox.

**What is missing.** A deployed database, calibration against live labels, packaging, a price. The 18
acceptance tests that do not run without that database are not "skipped tests" but an unmeasured
contour.

**How it is measured.** Precision and recall on a retro campaign with a known outcome. The threshold
is named before the run.

**What the product does not do.** It does not prove that a cluster belongs to one person. It shows
coordination. The difference is legal and substantive: coordination is an observable property of a
graph, and ownership is a claim about people that the graph does not support.

#### P2. The dictionary and a decode API

**What.** Two outputs. First, an open dataset mapping method ids to names. Second, a paid answer to a
hash: what happened, in words and fields.

**Against what.** The previous edition claimed TON has no analogue of Ethereum's four-byte registry.
The foundation's catalogue holds, as of 2026-08-21, 283 contracts, 332 code hashes, 1 299 opcode
prefixes and 600 get-method names. The claim "there is no analogue" leaves this plan; what replaces it
is a difference of nature rather than size: their artefact answers "we described the important
contracts", ours answers "we know what almost everything actually deployed is" — 86.2% of unique
mainnet codes, 337 of 391. Hand curation does not catch a walk, and a walk does not describe a
contract better than its author.

**What is built.** 1 388 method ids of machine origin, each with a record of where it was seen. ABI
extraction from Tolk through the compiler's own output, and from Tact and FunC through our own
extractors. A probe that recognises a contract without source by calling get methods against real
state.

**What has been measured against this product.** The instrument compared three public indexers with
the chain over 160 objects: 0.00% divergence on values, all three. The pitch "indexers have bad data"
is refuted by our own measurement. What sells is meaning: neither the chain nor an indexer answers
"what operation was that".

**How it is measured.** The share of requests the API answers with a decode rather than `Unknown`.
The threshold is the dictionary's mainnet coverage.

**What the product does not do.** It does not verify source. A mapping from code hash to operation
name is a claim about bytes, not about what the contract's author meant.

#### P3. Scam scoring for wallets

**What.** A risk label beside a jetton, a gift or an address: a sell trap, mint rights, concentration
in the owner's hands, a forged collection, a typosquatted domain.

**What is built.** Eleven signals. The typosquat is recognised on a real example: a hyphenated
imitation of the official domain, edit distance one, on a real collection.

**What is missing.** A labelled set of scams and a single integration.

**The particular shape.** This is an OEM sale to two or three buyers rather than a service of its own.
The first integration pulls the rest: a wallet without such a label starts to look worse than its
neighbour.

**What the product does not do.** It does not pass a verdict. It emits signals and their grounds; the
decision to show a warning stays with the wallet.

#### P4. Valuation and an oracle for the gift market

**What.** A gift's price from trades across every venue, a wash-trade filter, a collection index, an
authenticity check, collateral valuation.

**What is built.** A census of the market: 1 295 768 items, 117 collections. An authenticity rule: 27
forgeries across 167 collections. Real turnover measured — 11.7% of transfers are trades, and
everyone else counts turnover eight times larger than it is.

**What is missing.** Aggregation of trades across venues. It is off-chain, in the marketplaces'
internal balances.

**The competitor.** A gift analytics project with dozens of integrations and hundreds of millions of
data points. The niche is taken seriously.

**How it is measured.** The deviation of a valuation from the next real trade. The threshold is named
before the measurement.

#### P5. Compliance and AML

**What.** Address scoring, source of funds, clusters, the travel rule.

**What is built.** The three inputs such a product needs are all present: decoding (P2), clustering
(P1), risk signals (P3).

**What is missing.** The legal side, knowledge of the requirements, enterprise sales. This is a track
code does not close.

**How it is measured.** The first signed custodian. This product has no intermediate metric.

#### P6. A public security benchmark

**What.** An open method and corpus on which the accuracy of any TON analyser is measured, other
people's included.

**Against what.** Since May 2026 the official toolchain installs with the project and ships 39 lint
rules, three of which cover the same ground as `UBM`, `AC` and `EPS`. The question "why would a
developer run a second linter" was answered before we answered it: they would not.

**Where the contour moves.** To where a linter is empty by construction rather than by oversight:
cross-file taint, dynamic checking against real state, and the semantics of the chain's global
versions. A linter reads source; it does not emulate a transaction and does not know what version 15
did to libraries.

**What is published beside the good numbers.** The bad ones: two detectors sit near 33% precision. A
benchmark that publishes only its successes is not a measure. Neither the official linter nor the
other analyser in the niche publishes any accuracy figure at all — whoever holds the measure does not
compete with a linter, it measures one.

**The caveat.** 96.4% aggregate precision over the default set and 82.0% aggregate precision over
every measured detector were obtained before gate O, on a corpus 18.2% of which the analyser did not
parse. They are recomputed after gate O and before publication.

#### P7. A wind tunnel for economic mechanisms

**What.** Checking an economic mechanism before the market: agents, adversarial scenarios, a claim
registered before the run, a verdict reproducible from a seed.

**What is built.** The engine, the agents, five mechanisms, 439 of its own tests green, the claim discipline,
published negative results.

**What separates it from the rest.** Two refuted verdicts against thresholds named in advance: a
vouching ring, and a separability hypothesis at 33.0% against a threshold of 80%. Plus a third: the
instrument refuted a product hypothesis with 0.00%.

**Why tier C.** A long sales cycle, few buyers, no intermediate metric. As proof of method it has the
highest value; as a source of revenue, the lowest.

### 4.4. What is not a product

A raw data lake over the chain. The public pipeline is free and sits in a public cloud dataset, and
the analytics platforms stand on top of it. There is nothing to compete on at the raw layer.

An indexer of our own. Three public indexers diverge from the chain by 0.00% on values — the task is
solved and there is nothing to repeat.

A second Tolk linter. The official one installs with the toolchain and carries 39 rules.

---

## 5. The route

### 5.1. The order, and why this one

The previous edition put publishing the dictionary first, because it "takes an evening and needs no
new code". Publication leaves first place, and here is why: an artefact we cannot describe in one
line as different from a catalogue of 1 299 opcodes is published not quickly but early. And the
failure condition "nobody forked it in 30 days" measures somebody else's behaviour rather than our
artefact, which is why it is not a gate.

The order is set by dependency: gate O fixes what lies silently; gate M reduces five paths to one;
gate I describes what came out; the products are derived from the inventory.

### 5.2. The stages

#### P2 — the dictionary and the decode API

| Stage | Work | Gate | Failure condition |
|---|---|---|---|
| P2.1 | the dataset gets a schema, a provenance per entry, a version and a licence | a sample of 100 entries checked against the compiler's own ABI | divergence above 1% |
| P2.2 | the difference from the foundation's catalogue is measured | our ids absent there, and theirs absent here, both named as numbers | overlap above 90%: the dataset repeats somebody else's |
| P2.3 | publication | the dataset is available and rebuilds from the repository | it could not be rebuilt from the repository |
| P2.4 | the HTTP surface: a hash in, an operation out | median latency measured, `Unknown` at or below the coverage gap | `Unknown` above the dictionary's coverage |
| P2.5 | submissions for unknown method ids | the dictionary grows from outside contributions | every addition is the author's |
| P2.6 | a price | a first paying user | six months without one at live traffic |

#### P1 — sybil and anti-farm

| Stage | Work | Gate | Failure condition |
|---|---|---|---|
| P1.1 | deploy the database, unskip the 18 acceptance tests | ingestion reproduces its snapshot byte for byte | the determinism does not reproduce |
| P1.2 | a retro benchmark on a campaign with a known outcome | precision and recall above a threshold named in advance | below it |
| P1.3 | the realtime endpoint with quotas and keys | p95 measured, load sustained | latency above what was claimed |
| P1.4 | a first integration | one airdrop or mini-app in production | nobody connected in 90 days |

#### P3 — scam scoring

| Stage | Work | Gate | Failure condition |
|---|---|---|---|
| P3.1 | assemble labelled scams with a recorded source | at least 200 confirmed cases with their grounds | the labels come from one source |
| P3.2 | measure the eleven signals against the labels | precision published per signal, the bad ones included | the bad ones are not published |
| P3.3 | an OEM offer to wallets | one wallet shows the label | none agreed in 120 days |

#### P4 — gift valuation

| Stage | Work | Gate | Failure condition |
|---|---|---|---|
| P4.1 | aggregate trades across venues | at least three venues covered, the observed share of trades measured | fewer than 11.7% of transfers are observed |
| P4.2 | a wash-trade filter | the filtered share measured and justified | the filter cannot tell wash trading from a thin market |
| P4.3 | valuation and its check | deviation from the next trade below the threshold | above it |

#### P5 — compliance

| Stage | Work | Gate | Failure condition |
|---|---|---|---|
| P5.1 | the jurisdiction's requirements are worked out | requirements written down, the gap to the product named | requirements retold without naming the gap |
| P5.2 | a pilot with one custodian | a signed pilot | six months of talks without one |

#### P6 — the security benchmark

| Stage | Work | Gate | Failure condition |
|---|---|---|---|
| P6.1 | recompute accuracy after gate O | numbers obtained on a fully parsed corpus | not recomputed, and the old numbers published |
| P6.2 | publish the method and the corpus | a third party reproduces it | it could not be reproduced |
| P6.3 | run other analysers through it, the official linter included | numbers published, the inconvenient ones included | only the favourable ones published |

#### P7 — the wind tunnel

| Stage | Work | Gate | Failure condition |
|---|---|---|---|
| P7.1 | clustering answers the open question about a vouching ring | a verdict on a claim registered before the run | the claim was registered after the run |
| P7.2 | the method is written up publicly | an outside reader reproduces the verdict from a seed | it does not reproduce |

Runs of the vouching mechanism showed that fiscal and temporal defences do not work: the premium has
to reach 640% a year, the platform's share barely moves the outcome, ring size does not matter, and a
slow ladder buys delay rather than protection. One thing stayed unchecked — a rule that sees the
closedness of the graph. It is written in the clustering crate, and P7.1 is research rather than
engineering.

### 5.3. Dependencies

```
Gate O ─► Gate M ─► Gate I ─┬─► P2.1 ─► P2.2 ─► P2.3 ─► P2.4 ─► P2.5 ─► P2.6
                            ├─► P1.1 ─► P1.2 ─► P1.3 ─► P1.4
                            ├─► P3.1 ─► P3.2 ─► P3.3          (needs P2.4)
                            ├─► P4.1 ─► P4.2 ─► P4.3
                            └─► P7.1 ─► P7.2

P1.2 + P2.4 + P3.2 ────────────────► P5.1 ─► P5.2
Gate O ────────────────────────────► P6.1 ─► P6.2 ─► P6.3
Gate D ────────────────────────────► P6.2
```

### 5.4. The risk register

| Risk | Sign of it | What the plan does |
|---|---|---|
| The gates crowd out release | 120 days gone, no external users | gate O is seven pieces of work and gate I is a form of eight fields, rather than completeness |
| The chain moves again | a node release with a new global version | O5 pins the version and O6 schedules the detector review; the version check is part of the gate |
| The grammar stays behind for ever | the change is not accepted upstream | O3 keeps the compiler as an oracle: divergence stays visible whether or not the grammar is fixed |
| The official toolchain takes the niche | another of our detectors is covered by one of their rules | P6.3 measures their linter with our measure, and the contour moves to where a linter is empty |
| The dataset repeats the catalogue | overlap above 90% | P2.2 stands before publication and has a failure condition |
| Clustering cannot tell a ring from a dense community | retro precision below the threshold | the claim is registered before the run and the negative result is published |
| A product is built without a buyer | the stage passed and there is no buyer | every product's last stage has a failure condition about a buyer rather than about code |
| A duplicate reappears | a second path to the chain | the layer invariant, and review |

---

## 6. Documentation

The discipline is two gates rather than one, split by the fact that discipline and volume cost
differently. Discipline is cheap and needed first: gates O, M and I write their numbers into these
documents, and writing them into a set without rules means writing them twice. Volume is expensive
and waits for the inventory: describing four repositories in English costs more than describing one
platform with a finished list of capabilities.

**Gate D1 — discipline.** The document set is named and procedural files leave the index for the git
history. Sections are numbered and a cross-reference carries a section number. `ARCHITECTURE.md` §1
holds numbered invariants, each stated as a prohibition. English is canonical and `docs/ru/` is a
mirror with an identical heading tree. Every number in a document is derived from the code by a
script. D1 stands before gate O.

**Gate D2 — volume.** The set covers the whole platform, merged contours included: the clustering and
mechanism projects are described where `sting` is described rather than in their own directories. D2
stands after gate I, because describing a capability before the inventory describes it is work that
will be done twice.

**Failure condition for both:** one number in the documentation is not derived from the code.

The divergence is already there and already measured: the index called the crate count and the test
count wrong, and the architecture document gave two different crate counts in two places. While a
number is typed by hand it diverges from the code — the question is not whether but when.

---

## 7. How progress is measured

| Number | Now | Target |
|---|---|---|
| External users | 0 | 1 |
| Implementations of chain access | 2 live, 2 held in the closed project | 1 plus the named exception |
| Share of the corpus that parses without an error | 100% | 100% |
| Files the analyser is silent on without having parsed them | 0 | 0 |
| Commands needed to run the whole suite | 1 | 1 |
| Capabilities with a reproducible command | 0 | all |
| Gates passed | 2 of 5 — D1, O | 5 |
| Product stages passed | 0 of 21 | as decided |

Lines of code, crate count and test count are not progress. They grow both when the work moves and
when it is imitated.

---

## 8. What is deliberately not done

**The measuring instrument is not rewritten in Rust.** It runs a few times a year.

**No indexer of our own.** Three public indexers diverge from the chain by 0.00% on values.

**Tact support is not developed.** The language is deprecated and the extractor freezes at compiler
1.6.13. The measurement came before the freeze: 1.5% to 3.3% of unique mainnet code hashes declare
themselves Tact.

**No second Tolk linter.** The official one carries 39 rules and installs with the toolchain.

**No shared core extracted from the neighbouring projects.** What they share is a discipline, not
code.

**No TVM debugger.** The language shipped one with version 1.4.

**The unrelated businesses are not merged.** Separate buyers, separate products, and nothing in common
with a TON platform but their author.

---

The plan is five gates and seven products, and the order matters more than the list.

| Gate | What it buys |
|---|---|
| D1 | the documents get rules before the other gates write into them |
| O | the core regains the right to speak about what it has read whole |
| M | five paths to the chain become one |
| I | four repositories become a list of capabilities, each with a reproducible command |

Only after that is a product chosen from a list rather than from memory.