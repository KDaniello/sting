# Deep dives

One document per area with real subject matter of its own: what is being solved there, which number
confirms it, and where the boundary of applicability runs.

## 1. The documents

| Document | Area | The key number |
|---|---|---|
| [`landscape.md`](./landscape.md) | who else is in the niche: the official toolchain, its linter, the ABI catalogue, release pace | the official linter: 39 rules, **no** published accuracy figure |
| [`precision-sources.md`](./precision-sources.md) | where accuracy is found: code, audits, CTF ranges, registries — with dates and licences | **34** reports and **233** vulnerabilities; 5.5 of 18 official classes covered |
| [`benchmark.md`](./benchmark.md) | measuring an analyser, this one included: one dataset, one scoring rule | **81.5%** ours against **87.1%** theirs on the four classes both cover |
| [`hunt.md`](./hunt.md) | the detectors against code nobody wrote for them, inside published bug bounty scopes | **20.0%** precision on live code against **82.4%** on the fixture corpus |
| [`second-language.md`](./second-language.md) | reading FunC, and what a detector keeps when it crosses | reach **1 of 18** scope entries to **12**; Tolk unchanged to the digit |
| [`detectors.md`](./detectors.md) | the 14 detectors: what each looks for, its measured accuracy, its honest limits | the aggregate lives in `corpus/baseline.json` |
| [`dataflow-cfg.md`](./dataflow-cfg.md) | the generic worklist solver, control-flow construction, interprocedural reach and its limit | five dataflow detectors, interprocedural reach of **one hop** |
| [`chain.md`](./chain.md) | one client for every public TON service: pace, retries, keys | chain access existed **six** times in Rust and now once |
| [`emulator.md`](./emulator.md) | the FFI to `libemulator`, memory ownership, the static-to-dynamic bridge | the only one of four bindings that does not leak |
| [`abi-ir.md`](./abi-ir.md) | the normalised contract schema, and why exactly that shape | **100%** structural completeness across six categories |
| [`extractors.md`](./extractors.md) | three extractors: two through a compiler, one heuristic | the heuristic one: 10 of 13 real contracts from an external corpus |
| [`method-dictionary.md`](./method-dictionary.md) | recognising a contract with no source at all | **1388** method ids in the dictionary, **86.2%** of mainnet |
| [`decode.md`](./decode.md) | reading a message: what operation it is, in words and fields | **26.4%** of 10 000 mainnet transactions unnamed |
| [`sybil.md`](./sybil.md) | coordination between addresses: ingestion, the graph, the score, and what it refuses to claim | **155** tests green with the store up; the retro campaign has **no public labels** |
| [`service.md`](./service.md) | the HTTP service: limits, isolation, real load | two orders of magnitude between the two languages it serves |
| [`scamcheck.md`](./scamcheck.md) | risk signals for jettons and gift collections | four jetton signals, composite scoring |

## 2. What these areas have in common

**Each is taken to a number rather than to "it works".** Where there is no number, that is said
outright — the precision of one heuristic tier over storage fields is **methodologically
unmeasurable** on the current ground truth, which is not the same statement as "poor".

**Each knows its boundary.** `AC` does not separate a forgotten check from a deliberately public
function. `UDA` sees one hop, not two. The bridge does not reach findings that hinge on the message
sender. The emulator does not execute precompiled contracts. Those boundaries are written in the doc
comments of the modules themselves, not only here.

**Each grew from a real miss.** Not one recognition form in `UR`, `AC` or `GVR` was designed in
advance in case it might be needed: every one arrived after a specific site in real code that would
otherwise have stayed a false positive or a miss.

## 3. What this directory does not cover

It does not repeat the architecture: why the boundaries are where they are is
[`ARCHITECTURE.md`](../ARCHITECTURE.md). It does not restate a contract: what crosses a boundary is
[`API.md`](../API.md). A deep dive exists where the subject itself has depth, and where it does not,
there is no document.
