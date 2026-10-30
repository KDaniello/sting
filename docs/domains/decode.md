# Reading a message

What operation a message is, in words and fields, for anybody holding a transaction hash.

---

## 1. The gap this closes

A chain gives a reader a body: a run of bits opening with 32 that name an operation. An indexer gives
the same bits with the opcode pulled out as a number. Neither says what the operation **is** — three
public indexers were measured against the chain over 160 objects and diverged on values by 0.00%, so
the pitch "indexers have bad data" is refuted by our own measurement. What is missing is meaning.

An opcode is not a hash of anything. A get method's id is derived from its name, so recognising one
is reversing a one-way function through a dictionary of real names; a message's opcode is picked by
whoever wrote the contract. That makes it a number to be recognised rather than inverted, and
recognising it means having seen it declared in a contract somebody wrote.

---

## 2. Two datasets, and why they are two files

| Dataset | Learned from | Opcodes | Licence |
|---|---|---|---|
| [`operation-dictionary.json`](../../corpus-abi/operation-dictionary.json) | this repository's six corpora | **374** | CC0-1.0 |
| [`foundation-operations.json`](../../corpus-abi/foundation-operations.json) | `ton-blockchain/abis` at `90c4c6b` | **945** | MIT, [notice](../../corpus-abi/FOUNDATION-OPERATIONS-NOTICE.md) |

The first is the same walk that produces the method dictionary: one extraction, two datasets,
because extraction is what costs the minutes. The second is their catalogue read the same way every
other corpus is read — through this tree's own Tolk extractor, on their type files, with an
entrypoint appended that declares nothing (their files describe contracts and `tolk` will not emit an
ABI without one) and their siblings staged beside them (their files import each other). 285 of 285
files compile.

**They stay two files because they carry two sets of terms.** A file whose licence depends on which
entry a reader looks at is a file nobody can use. The merge happens in memory, at the moment a
question is answered, and what comes out of it is an answer rather than a dataset.

Two gaps in the learner surfaced while reading their sources. A contract that names a union as what
it accepts declares its messages in the arms rather than on the union, and a contract that names an
alias declares them one name further on. Both are followed now: the wallet's signed request is about
one in ten of the messages mainnet carries and was learned by neither dataset before.

---

## 3. What a decode says, and what it refuses to say

An opcode is chosen rather than derived, so two contracts are free to use one number for two
operations. When the datasets know two, **both readings come back and neither is preferred** —
choosing would invent the fact the datasets say they do not have.

A field whose shape the dataset states no reading for stops the reading there, and the answer says
which field and why. 110 of the 789 fields the learned dataset carries are in that state: 34 structs,
33 strings, 30 optionals of something other than a cell or an address, 12 maps, 1 builder. A decoder
that skipped them and carried on would present a partial read as a whole one.

Two opcodes are named by nothing a corpus can hold, because no contract declares them: TON's own
bounce marker and the plain text comment. A bounce also says **what** bounced — the marker is
followed by the first 256 bits of the body that came back, so its opcode is read and looked up. Its
fields are deliberately not: everything past the first few was cut off by the truncation, and
presenting those as values would be presenting the cut as data.

---

## 4. How much of real traffic can be read

Measured over 10 000 mainnet transactions, newest first, on 2026-08-23:

| Measure | Value |
|---|---|
| transactions read | 10 000 |
| carrying an operation | 6 967 |
| named | 4 331 |
| **unknown, of the operations** | **37.8%** |
| **unknown, of the transactions** | **26.4%** |
| read to the last field | 3 177 of the 4 331 named |
| distinct opcodes seen | 1 290 |
| **unknown, of the vocabulary** | **94.2%** |

Three shares because they answer three questions, and one number would have been the wrong count
whichever one it was. A message whose body carries no opcode is not an unknown operation — it is a
transfer of value, and saying so is an answer — which is the difference between the first two rows.
One busy application moves both of those and does not move the third.

Naming an operation and reading it are two different successes. 73.4% of the named messages read to
the last field of at least one candidate; the rest stopped at a field whose shape has no stated
reading.

**The gate is not met.** [`ROADMAP.md`](../ROADMAP.md) §10 asked for `Unknown` at or below 13.8%, and
that threshold came from a different quantity: 86.2% is the share of unique mainnet **code hashes**
the method dictionary recognises, and recognising a contract is not the same as knowing what a
message sent to it means. The measured share is what it is, and what would move it is written below
rather than the threshold being quietly restated to fit.

**Where the missing traffic is.** Four opcodes carry about a fifth of everything the datasets cannot
name, and the rest is a tail of more than a thousand. `abi-layer decode-sample` prints the twenty
most frequent unknown opcodes, which is the working list a growth pass starts from — the same loop
that took the method dictionary from 219 entries to 1 388: find the contracts receiving them, extract
their ABIs, learn. That pass has not been run.

---

## 5. The surface

`POST /v1/decode` takes a transaction hash or the body itself, and answers with the operation and its
fields. No source is read and nothing is compiled.

**Not knowing is an answer, not an error.** An opcode nothing names comes back `200` with
`unknown: true`. A `404` would say the request was wrong when it was this platform's knowledge that
ran out — and it would hide the share the product is measured by inside an error code.

Three limits bound three different things, and no two of them substitute for each other:

| Limit | What it bounds | Default |
|---|---|---|
| body size | what a caller may send | 1 000 000 bytes |
| request timeout | how long one call may take | compile timeout plus 30 s |
| result size | what the service will send back | 1 000 000 bytes |

A result over the limit is refused with a named error rather than truncated: a cut-off answer that
looks whole is worse than no answer.

Keys and quotas answer a separate question from the rate limit. The limit bounds a burst from one
address; a quota bounds a period for one caller however many addresses it comes from. A deployment
naming no keys stays open, which is what this service has been since it was written. An unknown key
and a spent quota are told apart — `unauthorized` and `quota_exhausted` — rather than collapsed into
one `429`, so a caller can tell "your key is wrong" from "come back tomorrow".

---

## 6. What it does under load

One machine, over a loopback, 600 requests per step, with the blanket rate limit raised so that what
is timed is the decode rather than the limiter:

| Concurrency | Throughput | p50 | p95 |
|---|---|---|---|
| 1 | 1 181 req/s | 0.8 ms | 1.1 ms |
| 4 | 4 305 req/s | 0.9 ms | 1.2 ms |
| 8 | 7 563 req/s | 1.0 ms | 1.5 ms |
| **16** | **8 467 req/s** | 1.8 ms | **2.4 ms** |
| 32 | 7 445 req/s | 4.1 ms | 5.0 ms |
| 128 | 7 943 req/s | 5.2 ms | 10.2 ms |

**Throughput stops rising at concurrency 32.** Latency past that point is queueing: every request is
still answered, and each waits longer.

**The first run of this measurement measured the wrong machine.** A connection per request exhausted
the host's ephemeral ports within seconds — 14 682 sockets in `TIME_WAIT` — and the steps that
followed reported a service answering three requests of six hundred when what had run out was
sockets, not capacity. The generator shares one connection pool now, never sends fewer than eight
requests per worker (a step where each worker sends two measures thread startup), and counts refused
requests apart from slow ones, so a step that measured this machine says so.

The generator is its own crate for a reason that is a rule rather than taste: only `sting-chain` may
name an HTTP client, because a second client is a second retry policy waiting to disagree with the
first. A load generator reaching the service through a paced, retrying client would measure the
pacer, so the exception is one crate that does nothing else and is recorded with that reason.

---

## 7. What this does not do

It does not fetch a contract's source, and it does not check that the contract at the other end is
the one the dataset learned the opcode from — that is a claim about an address, not about a body.

It does not decode a body's nested cells. A referenced cell is named by its hash and left unread,
which is what makes `forwardPayload` a measurement rather than a value.

It says nothing about **why** a message was sent, only what it is. An operation named
`AskToTransfer` is a request a contract accepts, not evidence that a transfer happened: what the
receiving contract did with it is a transaction's own outcome, and reading that is a different job.

---

## 8. What this document does not cover

It does not describe how the method-id dictionary recognises a contract with no source — that is
[`method-dictionary.md`](./method-dictionary.md) — nor how a schema is extracted from source, which
is [`extractors.md`](./extractors.md). It describes one product surface, the two datasets behind it,
and the share of real traffic it can read.
