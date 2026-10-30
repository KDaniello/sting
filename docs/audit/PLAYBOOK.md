# Playbook: what to look at

What to look for on each target. Grounded in the two official methodologies and in the official list
of eighteen vulnerability classes. Opens **after** [`RULES.md`](./RULES.md), [`SCOPE.md`](./SCOPE.md)
and [`GATES.md`](./GATES.md), and not before.

The playbook says **where** to look and **what shape** a defect takes. It does not say a defect is
there — that is settled by a probe through the [pipeline](./GATES.md) §3.

---

## 1. The virtual machine — our main edge

Why main: our binding gives a prefilter no other hunter has, and the required proof maps almost one to
one onto what that binding already does.

### 1.1 Classes of hypothesis

| Class | Shape | How it is proven |
|---|---|---|
| **an opcode crash** | an opcode takes the machine down natively | a prober with that opcode, run under a gas limit, watching for an abnormal exit |
| **undercharged gas** | an instruction does work more cheaply than it costs | compare gas against a charged baseline of the same class of work |
| **exponential work at linear gas** | processor or memory grow non-linearly while gas grows linearly | scale one parameter, measure resources against gas |
| **allocation without gas** | cells or stack entries are allocated and no gas is charged | compare the allocating path against the charged one |
| **accounting reset in a nested run** | a nested machine zeroes or distorts accounting | separate a stack built outside from a stack built and paid for inside |

### 1.2 Where to ground it in the source

The official skill names the places: the control-opcode implementations, the opcode implementations
themselves, the assembler mnemonics, the assembler's words, and where gas is charged.

The order: find the mnemonic, then its implementation, then **where gas is charged, where allocation
happens, and which version gates it**.

### 1.3 Our prefilter before their form

Our binding runs real bytecode. A hypothesis that an opcode behaves strangely can be run in memory in
milliseconds, the unfounded discarded, and only the survivor carried into a proof in their form, which
is mandatory under gate O1.

**The boundary**: a crash of the library itself on bad data is [out of scope](./SCOPE.md) §2. What
interests us is the behaviour of the machine **as a specification**, proven in their form, rather than
the fragility of one binding.

### 1.4 What to reject immediately

Do not spend a probe if the hypothesis runs into a bounded length, a bounded key depth, a memoised
traversal, explicitly linear gas, paid stack movement, or a version gate that already closed the fix.
The official skill requires knowing these facts in advance.

---

## 2. Consensus — the heavy target

It needs a mixed network with a baseline build and a probing build, rather than our tool. Our project
**does not help** here: it is pure work in the node's own language, by the official triage method.

### 2.1 Classes of probe

| Probe | When | Where |
|---|---|---|
| **withholding** messages or proposals | a defect from suppressed sending | the block producer and the consensus modules |
| **malformed** packet or proof | a defect from broken serialisation | the block implementation and the network layers |
| **reordering** messages | a defect from processing order | the consensus pool and its core |
| **delaying** responses | a defect from timeouts or races | the handlers, catch-up and retry paths |
| **an invalid** block or proof | checking that honest nodes reject it | the producer and block implementation |

### 2.2 The iron rules of every probe

- Gated behind an explicit environment variable.
- One marker line when it fires, so it can be found by search.
- **Self-immunity** (gate O7): the probing node must not fall first; if a probe would cost it
  viability, skip it and log the skip.
- The baseline build and the probing build stay separate.
- The success condition is named **before** the run: a target sequence number, a crash marker, a
  process death, a rejection by an honest node.

### 2.3 What a valid negative result looks like

A direct example from the official skill: a probing node reorders and withholds within a leader
window, and the network **continues** producing blocks to the target sequence number. That is a
valuable result — the path executed, the honest nodes are healthy, and there is no defect in that
configuration. It is recorded as a negative rather than discarded.

---

## 3. The standard contracts

Here our project **partly** helps: the heuristic extractor and the detectors sweep the code for
candidates, and a local test network does the rest.

### 3.1 Targets and their pre-subtracted non-defects

Before looking at a contract, check [`SCOPE.md`](./SCOPE.md) §4. Specific classes are already excluded
per target: a vote replay for the configuration contract; stake accounting, bounce semantics and
signature replay for the elector; redeployment for the wallets; two withdrawal issues for the
nominator pool.

Look for what is **outside** those exclusions: a new security effect that the known behaviours do not
already contain.

### 3.2 What to look for

From the official class list, filtered to what actually occurs in standard contracts:

- **signed and unsigned**: a negative value inverts an operation — the same class one of our detectors
  covers
- **missing return value checks**: an operation's result ignored
- **incorrect data type handling**: serialisation and deserialisation out of step
- **unprotected code updates**: code replaced without authorisation
- **race conditions in message cascades**: parallel message flows break an assumption checked earlier —
  a TON-specific class our product does not catch yet
- **improper gas excess handling**: excess not returned

**Our tool gives candidates and a test network gives the proof.** A detector firing on a standard
contract is a hypothesis rather than a finding. It is proven by deployment and a trigger, not by the
detector.

### 3.3 The minimal sequence

Compile the contract, build its initial state, deploy it, confirm the account is active once the
master chain has advanced, send the trigger message, and observe the effect in the resulting
transaction. Diagnostics for a hang at each step are in the official checklist
([`SOURCES.md`](./SOURCES.md)).

---

## 4. What we do not touch

From [`SCOPE.md`](./SCOPE.md) §2, so that no time goes into hypotheses that will not be accepted: the
compilers, the client library, the previous consensus protocol, non-final light-server state, storage,
payment channels, the proxies, the explorer, a crash of the emulator, debug opcodes, hygiene, the
bridges, overlay flooding, and timestamp overflow.

---

## 5. The priority of targets for us

In descending order of our real edge against the effort, honestly:

1. **Machine hypotheses** — the one place where our binding gives a unique prefilter and the required
   proof is reachable. Start here.
2. **The standard contracts** — the detectors generate candidates, but the code is audited and the
   obvious is excluded, so the expectation of a finding is low.
3. **Consensus** — the programme's most valuable category, where our project does not help; pure work
   in another language with a high barrier to entry.

The first target of the first session is not the most valuable but the most **ours**: a machine
hypothesis carried through the whole pipeline to a proof in their form or to an honest negative
result. The goal of that session is to debug the method on a real example rather than to find a
critical defect immediately.

---

## 6. What this document does not cover

It does not decide what may be submitted — that is [`GATES.md`](./GATES.md) — and it does not define
the boundary of the programme, which is [`SCOPE.md`](./SCOPE.md). It says where to look, and it says
nothing about whether anything is there.
