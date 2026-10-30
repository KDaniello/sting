# Landscape

Who else works in this niche, on what, and at what pace. Measured from primary sources rather than
retold from articles: dates and numbers from the repositories themselves, taken on 2026-08-05 and
re-checked on 2026-08-21.

The document exists because a previous edition of this documentation rested on a picture of the
competition from mid-2026, and that picture stopped being true in three places out of four within
weeks.

---

## 1. The chain changed hands, and it has already produced tooling

A seven-step public roadmap for the network has four steps closed: a consensus upgrade taking blocks
to roughly 400 ms with sub-second finality, a sixfold fee cut, a change of stewardship under which
the messenger became the largest validator, and the renaming of the token.

The practical consequence for us is in the third step's own wording: new developer tools on a two to
three week cadence. That is not marketing: over 2026 the official toolchain grew into a monorepo of
around forty Rust crates including a Tolk linter, an emulator, an indexer, an ABI catalogue and a
debugger.

**What it means strategically.** The question of whether there is demand for Tolk tooling at all got
an indirect but strong answer: the platform's owner is spending money and people on it rather than
declarations. The niche is alive. The bad news is the same sentence: it is alive **and taken**.

---

## 2. Release pace — measured, not estimated

**The language**: thirteen releases across seventeen months, about one every six weeks, with the
current one at 1.4.2 built in mid-2026.

**The node**: monthly, with releases through v2026.08 on 2026-08-17.

**The toolchain**: a 1.0 and a 1.1 within two weeks of each other, then a rolling trunk.

The planning consequence: **the window between language releases is about six weeks**. Any milestone
longer than a quarter has to contain a step that catches the language up, or it ends on a compiler
that has moved.

---

## 3. The official Tolk linter

The most important finding. Started in early 2026 with a commit describing a very basic linter, and
carrying about a hundred commits since. Its licence is compatible with ours.

**39 rules**, counted from its own documentation on 2026-08-21.

### 3.1 Five rules overlap with ours directly

| Their rule | Our detector | Comment |
|---|---|---|
| divide before multiply | **PL** | the same defect, theirs through dataflow |
| random requires initialisation | **EPS** | the same defect |
| unauthorised access | **AC** | the same defect — read line by line in §3.2 |
| no bounce handler | **UBM** | the same defect |
| deprecated symbol use | **DEPR** | the same defect, and duplicated again in the language server |

The rest are style, readability, safety-through-a-comment for the constructs that deserve one, and
error discipline. Their class of problem is wider than ours; their depth on security is narrower.

### 3.2 Their unauthorised-access rule, read line by line

Read as an implementation rather than as a description. It is an honest forward must-analysis: the
bottom element is "checked", the merge is a conjunction, and the transfer raises the flag on an
assertion node carrying an admin-sender check.

Its scope, by their own documentation:

- a preview rule, allowed by default — that is, off;
- the entry point only;
- **exactly one** recognised form of the check;
- no automatic fix.

**By coverage that is our own detector in its first version.** Ours carries a dictionary of markers,
alias tracking, delegation into free functions and methods, and cross-file resolution — and its
precision is still what the baseline records. Theirs has one form and no published figure at all.

The conclusion has to be held in both halves at once: **their rule is weaker than ours in coverage and
stronger in foundation.**

### 3.3 The foundation is where they are genuinely ahead

Their linter stands on a syntax crate, a **name resolver**, a **type system**, an analysis crate and a
dataflow crate. That is tree-sitter, as we have, **plus name resolution and types**.

That layer is exactly what we do not have, and exactly what [`detectors.md`](./detectors.md) records
as the ceiling of our access-control detector. It cannot know a receiver's real type from a syntax
tree, which has no types, nor from a flat table of signatures, which is not a table of variable
types. The limitation is therefore not absolute but **ours**: the neighbour has the layer.

Hence the decision to build a semantic layer of our own.

### 3.4 Their crates cannot simply be taken as a dependency

The resolver, the type system, the dataflow crate and the linter are all unpublished. The options are
vendoring, a fork, or a path dependency into their tree. The licence permits it and it would tie us
to their release pace and their internal APIs with no promise of stability.

The decision taken: **our own semantic layer, their architecture as the reference.** Their split into
syntax, resolver, types, dataflow and linter is right and worth repeating; pulling their code into our
dependencies is not.

---

## 4. The toolchain as a whole

Around forty crates, and half the list overlaps with what this project built for itself:

| Their crate | Our counterpart | Who is ahead |
|---|---|---|
| the compiler, syntax, resolver, type system | our wrapper over the compiler binary | **theirs** — they own the front end, we parse its artefacts |
| dataflow | `sting-dataflow` | parity |
| the linter | `sting-detectors` | theirs on foundation, ours on security coverage |
| the emulator and executor | `sting-emulator` | parity — **but their linter does not use theirs** |
| the ABI catalogue | the extraction crates | see §5 |
| indexer and liteserver crates | — | theirs; we have none |
| the language server | — | theirs |
| the IDE and debugger | — | theirs |
| — | **dynamic confirmation of a finding** | **ours, alone** |
| — | **a calibrated accuracy benchmark** | **ours, alone** |
| — | **recognition with no source at all** | **ours** |

The last three rows are what is left ours.

---

## 5. ABI: "nobody has closed the niche" is no longer literally true

Three events in a row: the language began emitting an ABI of its own; an official curated catalogue
appeared; and a crate inside the toolchain now works with that catalogue.

The catalogue's growth is itself the measurement that matters here:

| Measured | Contracts | Code hashes | Opcode prefixes | Get-method names |
|---|---|---|---|---|
| 2026-08-05 | 273 | 322 | 1253 | 575 |
| 2026-08-21 | 283 | 332 | 1299 | 600 |

**What that does not cancel.** Their catalogue is **curated** — hashes entered by hand, and the table
above shows the pace: ten in two and a half weeks. Ours is **automatic recognition**: 1388 method ids
in the dictionary assembled by machine, 86.2% of unique mainnet codes, including contracts that are in
no registry because nobody has their source.

This is complement rather than competition, and the dictionary has to be presented that way or it
reads as an attempt to out-argue the official registry. The honest phrasing: *the official catalogue
says what a contract is when somebody entered it; our dictionary says what a contract is when nobody
did.*

---

## 6. Two competitors that can be struck off

**The analyser that previous documentation treated as the main reference** last released in mid-2025,
its repository has not moved since early 2026, and a search of its code returns **zero** mentions of
Tolk. The project appears to have stopped, and it never gained the language — exactly what an issue
open since 2024 had been asking for.

What was borrowed from it stays useful — one detector is a direct port, and so is the suppression
mechanism that requires a reason — but as a competitor it is gone.

**The old editor extension** is archived and moved into the language server. That server is not a
competitor, but one of its Tolk inspections duplicates our deprecation rule, which is therefore now
duplicated twice over: in the language server and in the official linter.

---

## 7. The technical debt this survey exposed

**The grammar is two language releases behind.** The published grammar crate dates from January 2026;
its last language work was support for a construct from version 1.2, and the repository has not moved
since.

The compiler is on 1.4.2. So the syntax of two language versions is absent from the grammar, and the
entire static contour stands on it.

How badly that breaks parsing was **not measured** when this document was first written, and could not
be asserted without a measurement. It has been measured since: 54 of 297 corpus files carry an error
node, and five of twelve constructs of the live language do not parse. The measurement is in
[`../PLAN.md`](../PLAN.md) §0.3, and closing it is gate O.

**The vendored compiler is current** at 1.4.2, and its standard library was updated with it.

---

## 8. What is left ours

Three things nobody else has, verified from code and from the absence of publications:

1. **Dynamic confirmation of a finding by running it in a real virtual machine.** The official
   toolchain has an emulator and its linter does not use it. Nobody reproduces a static finding by
   execution and hands over the artefact of that reproduction.
2. **Calibrated accuracy.** The official linter publishes **no** precision or recall figure. No public
   security benchmark for TON exists at all — though they need one too.
3. **Recognising a contract that is in no registry.**

Plus the risk signals, where the overlap is zero.

---

## 9. Sources

Everything above comes from primary sources, taken on 2026-08-05 and re-checked on 2026-08-21:

- the official toolchain repository — the rule tree, commit history, manifests, and the implementation
  of the unauthorised-access rule
- the node repository's releases — the dates of language and node releases
- the ABI catalogue repository — its README and the size of its tree
- the language server repository — its inspection list
- the grammar repository and the crate registry — how current the grammar is
- the stopped analyser's repository — release dates and a code search
- the public roadmap and its coverage — the steps and their dates

---

## 10. What this document does not cover

It does not compare quality, only coverage and pace: a claim about whose analysis is better needs one
corpus and one method, and building that is a milestone of its own rather than a paragraph here. And
it does not stay true by itself — every number in it carries the date it was taken, because this is
the fastest-moving document in the set.
