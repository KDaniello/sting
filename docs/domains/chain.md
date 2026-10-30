# One path to the chain

The client every part of this platform reaches a public TON service through: one pace, one retry
policy, one place a key is read, one place it is sent.

---

## 1. Why one client and not five

Chain access existed eight times, and each copy had its own idea of what being refused means.

| Where | Transport | Retries | Pace |
|---|---|---|---|
| `sting-corpus` | `ureq`, blocking | 5 attempts, exponential | none |
| `sting-corpus::LibraryResolver` | its own `ureq` agent | 3 attempts, 400 ms | none |
| `sting-abi-coverage::walker` | its own `ureq` agent | 4 attempts, 500 ms | one pause per page |
| `sting-scamcheck::holder_concentration` | its own `ureq` agent | none | none |
| `sting-scamcheck::wallet_reputation` | its own `ureq` agent | none | none |
| `sybil/ingestion` | `reqwest`, asynchronous | 5 attempts through `backon`, wrapping the source rather than the request | none |
| `sting-karman-data` | 325 lines of `bash` and `curl` | 40 attempts | one `sleep` per call |
| `tools/ton-res` | Python and a liteserver | its own | its own |

Seven of the eight in Rust, and two of those inside one crate: the second was written because the
first's retry loop was private to its module, and the comment saying so is what made it visible. Two
more retried nothing at all, so a refusal reached the caller as a status. A run throttled in one of
them behaves differently from a run throttled in another, and no number of careful copies makes that
one behaviour.

**`sting-chain` is that one behaviour.** The core is synchronous, because four of the five callers
are, and the asynchronous one gets a thin adapter above the core rather than the core getting a
runtime. A library that pulls a runtime into a linter's dependency path to make one HTTP call has
decided for its callers.

---

## 2. What one call does

| Step | Rule |
|---|---|
| pace | wait until the interval since the last call has passed |
| ask | the query in the order it was written, the key in the header its provider reads |
| refused (`429`) | wait the server's own `Retry-After` if it sent one, otherwise double the backoff |
| unanswered | a dropped connection, a TLS failure or a timeout is tried again on the same count |
| server error (`5xx`) | tried again: the service is saying it could not answer this time |
| client error (`4xx`) | reported, never retried |
| answered | read as the JSON the caller asked for, or reported as a body that is not that |

**One limiter per service, not per client.** A limiter held by a client is a limiter per client, and
a caller building a client per call — which both risk signals did — has no pace at all. The limiter
is handed out per service and network for the life of the process, so ten clients against one
service queue behind each other.

**A pace is not a retry.** A retry answers a limit that has already been hit; a limiter keeps it
from being hit. The sweep that walks twenty-four thousand addresses spends its time in the limiter
and never reaches the retry, and a run that only ever backs off after a refusal has already cost
the service a refusal it had to compute.

**An attempt is an attempt, not only a refusal.** The library resolver measured what the narrower
rule costs: a transient failure taken as final left a library cell unresolved, so a reprobe pass
under-counted, and the same pass run again picked up a few more each time. Retrying only a `429`
was the reason that crate had a retry loop of its own.

---

## 3. Where a request goes

Three services, two networks, six hosts. All six answered a live request on 2026-08-22.

| Provider | Mainnet | Testnet | Key header |
|---|---|---|---|
| `toncenter v3` | `toncenter.com/api/v3` | `testnet.toncenter.com/api/v3` | `X-Api-Key` |
| `toncenter v2` | `toncenter.com/api/v2` | `testnet.toncenter.com/api/v2` | `X-Api-Key` |
| `tonapi v2` | `tonapi.io/v2` | `testnet.tonapi.io/v2` | `Authorization: Bearer` |

**The headers were confirmed against the services rather than their documentation.** A deliberately
wrong key went to both in both headers on 2026-08-22: toncenter answered `401` to `X-Api-Key` and
ignored the bearer, tonapi answered `401` to the bearer and ignored `X-Api-Key`. A key in the header
a service ignores is a key that buys nothing and says so nowhere.

The older toncenter API stays for the one capability the newer one does not publish:
`getLibraries`, which resolves a library reference cell to the code it points at
([`method-dictionary.md`](./method-dictionary.md) §4).

The network is named rather than inferred from a base URL. A key issued for one network answers
nothing on the other, and a test that quietly ran against mainnet is a test that spent somebody's
quota.

---

## 4. The keys

One per service and network, read from the environment after `.env` is loaded if it is there.

| Variable | Raises |
|---|---|
| `TONCENTER_API_KEY_MAINNET` | both toncenter APIs on mainnet |
| `TONCENTER_API_KEY_TESTNET` | both toncenter APIs on testnet |
| `TONAPI_API_KEY_MAINNET` | tonapi on mainnet |
| `TONAPI_API_KEY_TESTNET` | tonapi on testnet |

**A missing key is not an error.** Without one the public tier answers about one request a second
instead of ten, so a keyless client paces itself at one rather than asking for ten and collecting a
refusal every tenth call. What a slow run prints is the variable that would lift it.

**A key is carried without being printable.** `ApiKey` renders its length under `Debug` and has no
`Display`; the only way to the bytes is `expose`, whose name is what a reviewer greps for. A struct
holding a key does not leak it through its own derived `Debug`, and that is a test rather than a
convention.

---

## 5. What is tested, and against what

A stub server on a real socket, in every run, on every machine: refusal then success, refusal until
the attempts run out, a server stating its own delay, a server error retried and a client error not,
the query's order, the key's header, and the pace held between calls.

**A socket rather than a container.** What has to be exercised is one HTTP exchange, and a hundred
lines of `std::net` give it directly. A container would add a dependency the test skips itself on
when it is absent, and a skipped test that reads as a pass is the failure mode this repository
refuses by name ([`../TESTING.md`](../TESTING.md) §2). The container stays where it buys something a
socket cannot — a real database with a real schema.

---

## 6. The asynchronous caller

One of the six was asynchronous, and what it gets is an adapter rather than a second client:
`AsyncChainClient`, behind the `async` feature, off by default. A clone shares one client and
therefore one pace — two clients against one service is what the public tier refuses.

**It is not asynchronous underneath.** A call occupies one blocking thread for its duration, the
wait for the pace included, and `tokio::task::spawn_blocking` is what keeps that off the runtime's
own threads. The cost is real and bounded by the pace itself: ten requests a second is ten threads
a second, not ten thousand.

**The retry moved with it.** `sybil` wrapped its whole block source in a retry, so a failure to
read one page cost five re-reads of the page *and* whatever the transport did underneath. The
wrapper is gone, its two tests moved to where the retry now lives, and what they exercise is a
server that refuses twice and then answers — which is what a flaky indexer actually is.

---

## 7. The one that does not merge

`tools/ton-res` reaches a liteserver in Python and stays there. The reason is recorded rather than
implied, because an exception nobody argued for is an exception that grows.

| What it is | A spike that measured how far three public indexers diverge from the chain: 0.00% on values, over 160 objects |
| --- | --- |
| How often it runs | a few times a year, when the question is asked again |
| What it reaches | a liteserver, through `pytoniq` and `pytvm` — a protocol this platform speaks nowhere else |
| What it is worth | a proven state at a named masterchain block, not throughput |

**Porting it would buy nothing and cost the proof.** Its value is that it executes a get method
against state a liteserver served for one block, which is a different claim from anything an
indexer answers — and the library that speaks that protocol is the Python one. A Rust rewrite would
be a second implementation of a protocol nothing else here uses, to run a few times a year.

Its transport is therefore its own, and this is the boundary: `make lint` counts a crate naming an
HTTP client, and `tools/ton-res` is not a crate. A second Python tool reaching an indexer would not
be covered by that count either, which is the limit of the check and is named here rather than
discovered later.

---

## 8. What this crate does not do

It does not know what a contract is: it carries requests and answers as JSON, and every meaning
above that belongs to the crate asking. It is not an indexer and holds no chain state. It does not
cache — how long an answer stays true depends on what was asked, and the caller is what knows that.
It paces one client rather than a machine: two processes at ten requests a second are twenty.

---

## 9. What this document does not cover

It does not describe what is asked of a service: resolving an address to code is
[`../DATA.md`](../DATA.md) §5, and walking a block for transactions is
[`method-dictionary.md`](./method-dictionary.md) §6. The one tool that stays outside this client is §7, with the reason it stays.
