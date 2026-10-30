# Testing

What proves the tool is right, and where that proof stops. How the tree is built:
[`ARCHITECTURE.md`](./ARCHITECTURE.md). What crosses a boundary: [`API.md`](./API.md).

---

## 1. The problem: correctness here is not a property of the algorithm

`AC` cannot be proven. "This function does not check its sender" does not follow from the syntax.

A check can be an assertion, a helper wrapper, a membership test in a map, a comparison against a
config hash, or a payment threshold. A survey of real code found **at least a dozen** structurally
different forms. No static signal separates a forgotten check from a deliberately public function
where spam costs nothing.

The whole verification strategy follows from that:

> **Correctness here is the measured share of real code the tool is right about. Not a property — a
> number, with a date and a corpus attached.**

So there are five independent sources of evidence, and none replaces the others:

| Source | What it proves | What it does not prove |
|---|---|---|
| **Unit tests** | a specific branch does what it claims | that the pattern occurs in reality at all |
| **Golden fixtures** | a detector fires when the pattern is there and stays silent when it is not | anything statistical about wild code |
| **Mutation engine** | a detector does not miss an injected defect of a known class | that it finds the same defect uninjected |
| **The clean corpus** | how many false positives on real code written for other purposes | that the corpus represents the network |
| **External targets** | gaps that our own corpus cannot show **in principle** | reproducibility: they are not committed |

The fifth row is not a formality. Three of the most consequential fixes in this project were found
**only** on outside code and could not have surfaced on ours (§4).

---

## 2. Unit tests: density and distribution

1585 tests green, one command. Density inside the detectors runs about one test per fifteen to fifty
lines.

| Crate | Tests | Note |
|---|---|---|
| `sting-detectors` | 282 | the core of the value, and the densest area |
| `sting-pipeline` | 82 | orchestration plus the end-to-end integration tests |
| `sting-bench` | 50 | the instrument is measured too |
| `sting-scamcheck` | 48 | twelve of them against a real `libemulator` and captured HTTP responses |
| `sting-corpus` | 46 | retry, `Retry-After`, cache, address parsing |
| `sting-abi-func` | 43 | Tier 2 heuristics — one test per storage shape |
| `sting-tolkc` | 39 | parsing `abi.json` and `symbolTypes.json` |
| `sting-abi-service` | 33 | concentrated in end-to-end HTTP scenarios |
| `sting-abi-bench` | 30 | |
| `sting-ast` | 29 | |
| the remaining crates | 6–21 each | |
| `sting-abi-profiler` | **0** | a profiling tool outside the accuracy gate — a real gap, recorded in §9 |

**`unwrap()` and `expect()` in tests are allowed by an explicit `#[allow(…)]` carrying its reason**,
not silently: the prohibition is scoped to library code outside tests, and every such block says so.

---

## 3. The mutation engine: a second oracle for a small corpus

Forty clean projects is statistically small. The industry answer is sampling over a corpus we do not
have, so the answer here is different: **exhaustive hand labelling plus injection of a defect at a
known position.**

`sting-bench` takes a file known to be clean, injects a defect of a known class, and requires
**exactly one** new finding. One mutator per detector, no exceptions, and the rule is followed
literally: no detector has a second `mutate_`.

Two injection strategies, and the corpus dictates which:

- **Transform existing code** — swap two adjacent same-typed `store*` calls, delete an
  `onBouncedMessage` handler, shadow a storage field. Applicable where the pattern really occurs in
  the corpus.
- **Self-contained injection** — insert a divide-then-multiply expression, a modulo over the clock, an
  unguarded dictionary read. Applicable where honest code has no such site to transform: a grep while
  designing the precision-loss detector showed the real corpus almost never contains a ready
  divide-then-multiply chain.

### 3.1 The engine itself produced two real defects

**The identity mutator broke its own documented algorithm**: it swapped the arguments of **any** two
adjacent `store*` calls rather than two of the same type. That was harmless for `ID`, whose
comparison key is the argument text rather than the method name. Once `TLB` existed, the same
mutation began injecting a genuine `TLB` defect alongside, which the metric counted as a `TLB` false
positive, because the mutation's ground truth is labelled `ID` only. **Found by the first
`sting bench` run after `TLB` was added, not by an audit** — exactly the scenario the harness exists
for. The number of identity mutations halved; `ID`'s precision and recall did not move.

**The bounce mutator deleted "the first" `onBouncedMessage`** through the same
not-bound-to-a-contract search that was a real defect in the detector. It counts handlers now and
**refuses to mutate when the choice is ambiguous** rather than guessing. No number changed: a grep
confirmed today's corpus holds no such case, so what closed was a risk to future growth rather than
a current defect.

### 3.2 Cross-detector interference is measured rather than ignored

The access-control mutator injects a storage write as the first line of the entry point. On four
projects that line also creates a fresh "write then bounceable send" path, which `UBM` correctly
catches by its own rules. Technically neither mutator is wrong — both behave exactly as documented.

It does not deserve a fix of its own, but the report **has to count the joint effect honestly**:
`UBM`'s row in the accuracy table was corrected downward for this reason, because the figure had
simply not been recomputed when the access-control mutator arrived.

### 3.3 The honest boundary behind a recall of 100%

Recall of 100% is measured **on injections**. A detector finds the pattern it was written for — that
is proven, and across the whole of development not one mutation instance has been missed. That it
will find the same pattern in wild, unconstructed code is **not proven, and this method cannot prove
it**.

The boundary is written here rather than left for the reader to infer from small print.

---

## 4. External validation: what our own corpus will never show

The most valuable class of check, and the only one that produces results the corpus cannot produce in
principle. Three confirmed cases:

**`UBM`, on a live protocol.** The detector did not separate a real internal message, which can
bounce, from an external log message, which cannot bounce under any circumstances by TON's own
block schema. Four false `Critical` on real code. **No file in `corpus/clean/` uses that form even
once**, so calibration could not have surfaced it.

**`AC` and `UBM`, on a live CTF range.** Seven deliberately vulnerable levels, **zero findings on all
seven**, including a level whose only defect is an unconditional storage write with no check at all.
The root cause was recall rather than precision, and worse than anything found before: both
detectors recognised exactly one spelling of "write storage", `.save()`, while the primitive
underneath is `contract.setData(cell)` — a contract written on the low-level idiom was **completely
invisible to both**. The same round produced `UTA`, a class none of the nine existing detectors came
near.

**`GVR`, in bulk over ten independent GitHub projects** — 170 `.tolk` files, 37 raw hits. It exposed
a systematic false pattern, decode then mutate then store back, confirmed in **two** independent
projects. This is the one of the three whose fix also improved our own numbers: `GVR`'s precision
rose by ten points without adding a single miss.

**The rule that follows, and it is followed literally**: a pattern confirmed by **one** source is
documented in the detector's own source and **not fixed**. A targeted fix for a single site is
overfitting rather than generalisation. The threshold is two independent confirmations. That rule is
what kept `AUTH_MARKERS` from growing an entry per idiom, and what left four of `AC`'s gaps honestly
open.

**Nothing was reported to or disclosed with any of those external authors** — this was internal
validation of the tool.

---

## 5. Regression gates: two frozen snapshots

| File | What it freezes | Checked by |
|---|---|---|
| `corpus/baseline.json` | precision, recall and false positives per detector | the benchmark job, on every change touching the detectors or the corpus |
| `corpus-abi/baseline.json` | Tier 1 completeness, Tier 2 precision and recall, throughput | the extractor job, on every change touching an extractor |

**A single-tier `--update-baseline` once erased the whole file.** Updating Tier 2 zeroed an untouched
Tier 1 of forty-five contracts, because the report's default does not fill a tier that did not run.
Caught live while updating a real number, fixed by merging rather than replacing, with three tests
behind it.

**The mirror image was in the comparison rather than in the write**: a single-tier run falsely
reported a regression of the half that had not executed, because an empty map was compared against a
two-tier baseline and read as "fell to zero". The command's own help text suggests exactly that
invocation, so this was not a contrived misuse.

### 5.1 The meta-test that catches a forgotten detector

One test requires every registered detector except `DEPR` to appear in the benchmark's code list and
to own a positive fixture. It catches the class of mistake that **had already happened by hand**: a
detector registered and forgotten in the labelling.

`DEPR` is excluded explicitly — a style rule has no ground truth of true and false.

### 5.2 A defect in the instrument, found by a number that did not add up

The first honest run after `SUA` was added reported an aggregate about ten points below the expected
range. The discrepancy was **checked by hand rather than written off as "the new detector moved the
number"**: the table held `DEPR` with dozens of false positives and no true ones.

The cause: `DEPR` is deliberately outside the benchmark's code list, but the scoring pass ran the
**whole** registry and recorded a finding from whichever detector fired. `DEPR` has ground truth in
no manifest, so each of its findings quietly became a false positive. Fixed by a filter, and the
aggregate rose immediately to its real value. None of the ten existing detectors moved — their
counts are identical, and the difference is entirely the removal of phantom rows.

A regression test freezes it: `DEPR` findings never enter the per-detector table at all.

---

## 6. Tests that need a real external artefact

Part of the verification is impossible without a real binary. Those tests **skip rather than fail**
when the artefact is absent — otherwise a developer without a vendored compiler could run nothing at
all.

| Artefact | Variable or path | What it covers |
|---|---|---|
| `libemulator` | `STING_LIBEMULATOR_PATH` | the FFI crates, the static-to-dynamic bridge, the honeypot probe, the dictionary probe |
| `tolk` | `vendor/tolk/` | cross-file resolution for `AC` and `GVR`, the whole Tolk extractor |
| `tact` | `vendor/tact/`, `STING_TACT_JS_PATH` | the Tact extractor |

End-to-end fixtures that really compile exist separately from the unit tests: two cross-file projects
pass through the real compiler and confirm the resolution mechanism independently of the benchmark —
one finding through the path-based entry point, zero through the source-based one, and both results
are frozen.

**Why the source-based entry point is deliberately weaker.** The benchmark computes precision and
recall exclusively through it, and it **never** receives a project store, suppressions or a severity
floor. The guarantee is one-directional and intentional: no capability available only to the richer
entry point can quietly move a calibrated number. Verified beyond reading the code — findings on a
real project are identical with the vendored compiler present and absent.

---

## 7. Output is validated against the real schema

The report crate validates its actual SARIF output against the official SARIF 2.1.0 JSON schema,
rather than assuming correctness because the structure is typed. Those are different claims, and the
second is weaker.

---

## 8. What CI guards

| Workflow | Trigger | Platforms | What it guards |
|---|---|---|---|
| `ci.yml` | every push and change | ubuntu + windows | `make lint` and `make test`, which is the same gate a developer runs |
| `bench.yml` | a change touching the detectors or the corpus | ubuntu | precision and recall no worse than `corpus/baseline.json` |
| `abi-bench.yml` | a change touching an extractor | ubuntu | installs the real `tolk` and `tact` and runs the real extractor benchmark |
| `emulator.yml` | a change touching the emulator crates | ubuntu + windows | fetches `libemulator` at a pinned tag and runs the **real** tests, not just a compile |
| `tolkc.yml` | a change touching the compiler wrapper | ubuntu + windows | fetches the vendored compiler and runs a real compilation |
| `fuzz.yml` | a change and a schedule | ubuntu | sixty seconds per target on a change; a long nightly run that keeps its corpus and crashes |
| `release.yml` | a version tag | ubuntu + windows + macos | builds binaries — **`sting-cli` only** |

**Two platforms in the main workflow are not decoration**: Windows is the development platform and
Linux is where fuzzing and releases happen. Divergence in paths, separators and dynamic-library names
between them is a real source of defects in a project that carries vendored binaries.

**Fuzzing on Linux only**: the sanitiser path on Windows is effectively unusable, so Windows is for
development.

**The sanitiser covers the FFI boundary for free** on every fuzzing run — fuzzing here is not only a
search for defects in target contracts but regression protection for the FFI crate itself.

**The release workflow builds one binary of three.** The extraction CLI and the scam checker have no
release path at all, and that is recorded rather than implied.

---

## 9. Known gaps in coverage

Listed explicitly, because a silent gap is indistinguishable from the absence of a problem.

- **`sting-abi-profiler` has no tests.** A profiling tool outside the accuracy gate. A real gap, low
  priority.
- **`sting-abi-service`'s coverage sits in end-to-end HTTP scenarios**, and unit coverage of
  individual branches is thin.
- **The tree-sitter grammar is not protected against one class of defect repeating.** A `comment`
  node turned out to be *named* in the grammar, which silently broke the "exactly one statement in
  the block" check in `UR` — the most common real form of an early-return guard, with a comment
  inside the block, went unrecognised. Nothing in the AST or detector crates protects against that
  recurring on a future grammar update.
- **Service uptime is not measured, and a local run cannot measure it.** The one load test was
  single-shot, on one machine, on one day.
- **Extractor throughput and memory were measured on batches of four to twenty-eight contracts.** At
  that size the figures are indicative rather than statistically stable, and they are not offered as
  a service level.
- **Held-out validation of the scam signals has not been done.** A smoke test over eight live jettons
  exists; precision and recall against a set with confirmed outcomes is an open task, and the smoke
  test does not close it.

---

## 10. Before a commit

```
make pr
```

That is the whole gate. CI calls the same target, so there is one definition of it and not two. The
pre-commit hook runs the subset a hook can afford.

| What it runs | What it refuses |
|---|---|
| `fmt --check`, `clippy -D warnings` | unformatted code, any lint |
| `deny check`, `typos` | a new advisory, a bad licence, a misspelling |
| `cargo doc -D warnings` | a rustdoc link that resolves to nothing |
| `scripts/check-docs.py` | the documentation rules, and the same rules over the comments |
| `scripts/check-lints.py` | a crate whose lint table differs from the workspace's |
| `scripts/doc-numbers.py` | a number in a document or in `sting explain` that the tree contradicts |
| `nextest run`, `cargo test --doc` | a failing test |

**The documentation rules hold the code, not only the documents.** A comment carries no development
chronology and no path that resolves to nothing. A decorative bar, commented-out code and Russian
prose outside a named allowlist are defects. Every rule counts against a baseline that `--update`
only lowers, so a rule that has been paid for cannot be unpaid.

If the change touches the detectors or the corpus, add `cargo run -p sting-cli -- bench` and account
for **every** number that moved. An aggregate that shifted without an explanation is not noise but an
unchecked hypothesis: both real defects in the instrument (§3.1, §5.2) were found exactly that way.

---

## 11. What this document does not cover

It does not describe the tests themselves file by file — that is what the test names are for. It does
not state the current accuracy: those numbers live in `corpus/baseline.json` and in
`corpus-abi/baseline.json`, which are the files the gates compare against, and a document repeating
them would be a second copy of a number that moves. What it does state is what each number can and
cannot be read to mean.
