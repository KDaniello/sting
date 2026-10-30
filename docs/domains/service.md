# The HTTP service

`sting-abi-service` is the one containerised contour. What was measured under real load, where the
bottleneck is, and what is not in the container.

The protocol and the error codes are [`API.md`](../API.md) §4.

---

## 1. How it is built

An async web framework over a runtime. Contract compilation moves to a blocking pool — it is blocking
by nature, since it is a subprocess, and keeping it in the async executor would starve the whole
runtime. Inside that pool a batch is parallelised across cores.

The router is assembled from two sub-routers and glued back deliberately: the rate limit applies to
the extraction paths and **never** to health and version. Otherwise a monitoring system under load
would be the first to receive a refusal and would declare the service dead exactly when it is alive
and busy.

---

## 2. What was measured under load

Real measurements on one machine on one day, six virtual cores and under eight gigabytes. Not a
projection and not a claim that it will hold in production.

| Scenario | n | Concurrency | p50 | p95 | Throughput |
|---|---|---|---|---|---|
| single extract, heuristic path | 100 | 16 | **22.2 ms** | 45.7 ms | **591.4 req/s** |
| single extract, subprocess path | 60 | 8 | **6968 ms** | 10 189 ms | **1.06 req/s** |
| batch of 50, heuristic path | 200 contracts | 4 | 37.1 ms per batch | 39.5 ms | **4577 contracts/s** |
| batch of 10, subprocess path | 100 contracts | 4 | 44 520 ms per batch | 54 738 ms | **0.773 contracts/s** |

Memory and processor:

| Metric | Value |
|---|---|
| peak resident memory | 382 MiB |
| resident memory between bursts | 11 MiB |
| peak processor | 528%, about 5.3 of 6 cores |

The processor peak is direct confirmation that the parallelism in the batch handler really works
rather than being declared.

**The request bodies are the real corpus rather than synthetic**: nineteen real contracts in one
language and five fixtures in the other, cycled when the sample exceeds them. A body stays byte for
byte real code rather than noise.

**The numbers are split by language deliberately.** Mixing a microsecond path and a multi-second one
into one set of percentiles would produce a meaningless figure that misleads both sides.

---

## 3. The first run found a real operational limit

With the previous defaults, a batch of **three** contracts on the subprocess path reliably received a
gateway timeout.

**That was not a routing or parallelism defect** — parallelism worked and the processor reached 556%.
The default budget had been calibrated without the real cost of a cold subprocess start under
contention: the ninety-ninth percentile of a single call is near twelve seconds at a concurrency of
eight.

The defaults were raised to sixty and ninety seconds — **the values from the second run, where every
request finished successfully**, rather than arbitrary ones. The headroom between them is its own
named constant with the reason inside it.

**The honest caveat that remains**: the report **was not re-run** with the new defaults. They were
verified at a batch size near three, not at the full limit under concurrent load. The headroom does
not scale with batch size — an open point rather than a closed one.

---

## 4. Three timeouts rather than one

```rust
compile_timeout: 60s   // kills the compiler subprocess — real enforcement
request_timeout: 90s   // the async backstop over the whole blocking call
batch_max_items: 100   // a hard limit, not a recommendation
```

The compile timeout is the only one that actually frees a resource: it kills the process and returns
the thread. The request timeout is a separate field rather than a derived one for two reasons: tests
need to trip that path in milliseconds independently, and a batch has a total duration that a
per-item timeout does not bound at all.

The classification of the outcome is not accidental either: hitting the compile timeout produces a
timeout code rather than a compilation failure, distinguished by a downcast. A client that received a
compilation failure on a timeout would go and fix their contract instead of reducing their batch.

The batch limit is **hard rather than advisory**: otherwise one request would outrun the per-request
timeout simply by aggregating enough compilations.

---

## 5. A market reference point, and where we sit against it

A public indexing service reports several million requests a day, which averages to about forty-three
a second — computed from their own figure rather than quoted.

- **The heuristic path**: 591 requests a second singly, thousands of contracts a second in a batch —
  an order above the reference.
- **The subprocess path**: about one a second, noticeably below. The bottleneck is **entirely** the
  cold start: the controller and the normalisation themselves run in the same milliseconds as on the
  fast path once the subprocess time is subtracted.

**The practical recommendation for an operator** that follows from these numbers: cap the batch size
for the slow language separately from the others, on the client side. One limit across three
incomparable languages inevitably either starves the fast path or breaks the slow one.

---

## 6. What is not in the container

**Tolk.** The vendored build is Windows-only and no Linux binary exists to mount into the image. So
the platform's main language, and the one where this project is unique, **does not work over HTTP and
has never been tested there**.

Its native throughput was measured separately at 115.7 contracts a second, by a different method — a
direct compiler call rather than an HTTP round trip through a container. The two do not compare one to
one, and no attempt is made. Its character is probably closer to the fast path than to the slow one,
since it is a native compiler rather than a runtime — but that is **an assumption rather than a
measured fact**.

Recorded as a gap rather than as a known inconvenience.

---

## 7. Resistance to untrusted input

A review looking specifically for adversarial input **found no panic** in the service.

Every error goes through one shape, a malformed body and an oversized one included: the framework's
own rejection bodies do not leak out unstructured. An internal error returns only a general message to
the client — internal paths and state do not escape, and the full detail goes to the server log.

**One real exploitable denial-of-service vector was found and closed** by a hardening pass. It was the
service's main real problem at the time of the full review, and it is gone.

---

## 8. What this service does not promise

- **Uptime is not measured.** The one load test was single-shot, local, on one machine. It does not
  check sustained load, and a local run cannot check it in principle. An availability figure over
  weeks remains unmeasured.
- **There is no public deployment.** The image builds and validates and runs nowhere.
- **Three languages are promised and two work** — §6.

---

## 9. What this document does not cover

It does not describe what is extracted — that is [`extractors.md`](./extractors.md) — nor the shape
that comes back, which is [`abi-ir.md`](./abi-ir.md). It describes one deployment and the numbers one
machine produced on one day, which is exactly as far as they can be read.
