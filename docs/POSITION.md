# Position

What `sting` is inside the TON ecosystem, who it is sold to, where the large market is, and what to
strike at in which order.

This document answers a level above [`ROADMAP.md`](./ROADMAP.md). The roadmap answers what is being
built and in what order; this answers which business we are in and why that one. External numbers
were verified against their sources on 2026-08-21; internal ones come from the code and the baseline
files.

---

## 1. This is not one project but three businesses

Three product surfaces grew out of a common foundation and build with one command. The similarity
ends there. They have **different buyers, different competitors and different ceilings**, so
decisions about them cannot be taken together.

| Surface | Sold to | Who is already there | Ceiling |
|---|---|---|---|
| **security** — detectors, emulator, `verify` | whoever writes Tolk | the **official toolchain**, whose linter ships 39 rules | a services business |
| **abi-layer** — contract schemas, the method-id dictionary | **indexers, explorers, wallets, analytics** | nobody | load-bearing infrastructure |
| **scamcheck** — risk signals for jettons and gift collections | **wallets, marketplaces, the end user** | scattered trackers | a feature, or a price oracle |

From that follows the cheapest conclusion in this document, §9: **the project's main open question is
asked of all three surfaces and applies to one.**

---

## 2. security: proof of competence rather than a business

**What it does.** Reads Tolk source and finds classes of defect: an unhandled bounce, division before
multiplication, block time as a source of randomness, a serialisation mismatch, a dictionary lookup
read without checking that it was found. Separately, it downloads a **deployed** contract by address
and runs its bytecode in a real virtual machine. `sting verify` joins the halves: a finding in source,
confirmed on real bytes.

**The position changed during 2026.** The official toolchain's linter is a real, staffed project with
39 rules, standing on a name resolver and a type system, and three of its rules cover the same ground
as ours. The claim "nothing covers Tolk" is no longer true
([`domains/landscape.md`](./domains/landscape.md)).

**Where we are objectively stronger — three things, each with a number:**

| | `sting` | the official linter |
|---|---|---|
| Classes with no counterpart on the other side | **eight detectors** | — |
| Published accuracy | 82.4% aggregate precision over every measured detector, 96.4% aggregate precision over the default set, recall 100% on injections | **no published figure at all** |
| Dynamic confirmation of a finding | `sting verify`, a real virtual machine | an emulator exists and the linter does not use it |

**The ceiling.** A security audit scales by people rather than by code: it is a service with project
revenue. Useful and sellable, and not the kind of scaling a platform is built for.

**The real function of this surface is trust.** Nobody buys claims about what happens on TON from
someone who has not shown they read TON contracts better than others. A public benchmark makes us the
**measure** rather than a competitor: a competitor fights for one slot in a toolchain and loses to
distribution, while a measure sells for longer.

---

## 3. abi-layer: the missing layer of meaning

The main value of the project is here, and it is not obvious, so it starts from the beginning.

### 3.1 The problem

A TON transaction shows bytes. **It does not show what operation that was.** TON has no ABI at the
protocol level: no mandatory place where a contract declares its operations and their fields. So the
step from "a message with body `0x…`" to "a transfer of five USDT" has to be taken by somebody, and
that somebody is not the chain.

Ethereum closed the same hole with two registries — one from four bytes to a function name, one from
an address to source. Explorers, wallets, indexers, tax and compliance services all lean on them.
Boring invisible infrastructure without which nothing above it works.

### 3.2 What we have and what the foundation has

| | the official ABI catalogue (verified 2026-08-21) | this dictionary |
|---|---|---|
| contracts | 283 | 1386 |
| unique code hashes | **332** | — |
| get methods and method ids | 600 names | **1388 method ids in the dictionary** |
| opcode prefixes | 1299 | — |
| mainnet coverage | not claimed | **86.2%** of unique codes, 337 of 391 |
| origin | **curated by hand** | machine-derived, with a provenance per entry |

A measurement two and a half weeks earlier gave that catalogue 322 hashes and 575 get methods. It
grew by ten hashes and twenty-five methods in that time. That is the pace of hand curation, and it
does not catch up with a walk.

**The difference is in nature rather than in size.** Their artefact answers "we have described the
important contracts"; ours answers "we know what almost everything actually deployed is". An indexer
needs the second answer.

### 3.3 Who pays

Not the person writing Tolk. Indexers, explorers, wallets, analytics, exchanges, compliance, tax
services, trading bots, marketplaces — everyone **downstream of raw chain data**.

Those people are indifferent to the developer toolchain: they do not install it. The question in
[`OPEN-QUESTIONS.md`](./OPEN-QUESTIONS.md) §1 — why would a developer run a second linter — **does not
apply to this surface**.

---

## 4. scamcheck: half a product here, half in a neighbouring repository

**What it does.** Answers the end user's question — is this a scam? Can the owner mint more, is the
liquidity real, is this a trap where buying works and selling does not, is the supply concentrated in
a few wallets, is this collection genuine.

**About the last signal.** The crate carries the modules for a gift registry and for authenticity —
scaffolding without a rule. The rule is already measured, in another project's census of the gift
market:

> A walk found **167 collections** declaring themselves served by the official gift host; **eight**
> of them call themselves one popular series and **nineteen** another. The chain separates them: what
> the issuer stepped away from has no owner and does not number its supply, while a copy has both.

That is a finished authenticity detector with a rule derived from data rather than from an
assumption. Moving it was §7, strike 3, and the move measured the scale properly: over the whole
census 8 names are claimed by more than one contract across 53 rows, and 40 of those rows hold none
of their own supply while a namesake does. The two series above account for 27 of the 53.

**The ceiling.** A scam check by itself is a feature rather than a company. But it has an exit:
whoever can tell a real gift from a forgery and sees trades across every venue can compute a
**price**, and whoever computes a price becomes an oracle. That has the economics of an exchange
rather than of a utility.

---

## 5. The whole position: TON's semantic layer

Three surfaces separately are three different ceilings. Together they are one position, and it is
larger than the sum.

> **The chain gives bytes. Indexers give messages. Nobody gives meaning.**

Meaning is the answer to "what happened", in words and fields rather than a hash and a message body.
Giving it takes five capabilities.

| Capability | Where ours is | Number |
|---|---|---|
| Recognise a contract from its code | the method dictionary | **86.2%** of unique mainnet codes |
| Decode an operation and its fields | the extraction layer, three languages | Tier 1 at **100%** structural completeness |
| Know what actually executed | the emulator binding | the **only** binding to `libemulator` that does not leak; three third-party ones do |
| Say whether it is genuine | the risk signals plus the census rule | 40 copies behind 7 contested names, over 167 collections |
| Say whether it is dangerous | 14 detectors in sting | 96.4% aggregate precision over the default set |

**No team in TON has all five.** The foundation has the first and second partly, and the fifth partly.
Indexers have none: they hand over raw material. Auditors have only the fifth.

### 5.1 Why the foundation will not take this position

Three reasons, all structural rather than about pace.

1. **Neutrality.** A trust and risk layer has to be neutral towards the platform. The organisation
   that stewards the network and is its largest validator cannot sell an independent assessment of
   that network's risk.
2. **Curation against a walk.** The foundation curates — 283 contracts by hand. Coverage requires
   walking all of mainnet and re-measuring continually. That is a product's work rather than a
   foundation's, and the difference shows in the pace: ten hashes in two and a half weeks.
3. **Off-chain data.** Scam markers, forgery labels, the internal balances of gift marketplaces —
   none of it is on the chain and none of it will enter an official catalogue.

### 5.2 The moat

The moat accumulates by itself: every contract decoded improves the dictionary, and a better
dictionary makes decoding the next one cheaper. Code copies; coverage does not.

### 5.3 The honest limit on size

TON is on the order of half a percent of global DeFi value locked. A data layer over one network runs
into the size of that network.

A large outcome needs one of two things, and the first does not depend on us:

- Telegram genuinely brings its audience, the network grows by an order of magnitude, and the layer
  grows with it;
- **or** the architecture allows the same product to move to other networks, with TON as the place
  where the capability was proven.

The second is a design decision, and it has to be taken before the first integrations cement the
binding to TON.

---

## 6. What rots and what does not

This decides the order of going public more than value does.

| Asset | Rots | Why |
|---|---|---|
| Static detectors | **fast** | the language ships roughly every six weeks; the grammar knows an older version than the compiler does |
| The method dictionary | **no** | the hash of deployed code never changes |
| The corpus and its labels | **no** | 409 labelled sites in 89 contracts are data |
| The risk signals | slowly | they work on bytecode and do not depend on a language version |
| The discipline: a threshold before the measurement, a negative result published | **no** | it is a method |

**The rule that follows: what goes out first is what does not rot.**

---

## 7. What to strike at

Four strikes, cheapest first. Each has a gate with a failure condition, as in the roadmap.

### 7.1 Strike one — publish the dictionary

The artefact is ready, it needs no infrastructure, it has no competitor, and its audience does not
overlap with the developer toolchain's.

**Gate:** an outsider downloaded the dictionary and used it without building `sting`.
**Failure:** two months with no external consumer — meaning either the artefact was not found or
indexers do not need it, and §3 has to be revisited.

### 7.2 Strike two — turn the dictionary into an answer rather than a file

A dataset gets downloaded; an answer gets bought. "Give me a transaction hash, get back what
happened, in words and fields." That is the product of §5 rather than preparation for it.

**Gate:** an outside team calls the service in production, and losing it would break their product.
**Failure:** every consumer is satisfied with the dataset and none pays for the answer — then the
extraction layer stays a public good and the business is in §4.

### 7.3 Strike three — move the authenticity rule across

One measured rule, a known scale, and a crate already waiting for it. It adds a second answer beside
"what happened": "and was it genuine".

**Gate:** the rule reproduces on this corpus at an accuracy named **before** the move.
**Failure:** it does not transfer beyond the walk it was derived from — then it is an observation
about the gift market rather than a detector, and it stays where it is.

### 7.4 Strike four — keep security as the measure

A public security benchmark for TON, with the official linter run through it. The competitor
publishes no numbers, and nobody has a benchmark at all.

**Gate:** a third-party analyser is run, the numbers are published, the method is reproducible.
**Failure:** the method is disputed and not defended — then we are a participant rather than a
measure, and §2 loses its real function.

---

## 8. What not to do

| Not this | Why |
|---|---|
| Chase the official linter on rule count | a fight for one slot in a toolchain, which distribution wins |
| Build an indexer or an RPC | dozens of providers, and the open pipelines are free |
| Add more duplicate detectors | the contribution there is **a number on a shared corpus**, not a second copy of the same rule |
| Invest in grammar coverage for its own sake | measured: the gaps that remain lose no executable code |
| Build payment primitives, checkout, subscriptions | the foundation ships those, and three steps of its roadmap are undisclosed |

---

## 9. The correction this document makes to the open questions

[`OPEN-QUESTIONS.md`](./OPEN-QUESTIONS.md) §1 is phrased as a question about the project: what
separates this tool from the official linter in a user's eyes. That is a question about **one**
surface of three.

| Surface | Audience | Does the question apply |
|---|---|---|
| security | whoever writes Tolk | **yes, and sharply** |
| abi-layer | indexers, explorers, analytics | **no** — they do not install the toolchain |
| scamcheck | wallets, marketplaces, the end user | **no** |

While it stands as a general question, it holds two products in the state "demand unchecked" that it
has nothing to do with. The correction: split it into the security question, which is open; the
extraction question, whether anyone pays for an answer on top of the dataset; and the risk question,
whether the buyer is a wallet or a marketplace.

---

## 10. Risks

| Risk | How near | What softens it |
|---|---|---|
| **No external readers** | present | strike one; everything else is secondary until it is done |
| The foundation widens its catalogue into a walk | medium | the curation pace is measured: ten hashes in two and a half weeks |
| The static contour falls behind the language | permanent | any milestone longer than a quarter has to contain a step that catches the language up |
| The size of the TON market | structural | §5.3: portability to other networks as a design decision |
| One person | permanent | code closes with one; partnerships, grants and first integrations do not |

---

## 11. What this document does not cover

It does not say what is being built or in what order — that is [`ROADMAP.md`](./ROADMAP.md), and where
the two disagree the roadmap is about sequence and this is about direction. It does not measure the
competitors itself: the numbers come from [`domains/landscape.md`](./domains/landscape.md), which
records when each was taken. And it decides nothing about a buyer: a buyer is not a fact about the
tree, and every claim here about one is a hypothesis with a failure condition attached.
