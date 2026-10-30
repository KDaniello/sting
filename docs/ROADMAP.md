# Roadmap

Living document. It splits the work into milestones, and each milestone ends with a **gate**: a
threshold a machine checks, which either passes or does not. A milestone without such a gate has no
completion criterion and is not planned.

Three rules shape the whole plan, and they follow from [`PLAN.md`](./PLAN.md) §0.3 — the three
divergences the plan was derived from:

1. **A gate is absolute or it is not a gate.** Every row of every gate table names a number a
   command produces. "The report is clear", "the API is convenient", "a buyer appeared" are
   outcomes, not gates: the first two cannot be settled in advance, the third is not ours to decide.
   Both kinds are planned — outcomes live in §16 under a threshold registered before the attempt.
2. **The core catches up with the chain before anything is merged onto it.** A parser blind to 18.2%
   of the corpus multiplies that blindness across every crate merged onto it.
3. **When in doubt, cut.** [`PLAN.md`](./PLAN.md) §8 exists to be cited, and the answer to "should we
   also…" is almost always no.

Neighbouring documents: [`PLAN.md`](./PLAN.md) — the five gates and the product registry ·
[`ARCHITECTURE.md`](./ARCHITECTURE.md) · [`API.md`](./API.md) · [`TESTING.md`](./TESTING.md) ·
[`POSITION.md`](./POSITION.md).

This document replaces the phase-based roadmap that covered the security branch alone. That roadmap
described one of four contours, was written before `karman`, `sybil` and `tonomics` arrived in the
tree, and narrated development chronology — which belongs in the git history and not here.

---

## 1. Overview

| Milestone | Theme | Gate |
|---|---|---|
| **M0** | Documentation discipline | English canonical with an identical Russian mirror, every number in the documents derived from code, `make pr` green |
| **M0.1** | Comments carry the standard | project meta, dead document paths and Russian prose in the comments all reach 0, and `make lint` holds them there |
| **M1** | The parser sees the live language | 297 of 297 corpus files parse without an `ERROR` node, and a file that does not parse exits non-zero |
| **M2** | The core knows the chain version | `sting doctor` names four versions and refuses an unknown one; 14 detectors ruled against gv12–gv15 |
| **M3** | One path to the chain | five chain clients become one; every stored snapshot reproduces byte for byte |
| **M4** | One tree | `cargo test --workspace` runs 1 528 tests in one command; `exclude` holds one directory, and it is the one that must stay out |
| **M5** | The inventory | 37 capabilities, eight fields each; every offline command runs under `make pr` and every live one under `make test-live` |
| **M6** | The dataset | 47.2% of the foundation's catalogue and 20.1% of this dataset overlap; 706 060 bytes rebuild byte-identical |
| **M7** | Decode API | one surface, three limits, keys and quotas; `Unknown` 26.4% of a 10 000-transaction sample against a 13.8% gate, p95 2.4 ms |
| **M8** | Sybil in production | ingestion reproduces a snapshot byte for byte; retro precision against a threshold registered beforehand |
| **M9** | The benchmark | the numbers unchanged after the parser caught up; the neighbour measured by the same code at 87.1% against our 81.5% |
| **M10** | The remaining products | no gate of its own — see §16 |
| **M11** | The hunt | one entry in eighteen of the published scope is readable at all; precision 20.0% on live code against 82.4% on fixtures |
| **M12** | The second language | reach from 1 of 18 scope entries to 12 and from 49 files to 170; `LEP` reads both, Tolk unchanged to the digit |
| **M13** | Rules for the code we can now read | `UR` and `ID` cross; a detector wrong more often than right on live FunC is switched off for the language |

This table is an order, not a schedule. A milestone is done when its gate says so and not when a date
says so, and no work in this document is dated. A plan that promises a week either shapes the work to
fit the week or is quietly abandoned, and both are worse than a plan with no dates in it.

Nothing is published outside the repository before M6, and there is no remote to publish to until
the platform stands ([`DECISIONS.md`](./DECISIONS.md) §1). Every gate is therefore checked by
`make pr` rather than by a CI service; the workflow files are kept in step with the Makefile so that
connecting a remote later costs one push. The first thing to leave the repository is the dataset,
and it leaves only after §9's gate says what it is that the foundation's catalogue is not, and after
[`DECISIONS.md`](./DECISIONS.md) §8 says where it lives.

---

## 2. M0 — Documentation discipline

The documentation is rewritten to the discipline of the reference project: English canonical, an
identical Russian mirror, declarative rather than procedural, tables carrying decisions and prose
carrying reasons, and every number in a document derived from the code rather than typed by hand.

**What was measured when this was written**, by `make docs-check`, which is the rule set itself
rather than a description of it: 31 markdown files in `docs/`, 588 218 bytes. 27 of them had no
mirror. 34 links pointed into documents the repository has retired. 3 emoji, 4 filler phrases, 75
references to milestone names and development chronology. 181 tracked `.rs` files carried Russian
prose in their comments, and 61 more under `sybil/` wait for the merge. Heading trees and relative links were clean, and stay clean by the same
check.

The `docs-numbers` guard was red for a different reason: it watched `PRECISION_REPORT.md` under
`docs/security/`, and that file no longer exists. It was replaced rather than repaired.

**What the same command measures now**: eleven rules, ten of them at zero, and Russian prose in
sources — 120 files under `crates/` — held for the milestone that owns it. The rule count grew during the milestone
rather than after it: three rules were added because the work found what nothing was watching. A
plain path into a retired document is not a link and was invisible; a document that never says what
it leaves out passes every other rule; and two crates copy the workspace lint table by hand, since
`forbid` cannot be inherited and relaxed.

**Scope**

- **The target document set**, decided before anything is translated:

  | Document | What it answers |
  |---|---|
  | `README.md` | what the platform is, and which document answers what |
  | `ARCHITECTURE.md` | contours, layers, the numbered invariants, the code rules |
  | `API.md` | every contract that crosses a boundary |
  | `CAPABILITIES.md` | the inventory — one row per capability, created in M5 |
  | `DATA.md` | corpora, datasets, provenance |
  | `TESTING.md` | the test pyramid, the gates, what proves what |
  | `POSITION.md` | the landscape and what we are against it |
  | `PLAN.md` | the five gates and the product registry |
  | `ROADMAP.md` | this document |
  | `GLOSSARY.md`, `OPEN-QUESTIONS.md`, `DECISIONS.md` | terms, research questions, decisions awaiting the owner |
  | `domains/`, `audit/` | the deep dives and the audit method |
  | `repo/ci.md`, `repo/release.md` | what CI runs, how a release is cut |

- **What leaves the index and stays on disk.** `docs/phases_roadmap/` — 11 files, 251 133 bytes,
  procedural by construction — and the root `PLAN.md`, 57 039 bytes of session tracker whose every
  line is chronology. Both go into `.gitignore` and out of the index with `git rm --cached`
  ([`DECISIONS.md`](./DECISIONS.md) §3): the text stays on disk and in `git log`, and the repository
  stops carrying documents that contradict the live ones. This removes a third of the translation
  burden before it is paid.
- **`AGENTS.md` and `CLAUDE.md`** at the root: rule zero on authorship, the commit format, the
  comment rule, the rules that get a change rejected, and the index into `docs/`
  ([`DECISIONS.md`](./DECISIONS.md) §2).
- **`Makefile`** with `test-fast`, `test`, `lint`, `pr` — the same four verbs as the reference
  project, so that "what CI runs" is one command rather than a page.
- **Section numbering and cross-references** with a stable number: ``[`API.md`](./API.md) §2.3``.
  Renumbering a section means fixing every reference to it in the same commit.
- **`ARCHITECTURE.md` §1 becomes the numbered invariants**, each stated as a prohibition and each
  citable from source by number.
- **English canonical, `docs/ru/` an identical mirror**, `domains/` and `audit/` included
  ([`DECISIONS.md`](./DECISIONS.md) §4). Both sides change in the same commit, and `make pr` compares
  the heading trees and fails when they diverge.
- **`scripts/check-docs.py`** carries the mechanical rules themselves rather than a description
  of them: mirrors, heading trees, links, retired references and plain paths into them, emoji,
  project meta, filler, a closing section that names what a document leaves out, and Russian prose
  in sources. `scripts/check-lints.py` guards the two lint tables that cannot be inherited. Each rule counts against a baseline that `--update` only ever lowers,
  and the target of every one of them is zero — so the gate is usable on the day it is written
  instead of being red until the milestone ends, and a rule that has been paid for cannot be
  unpaid. Its output is the work list for this milestone.
- **`scripts/doc-numbers`**, rewritten: it derives crate count, test count, detector count,
  dictionary size, corpus coverage and parse share from the code, and fails when a document
  disagrees. The current script guards a deleted file and is replaced rather than repaired.
- **English in code.** 44 469 Cyrillic characters in 119 files under `crates/`, and 48 408 in 61
  files under `sybil/`, become English. Where Cyrillic is test data rather than prose, the file is
  named in an allowlist and the reason is recorded.

**Gate**

| Check | Threshold | Today |
|---|---|---|
| `make pr` from a clean clone | green | green |
| documents without a Russian mirror | 0 | 0 |
| mirror heading trees, `domains/` and `audit/` included | identical | identical |
| broken relative links | 0 | 0 |
| links into retired documents | 0 | 0 |
| plain paths naming a retired document, without the commit that holds it | 0 | 0 |
| emoji in documents | 0 | 0 |
| project meta in describing documents: milestone names, phase numbers, "for now", "later" | 0 | 0 |
| filler and marketing phrases in documents | 0 | 0 |
| documents whose last section does not say what they leave out | 0 | 0 |
| crates whose lint table differs from the workspace by more than one named lint | 0 | 0 |
| numbers in documents | every one produced by `scripts/doc-numbers` | 7 facts derived, 0 claims disagree |
| `docs/phases_roadmap/` and the root `PLAN.md` | out of the index, on disk, in `git log` | done |
| `AGENTS.md`, `CLAUDE.md`, `Makefile` | present; every commit after them matches the stated format | present |
| Russian prose in tracked `.rs` | 0 outside a named allowlist | 0, closed by M0.1 |

**Explicitly not in M0:** translating `tonomics/` documentation — that project is closed in M4;
rewriting `CHANGELOG.md`, which is a history and is allowed to read as one.

---

## 3. M0.1 — The comments carry the standard

M0 puts the documents under the discipline. The comments are the same text, read more often and by
people who never open `docs/`, and they are the part of the repository that has drifted furthest
from it.

**What was measured when this was written.** 194 files under `crates/`, 59 279 lines, of which
15 204 are comments — 25.6% of the tree — including 9 384 `///` lines and 3 516 `//!` lines. 481 of
those comments named a milestone, a block or a phase. 566 cited a document by path and the document
did not exist: 52 distinct paths, `docs/blocks/*`, `docs/critical/*` and `PROJECT_REVIEW.md` among
them, against 207 references that still resolved. 120 files carried 44 359 characters of Russian
prose. 12 decorative separator bars, 1 line of commented-out code.

A comment pointing at a deleted document is worse than no comment: it is believed, and the reader
who goes looking finds nothing and stops trusting the rest of the file.

**What the same command measures now**: zero on every count.

The chronology turned out wider than the first pattern looked for. A session, a risk spike, a
definition of done and "this block" date a comment exactly as a milestone's name does, and 75
comments carried them after the named milestones were gone. The rules catch those phrases now.

**Scope**

- **The rule, unchanged from [`AGENTS.md`](../AGENTS.md).** A comment explains a block, an algorithm
  or a contract — never a line — and says *why*, not *what*. Where the "what" is unclear the names
  and the structure are fixed and the prose deleted.
- **Every project-meta reference goes.** What was a reason stays, stated in the comment itself with
  no plan reference behind it; what was only a pointer at a plan is deleted with it.
- **Every dead document reference is settled.** It is re-pointed at the live document that now
  covers the subject, or the reason it carried is restated in place. The section number goes with
  the path: a section of a retired document means nothing against a live one. A path is kept only if
  it resolves.
- **Every file becomes English.** Russian that is test data rather than prose is named in the
  allowlist with its reason next to it.
- **Separator bars and commented-out lines are deleted, not rewritten.** A quotation of code in
  another language counts as the second: nothing recompiles either.
- **The `///` and `//!` lines are re-read against the rule**: the thesis in the first line, the why
  after it, nothing at all on an obvious accessor.
- **`make lint` gains every check that makes this mechanical.** The rules that hold the documents
  come to hold the code, and the gate below is the list.

**Gate**

| Check | Threshold | Today |
|---|---|---|
| project meta in comments | 0 | 0 |
| a development block named in a doc comment | 0 | 0 |
| document paths cited in comments that do not resolve | 0 | 0 |
| tracked `.rs` under `crates/` carrying Russian prose | 0 outside the named allowlist | 0, two files allowlisted |
| decorative separator bars | 0 | 0 |
| commented-out code | 0 | 0 |
| a precision `sting explain` prints against `corpus/baseline.json` | equal | equal |
| `make lint` | carries every check above | carries every check above |
| `cargo nextest run --workspace` | 901 green, unchanged by a comment pass | 901 |

**Explicitly not in M0.1: adding comments.** The comment count is expected to fall. A comment that
restates the code is deleted rather than translated, and a milestone that measures itself by how much
prose it produced is measuring the wrong thing.

---

## 4. M1 — The parser sees the live language

The security contour reads Tolk through `tree-sitter-tolk` 0.0.5, published 2026-01-17. The vendored
compiler is `tolk` 1.4.2, built 2026-07-15. When this was written, 54 of 297 corpus files carried an
`ERROR` node and `sting lint` reported every one of them as clean with exit code 0.

**All 297 parse now**, and the compiler reads nothing the grammar refuses.

**Scope**

- **A file that is not parsed stops looking like a file without findings.** `sting lint` emits a
  diagnostic naming the position of the first `ERROR` or `MISSING` node and exits non-zero. This is
  the cheapest step and it comes first: everything else in this milestone is measured against it.
- **The blindness gets a number in CI.** A command reports the share of corpus files that parse
  without an `ERROR` node — 81.8% when this was written — and a test fails when the share falls. It
  is a test rather than a report so that a decline fails a run instead of appearing in one.
- **The compiler becomes the oracle.** The vendored `tolk` 1.4.2 runs over the corpus, and every file
  the compiler accepts and the grammar refuses is listed by name. CI already fetches this exact
  compiler for two other jobs, so the oracle costs a job and not a toolchain.
- **The constructs close, and the oracle names them rather than a guess.** This document named five
  from a reading of the changelog and got three of them wrong: `??` already parsed, and neither
  `int[]` nor the lambda form written here is Tolk syntax at all — the compiler refuses both. What
  the oracle found instead is in
  [`vendor/tree-sitter-tolk/README.md`](../vendor/tree-sitter-tolk/README.md): the `contract { }`
  block at 36 files, a dotted `@abi.clientType` at 4, an annotation on a struct field at 4, a type as
  an annotation argument at 1, and `;` between enum members at 3. The grammar is patched, vendored
  the way `tree-sitter-func` already is, and the patch is offered upstream. Vendoring is not a fork:
  the oracle keeps any divergence visible whether or not upstream accepts.
- **The text patch is deleted.** `extract_contract_headers` and its tests exist because the grammar
  had no `contract` rule; they go in the same commit that gives it one. Detectors that attributed a
  function to "the nearest preceding contract header" read a real node instead.
- **The detector corpus is re-measured.** Findings before and after are diffed, and every new finding
  is classified true or false positive by hand. This number feeds M9 and is the reason M9 cannot
  precede M1. **Nothing moved**: every per-detector figure and both aggregates match
  `corpus/baseline.json` exactly. The blindness was real for reporting, where a half-read file looked
  clean, and the text scan had been covering attribution.

**Gate**

| Check | Threshold | Today |
|---|---|---|
| corpus files parsing without an `ERROR` node | 297 of 297 | 297 |
| files the compiler reads and the grammar refuses | 0, or each named with the reason | 0 |
| files the grammar reads and the compiler refuses | 0, or each named with the reason | 0 |
| `sting lint` on a file with `ERROR` nodes | non-zero exit, and the position named in the output | both |
| `extract_contract_headers` | absent from the tree, with its tests | absent |
| detector findings, before against after | diff recorded, every new finding classified TP or FP | no difference |
| the parse-share check | part of `make pr` and failing on a decline | part of it, floor at 100% |

**Explicitly not in M1:** new detectors, decompilation, a language server, a formatter. The grammar
gains what the corpus proves it lacks and nothing else.

---

## 5. M2 — The core knows the chain version

Global versions 12 through 15 changed contract semantics: the bounce format carries `extra_flags`,
`SENDMSG` stopped taking a user-supplied `fwd_fee` as a lower bound, library publishing is
restricted to governance, action fines are collected when the phase fails. Nothing in the tree
mentions any of it, and no run states which version it emulated.

**Scope**

- **`sting doctor`**, one command naming four versions: `libemulator` as it reports itself, the
  global version the emulation pins, the grammar, and the vendored compiler. It exits non-zero when
  any of them is unknown or unpinned. The emulator already parses and caches its own version string,
  so this is a report over data that exists rather than new plumbing.
- **A run on an unpinned global version refuses to start**, naming the expected version and the found
  one. Emulating semantics without saying which semantics is the same defect as parsing a file
  without saying it failed.
- **The 14 detectors are ruled against gv12–gv15**, one row each: changed, unchanged, or blocked on
  data, with the reason. Three verdicts are expected in advance and are not permitted to be
  "unchanged" without an argument: UBM against the gv12 bounce format, the send-shaped detectors
  against gv14, and anything reading libraries against gv15.
- **The language perimeter is declared** with the measurement behind it: the share of Tact among
  unique mainnet code hashes is measured, and the freeze of `sting-abi-tact` at compiler 1.6.13 is
  recorded with that number next to it. FunC stays for the chain, not for the language.

**Gate**

| Check | Threshold | Today |
|---|---|---|
| `sting doctor` | reports four versions, exits non-zero on any unknown one | four, and it refuses |
| a run against an unpinned global version | refuses, naming expected and found | refuses |
| gv12–gv15 against the detectors | 14 rows, every row with a verdict and a reason | 14 rows |
| the three expected verdicts | none of them "unchanged" without a stated argument | `UBM` changed twice, the other two argued |
| Tact share among mainnet code hashes | measured, recorded, cited by the freeze decision | 1.5% to 3.3% of 389 declare themselves |
| `libemulator` version | pinned in the manifest and asserted at load | commit `3f598aa2`, refused otherwise |

**Explicitly not in M2:** upgrading the emulator to the newest node release for its own sake. The
pin is what matters; moving it is a step with its own before-and-after measurement.

---

## 6. M3 — One path to the chain

Chain access exists five times: `sting` over `ureq`, `sting-karman-data` over toncenter v3 and tonapi,
`sybil/ingestion` over toncenter with ClickHouse and `tokio`, `tonomics-ton` over its own client,
and `tools/ton-res` over pytoniq and a liteserver. Five sets of retries, five address models.

**Scope**

- **`sting-chain`**: one client. A synchronous core, because four of the five callers are
  synchronous, with a thin asynchronous adapter above it for the one that is not. One retry and
  backoff policy, one rate limiter, one cache, one place where a `retry-after` header is honoured.
- **`sting-address`**: one address model, extracted from `sting-core` and `sybil`'s `common` — which
  is what that crate mostly is, and `common` is a module name the code rules forbid outright
  ([`DECISIONS.md`](./DECISIONS.md) §5). The compile-fail test that forbids passing an owner where a
  jetton wallet is expected moves to the shared crate and keeps failing to compile.
- **Decoding stops existing twice.** `sybil/graph/decode.rs` unpacks jetton transfers by hand;
  `sting-abi-core` unpacks an arbitrary operation against the dictionary. The second absorbs the
  first, and the graph over the same input produces the same edge set.
- **`sting-karman-data` calls the shared client**, and the market census reproduces from the new path.
  **Byte for byte holds for three of the four stages and cannot hold for the fourth**: the item
  service answers two identical requests with the same data and a different key order, measured on
  2026-08-22 by asking it twice. What the fourth stage reproduces is the value, ten items of ten
  equal after parsing. A gate is reworded to what is measurable rather than dropped ([§18](#18-what-would-change-the-plan)).
- **`tools/ton-res` stays Python and stays a tool.** It runs a few times a year, and its value is a
  proven state through a liteserver rather than throughput. It is the one of the five that does not
  merge, and the reason is recorded rather than implied.

**Gate**

| Check | Threshold | Today |
|---|---|---|
| crates naming an HTTP client, toncenter, tonapi or a liteserver | exactly one, plus `tools/ton-res`; `make lint` greps | 1 under `crates/`, 2 held in the closed project |
| the `sybil` ingestion snapshot | byte-identical from the new client | the snapshot test passes unchanged |
| the `karman` market census | identical from the new client, value for value; byte for byte where the service allows it | 3 stages of 4 byte for byte, the fourth equal after parsing |
| the graph over a fixed input | the same edge set before and after the decoding change | its acceptance test against a real captured message passes unchanged |
| the owner-versus-jetton-wallet compile-fail test | fails to compile, from the shared crate | fails to compile, in `sting-address` |
| retry and rate-limit behaviour | one implementation, tested against a stubbed server, including `retry-after` | 8 tests on a socket, `retry-after` among them |

**Explicitly not in M3:** an indexer of our own. Three public indexers diverge from the chain by
0.00% on values, measured over 160 objects. The task is solved.

---

## 7. M4 — One tree

Four workspaces become one. The cost is not the move — it is the reconciliation the move forces.

**Scope**

- **Every crate takes the `sting-` prefix**, by the mapping fixed in [`DECISIONS.md`](./DECISIONS.md)
  §5 — one commit per domain, and the mapping is not re-litigated afterwards. `sybil`'s crates cannot
  enter a shared workspace under their present names at all: `common`, `graph`, `features`, `cluster`
  say nothing about whose they are.
- **`karman-reference` does not come.** The Morpho Blue reference on `revm` stays in the standalone
  `karman` project ([`DECISIONS.md`](./DECISIONS.md) §6); the corpus it produces travels as a
  checked-in fixture, and the differential harness that reads that fixture merges normally. Nothing
  in the tree depends on the crate today, so the boundary costs nothing to hold.
- **Pins are reconciled** by [`PLAN.md`](./PLAN.md) §2.3's table: `thiserror` 2 everywhere, `toml`
  1.1, one generation of `rand`, one `pyo3`. The generator matters most: `karman` draws from
  `rand_chacha` 0.3 and `tonomics` from `rand` 0.10, and a different generation is a different seed
  semantics, which is a different determinism.
- **One lint regime.** Every merged crate inherits `[lints] workspace = true`. Exceptions are listed
  by name with the reason — `sting-karman-cli`'s counting allocator holds the only five `unsafe` blocks
  outside the emulator binding, and the root forbids `unsafe_code`, which no local `allow` can undo.
- **`sting-karman-core` gets a TVM executor** through `sting-emulator`, the single binding that does not
  leak memory. A mechanism against a real contract runs twice on one seed with the same verdict, and
  that verdict equals the pre-merge one.
- **The gift authenticity rule moves** from the census into `sting-scamcheck` and answers offline,
  because asking a chain 167 times to find out which rows copy somebody else's name is a
  measurement nobody runs twice. Reproducing it found the premise too narrow: the census read two
  popular series and reported 27 rows carrying their names, and over the whole catalogue 8 names
  are claimed by more than one contract across 53 rows. In 7 of those groups exactly one contract
  holds back part of its own supply, which is the series; the other 40 hold none of theirs. One
  group settles nothing — six contracts, not one of them holding any supply — and the rule declines
  rather than picking a winner.
- **`tonomics/` is closed.** Its lending milestone and its `DIAGNOSTICS.md` — where the engine is
  wrong and why — move into `karman` first; then the directory is deleted and its history stays in
  git. If nothing was taken, there was nothing to take and it should have been deleted at once.
- **`exclude` comes down to what genuinely cannot come in**, and every crate arriving in the tree
  arrives with its comments already meeting the standard M0.1 set: 61 files under `sybil` carried
  Russian prose and their project-meta references were unmeasured. The pass happens in the commit
  that moves the crate, never afterwards — a comment nobody re-read on the way in is a comment
  nobody will re-read at all.

**What the merge turned out to cost, against what this section assumed.** Three of its premises
were wrong and are corrected here rather than quietly met.

`exclude` cannot be empty while `karman/` holds the EVM reference, which §6 of
[`DECISIONS.md`](./DECISIONS.md) puts outside the tree deliberately. It names that one directory
and nothing else: `tools/` is Python and was never a member.

Every merged crate inherits the lint table, and inheriting it was not free. The table denies
`unwrap`, `expect` and `panic` outside tests, requires an error section on every fallible public
function, and refuses a cast that can truncate — so `allow` appears where a rule is answered rather
than obeyed: a match arm whose length is an enumeration's, a cast inside a range the code has just
checked, an exact comparison of a value read straight back out of a file, and test scaffolding
compiled into a library. Each is named with its reason at the site, and the count is a fact about
the merge rather than a target: what the gate below now asks is that no `allow` is anonymous.

`anyhow` came out of four libraries rather than staying at the top of a binary, because the tree's
rule says errors are typed per crate. That is the largest single change the merge made to code it
moved, and it is why a caller now learns which field did not parse rather than that ingestion
failed.

**Gate**

| Check | Threshold |
|---|---|
| `cargo test --workspace` | one command, 1 528 tests, green — 1 525 of them under `cargo nextest`, which does not run doc tests |
| `cargo clippy --workspace --all-targets -- -D warnings` | green; every `allow` added during the merge carries a `reason` at the site |
| crate names | every package in the workspace begins with `sting-` |
| comments in the merged crates | project meta 0, unresolved document paths 0, Russian prose 0 outside the allowlist |
| `revm`, `alloy-*` | absent from every manifest in the workspace |
| `exclude` in the root manifest | one entry, `karman`, for the reason [`DECISIONS.md`](./DECISIONS.md) §6 gives |
| chain-access implementations in the workspace | 1 |
| a `karman` mechanism against a real contract, on one seed | the same verdict twice, and the same as the run given that number by hand |
| the authenticity rule | reproduced from `sting-scamcheck` over the census, offline: 9 collections confirmed against the registry, 8 imposters by name, and 40 copies behind 7 contested names without one |
| `tonomics/` | absent from disk; `git log` answers for any of its files |
| the layer invariant | no crate depends upward; `make lint` checks the dependency graph |

**Explicitly not in M4:** merging the run discipline. `karman`'s `findings/` stays its own directory
with its own format — it is a register of claims, not code.

---

## 8. M5 — The inventory

What the platform can do has never been written down in one place. `API.md` describes the three
binaries of one contour; `karman` and `sybil` do not appear in it at all.

**Scope**

- **`CAPABILITIES.md`**, one row per capability, eight fields each: identifier, input, output with
  its schema version, the command, the number that confirms it, the cost of calling it, what it does
  not do, and the layer it sits on ([`ARCHITECTURE.md`](./ARCHITECTURE.md) §1).
- **Every command runs from a clean clone.** Offline capabilities run under `make pr`; the ones that
  need the network, a container or an API key run under `make test-live` and are marked as such in
  the table.
- **Every output has a versioned schema and a golden file.** SARIF 2.1.0, `ContractSchema`, the
  dataset, the run report.
- **The "what it does not do" field is filled for every row**, not for the interesting ones.

**What writing it down cost, and what running it found.** The inventory is 37 capabilities across
five contours and the foundation. Two machines stand behind it: one parses both the document and
its mirror and fails the gate on a missing field, an unknown mode, a layer the architecture does
not place, a structured output with no version, or a command naming a binary no manifest builds;
the other runs the commands.

Running them is what the milestone was actually for. It found the abi layer looking for its
compilers only beside the running executable — from a checkout that is `target/debug/`, so
`abi-layer extract` failed on a file in this repository and on every fresh clone. It found the
schema service starting a server when asked `--help`. It found four commands in the inventory
written wrong, including one pointing at a hypotheses file that did not exist. It found the
clustering chain stated with a range nothing had ingested, and the default column-store address
resolving to an IPv6 loopback nothing was listening on. None of those would have been found by
reading.

Eleven outputs stated no version, and the rule that would have caught it did not exist. It does
now, and they do.

**Gate**

| Check | Threshold |
|---|---|
| capabilities described | 37, every one with all eight fields; a row missing a field fails `make pr`, in the document and in its mirror |
| offline capability commands | 17 run under `make pr` from a clean clone, the two services started, watched and stopped; the costly ones under `make capabilities-full` |
| live capability commands | 10 run under `make test-live`, 2 carry a value the caller supplies; seven of them also driven by ignored tests against mainnet |
| outputs | every structured one states its version, checked by the gate; the report, the membership rows and the score response each carry a recorded example |
| the layer field | is the layer of the crate whose command the capability runs, checked against the same table `make lint` reads |

**Explicitly not in M5:** deciding which capability becomes a product. The inventory answers what we
can do; §16 answers who pays.

---

## 9. M6 — The dataset

The first artefact to leave the repository. It leaves only after the difference between it and the
foundation's catalogue is a number rather than a claim.

**Scope**

- **Provenance per entry**, a schema, a version and a licence. An entry without a recorded origin is
  removed rather than published.
- **A sample of 100 entries is checked against the compiler's own `abi.json`**, which is the ground
  truth for anything compiled from Tolk 1.4.
- **The difference from `ton-blockchain/abis` is measured in both directions**: how many of our
  1 388 identifiers it does not carry, and how many of its 1 299 opcode prefixes and 600 get-method
  names we do not. Both numbers are published, including the unflattering one.
- **The dataset rebuilds from the repository.** The published artefact is reproduced byte for byte by
  a command in the tree, or it is not published.

**What the gate measured.** The dataset carries a schema version, a licence and its build command,
and every one of its 1 433 entries names the contracts it was seen in; each of the six corpora names
where its own contracts came from, and a corpus with no `SOURCE.md` fails the build. The published
artefact rebuilds byte for byte — 706 060 bytes — which took making it ordered: a hash map serialises
in whatever order it iterates, so two builds of one corpus produced two files that meant the same
thing and differed in a way nobody could review.

Two checks answer two questions. `dictionary-verify` asks whether the file a consumer holds is the
file this corpus produces. `dictionary-audit` asks whether the corpus still says what the file
claims: 100 of the 115 compiler-extracted claims, across 40 contracts, recompiled and compared
against tolk's own `abi.json` — **divergence 0.0%** against a threshold of 1%, and an id moved by
hand makes it report 5.0% and fail.

The difference from `ton-blockchain/abis` is counted both ways, at their commit `90c4c6b`: their 600
get-method names across 285 type files against this dataset's 1 405, **283 in both, 1 122 only here,
317 only there** — 47.2% of their catalogue and 20.1% of this dataset. The 317 is the number that
does not flatter this repository. Their 749 test and helper files are left out of the 600, because
their test cases are written as get-methods and counting those would inflate the side this repository
is measured against.

The scope said 1 299 opcode prefixes and 600 get-method names. The names are 600; the opcodes are
1 296, and they are reported rather than compared — the opcodes this tree knows are the standard ones
in `sting-tep`, not a learned dataset.

Publication is the one step not taken, and [`DECISIONS.md`](./DECISIONS.md) §8 is why.

**Gate**

| Check | Threshold |
|---|---|
| sample of 100 entries against the compiler's `abi.json` | divergence at or below 1% |
| entries without recorded provenance | 0 |
| difference from the foundation's catalogue | measured both ways, both numbers recorded |
| overlap with the foundation's catalogue | below 90%, or the dataset is not published and the plan says why |
| the published artefact | rebuilt byte-identical by a command in the repository |
| schema and licence | present and versioned |

**Explicitly not in M6:** a tariff, an account system, a rate limit. Those belong to the API in M7,
and a dataset that waits for them is a dataset that does not ship.

---

## 10. M7 — Decode API

**Scope**

- One HTTP surface: a hash in, an operation out, in words and fields.
- One error shape, documented in [`API.md`](./API.md) and tested, including the shape of "I do not
  know this one".
- Three independent limits — body size, time, and result size — as the service already does.
- Quotas and keys.
- The `Unknown` share measured on a mainnet sample, not on the corpus. The corpus is what the
  dictionary was built from; a number measured there is a statement about ourselves.

**What was built, and what the gate measured.** The surface is `POST /v1/decode`: a transaction hash
or a body in, an operation and its fields out, reading no source and compiling nothing. Behind it are
two datasets rather than one — 374 opcodes learned from this repository's corpora under CC0, and 945
read out of the foundation's own type files through this tree's extractor under their MIT terms,
merged in memory at the moment a question is answered and kept apart on disk because a file whose
licence depends on the entry is a file nobody can use.

Not knowing is an answer: an opcode nothing names comes back `200` with `unknown: true`, and a test
holds that shape in place. A `404` would tell a caller their question was wrong when it was this
platform's knowledge that ran out — and it would hide the very share this milestone is measured by.

**The `Unknown` gate is not met, and the threshold was measuring the wrong thing.** Over 10 000
mainnet transactions: 6 967 carry an operation, 4 331 are named — **37.8% of the operations unnamed,
26.4% of the transactions, 94.2% of the 1 290 distinct opcodes**. The 13.8% came from the method
dictionary's 86.2% recognition of unique mainnet **code hashes**, and recognising a contract is not
knowing what a message sent to it means. Three shares are published rather than the flattering one,
and the twenty most frequent unnamed opcodes are printed as the working list a growth pass starts
from. That pass has not been run.

Naming and reading are two successes: 3 177 of the 4 331 named messages read to the last field, and
the rest stopped at a field whose shape the datasets state no reading for.

The rest of the gate holds. One error shape, tested, including the two nobody writes until they fire
in production — an unknown key and a spent quota, told apart rather than collapsed into one `429`.
Three limits that bound three different things, the third being a result size, over which a request
is refused rather than answered with a cut-off reading. And the load: **8 467 req/s at p95 2.4 ms at
concurrency 16, throughput stops rising at 32**, measured on one machine over a loopback with the
blanket limit raised so that what is timed is the decode rather than the limiter.

**Gate**

| Check | Threshold | Measured |
|---|---|---|
| `Unknown` share on a mainnet sample of at least 10 000 transactions | at or below 13.8% | **26.4% of transactions, 37.8% of operations — not met** |
| p95 latency | measured and recorded | 2.4 ms at concurrency 16 |
| sustained load | measured and recorded, with the point where it degrades named | 8 467 req/s, degrades at concurrency 32 |
| error shape | one shape, documented, tested | one shape, four new codes, nine tests |
| quotas and keys | enforced, tested including the exhausted-quota path | enforced; the unknown key and the spent quota are told apart |

**The unmet threshold stands as written.** It is not restated to fit what was measured: a gate moved
after the run is a gate that measures nothing. What the number says is that 13.8% was a figure about
recognising contracts and this milestone measures reading messages, and what closes the distance is a
growth pass over the opcodes [`domains/decode.md`](./domains/decode.md) §4 names, not a smaller
threshold.

**Explicitly not in M7:** a dashboard, a web interface, a wallet plugin.

---

## 11. M8 — Sybil in production

**Scope**

- ClickHouse in a single-node container, its compose file in the repository next to the tests that need it ([`DECISIONS.md`](./DECISIONS.md) §7); the 18 acceptance tests skipped without it run and stay green.
- Ingestion over a fixed block range reproduces its snapshot byte for byte, twice.
- A retro campaign against an airdrop with a known outcome. **The threshold is committed to the
  repository before the campaign runs**, and the result is published whichever side of it lands.
- The realtime endpoint with quotas, keys and a measured p95.

**What was built, and what the gate measured.** The column store is a compose file in the repository
rather than a host one machine has, and against it **155 sybil tests run green — the 18 that skip
without a store included**. Ingestion over masterchain blocks 87 210 000 to 87 210 003 writes every
row it produced, in identity order: 46 transactions, 60 messages, 50 543 bytes, reproduced three
times, and `make snapshot-check` is that comparison. The endpoint's tiers now bound a period as well
as a burst, and an unknown key, a burst and a spent quota are three answers rather than one `429`.
Measured with the tier limits raised: **9 999 req/s at p95 1.0 ms at concurrency 8**, throughput
stops rising at 16.

Two defects surfaced by running the live gate rather than reading it. The store's acceptance tests
share one database and were racing each other; they are a nextest group now instead of an instruction
in a doc comment. And the live tip test read the clock after its requests, charging the client's own
pacing to the chain — it failed at 16 s against a chain whose lag, sampled twenty times the same day,
had a median of 1 s.

**The retro campaign is not measured, and the reason is not schedule.** The threshold was registered
first, on its own, in commit `3cf08dc`: precision at or above 0.90, recall at or above 0.50, a
thousand owners on each side. `sting-sybil-cluster retro` holds a run against it and cites that
commit. What does not exist is the input: no TON airdrop publishes its exclusion list address by
address. Hamster Kombat banned 2.3 million wallets and published the count; `ton-studio/ton-labels`
carries about 2 485 addresses across 17 categories and none of them is sybil or farmer. A detector
measured against another chain's labels would be measuring another population, and labels made here
and then measured against here are a mirror.

**Gate**

| Check | Threshold | Measured |
|---|---|---|
| the 18 skipped acceptance tests | run and green | 155 sybil tests green, those 18 included |
| ingestion over a fixed range | byte-identical snapshot, twice | 50 543 bytes, three runs |
| the registered threshold | present in git before the campaign, with its commit hash cited in the result | registered in `3cf08dc`, cited by the tool |
| retro precision and recall | measured; above or below, the number is published | **not measured — no TON campaign publishes its exclusion list by address** |
| endpoint p95 and quota enforcement | measured and tested | p95 1.0 ms at concurrency 8; the spent-quota path is tested |

**Explicitly not in M8:** claiming a cluster belongs to one person. The graph shows coordination;
ownership is a statement about people that the graph does not support.

---

## 12. M9 — The benchmark

**Scope**

- Precision and recall re-measured after M1, on a corpus that is now fully parsed. The old numbers —
  96.4% aggregate precision, 100% recall on injections — were measured while 18.2% of the corpus was
  unread, and the difference between old and new is recorded rather than quietly replaced.
- The method and the corpus published so a third party reproduces the number.
- Other analyzers measured by the same method: the linter bundled with the official toolchain, and
  Misti. Both publish zero precision figures today; whoever holds the measure does not compete with
  a linter, it measures one.
- The unflattering numbers published alongside: AC at 32.8% precision, LEP at 32.6%.

**What was re-measured, and what it found.** The benchmark was re-run on the fully-parsed corpus and
**nothing moved**: 411 true positives, 90 false positives, no misses, 82.0% aggregate precision and
96.4% over the default set, identical to the baseline committed at `be97f6a` before the grammar
caught up. That is the finding rather than the absence of one, and its reason is what the missing
constructs were — a `contract { }` block, a dotted annotation, an annotation on a struct field, a
type as an annotation argument, `;` between enum members — none of which is a site any detector looks
at. The two detectors below half precision, AC at 32.8% and LEP at 32.6%, stay in the same table as
the rest.

**The neighbour is measured by the same code.** `acton` 1.1.0, the toolchain's own linter, over the
same 508 units through `sting-benchmark`: 93 excluded, and over the remaining 415, on the four
classes both tools have a rule for — **they score 87.1% precision at 76.4% recall as shipped, and
this repository scores 81.5% at 100%**. Their precision is higher than ours and the row is printed in
that order. With their preview `unauthorized-access` rule enabled they score 47.8% at 84.0%,
published beside the first because neither run answers the other's question.

The exclusions were 171 until the exporter stopped handing a participant our own splice: a mutated
unit is written back out as the files it came from, and a tool with a real resolver stopped seeing
two definitions of every symbol.

**The second participant could not be run at all.** Misti reads Tact and not Tolk, so the labelled
corpus is out of its reach; its Tact compiler dependency fails to install under npm's defaults
(`husky: not found`); and installed with `--ignore-scripts` it answers `Internal Tact Compiler Error`
on every input, including a nine-line contract, on Linux, in a clean container. No third static
analyser for Tolk exists to put in its place, so the "at least two" row is not met and the reason is
recorded with the commands that produced it ([`domains/benchmark.md`](./domains/benchmark.md) §6).

**Gate**

| Check | Threshold | Measured |
|---|---|---|
| precision and recall after M1 | re-measured; the difference from the old numbers recorded | 82.0% / 96.4% / 100%, unchanged; the difference is zero and recorded |
| the method | a third party reproduces the aggregate number from the repository | two commands and a container, with the participant's checksum |
| other analyzers | at least two measured by the same method, numbers published | **one measured in two configurations; the second does not run** |
| detectors below 50% precision | published in the same table as the rest | AC 32.8%, LEP 32.6% |

**Explicitly not in M9:** a safety claim. A corpus where nothing was found licenses one sentence —
the search did not reach a defect within this budget under this corpus — and never "this code is
safe".

---

## 13. M11 — The hunt

The detectors were measured on a labelled corpus of 297 files. We assembled that corpus and we wrote
its labels. M9 therefore answers one question — does the measure agree with itself — and leaves the
other one open: does it survive contact with code nobody wrote for us. M11 is that question, and its
subject is the published bug bounty scopes of the TON ecosystem.

**Scope**

- Targets are the code inside a published bug bounty scope, and nothing else. A repository outside a
  scope is read for calibration and never reported.
- Three tiers, by what the programme covers and what it pays.

| Tier | Target | Programme | Reward |
|---|---|---|---|
| Core | the main C++ repository: Simplex, Validator Node, Full Node, DHT Node | `ton-blockchain/bug-bounty` | up to $100 000, one-year lock-up on large payouts |
| Standard contracts | network config, elector, standard wallets, Wallet V4, Multisig V2, Highload V3, Nominator Pool, token contracts, TON DNS | the same | the same |
| Tools | MyTonCtrl, HTTP API, Python SDK | the same | the same |
| Ecosystem | STON.fi, EVAA, Torch Finance, Tonkeeper, Ton Whales | each its own | $5 000 – $20 000; Torch pays for Critical only |
| Frontend | TON web services | HackenProof | Critical $2 000 – $5 000, High $800 – $2 000, Medium from $300 |

The core programme currently reviews most submissions within 90 days and says why: it is buried in
reports generated by language models. A report from here is distinguished by carrying a reproduction
command and a corpus entry, not by carrying prose.

**Five prohibitions, and they are the milestone**

1. **Report only what reproduces.** A finding without a command that reproduces it on another machine
   is not a finding.
2. **Report only inside a published scope.** The scope table above is the whole permission; reading
   outside it is calibration and stays unpublished.
3. **Every live false positive enters the corpus the same day, labelled.** The hunt feeds the
   benchmark or it is entertainment. A detector that fires wrong on real code and leaves no trace in
   `corpus/` has taught this repository nothing.
4. **Never claim safety.** A target where nothing was found licenses one sentence — the search did
   not reach a defect within this budget under this scope — and never "this code is safe".
5. **No generated report.** The one thing that distinguishes this repository from the spam the
   programme is drowning in is that every claim here has a method behind it.

**Gate**

| Check | Threshold | Measured |
|---|---|---|
| coverage | every target in the scope table parses without an `ERROR` node; a file that does not parse exits non-zero and is counted, not skipped | 49 of 49, no `ERROR` node, nothing unreadable, exit zero |
| the run | one command produces the report for the whole target set, from a clean clone | `sting-hunt`, 25 s with an empty cache |
| corpus feedback | every false positive observed on a live target lands in `corpus/` as a labelled case within the same day | 205 findings judged the same day, three classes new |
| precision on live code | measured separately from the fixture corpus and published beside it, whichever way it falls | **20.0%** against 82.4% on fixtures |

The fifth row — a finding confirmed by a maintainer — is not here, because it is not settled by a
command. It lives in §16 with a threshold and a window.

**What this milestone actually tests.** Not the ecosystem, and not the payout. It tests whether
82.4% aggregate precision, measured on fixtures we chose, means anything on code written by people
who never heard of this repository. If live precision collapses, that is the finding, and it is worth
more than a bounty.

It collapsed, and the collapse is the result: **20.0% on live code against 82.4% on the corpus**, a
factor of four. Two other numbers came with it. One entry in eighteen of the published scope carries
code this analyser reads at all, 49 files of 2122 — the ecosystem's bounty-covered code is FunC, C++
and Python, and the reach was never measured before this. And the run paid for itself on the way:
`LEP` counted any mention of a slice parameter as a read, which seven rounds of self-chosen sources
never exposed and one round of somebody else's code exposed 22 times. Fixing it moved the corpus from
82.0% to 82.4% without losing a true finding. The whole of it is
[`docs/domains/hunt.md`](./domains/hunt.md).

**Explicitly not in M11:** a claim that the TON ecosystem is safe; a claim about money received; a
scan of anything outside a published scope; a report written by a model.

---

## 14. M12 — The second language

The detectors read Tolk. M11 measured what that costs: of eighteen entries in the published bug
bounty scopes, one carries code the static contour can read — 49 files of 2122. The chain does not
run on Tolk yet. It runs on FunC: the standard wallets, the elector, the network config, the token
contracts, TON DNS, and every ecosystem protocol that publishes a scope and pays for a finding.

**The analyser is not narrow because it has few rules — it is narrow because it reads one of the two
languages its subject is written in.** Twelve of the eighteen entries carry FunC, 122 files between
them. Reading it moves reach from 1 entry to 12 and from 49 files to 171, and no fifteenth detector
does anything of that size.

**Scope**

- A grammar-neutral surface in the parsing layer: a detector asks what a construct *is*, never what
  a grammar calls it.
- A FunC front-end behind that surface. The obstacle is measured rather than assumed, because FunC's
  tree is flatter than Tolk's in three ways that decide the design. There is no assignment node:
  `a = 2;` is an expression statement holding two named children and an unnamed `=`. A method call
  does not contain its receiver; the receiver is the preceding sibling. A mutating call differs from
  a plain one by `~` against `.` inside the node's own text. A surface that merely renamed node
  kinds would have answered none of the three.
- Detectors cross one at a time, each with its FunC vocabulary named in the same commit. **A
  detector is a pattern plus a vocabulary: the pattern crosses languages and the vocabulary does
  not.** `DEPR` does not cross at all — whether a name is deprecated is a fact about one compiler.
- `LEP` crosses first. `end_parse()` is FunC's `assertEnd()`, and `~load_*` against `preload_*` is
  the same question the hunt just answered for Tolk — does this call advance the cursor — asked in a
  second spelling.

**Gate**

| Check | Threshold | Measured |
|---|---|---|
| coverage | every FunC file in the published scope parses without an `ERROR` node; a file that does not parse exits non-zero and is counted, not skipped | 199 of 199 across eight targets, against 189 before three grammar edits |
| reach | scope entries the static contour reads: 12 of 18, and 171 of 2122 files | **12 of 18**, and 170 of 2121 |
| precision on FunC | measured against golden fixtures for every crossed detector and published beside Tolk's, whichever way it falls | `LEP` 2/0/0 on fixtures, and 32 of 32 live claims true after two fixes the live code found |
| no regression | Tolk precision and recall unchanged to the digit: a shared surface may not cost a true finding | `LEP` 15/29/0 at 34.1%, aggregate 82.4%, default set 96.4% — unchanged |

The precision row is worth reading twice, because 100% is the kind of number that should be
distrusted on sight. It means every one of the detector's 32 claims about live `FunC` is true of the
code, and it means nothing more: `LEP` is `Info` and outside the default set because an unchecked
tail is not a defect class anybody has named. The figure before the two fixes was 32 of 96, and both
fixes came from reading code nobody here wrote — a slice handed to a helper, and a sub-parser that
returns the slice because `FunC` has no `mutate` to declare it with.

The reach row came in one file under its threshold, and the reason is the run that judged it: reading
the scope found a third place where the table granted more than the programme did — "wallet plugins
are provided as examples and are out of the scope" — so the scope lost a file between the threshold
being registered and the run being made. 2122 files became 2121, and 171 readable became 170.

**What this milestone does not do.** It does not port every detector: one that has not crossed says
so in its own row rather than reporting nothing, because a detector with no rule for a language and
a detector that found nothing are different claims. It does not read Tact. And it does not pretend
that reading a language is understanding it — the live precision of the crossed detectors is
measured on real FunC before any of them is called useful.

---

## 15. M13 — Rules for the code we can now read

M12 opened twelve of eighteen scope entries and 170 files. It also left the thing that makes that
number worth less than it looks: **one detector of fourteen has a rule for the second language.** The
contour reads 121 files of `FunC` and looks into them with `LEP`, which is `Info`, outside the
default set, and reports a fact rather than a defect.

**Reach without rules is not coverage — it is a larger silence.** A file opened by a run that has
nothing to ask about it comes back exactly as clean as a file nobody opened, and the only thing
separating the two today is the line the run prints naming the thirteen detectors it held back.

**Scope**

- `UR` and `ID` cross, in that order, each with its `FunC` vocabulary named in the same commit.
- The material was counted before either was promised: across the 121 files in scope, 133
  `?`-suffixed lookups in 33 files for `UR`, and a `load_data`/`save_data` pair in 26 files for `ID`.
  A detector with no material would be measured against nothing, and a precision figure over an
  empty set is not a figure.
- `UR` first because `FunC` marks the question in the language itself: a lookup that may fail ends in
  `?` and returns its success beside its value, so using the value without reading the flag is
  visible without inference. Tolk carries the same fact in a field on a returned struct.
- `UDA`, `UBM`, `GVR` and the rest do not cross here, and say so in their own row rather than by
  keeping quiet.

**Gate**

| Check | Threshold | Measured |
|---|---|---|
| detectors with a rule for `FunC` | 3 of 14 — `LEP`, `UR`, `ID` | **`UR` did not cross**, and the row is not met on its account |
| precision on live `FunC`, per crossed detector | at least half its claims true, judged the same day; **a detector below that does not read `FunC`** and is switched off for the language rather than shipped |
| corpus feedback | every live false positive lands in `corpus/` as a labelled case within the same day |
| no regression | Tolk precision and recall unchanged to the digit, per detector and in aggregate |

The second row is the one with a decision attached, and it is meant to be able to fail. A detector
that is wrong more often than right on real code is not a detector that needs tuning; it is a
detector that has no business reading that language yet, and shipping it would spend the one thing
this repository has — that a finding from here is worth reading.

`UR` was the first attempt and it failed that row, which is what the row is for. The detector rests
on one claim — an unfound lookup's value is garbage, not a safe default — and in `FunC` that claim is
false in the language rather than unmet in the code. Every one of the 39 `?`-suffixed lookup
primitives in the standard library ends in `NULLSWAPIFNOT`, which puts `null` where the value would
be; the 37 mutating ones hand back the dictionary unchanged. A miss therefore either throws on the
next read, loudly, or is a legitimate sentinel — which is what TEP-81 name resolution relies on, and
what `TON DNS` does in the one finding that survived a real fix.

Three findings on live code, none of them true. The port is reverted and the measurement is what
survives, in [`corpus/triage/func-unchecked-return.json`](../corpus/triage/func-unchecked-return.json).
**A detector whose premise the language contradicts does not need tuning.** `ID` is the next attempt,
and the row stands unmet until something meets it.

**What this milestone does not do.** It does not port the remaining eleven detectors, and it does not
claim that three of fourteen is coverage. It does not read Tact. And it does not turn a fact into a
defect: `UR` and `ID` describe real defect classes where `LEP` does not, but a finding is still a
hypothesis until something confirms it against deployed bytecode.

---

## 16. Signals, not gates

Four things cannot be gates, because they are not settled by a command: a buyer, an integration, a
signed pilot, a fork of a dataset. Planning them as gates would make the plan unfalsifiable in one
direction and unfair in the other.

They are planned as **signals with a threshold and a window registered in advance**, and the window
expiring is a decision point rather than a failure to be argued away. This is the one place in the
document where a period of time appears, and it appears because a claim nobody can settle by command
needs some other way to be wrong.

| Signal | Threshold, registered in advance | Deadline | What happens when it expires |
|---|---|---|---|
| The dataset is used | one external fork or one issue that is not ours | 60 days from M6 | the dataset stays and stops being called a product; the API in M7 carries the claim instead |
| Decode API has a paying user | one | 180 days from M7 | pricing is wrong or the buyer is wrong; the surface stays, the product claim is withdrawn in public |
| A wallet shows a scam label | one integration | 120 days from the offer | П3 stops being sold as OEM and stays a signal source for П5 |
| A VASP pilot is signed | one | 180 days from M8 | П5 closes; its inputs keep serving П1 and П2 |
| A gift-market aggregation covers three venues | three | measured at M10 | П4 closes as a product and stays a measurement |
| A finding is confirmed by an outside maintainer | one | 120 days from M11 | the detectors measure our corpus and not the chain; the hunt closes and the benchmark keeps the claim alone |

Products П3, П4, П5 and П7 live here rather than in a milestone of their own. Each is blocked on
something that is not code — labelled scam data, off-chain trade aggregation, jurisdiction
requirements, a research verdict — and a milestone wrapped around work that cannot start is a plan
that lies about what it controls.

П7 has one exception with an absolute shape: the clustering rule against the vouching-ring question
is a run whose claim is registered before it, exactly as M8's campaign is. It is scheduled when M4
puts both codebases in one tree.

---

## 17. Where the numbers live

Every number a gate names is written to [`benches/baseline.toml`](../benches/baseline.toml) under
the milestone's own key — `[m0]`, `[m1]`, and so on — at the moment it is measured, together with
the command that produced it and the host it ran on. A number quoted in a document and absent from
that file is a number nobody can check.

An entry naming a fact the tree can rederive is checked against the tree by `scripts/doc-numbers`,
and it states what it promises: a floor the tree must not fall below, a ceiling it must not exceed,
or a value that must not move. The number stays what the gate was passed with — a file rewritten
whenever a test is added is a copy of the tree rather than a record of it.

Three numbers are recorded even when they are bad, because they are the baselines everything later
is compared against: the parse share before M1, the aggregate detector precision before M1, and the
per-run cost of an emulation before M2.

---

## 18. What would change the plan

Written down in advance, because a plan revised under pressure without stated triggers is a plan
abandoned.

| Signal | Response |
|---|---|
| the grammar patch is not accepted upstream | vendor it and keep going; the compiler oracle makes divergence visible whether or not upstream moves |
| Tolk 1.5 lands mid-milestone | `sting doctor` names the drift the day it is installed; the grammar work reopens as a numbered step, never as a rewrite |
| the official linter publishes precision figures | П6's leverage weakens on the static half; the benchmark still measures, and the position moves to the dynamic half, where a linter cannot follow |
| the merge stalls everything else | cut the asynchronous adapter, keep the synchronous core, and leave `sybil` outside the workspace with the reason recorded — a merge that blocks the tree is worse than a merge that is incomplete and says so |
| a container is not enough for a real campaign in M8 | the campaign runs at the size the container allows, the number is published with the size next to it, and scale becomes a second measurement rather than a blocked milestone |
| a milestone turns out to be larger than it looked | it is split, and the new part gets its own gate. Nothing is declared done because it took long enough |
| a new product idea arrives before M7 | no. The scope risk is the one that has already cost this project a plan, and [`PLAN.md`](./PLAN.md) §8 is cited rather than debated |
| the parse share cannot reach 100% | the residue is named file by file with the construct that defeats it, and the gate is reworded to that list — never dropped |

---

## 19. What this roadmap does not do

It does not schedule a buyer. Four of the seven products are blocked on something no commit
produces, and §16 says so instead of pretending a date moves them.

It does not date anything. The only period of time in this document is the window in §16, and that
window is not a schedule: it is the failure criterion of a claim that no command can settle.

It does not replace [`PLAN.md`](./PLAN.md). The plan says which gates exist and why; this document
says in what order they are approached and by which number each is judged. When the two disagree,
the plan is right and this document is wrong.

Taken together: eleven milestones, ten of them ending in a threshold a command checks, and the
first artefact leaving the repository at M6 — after the core sees the language it analyses, after
five paths to the chain become one, and after the inventory says what there is to sell. A gate
without a failure criterion is not a gate, and a milestone without such a gate is not planned.
