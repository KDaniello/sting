# The method dictionary

How to recognise a contract nobody has the source for. The project's most differentiated asset and the
candidate for its first public win.

---

## 1. The idea

A get method on TON is identified not by name but by a **method id** — a number derived from the name
by a fixed formula. The formula is one-way but finite: name to number. So with a large enough list of
real names, a reverse dictionary can be built and the **presence** of a method checked on any
bytecode, with no source at all.

The direct analogue of Ethereum's four-byte registry. Nobody has one for TON.

The mechanics are literally this:

```
address → real code and data from the chain → for every dictionary entry:
    call its method id through a real virtual machine → the method is present or not
```

Not one line of source is read, and nothing is guessed: it is the real emulator on real bytes.

---

## 2. The numbers

| Metric | Value |
|---|---|
| Entries | **1388 method ids in the dictionary** |
| Source contracts | 1386 |
| Recognition of unique mainnet codes | **86.2%**, 337 of 391 |
| Deduplication during a walk | sixty to ninety times in any window |

The path of that recognition figure is instructive because it was **not** linear:

| Stage | Dictionary | Recognition |
|---|---|---|
| first pilot | 116 | 50.0% |
| after a re-probe and a second run | 116 | 60.9% |
| after reactively filling gaps from the registry | 183 | 75.3% |
| after a **complete mechanical walk of the registry** | **1388** | **86.2%** |

---

## 3. Why walking the network stopped working and walking the registry did not

The direct question was asked and **checked rather than assumed**: would more mainnet history fill the
dictionary faster?

The answer is no, and the reason is structural. The walker already moves backwards through real
history, so "more history" is not a new lever. Deduplication by code hash holds at sixty to ninety
times in **any** window, so continuing the sweep mostly rediscovers known codes: **a sweep over thirty
thousand addresses found four new unique codes** in a long run.

The real lever turned out to be different: decouple the dictionary's growth from collection speed
entirely by walking **the whole** public ABI registry mechanically, rather than reactively checking
gaps a pilot happened to find.

Checked empirically **before** committing to it: the registry's whole tree in one request — twenty
thousand entries, not truncated, **1348 unique verified code hashes**.

The full walk ran: about 7677 source files fetched from a raw file host rather than through an API
that would have rate-limited it, filtered down to 2016 files carrying a get-method marker, and run
through this workspace's real extractors:

| Language | Succeeded of total | New methods |
|---|---|---|
| FunC | 1185 of 1207 | **+874** |
| Tolk | 17 of 100 | the compiler is strict about resolving imports |
| Tact | 137 of 709 | stricter still |

**The dictionary went from 219 to 1388**, six times over. Live re-probe validation moved recognition
from 75.2% to **86.2%**, and convergence was confirmed by **three consecutive runs** giving the same
number.

The network sweep was stopped early: the registry walk proved an order of magnitude more productive
per unit of effort.

**The engineering cost**: the project discovery was flat, one level deep, and was extended to a
recursive walk with a regression test proving the old flat labels do not change.

**A mistake caught before it was committed**: 137 files from the registry first landed straight in the
directory the benchmark uses as its Tier 1 accuracy baseline. Contaminating a calibration corpus with
material extracted by the same tool would have made the measurement meaningless. They were moved into
a separate root and wired in as a second independent source; a live run after the fix confirmed full
completeness and no regression.

**Traceability** for twelve hundred files is not hand-written prose but a machine-readable manifest in
every new directory: code hash, verifier, original path, extracted methods. The method sits beside it.

---

## 4. Two real defects in library-cell resolution

The largest single finding of this whole direction, and it was **not** about the dictionary.

**38.6% of "unrecognised" contracts were not a dictionary gap** but unresolved library-reference
cells: a cell points at code stored separately in the network, and until it is resolved the bytecode
looks unrecognisable.

Two defects in the resolution itself: it went around a stale cache, and it did not retry. One fix
moved the coverage picture further than a month of growing the dictionary.

**The lesson, recorded explicitly**: before explaining low coverage by a lack of data, check that what
is being measured is what it appears to be. The same class of error as the precompiled-contract check
that matched the wrong log ([`emulator.md`](./emulator.md) §8).

---

## 5. Re-probing exists because a gap was found, not because it was designed

A real gap: a dictionary that grew **after** a code was first probed never got a chance at that code,
because an already-probed code was not re-examined. So a separate dictionary rebuild could double the
dictionary without moving a single coverage figure.

Re-probing re-examines previously unrecognised codes and leaves recognised ones alone.

---

## 6. The walk's storage

A SQLite database holds addresses, code hashes, probe outcomes and the walk position for resuming.

Deduplication happens **before** the fetch: an address whose code hash is already known is not
downloaded at all. That is where the sixty-to-ninety-fold saving comes from, and it is the real limit
on speed rather than the API's throughput.

The coverage report prints aggregate coverage plus the top candidates for extending the dictionary — a
code unrecognised at many different addresses, sorted by how many. That is the working loop: the
report names the gaps, the rebuild closes them, the re-probe measures the effect.

---

## 7. The remaining share

Read rather than written off:

- **Multi-file messaging infrastructure** — a structure too complex for today's single-file heuristic
  extractor. The largest of the three pieces.
- **A recognised project with no compilable source** — the source exists and does not build in
  isolation.
- **Singly-encountered contracts** — a long tail with nothing to extract from.

The source of growth that worked is **exhausted**: the registry has been walked completely. Three
hypotheses for further growth and their cost are [`OPEN-QUESTIONS.md`](../OPEN-QUESTIONS.md) §4. The
cheapest check — the share of the tail whose source exists **nowhere** — has not been done.

---

## 8. Which compiler wrote the chain

The language perimeter is a decision, and a decision without a measurement is a preference. One
command counts Tact over the same unique code hashes §2 counts:

```sh
abi-layer tact-share --db coverage-data/pilot.sqlite3
```

| Verdict | Codes | Read as |
|---|---|---|
| declares itself Tact | 6 | two of the compiler's markers, or three |
| one marker | 7 | one marker is not resolved by guessing |
| no marker | 376 | Tact compiled with its defaults lands here too |
| no data cell | 2 | the first-seen address no longer resolves, so no probe ran |

**Between 1.5% and 3.3% of the 389 unique code hashes probed declare themselves Tact.** The band is
the ambiguity of a single marker, not a measurement error.

The markers are `supported_interfaces`, `get_abi_ipfs` and `lazy_deployment_completed`. Two together
are a compiler's fingerprint; one is not, and the corpus says why: of the 46 files carrying
`supported_interfaces`, 2 are hand-written FunC, `whales-nominator-pool-get.fc` among them.

A marker counts as present when the call returns cleanly, and absent on any other exit code. That
rule is stricter than the dictionary probe's, and the three markers are why: each returns constants
and reads no storage, so on a contract Tact emitted none of them can fail. The looser rule was tried
first and was not usable — 324 of 389 codes answer the dispatch-table miss, 56 answer a wallet's own
`THROWIF 32` after comparing the id against `seqno` and `get_public_key`, and counting those 56 as
unclear turned a 3.3% upper end into 18.3%.

**The floor is measured; the ceiling is not measurable this way.** Tact 1.4.1 (2024-07-26) turned
`supported_interfaces` and `get_abi_ipfs` into per-project options, and 1.6.0 (2025-02-28) stopped
emitting `lazy_deployment_completed` by default. A contract compiled since then with the defaults
carries no marker and is counted in the 376. Bytecode leaves no other Tact fingerprint, so bounding
the real share from above needs source rather than a probe.

**The freeze rests on both halves.** `sting-abi-tact` stays at compiler 1.6.13 — the version whose
own install prints a deprecation notice pointing at Tolk, which is the argument that does not depend
on any share. The measurement is what says the declared population is small rather than assumed to
be. Its tests run and its extractor is not developed further. FunC stays for the opposite reason: it
is legacy as a language and not as a chain.

The numbers are recorded in [`benches/baseline.toml`](../../benches/baseline.toml) with the command
that produced them.

**What the measurement does not say.** It does not give Tact's share of addresses: deduplication
runs sixty to ninety times, so the distribution over addresses is a different number. It does not
name a Tact version. It separates neither FunC from Tolk, since neither leaves a fingerprint.

---

## 9. Why this is the candidate for the first public win

Three properties the rest of the project does not have:

- **The artefact is ready and measured.** The dictionary is a file rebuilt by one command, with a
  machine-readable provenance for every entry.
- **It needs no infrastructure.** A dataset does not have to be hosted, monitored and kept up, unlike
  an HTTP service.
- **The niche is taken by nobody.** Ethereum's equivalent is basic infrastructure that explorers and
  wallets lean on. TON has none.

The only thing missing is somewhere to put it.

**The caveat that has to be published with the data**: 86.2% is the share of **unique code hashes**
met by one walk on one date, not the share of the network's contracts and not the share of addresses.
Deduplication at sixty to ninety times means the distribution over addresses is different, and
presenting one number as the other would be a substitution.

---

## 10. What the dataset says about itself, and what says it is true

A file that leaves this repository answers three questions its reader cannot ask anybody: where the
data came from, what may be done with it, and whether the file in hand is the file this corpus
produces. Until this milestone it answered none of them.

| Field | Value |
|---|---|
| `schema_version` | `1.1` |
| `licence` | `CC0-1.0` |
| `built_by` | `abi-layer dictionary-build` |
| `sources` | six corpora, each with its own `SOURCE.md` |
| entries without provenance | 0 of 1 433 |

Provenance is recorded twice, because one level of it is useless on its own. Every entry names the
contracts it was seen in; every corpus names where its contracts came from. "Seen in
`counter/Counter.tolk`" means nothing to a reader who does not have this checkout, and a corpus whose
own `SOURCE.md` is missing now fails the build rather than producing a dataset that points at
nothing.

**The rebuild is the check.** `abi-layer dictionary-verify` rebuilds into a scratch file and compares
byte for byte: 706 060 bytes, reproduced. That took making the artefact ordered — a hash map
serialises in whatever order it iterates, and two builds of one corpus produced two files that meant
the same thing and differed in a way nobody could review. Ordering now happens inside the write, not
in a step a caller has to remember.

**The audit is the other check, and it answers a different question.** The rebuild says the file is
what this corpus produces; it says nothing about whether the corpus still says what the file claims.
`abi-layer dictionary-audit` recompiles a sample and compares it against tolk's own `abi.json`, read
from the compiler's artefact rather than through the extractor that built the dictionary: **100 of
the 115 compiler-extracted claims, across 40 contracts, diverge by 0.0%**, against a threshold of 1%.
An id moved by hand makes it report 5.0% and exit non-zero.

It says nothing about the Tact and `FunC` entries, which are the larger part of the dataset. Their
ids come from the same one-way formula, and there is no compiler artefact to check them against.

---

## 11. The difference from the foundation's catalogue

`ton-blockchain/abis` is a hand-written catalogue of described contracts. This dataset is a machine
walk of what is deployed. Which one carries what was a claim until it was counted, and a claim of
that shape flatters whoever makes it — only one direction of it ever gets stated.

Both are stated. Against commit `90c4c6b`, read on 2026-08-23:

| Measure | Value |
|---|---|
| their get-method names, across 285 type files | 600 |
| ours | 1 405 |
| in both | 283 |
| only here | 1 122 |
| only there | **317** |
| share of their catalogue this dataset carries | **47.2%** |
| share of this dataset their catalogue carries | 20.1% |

The 317 is the number that does not flatter this repository, and it is the one worth reading first:
a hand-written catalogue describes contracts a walk of deployed code does not reach, and no amount of
walking replaces it.

Their 749 test and helper files are left out of that 600 deliberately. Their test cases are written
as get-methods, and counting them would credit the catalogue with names no contract on chain answers
to — inflating the side this repository is measured against is as dishonest as deflating it.

Opcodes are reported and not compared: their catalogue declares 1 296 of them, and the opcodes this
tree knows are the standard ones in `sting-tep` rather than a learned dataset.

`abi-layer catalogue-fetch` rebuilds their two lists from a clone, reading the commit out of the
checkout rather than taking it on trust and stating the read date instead of reading a clock, so the
same checkout produces the same bytes. The lists are committed as a fixture because a comparison
whose input can change under it is a comparison nobody can reproduce.

**Where the dataset goes is still open**, and that is [`DECISIONS.md`](../DECISIONS.md) §8 rather
than an oversight: publication is the one step that cannot be undone.

---

## 12. What this document does not cover

It does not describe how a schema is extracted from source — that is
[`extractors.md`](./extractors.md) — and it does not describe the emulator it calls, which is
[`emulator.md`](./emulator.md). It describes one capability, the numbers that say how far it reaches,
and what the dataset it produces does not cover.
