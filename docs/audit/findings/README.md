# The findings log

One entry per hypothesis, refuted ones included. A negative result weighs the same as a finding: it is
data rather than wasted time ([`../RULES.md`](../RULES.md) §3).

A hypothesis that quietly disappears is a defect of the same class as documentation disagreeing with
code: three rounds later nobody can recall what was already checked, and the cycle starts rediscovering
the same thing.

---

## 1. How it is kept

- One file per hypothesis, numbered and named.
- Created **at the moment the hypothesis is stated**, before the probe, which is how the expected
  outcome gets fixed (gate S1 in [`../GATES.md`](../GATES.md) §2).
- Updated as it moves through the [pipeline](../GATES.md) §3, never rewritten after the fact.
- The status only ever moves forward: from a hypothesis to one of the terminal states.

## 2. Statuses

| Status | Meaning |
|---|---|
| hypothesis | stated, no probe started |
| screening | the prefilter is running |
| building | a proof or a reproduction is being built |
| refuted | refuted, with the durable negative fact named |
| negative | the probe executed correctly and there is no defect — a valid negative |
| out of scope | outside the scope or a known behaviour, with the clause named |
| confirmed | the proof shows the effect and every gate passed |
| submitted | sent, with the date |
| accepted or rejected | the programme's answer |

---

## 3. The summary table

Updated at every change of status. Empty: the direction is only starting.

| # | Hypothesis | Target | Status | Date |
|---|---|---|---|---|
| — | — | — | — | — |

---

## 4. The entry template

```markdown
# NNN — <short name of the hypothesis>

- **Status**: hypothesis
- **Target**: machine | consensus | contract:<name> | tooling
- **Scope**: in scope / the clause — checked <date>
- **Version**: <the version at the time of the probe>
- **Build**: <commit, build date>

## The hypothesis, written BEFORE the probe

I claim that <…> and expect to see <a specific observable outcome>.

## Grounding in the code

- mnemonic or function: <file and line>
- where gas is charged: <…>
- version gate: <yes or no>
- surface: consensus / get method / tooling / debug / host library

## Prefilter

- run: <what was executed>
- result: <confirmed or discarded>

## Proof

- kind: their prober / local network deploy and trigger / mixed network
- prober: <path>
- command: <the exact command>

## Measurements

| Parameter | Value |
|---|---|
| exit code | |
| gas | |
| peak memory | |
| user, system, elapsed | |
| scale parameter | |

The baseline compared against: <the charged equivalent> and its numbers.

## The attempt to refute it (gate S2)

What it was attacked with: <a version gate? gas charged somewhere? an unreachable path?>
Result: <it held, or it was refuted by this durable negative fact>

## Verdict

<confirmed / refuted / negative / out of scope> — because <…>

## Readiness check, if confirmed

- [ ] O1 a proof in their form
- [ ] O2 the current version
- [ ] O3 no debug flags
- [ ] O4 a realistic path
- [ ] O5 in scope today
- [ ] O6 not a known behaviour
- [ ] O7 self-immunity, for consensus
- [ ] S3 numbers measured
- [ ] S5 a clean checkout
- [ ] S7 one report
```

---

## 5. What this log does not hold

It does not hold an exploit. A proof is minimal and bounded (gate S4), and nothing here is published
before a fix reaches the network. It also does not hold anything about a target outside the scope
beyond the line recording why it was dropped.
