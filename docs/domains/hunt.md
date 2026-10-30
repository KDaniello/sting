# The hunt: the detectors against code nobody wrote for them

The corpus the detectors are measured on was assembled here and labelled here. That answers one
question — whether the measure agrees with itself — and leaves a second one open, which is the only
one that decides whether any of this was worth building: does the measure survive contact with code
written by people who never heard of this repository.

This is that second question. Its subject is the published bug bounty scopes of the TON ecosystem,
and its answer is a number that fell by a factor of four.

## 1. What the hunt is, and what it is not

**The hunt is not a scan and not a search for a payout — it is a measurement of a measurement.** A
scanner reads whatever it is pointed at. This reads only what a published programme has permitted,
because the permission is the whole difference between research and trespass, and it reports only
what reproduces.

The permitted set is not prose. It is
[`crates/sting-hunt/src/scope.rs`](../../crates/sting-hunt/src/scope.rs), it is committed, and it
carries what each programme *excluded* beside what it named. That second half is not decoration. The
first draft of the table granted this repository the whole core repository and the whole
`crypto/smartcont` folder, which is wider than the programme's own text; the run caught it by trying
to analyse the 656 files of the Tolk compiler's test suite. A permission a tool writes for itself is
not a permission.

## 2. Reach, before anything else

Most of the code these programmes pay for is written in languages the static contour does not read.
A findings table printed without that fact invites exactly one conclusion this repository may never
publish — that what was never examined came back clean.

So the run measures reach first, from the pinned commit rather than from a hand-typed column: a
language written into a table stops being true the first time a project migrates, and a migration is
what this measurement is watching for.

| | |
|---|---|
| scope entries | 18 |
| entries carrying code this analyser reads | **12** |
| files in scope | 2121 |
| files this analyser reads | **170** |
| files the programmes' own exclusions remove | 1158 |

When this was first measured the two bold rows read 1 and 49: the static contour read Tolk and
nothing else, and one entry in eighteen is written in it. Reading `FunC` moved them, and nothing
else here moved with them — the ecosystem's bounty-covered code did not change, only how much of it
this analyser can open.

The official core programme titles its contract section "Smart Contracts (FunC)". The standard
wallets, the elector, the network config, the token contracts, TON DNS, STON.fi's four named
contracts, Ton Whales' nominator pool, Torch's own stable swap: FunC. The node is C++, the tools are
Python, the frontend is reported through a platform and is not a repository at all. One entry in
eighteen is written in Tolk — Torch Finance's tgUSD stablecoin, whose programme scopes it as "all
code under the contracts folder in the Telegram USD repo".

Three entries resolve to no commit at all, and each says why in place of an address. Tonkeeper
publishes reward tiers up to $30 000 and names "products and services". TON Society takes frontend
reports through HackenProof. EVAA's programme is live and pays, but its asset list sits behind a
platform that refuses an automated read: the organisation publishes a contracts repository, and
that repository stays out of the table for exactly the reason the table exists — **an address
inferred from what an organisation happens to publish is not the address a programme named**. All
three are counted, because dropping a row would hide that a programme paying for findings can name
no asset a run could reach.

### 2.1. The exclusion that had to be reasoned about

The core programme excludes the FunC and the Fift compilers by name. It does not mention the Tolk
compiler, because the list was written before Tolk existed. Read literally, the largest body of Tolk
in the ecosystem — 656 files of `tolk-tester` — sits inside the scope of a programme that pays for
node crashes.

The narrow reading is taken and written down: a compiler is a compiler, and its tests are not a
node. The standard library beside the reference contracts goes out under the programme's own
sentence that the other contracts in that folder are examples. Both exclusions are counted rather
than applied silently, and an exclusion that removes nothing at the pinned commit fails the run — a
stale exclusion hides the day a project moves code back into scope.

## 3. The gate, and what it measures

| Check | Threshold | Measured |
|---|---|---|
| coverage | every target parses without an `ERROR` node; a file that does not parse exits non-zero and is counted | 49 of 49 parse, 0 unreadable, exit 0 |
| the run | one command produces the report for the whole target set, from a clean clone | `sting-hunt --cache target/hunt --output …`, 25 s with an empty cache |
| corpus feedback | every live false positive lands in `corpus/` as a labelled case within the same day | 205 findings judged the same day, [round 8](../../corpus/triage/torch-tgusd.json) |
| precision on live code | measured separately from the fixture corpus and published beside it | **20.0%** against 82.4% |

A run that could not read everything it claims to cover exits non-zero even though the report is
written and the findings are in it. Silence about an unread file is indistinguishable from a file
with nothing in it, and the only way to keep that from decaying into a footnote is to make it fail.

Two runs over the same cache produce byte-identical reports, which is what makes a second reading of
the same claim worth anything. The cache is why: a clone is reused rather than re-resolved, so the
commits a report names stay the commits the next report names. The cost is that a target's reach is
as old as its cache — `--refresh` resolves every reference again, and until somebody asks for it, a
project that migrated last month is still printed at the commit it had. The commit sits in every row
for that reason, so a stale number reads as a stale commit rather than as a current one.

## 4. The number that fell

| Set | Precision | Over what |
|---|---|---|
| fixture corpus | 82.4% | 499 findings, labels written here |
| live code in scope | **20.0%** | 10 findings over the same thirteen detectors, 2 confirmed |

Read the second row before reading anything else in this document. Precision measured on a corpus
half-filled with defects this repository planted does not transfer to production code, and it does
not transfer by a factor of four.

`DEPR` enters neither number, in either place, for the reason it was kept out of the benchmark's
detector set to begin with: whether a name is deprecated is a fact about the compiler, not a
judgement. Reported on its own it fired 177 times and was right 177 times, checked against the
current standard library fetched from `ton-blockchain/ton` — four names are present carrying a real
`@deprecated` attribute, three do not occur in it in any form. The consequence is a fact about the
target and not a defect worth a programme's time: the contracts do not build against a current
toolchain.

## 5. What the hunt gave back

The run's first version reported 28 findings over the comparable detectors and 2 of them were true.
22 of the 26 false ones were one shape:

```
fun requireAdmin(self: slice, admin: slice) {
    assert(self.isSliceBitsEqual(admin), ERROR_NOT_ADMIN);
}
```

`LEP` fires on a `slice` parameter that is read and never `assertEnd`-ed. Here the parameter is an
address, the only method invoked on it compares bits, nothing is consumed, and there is no tail
whose end could be asserted. The detector's own doc comment states the contract as "receives a slice
parameter, *reads from it*", and every one of its tests reads with `loadUint` — but the
implementation marked the parameter used on any mention of its name. **A wrong answer on real code
is a bug, not a boundary**, and the fix is in `lep.rs`.

The rule now comes from the language rather than from intuition: in Tolk a method advances the
caller's read cursor only when it takes `mutate self`, so the exclusion list is every `fun slice.*`
in the standard library that does not. `preloadUint` and its siblings are on that list for that
reason and not because they sound harmless — `assertEnd` after a preload fails on any slice carrying
anything at all. A method absent from the list advances the cursor, including one never seen before:
a project's own `mutate self` parser is unknown here, and treating an unknown method as harmless
turns a true finding into silence.

| | Fixtures | Live |
|---|---|---|
| precision before | 82.0% | 7.1% |
| precision after | 82.4% | 20.0% |
| false positives removed | 2 | 18 |
| true findings lost | 0 | 0 |

Recall on the corpus stayed at 100.0%. **The hunt feeds the benchmark or it is entertainment**, and
this is what feeding it looks like: a class of false positive that seven rounds of self-chosen
sources never produced, found the first time the source was chosen by somebody else.

## 6. What is still wrong, and why it stays

Four false positives survive the fix, and their cause is named rather than rounded off. `LEP` still
fires on `ctxSender` in two routers and on both parameters of one guard, because the method invoked
on the slice is the project's own — `requiretgUSDEngine`, `addressIsNone` — and it is declared in
another file. It takes `self: slice` without `mutate` and therefore consumes nothing, but knowing
that needs the cross-file index, and the index is only built when a real `tolk` has compiled the
project. This target does not compile with a current `tolk`: it is written against names the
standard library no longer has, which is the same fact `DEPR` reported twenty times.

`AC`'s four findings are the class `auth-inside-delegated-method`, now at eight independent
confirmations. Every op branch of every router opens with a constraint on the sender, written either
as `requireX(ctxSender, …)` or as a bits comparison with a throw on mismatch; `AC` sees the
entrypoint body and one delegation hop by bare name. One branch out of 57 across the four contracts
constrains nothing — `tgusd-staking`'s `OP_PROVIDE_CURRENT_QUOTE` — and it is a public quote
endpoint by design: it assigns no storage field and forwards the caller's own value. The detector's
premise is true there and its conclusion is still wrong.

## 7. What this does not do

It does not say the TON ecosystem is safe, and it does not say tgUSD is safe. Nothing here reached a
scenario of exploitation, nothing was reported to a programme, and no money was received. The one
sentence a target with no finding licenses is the one the report prints: the search did not reach a
defect within this budget under this scope.

It does not read anything outside a published scope, and it does not vendor a target — what enters
this repository is the review, never the code. It does not measure the ecosystem's security; it
measures this repository's reach into it, which turned out to be one entry in eighteen.

And it does not settle the question it was built for. A precision of 20.0% on live code says the
detectors are calibrated against a corpus that does not look like production. Whether that is fixed
by better detectors or by a corpus built out of code nobody here chose is the question the next
round inherits, and the number to beat is in the table above.
