# Coordination, and what it is not

Who funded whom, who moves together, and how sure of it this platform is willing to be.

---

## 1. What is being detected

A farm is not a person with many wallets — that is a conclusion about people. What the chain shows
is coordination: addresses funded from one place, acting inside the same minutes, in the same shape.
Nine crates take that from raw blocks to a risk score, and every one of them stops short of the
sentence "this is one person".

| Crate | What it does |
|---|---|
| `sting-sybil-types` | the chain facts the contour shares: logical time, hashes, amounts, finality |
| `sting-sybil-ingestion` | follows the masterchain tip, writes raw rows idempotently, replays a range |
| `sting-sybil-store` | partitioned columnar files: one writer, one rule about what a partition is |
| `sting-sybil-graph` | jetton wallets resolved to owners, transfers correlated, edges between people |
| `sting-sybil-features` | what a wallet looks like as numbers: spacing in time, counterparties, shape |
| `sting-sybil-scorer` | the distilled linear model the request path scores with |
| `sting-sybil-cluster` | who belongs together by topology, with the giant component broken down |
| `sting-sybil-api` | the request path: an address in, a risk score out |
| `sting-sybil-python` | the Arrow bridge, so the training side reads what Rust built without a copy |

**155 tests, all green**, with the column store up — the 18 that skip without one included: 14 need
the store, and the rest need a live chain or a full-scale run.

---

## 2. The store is in the repository, not on one machine

[`compose.yml`](../../compose.yml) is one service bound to the loopback. A compose file is reviewed;
a hand-configured host is not, and a host only one machine has is a suite only one machine can run.

The store's acceptance tests share one database and assert about its contents, so two of them at once
see each other's rows. That is a nextest group now rather than an instruction in a doc comment
somebody has to remember — an instruction that has to be remembered is a rule the gate does not have.

---

## 3. A fixed range replays to the same rows

Ingestion writes rows. A summary of counts is not a snapshot of them: two ingestions of one range can
agree on how many transactions they hold and disagree about which, and a check that compares only the
counts passes on the day that matters.

So the whole of it is written down, in identity order, with the bodies as they arrived:

| Measure | Value |
|---|---|
| range | masterchain blocks 87 210 000 to 87 210 003 |
| transactions | 46 |
| messages | 60 |
| distinct opcodes | 19 |
| bytes | 50 543 |
| runs that agreed | 3 |

A finalized block is a fact that does not move, so a replay has to reproduce it. `make
snapshot-check` is that check, and `make test-live` runs it. When it fails, either the chain answered
differently or this ingestion is not deterministic — and both are things whoever depends on it needs
told.

---

## 4. The threshold was registered before there was a campaign

A threshold named after a run is not a threshold: it is a description of the result, and every result
passes one of those. So it went into the repository on its own, in commit `3cf08dc`, before anything
was measured:

| Check | Threshold |
|---|---|
| precision | at or above 0.90 |
| recall | at or above 0.50 |
| owners on each side | at least 1 000 |

Precision above recall deliberately. A false positive is an accusation against somebody who did
nothing, and its cost is not the mirror image of a miss.

`sting-sybil-cluster retro` holds a run against that file and cites the commit it was registered in,
read out of git rather than typed. Three decisions in its arithmetic are not arbitrary: an owner the
campaign excluded that no score covers counts as missed rather than dropped, a run that flags nothing
scores zero precision rather than one, and a comparison smaller than the registered size fails on its
size and says so.

**The campaign has not been run, and the reason is that the labels do not exist in public.** What was
looked for, on 2026-08-23: an airdrop on TON whose exclusion list names addresses. Hamster Kombat
banned 2.3 million wallets and published the count rather than the wallets. `ton-studio/ton-labels` is
a real labelled dataset of TON addresses — MIT, about 2 485 addresses, 17 categories — and none of its
categories is sybil, farmer or excluded. The exclusion lists that do exist address-by-address belong
to other chains, and a detector measured against Ethereum labels would be measuring a different
population.

So the gate's precision and recall are **not measured**, and this is not a soft failure to be
reported as "in progress": everything the measurement needs exists except its input, and the input is
somebody else's decision to publish. What would unblock it is one campaign that publishes addresses,
or a labelled set built here with its own provenance — which is a different piece of work, with its
own honesty problem, since labels this platform makes and then measures itself against are a mirror.

---

## 5. The request path

An address in, a risk score out, from a warm cache with no network call in the way. A key names a
tier, and a tier carries two limits, because they answer two questions:

| Limit | What it bounds |
|---|---|
| rate | how hard one key may press at one moment |
| quota | how much it may spend before the window turns over |

A tier with only a rate lets one key spend a day's worth of the service every day forever; a tier
with only a quota lets it spend the day's worth in a second. An unknown key, a burst and a spent
quota are three answers rather than one `429`, because a caller told only that cannot tell "your key
is wrong" from "slow down" from "come back tomorrow".

Measured on one machine over a loopback, 600 requests per step, with the tier's limits raised so that
what is timed is the service rather than the limiter:

| Concurrency | Throughput | p50 | p95 |
|---|---|---|---|
| 1 | 2 855 req/s | 0.3 ms | 0.5 ms |
| 4 | 8 626 req/s | 0.5 ms | 0.6 ms |
| **8** | **9 999 req/s** | 0.8 ms | **1.0 ms** |
| 16 | 9 749 req/s | 1.6 ms | 1.8 ms |
| 64 | 8 425 req/s | 7.4 ms | 7.8 ms |
| 128 | 7 596 req/s | 16.4 ms | 17.8 ms |

**Throughput stops rising at concurrency 16.** A miss is answered as partial rather than sent to
look, which is what makes a latency promisable at all: the alternative is a request whose time is
somebody else's endpoint's time.

---

## 6. What this contour does not claim

**It does not say a cluster is one person.** The graph shows coordination — shared funding, shared
timing, shared shape — and ownership is a statement about people that no edge supports.
[`ROADMAP.md`](../ROADMAP.md) §11 puts that outside this milestone deliberately, and it is not a
formality: a product that reported ownership would be reporting a guess in the voice of a
measurement.

**It does not claim its own labels are truth.** A retrospective comparison measures agreement with
somebody else's exclusion list. An owner this platform flags and a campaign did not may be a farm the
campaign missed, and the arithmetic cannot tell that from a false positive — which is why the result
is precision and recall against a named list rather than accuracy.

**It does not resolve an address it has not ingested.** A miss is a partial answer, said as such.

---

## 7. What this document does not cover

It does not describe the funding graph's construction or its exclusion rules in detail, nor the
feature vocabulary the model reads — those live in the contour's own crate documentation. It
describes what the contour is for, what is in the repository so that it can be run, and which of its
numbers exist.
