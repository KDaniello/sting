# Gates

The conditions a finding has to satisfy before it goes to the intake channel. Two levels:
**official**, whose violation is an instant refusal by the programme, and **ours**, whose violation
means sending noise to the team we want to join.

A gate is a checkable condition rather than an intention. "Looks like a defect" is not a gate.

---

## 1. The official gates — instant refusal on violation

Taken directly from the programme's documentation and its proving skill. Each is hard.

| # | Gate | What to check |
|---|---|---|
| O1 | **A machine finding carries an executable proof in their form** | the prober runs under their assembler and shows the effect |
| O2 | **Only the current version** | the version is recorded in the finding's entry |
| O3 | **Not a debug opcode, not a debug sink** | the effect reproduces without debug flags |
| O4 | **A realistic path in a normal network** | no control of the host, of a validator majority, or of leaked keys |
| O5 | **The target is in scope** | checked against [`SCOPE.md`](./SCOPE.md) on the day of submission |
| O6 | **Not a known behaviour** | checked against [`SCOPE.md`](./SCOPE.md) §4 |
| O7 | **The probing node is self-immune**, for consensus findings | the node is alive until the effect is observed on honest nodes, or the run is void |

**O1 is the most frequent failure.** A machine hypothesis proven in our emulator **is not** an official
proof: the programme requires their form. Our binding is the prefilter, their prober is the artefact.
Both are needed, and in that order.

---

## 2. Our gates — do not send noise

Stricter than the programme's minimum. The programme would refuse a bad report; we want every report
to **raise** trust.

| # | Gate | Why |
|---|---|---|
| S1 | **The hypothesis is written down before the probe, with its expected outcome** | fitting a claim to what was seen is self-deception; the same null model the product's own method uses |
| S2 | **The finding survived an attempt to refute it** | before believing it: a version gate? gas charged somewhere we missed? an unreachable path? |
| S3 | **Every number is measured rather than estimated** | gas, memory, processor and sequence through real instrumentation; the build, the version, the prober and the exit code recorded |
| S4 | **A minimal proof rather than a maximal exploit** | proof at the boundary where the trend is visible, not taking a machine down |
| S5 | **It reproduces from a clean checkout** | before calling anything maintainer-ready, run it from a clean tree with only the needed artefacts |
| S6 | **The log entry exists and is complete** | one finding is one entry in [`findings/`](./findings/README.md), a negative result included |
| S7 | **One report, one finding** | not a bundle of raw hypotheses; each passes O1–O7 and S1–S6 separately |

---

## 3. The pipeline of a finding

The order is mandatory. A skipped step means the finding is not ready, rather than nearly ready.

```
  hypothesis (S1: written before the probe, with its expected outcome)
      │
      ├─ coarse scope check (O5, O6) ── out of scope? → log, stop
      │
      ├─ PREFILTER in our emulator ── for machine findings: fast screening in memory
      │       │                        hypothesis not confirmed? → log the negative, stop
      │       ▼
      ├─ proof by the official method
      │       ├─ machine   → their prober (O1)
      │       ├─ consensus → a mixed network, self-immune node (O7)
      │       └─ contract  → a local test network, deploy and trigger
      │
      ├─ measurement (S3) plus comparison against a charged baseline
      │
      ├─ attempt to refute (S2) ── refuted? → log the negative, stop
      │
      ├─ final gates (O2, O3, O4, S4, S5)
      │
      ├─ a clean run from scratch (S5)
      │
      └─ the entry is complete (S6) → submission (S7, one report)
```

**A negative result at any step is a result.** It goes into the log with the same weight as a finding,
and the cycle stops rather than twiddling parameters at random — which is the official checklist's own
rule.

---

## 4. A machine finding: the exact sequence

The most frequent and the most regulated case. The steps come from the official proving skill.

1. **Ground the hypothesis in the current code.** Find the mnemonic and its implementation: where gas
   is charged, where cells and stack entries are allocated, which version gates it, and which surface
   it belongs to.
2. **Prefilter in our emulator** — run the pattern against real bytecode in memory and discard what is
   obviously unfounded before touching their form.
3. **A minimal prober.** One opcode or a tiny sequence, run under a gas limit. Scale **one** parameter
   at a time.
4. **Measure host resources separately from the machine's own gas.** Memory and processor through real
   instrumentation, hangs through a timeout, huge output to a file. Record the stack, the exit code,
   the gas, the peak memory, the times and the scale parameter.
5. **Compare against a charged baseline.** For cells, the cheap conversion against the paid
   construction; for a nested run, a stack built outside against a stack built inside and paid for.
6. **Classify.** Confirm only when a bounded proof shows a **production-reachable** path where the
   machine's gas is flat or far below the host's memory, processor, output or cryptographic work,
   **and the code explains why**. Down-rank if a debug sink is needed. Reject on durable negative
   facts: bounded lengths, bounded key depth, a memoised traversal, explicitly linear gas, paid stack
   movement, a version gate closing a historical fix.

**Do not hard-code a gas number across versions.** Record the build, the version, the exact prober,
the exit code and the measured gas.

---

## 5. What makes a finding **not ready** — the check before submission

The quick final list. Any "no" sends the finding back into the pipeline rather than out.

- [ ] Is the target in scope **today**, with the documentation re-read?
- [ ] Is it absent from the known-behaviours list?
- [ ] For a machine finding: is there a proof in their form, not only our reproduction?
- [ ] The current version?
- [ ] Without debug flags?
- [ ] Is the path realistic without control of a host, a quorum or keys?
- [ ] Is every number measured, with the build and version recorded?
- [ ] Was the hypothesis attacked and did it survive?
- [ ] Does it reproduce from a clean checkout?
- [ ] Is the log entry complete?
- [ ] One report, one finding?

Eleven yeses mean it can be sent. Fewer means it cannot.

---

## 6. What this document does not cover

It does not say what to look for — that is [`PLAYBOOK.md`](./PLAYBOOK.md) — and it does not define
conduct, which is [`RULES.md`](./RULES.md). It defines the conditions under which something may leave
this repository, and every one of them is checkable.
