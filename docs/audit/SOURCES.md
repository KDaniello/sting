# Primary sources

Everything in this directory is grounded in the sources below, read **literally** on 2026-08-05 rather
than paraphrased. Every fact about the scope, every gate and every probe has an address here.

The programme's scope moves, so the first two sources are re-checked before every session
([`SCOPE.md`](./SCOPE.md) §6).

---

## 1. The programme and its rules

- **The bounty repository** — rules, scope, reward, intake channel. Its documentation was read in full.
  Status on 2026-08-05: not archived, pushed within the preceding month, updated days before. Its last
  commits are scope curation: removing one component, putting the proxies out of scope, declaring
  payment channels not accepted.
- **The intake channel** for core reports. Responding on 2026-08-05.
- **A separate programme** for front-end reports, which is not our track.

## 2. The official method

The team's own skill repository, pushed in mid-2026, holds two methodologies:

- **triage** — local test networks, the deploy-and-trigger workflow, the validator-behaviour workflow,
  the core rules and the scripts.
- **hypothesis proving** — turning a machine hypothesis into a minimal prober, running it under a gas
  limit, and classifying the outcome as confirmed, down-ranked or rejected.
- **a diagnostic checklist** — what to do when a run finished and the result is unclear, and what a
  valid negative looks like.
- **probing patterns** — five classes of consensus probe, their environment variables, and
  self-immunity.
- **a worked example** — a valid negative result on a mixed network.

## 3. The target code

- **The chain's monorepo** — the node, the virtual machine, the standard contracts. It keeps its own
  rules file, which is to say the team audits its own code with agents.
- The standard contracts kept outside the monorepo: the wallets, the multisig, the high-load wallet,
  the nominator pool, the token contracts, the naming contracts.

## 4. Vulnerability classes

The official documentation's list of eighteen classes, used in [`PLAYBOOK.md`](./PLAYBOOK.md) §3.2.

## 5. Ecosystem context

- **The node's changelog** — every release of that period thanks bounty hunters, and one of them says
  outright that the code is being refactored to be easier for agents to understand and audit. Direct
  evidence that the programme pays, that it reaches releases, and that the team values rigour a
  machine can read.
- The wider picture of competition and release pace: [`../domains/landscape.md`](../domains/landscape.md).

---

## 6. How to re-check

Before every session, re-read the programme's documentation and confirm the repository is live and not
archived. Less often, when the process changes, re-check the skill repository. Before any machine
finding, confirm the current version from the changelog.

If the programme's documentation has changed since this file's date, update [`SCOPE.md`](./SCOPE.md)
first and work afterwards.

---

## 7. What this document does not cover

It holds addresses rather than conclusions: what the sources say is in the other files of this
directory, each citing the source it rests on. A source without a date is not a source, which is why
every entry here carries one.
