# Detectors

Fourteen detectors: thirteen for security and one for reference. Each with its real number, its real
boundary, and the story of how that boundary was found.

The numbers come from `corpus/baseline.json`, computed from it rather than copied out of an older
report. The method is [`TESTING.md`](../TESTING.md).

---

## 1. The summary table

The corpus: forty clean projects, the mutation instances, the golden fixtures.

| Code | What it looks for | Severity | Kind | TP | FP | Precision |
|---|---|---|---|---|---|---|
| **ID** | the order of stores and loads diverged between write and read | High | dataflow | 8 | 0 | **100.0%** |
| **TLB** | a field's width or type diverged between write and read | High | dataflow | 16 | 0 | **100.0%** |
| **PL** | division before multiplication — lost precision | Medium | AST | 46 | 0 | **100.0%** |
| **BR** | time as a source of randomness | Medium | AST | 46 | 0 | **100.0%** |
| **UTA** | a debit and a credit of one amount with only one side checked | High | AST | 45 | 0 | **100.0%** |
| **SUA** | `x =+ 1` written for `x += 1` | Low | AST | 45 | 0 | **100.0%** |
| **EPS** | the generator's seed is not protected | Medium | AST | 45 | 0 | **100.0%** |
| **UR** | a dictionary value read without checking that it was found | High | AST | 46 | 1 | **97.9%** |
| **UDA** | untrusted data reaching a dangerous operation | High | dataflow | 33 | 2 | **94.3%** |
| **GVR** | a local variable shadowing a storage field | High | AST | 21 | 3 | **87.5%** |
| **UBM** | storage written, then a bounceable send with no handler | Critical | dataflow | 23 | 8 | **74.2%** |
| **AC** | a path to a storage mutation with no recognised check | Medium | AST | 22 | 45 | **32.8%** |
| **LEP** | an unchecked slice tail | Info | AST | 15 | 31 | **32.6%** |
| **DEPR** | a deprecated name from the standard library | Info | AST | — | — | outside the measurement |

**82.4% aggregate precision over every measured detector**, recall 100.0%, no misses.

**96.4% aggregate precision over the default set.** The second number answers a different question:
not how noisy everything we ship is, but how noisy what a user sees without configuring anything is.
Excluded from it:

- **`AC` is experimental and off by default.** It is enabled by naming it in the configuration. The
  basis is the question answered negatively over 409 labelled sites in 89 real contracts: none of the
  four separation hypotheses named in advance survived the held-out half.
- **`LEP` sits in the reference category beside `DEPR`**, at informational severity. The basis is the
  other question answered negatively: no publicly documented incident on TON has this root cause.

Both keep being measured by the benchmark and stay in the table above: off is not deleted.

**`DEPR` is deliberately outside the table.** "Deprecated but correct" has no true and false in the
same sense — it is a fact about the compiler. It gets the same trait, the same output and the same
filtering, and it does not enter the precision measurement. That was once not really so: the scoring
pass ran the whole registry and recorded a finding from **any** detector that fired, so dozens of
deprecation findings quietly became false positives and pulled the aggregate ten points below its
real value. Found by a number that did not add up, checked by hand, frozen by a regression test.

---

## 2. The eight detectors above 95%

One property unites them: **a narrow, unambiguous syntactic pattern** with little room for a second
reading.

**`ID` and `TLB`** share one event-driven dataflow machine, factored out so it is not duplicated. `ID`
compares the **order** of fields; `TLB` compares their **shape** — the method plus the statically
readable width. A 32-bit store read back as 64 bits is `TLB`; two fields swapped is `ID`.

Splitting them into two classes rather than widening `ID` is justified twice over. The errors have
different root causes and different fixes — reorder the fields against fix the width. And widening
`ID` in place would have silently disturbed its own published, cross-cited numbers instead of
producing an honest new measurement.

**`PL` and `BR`** are pure AST patterns with no dataflow at all. `PL` looks for a multiplication whose
operand is a division. One deliberate exception: rounding down to a multiple, a common uncompromised
idiom in gas and storage code, is not flagged — the divisor and the second factor are compared
textually.

`BR` looks for a modulo whose operand is a direct call to the block clock. **Both the receiver and the
method name** are checked, so the real separate random module does not collide structurally and needs
no exception.

**The honest caveat these two share**: the block clock is one of the most frequent calls in the whole
corpus, with dozens of legitimate uses for deadlines. The narrow scope separated them from the risk
pattern **without a single miss**. But real **positive** examples — a modulo over time — do not occur
in honest code, as expected, so the 100% is measured almost entirely on golden fixtures and
injections.

**`UR` is the most thoroughly grounded detector in the project** and the best illustration of the
method. The first version, with three guard forms, produced **seventeen false positives** on real
code. Each of the four forms added afterwards came from a specific site:

| Form | What it exposed |
|---|---|
| an assertion on the found flag | seventeen down to fourteen |
| a guard inside a match arm | match arms were not walked at all; fourteen down to eleven |
| a compound condition | a loop in a signer lookup |
| a boolean alias for the found flag | a do-while in two real contracts; the remaining eight down to zero |

A defect belonging to none of that was found on the way: the comment node turned out to be *named* in
the grammar, so an early-return guard with a comment inside the block — the most common real form —
broke the "exactly one statement in this block" check.

The single remaining "false positive" is **a genuine finding**: a real contract calls the unchecked
read with the developer's own comment saying the value is assumed always to be found. It is
reproduced as a positive golden fixture rather than invented.

**`UTA`** is the only detector found neither on our corpus nor in somebody's taxonomy but on a live
CTF range. A contract debits one balance and credits another by the same amount, checking only the
first, and the amount is read **signed** in a **different** function. A negative amount mints tokens
out of nothing while trivially passing the check on the first balance. None of the nine detectors
that existed came near the class.

It is implemented deliberately narrowly. It demands **proof** that the amount traces back to a signed
read, in the same function or through **one** interprocedural hop. It fires only when **exactly one**
of the two sides is checked: not zero, which is honest unsigned arithmetic, and not both, which is
safe.

**`SUA`** is a port of a rule from a neighbouring analyser. It detects by **exact byte adjacency**:
the assignment and the following unary operator with not one byte between them — the real typo
pattern — as against the case with a space, which is a deliberate unary expression. Verified by
parsing rather than by assumption: the byte offsets coincide in the first case only.

**A second candidate from the same survey was deliberately not ported.** Attempting it showed that the
language has a type-safe replacement of its own: the absence of an address is a nullable type or an
explicit constructor rather than a magic all-zero value. A one-to-one port risked either never firing
or catching the wrong class.

---

## 3. `GVR`, and how it got where it is

Three rounds, each its own cause rather than one.

**Round one: binding a function to its contract.** The detector iterated over *all* contracts by *all*
functions with no binding between them. In a directory holding several contracts, a local of contract
A was compared against the storage fields of contract B that happened to share a field name. Fixed by
binding to the nearest preceding contract header.

**The same class of defect sat in `ID` and `UBM`** — found by a full review rather than by reading
findings. In `ID` it produced false positives. In `UBM` it produced **false negatives**: a contract's
risky send was covered by another contract's handler. For a security detector the direction of the
error is worse. Both were fixed by a shared helper factored out of `GVR`.

A defect in the parsing crate surfaced with it. Two contract blocks in a row with nothing between
them are merged by the parser into one error node, and the old code recovered exactly one header from
it, losing the second contract entirely. That undermined the very "nearest preceding" guarantee the
fix relied on.

**Round two: read, mutate, write back.** Found on outside code: a value is loaded from a map, changed,
and stored back — an ordinary correct idiom, syntactically indistinguishable from a forgotten write.
Confirmed in **two** independent projects, which is what earned it a fix.

**Round three: cross-file storage.** The storage field names now resolve through the project index
when the structure is not in the same file. The pattern is not hypothetical: **every** real contract
in the two main corpora keeps its storage declaration in a separate file.

**The numbers did not move, and that is expected rather than an oversight**: the benchmark scores
through the entry point that never receives a project index, and concatenating a project's files
makes any once-cross-file structure visible before that path would be needed. The mechanism is
confirmed separately, by a two-file fixture that really compiles.

**The three remaining false positives were read by hand and are not defects:**

- read, mutate, write back **through another function** — the variable leaves as a mutable argument
  and the helper writes it back. The write is physically outside the scope of a strictly
  intra-procedural analysis.
- two cases of a different shape: the function **builds a new** storage at deployment, so the locals
  are not copies of an existing field but the computed value of a future one.

---

## 4. `UBM`, and why its number fell honestly

An older version of the table showed a higher figure. The difference is not a regression in the
detector but unaccounted **cross-detector interference between mutators**. The access-control mutator
injects a storage write as the first line of the entry point, which on four projects creates a fresh
"write then bounceable send" path that `UBM` correctly catches by its own rules. The figure had
simply not been recomputed when that mutator arrived.

The three remaining non-mutation hits were read by hand:

- a real true positive named as such at implementation time: a supply counter is increased, saved, and
  a deploy message goes out bounceable with no handler in the file. Before the contract-binding fix
  this finding was **masked**, because the contract was falsely covered by a neighbour's handler.
- a real pattern of low significance: reverting an addition to a whitelist on bounce is not a loss of
  funds.
- the most substantive result of the manual review: a handler in an official contract literally
  contains the developer's comment saying that the contract sends bounceable messages and does not
  handle them. Exploitability was not checked and is not claimed.

**The real defect in `UBM` was found only on outside code.** The detector did not separate a real
internal message from an external log message, and the second cannot bounce by TON's own block schema.
Four false `Critical` on a live protocol. No file in `corpus/clean/` uses that form even once, so the
gap could not have surfaced on our corpus, and `UBM`'s own numbers did not change after the fix.

---

## 5. `AC` — half a ceiling, half recognition gaps

**An expected result rather than a surprise.** The survey done before implementation warned outright
that this detector would land closer to the noisy end, and the decision to build a narrow first
version was taken with that in mind.

**The root.** Recognising authorisation rests on four literal lexical markers. Any check written in
other words is a false positive. The survey found **at least a dozen** structurally different forms
and, more importantly, **no** static signal separating a forgotten check from a deliberately public
function where spam costs nothing ([`OPEN-QUESTIONS.md`](../OPEN-QUESTIONS.md) §2).

**What the labelling added.** The question was answered negatively over 409 mutation sites in 89 real
contracts, and the detector moved to experimental and off by default. But the same labelling showed
the premise is only **half** true: of 87 hits on labelled sites, **60% fell on guarded code** — the
check was there, in an idiom the detector does not know. At least seven forms exist, and two of them
place the check outside any condition enclosing the mutation: an early-return guard, and an
`if`/`else` split where the assertion covers one branch and the other branch is itself the
authorisation.

One form was fixable immediately: the block walk had no branch for a try-catch statement, so in a
handler wrapped entirely in one, **every** check inside was ignored. Fixing it took the hits on real
code from 118 to 94, and 22 of the 24 removed were genuine false positives. It changed nothing in the
committed baseline: the corpus holds no handler inside a try.

**The false positives decompose into four different sources, not one:**

| Source | How many | What it is |
|---|---|---|
| corpus composition | about eighteen | minimal fixtures written to calibrate **other** detectors; by construction they hold no sender check |
| real contracts | nine | all explained by documented limits of the first version |
| a consensus idiom | about two | authorisation by a validator's position or by a config contract's address |
| a low-level idiom | two | membership in a map, and a payment threshold, both already described categories |

None of them is a defect in the current implementation. The one real defect — a disjunction of two
trusted parties read as a bail-out — was found and fixed **before** this measurement.

**What was closed, and on what evidence:**

- **Alias tracking for the sender** — a variable bound to the sender is remembered by name, and later
  conditions are matched on an **exact identifier**, not on textual containment, so a similarly
  spelled unrelated name cannot collide. Confirmed by three independent sources, including a project
  where the sender is parsed by hand from a low-level signature and the usual word does not appear in
  the file at all.
- **Recognising code replacement as a storage-class mutation** — the project's largest recall fix.
  Replacing a contract's code without a check is a `Critical` finding of its own rather than a Medium
  one.
- **Delegation into free functions and methods**, in the same file and across files through the project
  index. It requires **exactly one** match by bare name: the module cannot know a receiver's real type
  from a syntax tree, which has no types, nor from a flat table of signatures. Ambiguity is treated as
  unresolvable — the same fail-safe principle as everywhere else in the module.

**Method delegation cost precision, and that is recorded as an accepted trade rather than hidden.**
Both new false positives are the same pattern: a storage method saves unconditionally inside itself
and is called under a condition computed from a hash comparison. The code **is** protected, and that
form was never in the marker set. It was not widened for the case, because doing so would be exactly
the overfitting this project's own method warns against.

**Open, honestly described gaps** — each with one confirming source, none reaching the threshold of
two: matching parameters across a call; an exhaustive `if`/`else if`/`else` chain; an open relay,
which needs taint rather than a dictionary; predictable randomness through equality rather than
modulo; execution of untrusted code.

---

## 6. `LEP` — and the question turned out to be the wrong one

The project's worst number, and the review showed something uncomfortable: **most of the false
positives are syntactically true** — the assertion really is not called. Among them is one of the most
widely deployed and most audited wallets on the network.

The detector is technically right. But it cannot — and does not try to — separate a tail after a
signature check at an authentication boundary from a tail in an admin-only branch where access is
already restricted otherwise. Without semantic understanding of what a particular check guards, those
cases are indistinguishable.

**The question was taken to an answer, and it is negative.** No publicly documented incident on TON
has this root cause. It is not in the official list of eighteen vulnerability classes, and not in a
survey of 34 professional audit reports covering 233 real vulnerabilities, where parsing and
serialisation together account for six findings and the words for this specific form appear zero
times.

`LEP` was lowered to informational severity and moved to the reference category beside `DEPR`: it
reports the same thing, and the claim attached to it changed. One public incident brings it all back,
so [`OPEN-QUESTIONS.md`](../OPEN-QUESTIONS.md) §3 stays open with its answer recorded.

**What the rest of the false positives are:**

- **Six of the original set were a real defect**, since fixed: the detector did not separate a slice
  passed by reference, deliberately for partial parsing, from an ordinary one. Demanding the assertion
  from six real unpacking helpers was a semantic error rather than a matter of low significance.
- **Two are a limit known from the start**: an alias created through a method call, where the detector
  tracks only direct reassignment.
- **Ten of the growth are entirely a side effect of other fixtures**: the golden fixtures for two other
  detectors are taken from real low-level code and by construction never call the assertion. Not a
  regression in `LEP`.

---

## 7. Overlap with the official linter

Checked against the official linter's own code and documentation
([`landscape.md`](./landscape.md) §3). Five of our classes now have an official counterpart:

| Their rule | Ours | Who is wider today |
|---|---|---|
| divide before multiply | `PL` | parity on the class; theirs is dataflow over types, ours a pure AST match |
| random requires initialisation | `EPS` | parity |
| unauthorised access | `AC` | **ours** — theirs is one form, preview, off by default |
| no bounce handler | `UBM` | **ours** — we analyse write-then-send rather than the presence of a handler |
| deprecated symbol use | `DEPR` | **theirs**, and the same exists in the language server: ours is the third copy |

Eight detectors have no counterpart at all: `ID`, `TLB`, `UR`, `UTA`, `SUA`, `UDA`, `GVR`, `LEP`.

**What follows for detector work**, recorded as a decision rather than an observation:

- **A second rule is not written where they already have one.** The contribution in an overlapping
  class is not a duplicate but **the measured accuracy of both** on a shared corpus.
- **`DEPR` is a candidate for leaving the default set.** It is duplicated twice by official tools, and
  its own value was reference rather than security to begin with.
- **Their preview status is worth adopting.** A raw rule that exists, is documented and is **off by
  default** is exactly the mechanism `AC` lacked.
- **The eight non-overlapping rules are candidates for going upstream.** The consequences are
  [`OPEN-QUESTIONS.md`](../OPEN-QUESTIONS.md) §7.

---

## 8. The one taxonomy code that will not be implemented

Of the eight codes in the taxonomy this project started from, seven are implemented. The eighth — an
improper function modifier — is **structurally inapplicable to Tolk** rather than deferred, as three
others once were.

The difference shows in the validation itself: disabling it by name is **rejected**, with the
registered detectors listed. It is a real taxonomy code and not a registered detector, and that is
exactly the class of typo the validation exists for.

---

## 9. The detectors against global versions 12 to 15

Every detector claims something about what a contract does, and what a contract
does belongs to a global version. Four of them landed on mainnet between
2025-11-13 and 2026-07-23, and each is ruled against here rather than assumed
harmless. The source is the chain's own `doc/GlobalVersions.md`, read on
2026-08-21.

| Detector | Verdict | Why |
|---|---|---|
| **ID** | unchanged | the order of stores against loads is a property of the source, and no version touched it |
| **TLB** | unchanged | a field's width and type are read off the same source, from both sides |
| **PL** | unchanged | gv13 changed how `LSHIFT`, `RSHIFT`, `AND` and `OR` answer on `NaN`; division before multiplication is neither |
| **BR** | unchanged, and gv13 sharpens it | block timestamps became non-strictly increasing, so two blocks may carry the same `now()` — the value this detector calls unfit for randomness got less unique, not more |
| **UTA** | unchanged | a debit and a credit with one side checked is arithmetic in the source |
| **SUA** | unchanged | `x =+ 1` is a typo the parser sees |
| **EPS** | unchanged | an unprotected seed is a call in the source |
| **UR** | unchanged | `.isFound` before `.loadValue()` is a standard-library contract, not a chain one |
| **UDA** | unchanged | its sinks are `sendRawMessage` and `<msg>.send`; gv15's library restrictions touch neither |
| **GVR** | unchanged | shadowing is a name resolved in one file |
| **UBM** | **changed by gv12 and gv14** | see below |
| **AC** | unchanged | a mutation reached with no recognised check is a path through the source |
| **LEP** | unchanged | an unread slice tail is a call that is absent |
| **DEPR** | unchanged by any of them | it tracks the compiler's standard library, which moves on the compiler's schedule rather than the chain's |

**`UBM` is the one that moved, and it moved twice.**

gv12 renamed the internal message's `ihr_fee` to `extra_flags` and made
`(extra_flags & 1)` switch on a new bounce format: the bounced body is
`new_bounce_body#fffffffe`, carrying the original body, the original value and
timestamps, which phase failed, and the exit code. The detector's premise is
untouched — storage saved before a bounceable send still needs a handler — and
what changed is what the handler receives. A handler written against the old
format reads the new one wrongly, and this detector does not check that.

gv14 changed what a bounce returns when the action phase fails with
bounce-on-fail: the whole remaining message balance before the action phase,
rather than what was left after it. That makes an unhandled bounce cost more
than it did, which raises the consequence the detector's `Critical` severity
already claims.

**Neither is a false-positive risk and both are gaps.** `UBM` still fires on the
same code and still means what it says; what it does not do is judge a bounce
handler's own parsing against the version it will run under. That is recorded
here as absent rather than left to be discovered.

**One consequence outside the detectors.** gv15 restricted the "change library"
action to governance contracts and stopped private libraries being added or used
in the TVM library context. Nothing in the detector set reads libraries, and the
one place that does is `sting_corpus::LibraryResolver`, which resolves a library
*reference cell* to real code by fetching it. Reading a published library is
unaffected; publishing one is not something this tree does.

---

## 10. How a detector is added

The order comes from the history of `UR` and `AC` rather than from taste:

1. Grep the supposed pattern across the whole clean corpus **before** writing code: how many real
   sites, and of what shape.
2. A deliberately **narrow** first version. Widen on a real miss, never on an imagined one.
3. Golden fixtures from **real** code: at least three positive and three negative.
4. One mutator in the benchmark. One per detector, with no second variant.
5. The whole benchmark, and **every** new hit read by hand.
6. Registration. Forgetting it is impossible: a meta-test catches exactly that class of mistake, and
   it exists because it happened by hand.

**The two-source rule.** A pattern confirmed by one source is documented in the detector's own code
and **not fixed**. The threshold for a fix is two independent confirmations. That rule left four of
`AC`'s gaps open and, at the same time, kept the detectors from turning into a set of patches for
individual sites.

---

## 11. What this document does not cover

It does not explain how a detector is built internally — the dataflow machinery is
[`dataflow-cfg.md`](./dataflow-cfg.md) — and it does not describe the harness that produced these
numbers, which is [`TESTING.md`](../TESTING.md). The numbers here are a snapshot of one committed
file; where this document and `corpus/baseline.json` disagree, the file is right.
