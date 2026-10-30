# Glossary

Terms from the three layers this project mixes: the TON chain, static analysis, and `sting`'s own
vocabulary. Nothing is translated where the industry has settled on a word.

---

## 1. TON and the TVM

**BoC**, a bag of cells — TON's serialised cell tree. Everything that moves between `sting` and the
emulator — code, data, stack, messages — is a BoC in base64.

**Cell** — the unit of storage: up to 1023 bits of data and up to four references to other cells. Not
a byte buffer: it is addressed bitwise, which is why a field's width is part of the contract rather
than a detail.

**Slice** — a read cursor over a cell. Reading advances it, and the unread tail is the source of a
whole class of defects, which is what `LEP` looks for.

**code and data** — the two cells that fully determine a contract's state: compiled bytecode and its
persistent storage. `sting fetch` pulls exactly that pair, and no source is among them.

**code hash** — the hash of the code cell. A fact that shapes data collection: thousands of addresses
share one code hash, so deduplicating by it saves sixty to ninety times the work on a mainnet walk.

**library cell** — a reference to code stored separately in the network. Until it is resolved the
bytecode looks unrecognisable, and the cost of getting that wrong is measured: 38.6% of
"unrecognised" contracts were unresolved library cells rather than a gap in the dictionary.

**precompiled contract** — a contract registered in `ConfigParam 45` and executed by a node in a
special way. Its raw bytecode **cannot run under an emulated TVM at all**, even on entirely real
data, which is what `ProbeOutcome::PrecompiledContractUnsupported` records.

**get method** — a read-only function callable without a transaction. It is identified not by name
but by a **method id**, a number derived from the name by a fixed formula.

**method id** — that number. The direct analogue of a function selector on Ethereum, and for the same
reason it admits a reverse dictionary — see [`domains/method-dictionary.md`](./domains/method-dictionary.md).

**bounce** — the mechanism returning an internal message the recipient did not accept back to the
sender. Sending a message **after** writing storage and not handling the bounce is what `UBM` looks
for.

**an external log message against an internal message** — the first **structurally cannot** bounce.
The distinction cost four false `Critical` before it was accounted for.

**TEP-64** — the metadata standard for jettons and NFTs. The decoder in `sting-scamcheck` is written
by hand because no ready one exists in the crate that would have supplied it.

**TEP-74** — the jetton interface standard. It standardises the **external interface and not the
storage layout**, which is why guessing a jetton wallet's internal structure was a real source of
false signals until it was replaced by the real bytes of real holders.

**Tolk** — TON's current smart-contract language and the successor to FunC. The single target of the
security contour.

**FunC** — Tolk's predecessor. Here it is a source of Tier 2 heuristics and of the converted corpus,
not a target of analysis.

**Tact** — a third TON language with its own Node.js compiler. Supported in the extraction layer for
the code base that exists, not for code being written now.

**Fift** — TON's low-level assembly language. Used here only to reassemble bytecode during
verification.

---

## 2. Static analysis

**AST** — the parse tree of a source file. Built by `tree-sitter`, whose parse survives syntax errors.

**CFG** — the control-flow graph inside one function.

**Dataflow, worklist** — the iterative algorithm that propagates facts across a CFG to a fixed point.
`sting-dataflow` is a textbook-correct generic implementation that knows nothing about Tolk.

**Semilattice** — the lattice of analysis values with a bottom element and a join. Exactly what a
forward "may" analysis needs.

**Intra-procedural against inter-procedural** — analysis within one function against analysis across
call boundaries. Every dataflow detector here is intra-procedural; interprocedural reach was added
narrowly and deliberately — one hop for `UDA`, cross-file resolution for `AC` and `GVR`.

**Taint tracking** — following whether an untrusted value reaches a dangerous operation. One detector
has it, and its limits are stated rather than implied.

**Precision** — the share of true findings among all of them, `TP / (TP + FP)`. The metric that
matters most: a tool with poor precision stops being read.

**Recall** — the share of existing defects that were found, `TP / (TP + FN)`. Here it is 100% for
every detector the baseline measures — **on injections**, and that qualifier is not cosmetic.

**False positive** — a finding where no defect exists. **False negative** — a miss.

**Golden fixture** — a reference pair of input and expected result, committed to the repository.

**Mutation engine** — injection of a defect of a known class at a known position in a file known to
be clean. The answer to a small corpus, instead of sampling.

**SARIF 2.1.0** — the standard output format for static analysers, understood by code-scanning
services and IDEs.

**`physicalLocation` against `logicalLocations`** — an exact coordinate in a file against a method
name and an offset. Bytecode-level tools are structurally capable only of the second; `sting` works
over source and owes the first.

---

## 3. `sting`'s own vocabulary

**Detector** — one registrable check. A trait of four methods: id, kind, default severity, and the
check itself.

**DefectClass** — the taxonomy of defect classes grown from eight external codes. **Not a ceiling**:
`EPS`, `UDA` and `DEPR` are real detectors outside that enumeration, which is why a detector id is a
string rather than an enum.

**Finding** — one result: detector, severity, message, a **required** remediation, a location, and
optional confidence, dynamic verification and suggestion.

**Remediation** — what to change, kept separate from what is wrong. The analogue of `rustc`'s `help:`.

**Suggestion** — a machine-applicable edit, filled by **exactly two** detectors and no others; why is
in [`API.md`](./API.md) §1.4.

**Confidence** — a number in `[0,1]` guaranteed by its constructor. Filled only after the scoring
pass, and the calibrated precision of the detectors is an input to the formula rather than a separate
metric.

**DynamicVerification** — the three outcomes of a dynamic check: confirmed with a proof of concept,
refuted, or inconclusive with a reason. Refuted does not delete a finding; it lowers its rendered
severity.

**Controller** — a unit of orchestration: lint, bridge, scoring, corpus, fix, report, emulation pool,
extract, registry.

**The path-based entry point against the source-based one** — the full-capability path, with a
project index, suppressions and a severity floor, against the deliberately narrowed path through
which precision is computed. The guarantee is one-directional: nothing available only to the first
can quietly move a calibrated number.

**ProjectStore** — the compiled index of a project that lets calls and structures resolve across
files.

**Hypothesis** — the description of a dynamic check's scenario. Written by a person in TOML; deriving
one from source is an openly unsolved research problem rather than a corner cut.

**Tier 1, 2, 3** — three ways to obtain a contract's schema: from the real compiler, from an AST
heuristic over source, and by statistical inference from on-chain behaviour with no source at all.

**ContractSchema** — the normalised schema, one for all three languages and all three tiers.

**`ir_schema_version`** — the version of **that** schema, moving independently of the versions of the
source compilers' artefacts.

**Method dictionary** — the reverse dictionary from method id to name, assembled from the real
corpus. It is what lets a contract be recognised when nobody has its source.

**Sweep** — a mainnet walk deduplicated by code hash, probing every new code.

**Reprobe** — re-examining previously unrecognised codes with a dictionary that has grown since. It
exists because a gap was found, not because it was designed.

---

## 4. What this document does not cover

It defines terms and does not argue for them: why a boundary is where it is belongs to
[`ARCHITECTURE.md`](./ARCHITECTURE.md), what a type guarantees belongs to [`API.md`](./API.md), and
what a number means belongs to [`TESTING.md`](./TESTING.md).
