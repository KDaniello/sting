# Dataflow and control flow

How the dataflow analysis is built, where its boundary runs, and why it runs exactly there.

---

## 1. Two layers, deliberately separated

**`sting-dataflow`** is a generic worklist solver that knows nothing about Tolk: a semilattice with a
bottom element and a join, a transfer function per block, the solver, the graph and its block
identifiers. Exactly what a forward "may" analysis needs, and nothing beyond it.

**The control-flow builder in `sting-detectors`** sits on top of it and knows the language's syntax.

The separation paid for itself twice. First, the solver is tested against an honest fixed point over a
diamond-shaped graph, separately from any detector. Second, the solver crate is the one whose own
documentation claimed independence from the language; when a review found a declared but never used
dependency contradicting that claim, the contradiction was resolved in favour of the claim rather than
of the manifest.

---

## 2. Four detectors on dataflow, ten on the syntax tree

| Kind | Detectors | Why |
|---|---|---|
| dataflow | `ID`, `TLB`, `UBM`, `UDA` | they need to know what happened **earlier** along an execution path |
| AST | `GVR`, `LEP`, `PL`, `BR`, `UR`, `AC`, `UTA`, `SUA`, `EPS`, `DEPR` | structure is enough |

The split is not cosmetic: a detector declares its own kind, and that decides which machinery is
brought under it.

**`ID` and `TLB` share one event machine.** It classifies functions as store-like and load-like and
matches events; `ID` compares order and `TLB` compares a field's shape. The common code was factored
out when the second consumer appeared, not in advance.

**A known limitation of that machine, conservative in direction**: it requires an **exact method-name
match** between branches to classify a function. Two branches writing one field through different but
equivalent methods produce an unclassified conflict. That is not a false positive but a documented
recall gap, and the direction of the error is the right one.

---

## 3. Panics the linter cannot see

The three panic paths are denied workspace-wide. Assertions and slice indexing **are not covered by
that**.

A full review found the one place in the security contour where "no panics in library code" rested on
convention rather than on the tool: the control-flow constructor asserted on a malformed graph
request — an edge to a block that does not exist, for instance.

**Fixed**: it returns a typed error. The reason is recorded in the error type itself: this is the
caller's defect in how the graph is built, not a runtime condition the constructor could recover
from.

**Block and predecessor access were deliberately left alone.** Their indices always come from an
already-validated graph — the same guarantee that makes indexing a vector after checking its length
safe. Extending the result type there would add noise at every access without closing one real
scenario.

---

## 4. Three boundaries to know before reading findings

### 4.1 Branch conditions are not scanned for events

The builder **never** looks inside the condition of a branch for events. The consequence is concrete
and unpleasant: a send guarded by a comparison inside the condition itself is invisible to `UDA` **by
construction** — although that is exactly the open-relay pattern the detector was built for.

The boundary is written here rather than inferred by a reader from the absence of findings.

### 4.2 Interprocedural reach is one hop, and that is a hard limit

`UDA` is bounded to a single hop by an explicit flag. A chain of two hops is **invisible in
principle**.

`UTA` uses the same one-hop mechanism to prove that an amount traces back to a signed read: either in
the same function, or through one call passing into that parameter position a value bound to a signed
read in the caller. That is exactly the shape the real CTF contract had, and deliberately no more.

### 4.3 Two detectors of fourteen are cross-file

`AC` and `GVR` resolve names through the project index — a real compiled index of symbol types. The
other twelve work within one file.

**Exactly one match** by bare name is required. The module cannot know a receiver's real type from the
syntax tree, which has no types, nor from a flat table of signatures, which is not a table of variable
types. Ambiguity — zero matches or two — is treated as unresolvable: the same fail-safe principle as
everywhere else in these modules.

---

## 5. A negative result worth more than the code

An attempt to add cross-file resolution of the bounce handler for `UBM` was written, tested, and
**deleted** rather than left as a dead path.

The reason was established by running the real compiler rather than by reasoning. The compiler
**explicitly refuses** to compile when a contract's entry point is declared outside the contract's own
file, and says so:

```
error: all contract entrypoints must be placed in the contract file 'Main.tolk'
help: keep onInternalMessage and get fun just below contract, not in other files
```

Checked two ways: a handler file with a duplicate contract header compiles, and its handler **does not
appear** in the symbol index at all; without the duplicate header, the compilation error above. A
control check on an ordinary non-entry-point function in the same imported file resolves normally,
isolating the problem to entry points.

**The conclusion**: the pattern that work set out to close cannot exist in any Tolk project that
actually compiles. The mechanism would have resolved something that does not occur.

Symmetry with `GVR` for its own sake did not happen, and the conclusion turned out to be worth more
than working code.

---

## 6. A risk that is not closed

**A grammar update can silently break the detectors.** There is a precedent: the comment node turned
out to be *named*, so the most common real form of an early-return guard — with a comment inside the
block — broke the "exactly one statement in the block" check in `UR`.

Nothing in the parsing or detector crates protects against that class recurring on the next grammar
update. It is recorded in [`TESTING.md`](../TESTING.md) §9 and in the risk register of
[`../ROADMAP.md`](../ROADMAP.md).

The published grammar did not parse a modern contract block at all, and a text scan stood in for it.
That scan folded two adjacent blocks into one and lost the second contract entirely, taking the
per-contract attribution three detectors rest on with it. The vendored grammar has the rule, the scan
is deleted, and a real node cannot fold.

---

## 7. What this document does not cover

It does not list what each detector looks for — that is [`detectors.md`](./detectors.md) — and it does
not describe the parse itself. It describes the machinery under both, and the three boundaries a
reader has to know before treating an absence of findings as evidence.
