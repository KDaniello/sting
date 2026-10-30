# Scope

What is in the programme, what is outside it, and where our tool genuinely helps. Everything below
comes from the official documentation read literally on 2026-08-05 rather than from a paraphrase. The
scope is actively curated, so this file carries its date and is re-checked before every session.

---

## 1. In scope and paid

### 1.1 The chain's core

The priority target: the consensus protocol, the validator node, the full node, the distributed hash
table node.

- The test branch counts as experimental and **is accepted** — defects there are valuable during an
  audit period, and consideration may be deferred until it ends.
- Findings about the secondary currency mechanism are considered at **low priority**, since it is not
  deployed on the main network.

### 1.2 The virtual machine and the computation phase

A crash from an opcode, exponential processor or memory use against linear gas, undercharged gas, and
accounting reset across a nested run are **in scope**. The gate is hard: **a proof of concept in the
chain's own assembly language is strictly required**, or the finding is refused immediately
([`GATES.md`](./GATES.md) §1). Only against the current version.

### 1.3 The standard contracts

The network configuration contract, the elector, the simple wallets, the fourth wallet version and its
subscriptions, the second multisig version, the high-load wallet, the nominator pool, the token
contracts and the naming contracts.

**Important**: other contracts in the same directory are examples and are **out of scope**. Sale and
distribution contracts and wallet plugins are examples too.

### 1.4 The operational tooling

The node control tool, the HTTP API and its public deployment, and the Python client library.

---

## 2. Out of scope and unpaid

Listed explicitly in the programme's own documentation. We build no hypotheses on any of it.

**Excluded from the core:**

- non-final light-server state
- storage, payment channels and the HTTP proxies
- the explorer built from the same monorepo
- a misconfigured validator control interface
- **a crash of the emulator on malformed data** — the substrate our binding runs on, §5
- the client library, removed from scope in mid-2026
- **the compilers**, both of them
- the previous consensus protocol

**General exclusions:**

- anything requiring control of the local host, its files, environment or launch flags
- anything requiring control of most honest validators — a deliberate halt on a broken quorum is
  intended behaviour, since stopping beats continuing
- anything requiring leaked private keys
- misuse of a client library by an integrating application without crossing a privilege boundary
- hygiene of validation, messages or status codes while the service is healthy
- crashes of local tools and debug paths that do not affect normal operation
- long-known implementation behaviours with no new security effect
- future timestamps that do not fit an unsigned field
- identity flooding of a public overlay
- the deprecated bridges

---

## 3. The current language: the honest boundary

**Its compiler is absent from the bounty scope.** It is neither in the priority list nor in the
exclusions, because the programme is built around the core and the older contracts, and the language
is younger.

That has to be accepted plainly, because the original premise — go and look for defects in the new
language — does not map onto this programme:

- **A compiler defect** — a miscompilation, a lost semantic — is real value and goes **to the language
  team directly** rather than into the core bounty, and without a reward under it. There is a
  precedent: a compiler fuzzer that works exactly that way.
- **A vulnerability in a contract rather than in the compiler** is the main product's work, and its
  addressee is the contract's author rather than the bounty. The exception: a standard contract in the
  older language is in scope (§1.3).
- **The virtual machine executes bytecode compiled from anything.** So a machine-level finding (§1.2)
  does not depend on the source language — it is about the machine itself. Here the contract's origin
  is irrelevant, and this is the one path where working with contracts in the current language meets
  the bounty: through machine behaviour, proven in their form.

The conclusion: **the main product and this track meet in two narrow places** — machine behaviour
(§1.2) and the standard contracts (§1.3). Everything else in the project is valuable and goes to a
different address.

---

## 4. Never report: the known non-defects

The programme lists them explicitly. Reporting any of them is an instant loss of credibility with the
team we want to join.

| The behaviour | Why it is not a defect, per the programme |
|---|---|
| a vote replay in the configuration contract across rounds | a known behaviour under specific conditions |
| stake accounting and recovery limits in the elector | a known behaviour, not an issue in itself |
| the elector ignoring bounce semantics | a known behaviour |
| replaying slashing signatures | impossible: the election identifier differs each round |
| redeploying or reusing a deleted wallet | deletion is final, and old external messages expire |
| two specific nominator withdrawal issues | explicitly not considered issues |
| the absence of a misbehaviour proof in the consensus protocol | a known low-priority work in progress; reports are **not accepted** |
| a misbehaving validator making others do useless work | out of scope |
| future timestamps overflowing a field | out of scope |

Plus the general class: **hygiene** and **debug opcodes or debug flags**.

---

## 5. Where our tool helps and where it does not

The assessment is deliberately restrained. Overstating our contribution to that team would cost more
than a modest real result.

| Target | Does it help | How, or why not |
|---|---|---|
| **machine behaviour** (§1.2) | **yes, as a prefilter** | our binding runs real bytecode through the real library in memory — fast screening of a hypothesis before an expensive proof. **The submittable artefact is still in their form** ([`GATES.md`](./GATES.md) §1) |
| **standard contracts** (§1.3) | **partly** | the heuristic extractor and the detectors can sweep them for candidates. The caveat: this code has been audited many times and the obvious is in §4 |
| **the core node** (§1.1) | **no, not directly** | we analyse source and bytecode rather than the node's own language. Consensus defects need a mixed network by the official method, not our tool |
| **an emulator crash** | **no — out of scope** | it is the substrate our binding runs on, and crashing it with malformed input is not a finding (§2) |
| **the operational tooling** (§1.4) | **no** | outside the project's capabilities |

**The honest conclusion.** Our real edge is **two narrow places**: machine hypotheses, where our
binding gives screening speed and the official skill gives a submittable proof; and a static sweep of
the standard contracts as a generator of candidates. Finding new critical defects in repeatedly
audited code is hard — so the near-term value of this direction is less in an immediate finding than
in **the strict method built**, and in the fact that it coincides with the product's existing moat.
That is recorded without embellishment, because an honest assessment of one's own edge is part of
standing with the same team.

---

## 6. Re-checking before a session

The scope moves. Before every audit session, re-read the programme's documentation and check that the
repository is live. If it has changed since this file's date, update this file first and work
afterwards. A stale scope in one's head is a report out of scope, which is spent trust.

---

## 7. What this document does not cover

It does not say how to look — that is [`PLAYBOOK.md`](./PLAYBOOK.md) — and it does not define what may
be submitted, which is [`GATES.md`](./GATES.md). It defines the boundary, and the boundary is what
makes the difference between research and an attack.
