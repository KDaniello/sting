# Auditing the chain's own code

A parallel direction: look for real defects in the **official TON crates** — the node, the virtual
machine, the standard contracts — and submit them through the official programme, the way that team
runs it.

This is not abstract ethical hacking. It is work against an existing, demonstrably live programme with
published rules, an intake channel and a reward. Our rules **sit on top of** the official method
rather than replacing it, because the goal is not only to find a defect but to enter the process of
the team that accepts them.

> **The programme's status was verified on 2026-08-05.** The bug-bounty repository is not archived and
> was updated days before; its last commits are scope edits from the preceding months, which is live
> curation rather than a forgotten file. The intake channel answers. Every node release of that period
> thanks bounty hunters in its changelog. The reward reaches six figures for a critical finding, with
> large awards locked for a year. Primary sources: [`SOURCES.md`](./SOURCES.md).

---

## 1. The first rule: rules before code

Not one line of somebody else's code is touched with intent to look until three conditions hold:

1. **The rules of conduct are accepted** — [`RULES.md`](./RULES.md). What we do and, more importantly,
   what we never do.
2. **The scope is understood to the letter** — [`SCOPE.md`](./SCOPE.md). What is in the programme, what
   is outside it, and where our tool genuinely helps and where it does not.
3. **The gates are defined** — [`GATES.md`](./GATES.md). The condition under which a finding may be
   submitted at all: the official ones, whose violation is an instant refusal, and our own, whose
   violation embarrasses us in front of the team we want to join.

Until those three are read, [`PLAYBOOK.md`](./PLAYBOOK.md) — what to look for on each target — does not
open.

---

## 2. Three facts that decide everything

Established by reading the official documentation literally rather than by paraphrase.

1. **A virtual-machine finding without a proof of concept in the chain's own assembly language is
   refused immediately.** Not a reproduction in the emulator — an executable prober. The official
   toolset has a skill for exactly that. Our emulator binding is fine as a fast prefilter for a
   hypothesis, and **the artefact that is submitted has to be in their form**.

2. **The compilers, the assembler, the client library and "the emulator crashes on bad data" are
   outside the bounty scope.** That is stated in their own documentation. Work on our own analyser is
   valuable and **is not the path into this programme**; compiler defects go to the language team
   directly. In detail: [`SCOPE.md`](./SCOPE.md) §3.

3. **There is a list of known behaviours that are not defects.** Reporting any of them is an instant
   loss of credibility. The full list is [`SCOPE.md`](./SCOPE.md) §4.

---

## 3. What is in this directory

| File | About | When to read it |
|---|---|---|
| [`RULES.md`](./RULES.md) | the rules of white-hat work: legality, do no harm, disclosure discipline, what we do not do | **before anything** |
| [`SCOPE.md`](./SCOPE.md) | the exact scope, the honest boundaries, where our tool helps | before planning a target |
| [`GATES.md`](./GATES.md) | the gates: official ones, refusal on violation, and ours, which stop us submitting noise | before every submission |
| [`PLAYBOOK.md`](./PLAYBOOK.md) | what to look for per target, tied to the official skills and to what our tool can do | before starting a target |
| [`findings/`](./findings/README.md) | the log: one entry per hypothesis, with its status and verdict | at every finding |
| [`SOURCES.md`](./SOURCES.md) | primary sources with the dates they were checked | when checking any fact |

---

## 4. How this connects to the rest of the project

Auditing the chain is a **parallel** track rather than a milestone of [`../ROADMAP.md`](../ROADMAP.md).
It leans on two assets the project already built:

- **The emulator binding** — the only one that does not leak. Its get-method and message calls map
  almost one to one onto the official proving skill, which gives us a **fast prefilter** for
  hypotheses in memory, before an expensive proof of concept.
- **The detectors and the heuristic extractor** — they can statically sweep the standard contracts for
  candidates. With an important caveat: that code has been audited many times, and most of what is
  obvious is already on the not-a-defect list.

An honest assessment of our edge is [`SCOPE.md`](./SCOPE.md) §5. It is deliberately restrained:
overstating our contribution in front of that team would cost more than a modest real result.

---

## 5. One sentence on why

The project's moat is dynamic confirmation of a finding by running it in a real virtual machine. The
programme requires exactly that: it does not accept a finding without an executable proof. Our moat
and their requirement are the same thing. This direction is not a distraction from the product but its
most direct test under fire.

---

## 6. What this directory does not cover

It does not describe the analyser — that is the rest of [`../README.md`](../README.md) — and it does
not hold findings themselves: those live in the log, one entry per hypothesis, including the ones that
were refuted.
