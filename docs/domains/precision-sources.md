# Sources of accuracy

Where to get data that raises an analyser's accuracy by measurement rather than by guesswork. The
catalogue was assembled on 2026-08-05: every row checked through an API or a direct request, with a
last-updated date and a licence.

Why the document exists: our own corpus is exhausted as a source of growth. Forty projects, the golden
fixtures, the injections — and all three real breakthroughs of the last year came from **outside**
([`TESTING.md`](../TESTING.md) §4). What comes next needs new sources, and they have to be fresh and
parseable rather than merely large.

---

## 1. Three different deficits, not one

The split matters, because different sources cure different things, and confusing them means
collecting the wrong material.

| Deficit | What it means | What cures it |
|---|---|---|
| **Real code for false positives** | precision is measured on forty projects, so each false positive moves the aggregate noticeably | volume of fresh code (§2) |
| **Real vulnerabilities for recall** | recall of 100% is measured **on injections**; natural positive examples are almost absent, and for four detectors there are none | audit reports and CTF ranges (§3) |
| **Labelled ground truth** | our labelling is our own, with nothing to compare against | other people's linter tests, registries (§4, §5) |

The second is the heaviest and the most valuable. It is why eight detectors carry the honest caveat
that their perfect number is measured almost entirely on constructed input.

---

## 2. Fresh real code

The volume, measured by code search: **1748** files containing a contract declaration, **682** with an
entry point. Our corpus holds 154. An order of magnitude more is available than is used.

Ranked by value rather than by size.

| Source | Files | Updated | Licence | Why it is valuable |
|---|---|---|---|---|
| a production cross-chain protocol | **138** | 2026-08-04 | its own | production quality: routers, ramps, fee quoting, token pools, proofs. This one repository nearly equals our whole corpus |
| the messenger's own contracts | 9 | 2026-04-21 | Apache-2.0 | **the platform owner's code**. The classic multi-file layout — storage, messages, opcodes, errors in separate files — exactly the shape cross-file resolution was built for |
| the official toolchain | 164 | 2026-08-05 | Apache-2.0 | standard library, project templates, integration data — the reference modern style |
| the official contract set | — | 2026-06-18 | Apache-2.0 | already in the corpus, and the copy is stale: worth resynchronising |
| the IDE plugin's fixtures | 21 | 2026-05-24 | Apache-2.0 | short and syntactically varied |
| a client library's fixtures | 29 | — | MIT | fixtures of a Go library |
| two contracts from a known author | 4 | — | — | unusual, rare patterns |
| four independent production projects | 2–4 each | 2026 | various | each is a new source under the [two-confirmation rule](./detectors.md) |

**The priority is obvious**: the cross-chain protocol and the messenger's contracts first. The first
gives volume and complexity; the second is the platform owner's code, which carries the highest
reputational cost for a false positive and the highest value for a true one.

**An honest note on licences.** These repositories are a source to **run against**, not to vendor.
Copying somebody's code into the corpus needs a compatible licence and a recorded origin; everything
else is run in place, and only the analysis of the findings enters the repository. The same regime
already applied to the external targets and the CTF range.

---

## 3. Real vulnerabilities with labels — the main deficit

### 3.1 An academic study with a ready sample

A 2025 paper on the audit landscape, published under an open licence.

The key figures: **34 professional audit reports, 233 real vulnerabilities, 29 projects, 11 audit
firms, 2023–2025**. The inclusion criterion is **public availability**, so all 34 reports can be
downloaded.

That is exactly the ground truth this project does not have: not injections, not a CTF, but
vulnerabilities found by people in deployed code, with a class and a severity attached.

### 3.2 The reports themselves are public

| Auditor | Reports |
|---|---|
| the largest contributor | 13 |
| the second | 5 |
| the third | 4 |
| the fourth | 3 |
| four others | 4 between them |

### 3.3 One pair with both reports and source

One staking protocol publishes its contracts under a permissive licence and keeps **four independent
audits of the same code** beside them.

That is a rare configuration: open source plus several independent labellings of the same code. It
allows measuring not only whether we found what people found, but whether people agree with each other
— that is, the upper bound of what is achievable at all.

**Start there**, rather than with the largest source.

### 3.4 The official class list, and our gap against it

The official documentation lists **eighteen vulnerability classes**. The mapping against our detectors
was done by hand:

| Official class | Our detector |
|---|---|
| incorrect data type handling | **TLB**, **ID** |
| missing function return value checks | **UR** |
| insecure random numbers | **BR**, **EPS** |
| unprotected contract code updates | **AC**, **UDA** |
| signed and unsigned integers | **UTA**, partially — transfers only |
| gas limitation | — |
| missing replay protection | — |
| unconditional external message acceptance | — the access detector deliberately excludes external messages |
| invalid throw values | — |
| improper gas excess handling | — |
| executing third-party code | — found on a CTF range and documented as unimplemented |
| race conditions in message cascades | — |
| front-running via signature reuse | — |
| account destruction | — |
| sending sensitive data on chain | — |
| direct data pulling between contracts | — |
| address representation | — |
| name collision | — a detector is adjacent, and it is about shadowing |

**Five classes are covered fully and one partially.** That is not a reason to write twelve detectors
immediately — but it is the first time coverage has been measured against the **official** list rather
than against the taxonomy this project chose for itself.

Two classes stand out for cheapness: invalid throw values, a pure syntactic pattern where the official
linter already has a relative, and replay protection for external messages. One auditor separately
names gas limitation as the most frequent class in TON audits, and we do not cover it at all.

### 3.5 CTF ranges and deliberately vulnerable code

| Source | Files | Updated | Status |
|---|---|---|---|
| one CTF range | 10 | — | **already used** — it produced `UTA` and the project's largest recall fix |
| a second | 16 | 2025-12-02 | unused; solutions are included, so the labels exist |
| a third, from the same authors as the study in §3.1 | — | 2025-08-14 | typical vulnerabilities |
| a fourth, small and independent | 2 | 2025-09-25 | |

**A CTF is worth more than it looks**: the defect is **known to be there** and **known to be what it
is**, which makes it the only cheap source of true positives. The first one proved it in practice —
zero findings across seven levels exposed a recall gap the whole corpus had not shown.

---

## 4. Somebody else's labels for overlapping rules

The official language server keeps its inspection test cases as separate files, eleven of them plus a
multi-file set.

That is ready-made labelling of where a rule must fire and where it must stay silent, written **by
somebody else**. For the one overlapping class it gives a direct external check; for the rest it is a
set of short, syntactically varied fixtures of modern code, useful also as material for measuring the
grammar.

The official linter has no separate fixture directory in its tree — its tests appear to be inside the
rule modules. So its ground truth is extractable only by running the tool itself, which is what the
benchmark milestone does.

---

## 5. Verified on-chain sources

| Registry | Size | Our status |
|---|---|---|
| the community ABI registry | 1348 unique code hashes, about 7677 files | **walked completely** — it grew the dictionary six times over |
| the official catalogue | 332 code hashes, 283 contracts, 600 get-method names, 1299 opcode prefixes, measured 2026-08-21 | **not walked** |

The second is new and official and is not in the corpus. It is smaller in hashes and differently
built: curated, with known addresses and interface descriptions. For recognition it is a direct gain;
for the detectors it is another source of modern code as in §2.

---

## 6. A side source that is real: formal verification

One project does inter-contract symbolic execution of jettons.

Its value is not volume but nature: formal verification produces **proven** properties, which means
high-quality true negatives. A place where a verifier proved correctness and our detector fired is
almost certainly our false positive. That is a cheap and precise false-positive filter, and the
project has nothing of the kind.

---

## 7. What these sources do **not** give

Required reading before planning, or the work will be built on an inflated expectation.

- **The audit reports are mostly about the older languages rather than Tolk.** The current language
  reached its first stable release only in 2025, and wide use came later. So the 233 vulnerabilities
  give **a taxonomy of classes** and ground truth for the heuristic tier, but **not** direct labels for
  the Tolk detectors. Expecting to run the tool over 233 defects and measure recall is wrong; the right
  expectation is understanding which classes actually occur and which of them we do not look for at
  all.
- **The reports are documents rather than machine-readable labels.** Binding a vulnerability to a line
  of code is manual work. That bounds the pace: a few reports per round, realistically, rather than all
  34 at once.
- **Not every audited project is open.** One is confirmed open; the rest need checking one by one.
- **CTF code is unrepresentative in style** — it is written to contain a defect rather than to solve a
  problem. Excellent for recall, dangerous for precision: false positives may not be counted on it.
- **No source solves the small-corpus problem by itself.** They move it; whether it converges is a
  question for a measurement rather than an assumption.

---

## 8. The order of use

Derived from the ratio of analysis cost to expected gain rather than from the size of a source:

1. **The staking protocol with four audits** — the only ready pair of code and independent labelling.
2. **The messenger's contracts** — nine files, and the highest cost of an error.
3. **The cross-chain protocol** — volume and complexity.
4. **The CTF set** — true positives.
5. **The official catalogue** — recognition and modern code at once.
6. **The language server's tests** — somebody else's labels.
7. **The remaining reports** — in descending order of source availability.

---

## 9. What this document does not cover

It does not say what will be done with a finding once a source produces one — that is the detector
procedure in [`detectors.md`](./detectors.md) §10, including the rule that one confirmation documents
and two confirmations fix. And it does not promise that any of this raises a number: it names where
data exists and what each kind of data can and cannot prove.
