# Measuring an analyser, including this one

One dataset, one scoring rule, and every participant judged by the same code — this repository's
detectors first among them.

---

## 1. Why a benchmark rather than a claim

Every static analyser on TON publishes rules and none publishes accuracy. The official toolchain's
linter carries 39 rules and no precision figure; Misti carries its own and no figure either. A rule
count is a statement about effort, and the question a buyer actually has — how often is it right —
has no published answer anywhere in the ecosystem.

So the measure is built before the claim, and it measures us too. What follows is a number about this
repository's detectors that is worse than the neighbour's on the classes both cover, published in
that order because the point of holding a measure is not to win with it.

---

## 2. The dataset

| Half | Count | What it is |
|---|---|---|
| real code | 43 | vendored production contracts, untouched, **no ground truth** |
| injected | 368 | one defect placed into real code by the mutation engine |
| hand-written | 97 | golden fixtures: 43 carrying a labelled defect, 54 negatives that must produce nothing |
| total units | 508 | |

**Real code carries no labels on purpose.** Nothing in it is known to be defective, so no finding
there can be a true positive: what those 43 projects measure is noise — 66 findings, 1.5 per project.
Recall is therefore measured only on defects this harness injected or hand-wrote, and no organic
defect in untouched production code is part of any recall figure here.

**An injected defect has to survive the compiler.** A mutation that stops the file compiling is
rejected, because a file that is not code measures a participant's parser rather than its rules. Four
mutators once produced 100% uncompilable instances, every one with `unknown type name` — found by
measuring against a real compiler rather than by reading the mutator.

---

## 3. The two scoring rules, and which is for what

| Granularity | Rule | Used for |
|---|---|---|
| line | a finding within three lines of the labelled position, of the labelled class | our own regression gate |
| unit | the class is in the label and the participant reported it *in this unit* | a public comparison |

The line rule is stricter and belongs to a gate that watches one implementation over time. A public
comparison uses the unit rule, because a line-for-line match is unfair to everyone but us: another
tool reports the line it decided on, and a detector's idea of where a defect *is* is a property of
the detector rather than of the defect.

**A pair that cannot be scored is excluded rather than assigned to somebody.** If a class was present
in the project before the injection, a participant's answer proves nothing: it may have seen the old
defect. Counting it as a hit would be a gift and as a miss a theft, so it is not counted.

---

## 4. Our own numbers, re-measured after the parser caught up

The grammar caught up with the live language in `9e414c7`: 54 of 297 corpus files carried an
`ERROR` node before it and none does after. The benchmark was re-run on the fully-parsed corpus, and
the result is stated whether or not it moved:

| Measure | Before, at `be97f6a` (2026-08-06) | After, on the fully-parsed corpus | Difference |
|---|---|---|---|
| true positives | 411 | 411 | 0 |
| false positives | 90 | 90 | 0 |
| false negatives | 0 | 0 | 0 |
| aggregate precision | 82.0% | 82.0% | 0 |
| precision, default set | 96.4% | 96.4% | 0 |
| recall on injections | 100.0% | 100.0% | 0 |

**Nothing moved, and that is the finding rather than an absence of one.** The parser work's own gate
recorded it at the time — 297 of 297 parsing, no divergence in either direction, and no detector finding moved —
and this run reproduces it from the committed baseline. The reason is what the constructs were: a
`contract { }` block in 36 files, a dotted annotation in 4, an annotation on a struct field in 4, a
type as an annotation argument in 1, `;` between enum members in 3. None of them is a site any
detector looks at, so the trees changed and the findings did not.

The two detectors below half precision are published in the same table as the rest, not in a
footnote: **AC at 32.8%** and **LEP at 32.6%** ([`detectors.md`](./detectors.md) §1). Both are why
the aggregate over every measured detector is 82.0% while the default set — which excludes them —
is 96.4%. Two numbers, because one of them would be a choice about which set to show.

---

## 5. The neighbour, measured by the same code

`acton` 1.1.0, the toolchain's own linter, over the same 508 units, scored by the same function.

**93 of 508 units are excluded, counted by why.** 40 are fragments rather than whole contracts —
hand-written fixtures reaching for a type or a symbol that only exists in the detector's imagination;
26 import generated code this corpus does not carry; 15 carry syntax their compiler refuses; and 12
are mutations that a type system catches and ours cannot, from a method that does not exist on a type
to an assignment between two of them.

The largest cause used to be ours and is gone: a mutated unit was published as the merged text the
harness works on, which a resolver reads as two definitions of every symbol. It is written back out
as the files it came from now, which moved the comparison from 337 units to **415**.

Their rules map onto four of our thirteen classes — `no-bounce-handler` to UBM,
`unauthorized-access` to AC, `random-requires-initialization` to EPS, `divide-before-multiply` to PL.
A class they have no rule for prints as "no rule" rather than as a zero: "there is no rule" and "the
rule is wrong" are different claims, and conflating them flatters whoever has fewer rules.

| Over the four shared classes | TP | FP | FN | Precision | Recall |
|---|---|---|---|---|---|
| sting | 106 | 24 | 0 | 81.5% | **100.0%** |
| acton, as shipped | 81 | 12 | 25 | **87.1%** | 76.4% |
| acton, preview rule enabled | 89 | 97 | 17 | 47.8% | 84.0% |

**Read the first two rows together.** As shipped they are more precise than we are and we miss
nothing they miss: their 25 misses are ours to keep, and our 24 false positives are theirs to point
at. On EPS and PL the two tools agree almost exactly — 40 and 41 true positives each, one false
positive each. The whole difference is AC and UBM: their bounce rule reports on 10 units where the
label is elsewhere and on none where it is, and their access-control rule is off.

Their `unauthorized-access` rule is a preview and **off** in a default install. Both runs are
published because neither answers the other's question: the first says what the tool anybody installs
does, the second says what the rule can do. Turning it on takes their false positives from 12 to 97
and their true positives from 81 to 89 — eight false for every true it adds, which is what a preview
looks like.

Every number here is reproducible from the repository:
[`corpus/benchmark/SOURCE.md`](../../corpus/benchmark/SOURCE.md) carries the commands, the release
tarball's checksum and the container image.

---

## 6. The participant that could not be measured

Misti is the other analyser the plan named, and it was not measured. Three findings, all of them
about the tool rather than about the dataset, on 2026-08-23:

- **It reads Tact, not Tolk.** The labelled corpus is Tolk, so the dataset is out of its reach by
  construction.
- **It does not install with npm's defaults.** Its dependency `@tact-lang/compiler@1.5.0` runs
  `husky` in a prepare script, and the install fails with `husky: not found`.
- **Installed with `--ignore-scripts`, it fails on every input.** Misti 0.2.1 answers
  `Internal Tact Compiler Error: Cannot read properties of undefined (reading 'get')` on our Tact
  corpus, and on a nine-line contract that any Tact version accepts, on Linux, in a clean container.

That npm dependency prints its own notice on install: Tact is deprecated in favour of Tolk. A tool
for a deprecated language, which does not currently run, is not a participant this benchmark can
measure — and saying so with the commands that produced the error is more useful than a blank in a
table.

**So the gate's "at least two other analysers" is not met.** One was measured, in two configurations;
the other cannot be run; and no third static analyser for Tolk exists to put in its place.

---

## 7. What these numbers are not

They are not a ranking. Four classes of thirteen is what two tools share, and a comparison over a
quarter of one participant's coverage says nothing about the rest of it.

They are not a safety claim. A corpus where nothing was found licenses one sentence — the search did
not reach a defect within this budget under this corpus — and never "this code is safe".

They are not stable by construction. The dataset grows, the neighbour releases, and a number that is
not dated and not tied to a version is not a number. Every row here carries both.

---

## 8. What this document does not cover

It does not describe the detectors themselves, which is [`detectors.md`](./detectors.md), nor where
the labelled material came from, which is [`precision-sources.md`](./precision-sources.md). It
describes how a measurement is taken, what it produced, and which participant it could not take.
