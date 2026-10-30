# Extractors

Three paths to one schema: two languages through their real compilers, one through heuristics over a
syntax tree. The difference between them is not implementation quality but where the truth comes
from.

---

## 1. Two tiers, two natures of guarantee

| | Compiled tier | Heuristic tier |
|---|---|---|
| Source | structural metadata from a real compiler | a walk of the syntax tree |
| What bounds accuracy | the correctness of **our normaliser** | the correctness of **our heuristics** |
| Confidence | none — not needed | required |
| Metric | structural completeness | precision and recall |
| Today | 100.0% across six categories | §4 |

The split is fundamental. In the compiled tier an error is only possible on our side: the compiler has
already told the truth, and the question is whether we lost it in normalisation. In the heuristic tier
nobody has the truth — there is a guess with a calibrated probability.

---

## 2. Tolk, through the native compiler

The extractor reads the two metadata files the compiler emits, which the compiler wrapper already
parses. It **reads** those types rather than forking or rewriting them — the same wrapper that serves
the detectors' cross-file resolution.

The mapping is two-pass for the reason described in [`abi-ir.md`](./abi-ir.md) §4.1: the positional
pass builds the type table as it is, and the second expands the compiler's short forms, which do not
fit a positional table.

**Throughput: 115.7 contracts a second** — a native compiler, no Node.js.

**A blocking limitation**: the vendored build is Windows-only. Over HTTP in the container this
language **does not work and has never been tested** — the platform's main language is unavailable in
its main delivery channel.

---

## 3. Tact, through its own compiler

This extractor calls the real Tact compiler, on Node.js, and normalises its output.

**Throughput: 4.4 contracts a second** — two orders below the native path, entirely because of a cold
start on every call. Over HTTP that becomes about one request a second against several hundred for
the heuristic path.

**An optimisation that was made**: the type interner stopped building cache keys through string
formatting. A real improvement, and not where the bottleneck is — the bottleneck is the subprocess.

**A documented gap**: outgoing messages are always empty for this language, unconditionally. It is
recorded in the module's own documentation — it was not before — and a stale comment in the checker
was fixed along with it, which claimed outgoing was always empty for the other language too. That was
untrue: there it really is scored.

Whether this language earns its place is [`OPEN-QUESTIONS.md`](../OPEN-QUESTIONS.md) §6 — the data for
the answer is collected and the conclusion is not drawn.

---

## 4. FunC — heuristics taken to three storage idioms

The most interesting extractor, because there is no truth here and it has to be assembled piece by
piece. Pure parsing in memory, no subprocess at all: **946 contracts a second**.

### 4.1 Three ways to find storage, each added because of a real file

| Idiom | What it recognises | Confidence |
|---|---|---|
| literal pair | functions with exactly the conventional names | 0.5 |
| named pair | the same **structure** under any names | 0.45 |
| inline | both the read and the write inside one function | 0.4 |

**The confidence values are not calibrated**, and are declared as such. They are ordered by how
reliable the idiom is, and the number itself is not derived from a measurement — an honest placeholder
rather than an estimate.

The named pair is recognised **structurally** rather than from a list of names: the getter is a
function whose return produces a tuple of loads on a name bound to the data cell; the setter is a
function whose body stores a matching number of named parameters back. Confirmed live on a real
subscription contract: ten fields, two of them addresses, one of them coins. It also surfaced that a
real standard-library alias for loading coins was not mapped at all.

### 4.2 Measured on an external corpus rather than on our own fixtures

The tier's benchmark corpus is four pairs, and it is poorly suited to this purpose (§5). The real
measure is 13 real entry-point contracts from two public repositories:

| Stage | Storage recognised |
|---|---|
| before the work on idioms | 8 of 13 |
| after the inline form | 9 of 13 |
| after the named pair | **10 of 13** |

**The inline form yielded exactly one file** — noticeably less than the theoretical share, exactly as
the plan honestly predicted in advance: not every inline case has a paired read and write in one
function.

**The three that remain unrecognised were read by name** rather than written off as tail:

- a discovery contract — an **honest true negative**: the read exists and the contract never writes
  storage anywhere. There is no pair because there is no write.
- an NFT item — a documented true negative.
- a draft editable item — **re-examined, and it had been characterised imprecisely**. The first
  characterisation was based on a shallow grep. Reading the structure showed the pair exists, and the
  read has a **conditional return**: the contract may be uninitialised, in which case some fields are
  null. Which branch is the "real" storage schema cannot be determined statically. **Deliberately not
  attacked**: deciding which of two branches is canonical is a different and riskier problem, not a
  variation of the same pattern.

---

## 5. One category is honestly unmeasurable

| Category | TP | FP | FN | Precision | Recall |
|---|---|---|---|---|---|
| get methods | 6 | 0 | 0 | 100.0% | 100.0% |
| storage fields | 0 | 11 | 0 | **0.0%** | n/a |

**The zero is not a defect in the extractor but a methodological limit of the ground truth**, and that
is a checkable claim rather than an excuse.

The corpus is four pairs of an original in one language against a converted and verified equivalent in
the other. The converter preserves the source language's own tuple idiom **literally** rather than
translating it into the target's native sugar. In that case the target's compiler simply has **no**
storage type index — the ground truth for storage fields is physically absent.

The extractor meanwhile really does extract four correct fields from each of two real contracts,
matching a hand check at review time. There is simply nothing to compare them against in this pair.

**The rise in false positives after the inline work is not a regression but the same artefact**: every
newly recognised field counts as false not because it is wrong but because there is nothing to check it
against.

**The corpus is honestly small and not inflated.** Four pairs are all that exist where both the
original and the converted equivalent are present in this workspace at once.

---

## 6. A real defect found on a real deployed token

One heuristic recognised only a direct tuple return. A widely held real token uses the assign-then-
return form instead.

Found on a held-out target and **honestly not fixed immediately**, because the rules of a held-out
test do not permit it. Fixed afterwards, with a unit test reproducing byte for byte the form that
broke the heuristic.

**The benchmark numbers did not move, and that is recorded rather than tidied away**: the four
calibration pairs physically do not contain the pattern. The fix is real and directly validated, and
it is not observable in the aggregate metric.

---

## 7. Two defects in the instrument itself

**A single-tier run falsely reported a regression of the half that did not execute.** Running one tier
fills only its own half of the report, and comparing that against a two-tier baseline read the
emptiness as a fall to zero — a real non-zero exit. The command's own help text suggests exactly that
invocation, so it was not a contrived misuse.

**One reference element counted twice.** The scoring did not check that a ground-truth element had
already been matched, so when two **different** candidates normalised to the same reference name, both
went into true positives. It does not show on the current four-pair corpus — the numbers did not
change — but it is a real defect in the counting algorithm that would have inflated precision as the
corpus grew. A second candidate for a taken key now counts as a false positive.

---

## 8. Open work

- **Extend the heuristic tier's ground truth past four pairs** — the most direct way to make the
  unmeasurable category measurable.
- **Find at least one conversion that uses the target language's native sugar** for storage; then
  precision in that category becomes measurable on at least one pair.
- **Extend the second language's corpus past nineteen** and add real production contracts rather than
  only the compiler's own test fixtures.
- **A multi-file heuristic extractor** — directly connected to the recognition ceiling: a large part of
  the unrecognised mainnet tail is multi-file messaging infrastructure
  ([`OPEN-QUESTIONS.md`](../OPEN-QUESTIONS.md) §4).

---

## 9. What this document does not cover

It does not describe the shape they all produce — that is [`abi-ir.md`](./abi-ir.md) — and it does not
describe recognition without source at all, which is
[`method-dictionary.md`](./method-dictionary.md). It describes three ways of getting a schema and the
one place where a number cannot honestly be produced.
