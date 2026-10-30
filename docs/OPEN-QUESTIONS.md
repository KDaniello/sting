# Open questions

Questions whose answer is not known and **would change a decision**. Not a list of things to sort out
later: every entry carries the way it will be checked and what happens if the answer is negative.
Three of them can close a direction entirely.

Technical limitations that are already understood and simply not fixed do not live here. They live in
the doc comments of the modules that carry them, and in [`TESTING.md`](./TESTING.md) §9.

---

## 1. What separates this tool from the official linter, in a user's eyes?

**How the question stood before the landscape was measured.** The niche looked empty — one analyser
covered another language, one worked over bytecode, one was a paper with a closed dataset. An empty
niche had two explanations: nobody had taken it, or there was nobody in it to pay.

**The answer the survey gave** ([`domains/landscape.md`](./domains/landscape.md)): **taken, and the
demand is real**. The official toolchain grew into a monorepo of Rust crates, its linter ships 39
rules, and three of them cover the same ground as `UBM`, `AC` and `EPS`. Tooling for the language is
being funded and staffed. The previous reference point stopped shipping, and the new one is official.

**What stayed open, and it is a different question.** Demand for tooling is confirmed; demand for a
**second** tool beside the official one is not. A developer already installs the official toolchain —
what would make them run this one as well?

Three hypotheses, all three checkable:

| Hypothesis | How it is checked |
|---|---|
| "It finds what the official linter does not" | our detectors that have no counterpart there, measured on one corpus |
| "It proves a finding rather than asserting it" | dynamic confirmation; the official toolchain has an emulator and its linter does not use it |
| "Its numbers are published and theirs are not" | the official linter publishes no accuracy figure at all |

**How it is checked in substance.** An outside project keeps `sting` in its workflow **beside** the
official check rather than instead of it. The wording changed deliberately: "instead" is a
competition for one slot that distribution wins; "beside" is the answer to the question above.

**If the answer is negative.** The centre of gravity moves to where the official toolchain does not
work at all: recognising a contract that is in no registry, the risk signals aimed at an end user
rather than a developer, and the benchmark as a product of its own. The static detectors in that
scenario go upstream, where they help more than they do in a second tool nobody runs.

**Status**: demand for tooling is confirmed indirectly. Demand for a second tool is unchecked, and
there are no external users.

---

## 2. Can a forgotten check be told from a deliberately public function, statically?

**Why it is a real question.** It is the root of `AC`'s precision. A survey of real code — done before
the detector was written, not after — found **no** static signal separating the two cases. A counter
mutates storage with no check at all, and that is correct, because spam costs nothing. A token
contract mutates storage with no check at all, and that is a hole. Syntactically they are identical.

It was also established that an `owner` or `admin` field in storage is **neither necessary nor
sufficient**: three real wallet and multisig contracts gate behaviour without such a field at all.

**How it was checked.** Label by hand every mutating function in real contracts as "should be
guarded" or "deliberately public", hold half of the labels out, and see whether **any** signal
separates the classes. The threshold was named in advance: separation at 80% precision on the
held-out half.

### 2.1 The answer is negative

**409 mutation sites in 89 real contracts** were labelled, 73 families, half held out —
[`corpus/labels/auth-sites.json`](../corpus/labels/auth-sites.json). None of the four hypotheses named
in advance passed: the best, "no read of the sender on the path", reached **33.0%** precision on the
held-out half against a threshold of **80%**; "depth from the entry point above zero" reached 3.6%. A
trained ensemble reaches 84.6% only at a cut-off that fires on 13 sites out of 199 — overfitting
rather than a rule a static analyser can carry.

`AC` is marked experimental and is off unless the configuration names it explicitly.

**What the work corrected in the question itself.** The premise is only half true. Of 87 hits on
labelled sites, **60% fell on guarded code**: the check was there, in one of at least seven idioms
the detector does not know. A helper call, a method on a storage field, an early return, an
`if`/`else` branch, delegation to a library through a boolean, membership in a dictionary, an
economic threshold. One of them was fixed in the same pass: losing the "a check happened" state at a
`try` boundary removed twenty-two genuine false positives on real code.

So `AC`'s precision has two independent sources and only one of them is a ceiling. The second keeps
being fixed, which is why the detector is off by default rather than deleted.

**Status**: **answered negatively.**

---

## 3. Is an unchecked slice tail a defect at all?

**Why it is a real question.** Manual review of `LEP`'s false positives showed something
uncomfortable: most of them are **syntactically true** — the end-of-parse assertion really is not
called. Among them is one of the most widely deployed and most audited wallet contracts on the
network. The tool is technically right and the industry's practice is against it.

The question is not how to improve the heuristic but **what question the heuristic answers**. A tail
after a signature check in an authentication boundary and a tail in an admin-only branch are
different in significance, and without semantic understanding of what each check guards, the detector
does not separate them and does not try.

**How it was checked.** Find at least **one** publicly documented incident on TON whose root cause
was an unchecked slice tail. If there is none, the question is answered negatively, empirically.

### 3.1 The answer is negative

No incident was found. Three sources of different natures, none of them positive.

**The official list of vulnerability classes does not contain it.** Eighteen classes are listed, and
an incomplete slice parse is in none of them: signed and unsigned integers, sensitive data on chain,
account destruction, replay, unconditional acceptance of external messages, throw codes, gas,
randomness, executing foreign code, message races, front-running, refunding excess gas, reading
another contract's data, address representation, name collisions, wrong type handling, an unchecked
function return, code upgrade. The end-of-parse call appears in the same documents, as a practice.

**A survey of 34 professional audit reports has the class as marginal and never named.** In a study
of 233 real vulnerabilities across 29 projects and 11 audit firms, the taxonomy holds a
`Parsing and Serialization` leaf with **6** findings out of 233. Checked mechanically against the
text rather than by paraphrase: the words for the end-of-parse call and for a slice appear in that
work **zero times**. Over three years of public audits, this particular form did not enter a finding
description at all.

**A search for incidents and postmortems returns nothing.** What comes back is adjacent and different:
a null-pointer and a continuation-stack overflow found in the virtual machine rather than in
contracts, storage layout corruption from mismatched load and save, risks specific to another
language. No case where an unread tail was the root cause.

**A control argument from our own corpus.** The widely deployed wallet contract does not call the
assertion. If the class were a defect, it would have been found there first and many times.

**The boundary of the answer.** The negative is about **significance, not syntax**: most of those
false positives are syntactically true. The claim is exactly one — there is no public evidence that
it has led to losses. The six parsing findings are not broken down by form in that study, so "no
auditor ever flagged a slice tail" is **not** claimed.

**Status**: **answered negatively.** The question stays here with its answer rather than being
deleted: one public incident reopens it.

---

## 4. Does contract recognition scale past its current coverage?

**Why it is a real question.** Recognition of mainnet code hashes grew in four steps, and the last
jump came **not** from continuing to walk the network. It came from a mechanical walk of a public ABI
registry: a sweep over thirty thousand addresses found four new unique codes in a long run, while the
registry added over a thousand method ids in one pass.

The source of growth that worked is therefore exhausted: the registry has been walked completely. The
remaining share is a long tail of real but singly-encountered contracts, many of them messaging
infrastructure with a multi-file structure the current single-file extractor does not handle.

**How it is checked.** Three candidates, each with its own price:

| Hypothesis | Check | Expected cost |
|---|---|---|
| A multi-file extractor takes most of the tail | take twenty unrecognised codes and see how many are multi-file | medium |
| A second public verification registry of the same scale exists | search, compare sizes | low |
| The tail is unverifiable in principle, because nobody has the source | measure the share where no source exists **anywhere** | low, and therefore first |

**If the answer is negative.** The current figure is recorded as a reached ceiling and presented as
one: an honest "we recognise this share of mainnet" is stronger than a vague "we keep growing". The
dataset is published either way.

**Status**: the third hypothesis is the cheapest and has not been checked.

---

## 5. Is emulation without symbolic execution enough?

**Why it is a real question.** Dynamics here is direct emulation and fuzzing, a deliberate trade of
path coverage for speed. But one concrete, non-hypothetical limit is already recorded: findings that
hinge on the message sender are **unreachable** through the emulator's internal-message entry point
by construction — they come back inconclusive rather than confirmed or refuted.

If there is more than one such class, the static-to-dynamic bridge confirms a narrow band of findings
and creates a false sense that the rest were checked.

**How it is checked.** Count, across every detector, for how many classes the bridge can build a
reachable scenario at all. The number is exactly knowable; nobody has counted it.

**If the answer is negative.** Not a move to symbolic execution — that is another tool's niche and an
expensive entry — but an honest relabelling: inconclusive stops being an exception and becomes the
documented normal outcome for the listed classes, with the list printed by `explain`.

**Status**: not counted.

---

## 6. Does Tact earn its place in the product?

**Why it is a real question.** Tact serves about one request a second over HTTP against several
hundred for FunC — two and a half orders of magnitude worse, entirely because of a Node.js cold start
on every call. It is also what forced the service's default timeouts up threefold. It is the only
reason the container needs Node.js at all. And the language is being deprecated by its own ecosystem.

**How it is checked.** Measure the share of Tact among the code hashes actually deployed on mainnet,
using the same registry walk: of 709 Tact files in the registry, 137 processed successfully, against
1185 of 1207 for FunC. The figures are collected; the conclusion has not been drawn from them.

**If the answer is negative.** The Tact extractor is not deleted: it works and it cost real effort. It
moves behind a build flag instead. The default container stops pulling Node.js, the default timeouts
return to something reasonable, and the batch limit stops being a compromise between two incomparable
languages.

**Status**: the data exists, the analysis has not been done.

---

## 7. What is left of this tool if the official toolchain takes our detectors upstream?

**Why it is a real question.** Seven rules here have no counterpart upstream and need no emulator:
five of them are at 95–100% precision. Offering them upstream looks unambiguously good — and contains
a fork that has to be named in advance rather than discovered afterwards. If the rules are accepted,
much of what distinguishes this linter from the official check ends up **inside** the official check.
What remains is exactly what does not transfer: dynamic confirmation, recognition without source, the
benchmark, and the risk signals.

The question is not whether to offer them — a rule running for thousands of people is worth more than
a rule running for nobody. The question is **what this tool is afterwards**, and whether that is
enough.

**How it is checked.** Cheaply, without waiting for anyone's reaction: write down what would be left
if all seven rules were already upstream, and ask honestly whether the author would run that a second
time. If the answer is no, the centre of gravity is already not in the static detectors, and the
roadmap has to say so **before** the product surface is built rather than after.

**If the answer is negative** — that is, if the linter loses independent meaning after the transfer —
it is a clarification rather than a defeat. The tool stops being another linter and becomes what its
strengths already make it: an instrument of confirmation and measurement. A finding is proven by a run
in a real virtual machine, a tool's accuracy is measured by a benchmark, a contract is recognised
without its source. Static linting in that scenario is an input to the pipeline rather than the
product.

**Status**: unchecked. The check costs an evening.

---

## 8. How this file is kept

A question enters here when three conditions hold: the answer is unknown, the answer **changes a
decision**, and the way to check it can be named in advance. A question leaves only with a recorded
answer, a negative one included. A question that quietly disappears is a defect of the same class as
documentation disagreeing with code ([`ARCHITECTURE.md`](./ARCHITECTURE.md) §8.3).

---

## 9. What this file does not hold

It does not hold a question whose answer changes nothing — that is curiosity, and it belongs in a
notebook. It does not hold a task: a question here names what is unknown, never what is to be built,
and the moment an answer arrives the work it implies goes to [`ROADMAP.md`](./ROADMAP.md) while the
answer stays here. And it does not hold a decision the owner has already taken, which is
[`DECISIONS.md`](./DECISIONS.md).
