# Decisions

A decision recorded here is settled. Each states what was decided, the reason it was decided that
way, and the consequence — which document, gate or file changes because of it. A decision without a
consequence is an opinion and does not belong here.

Questions still open are in §8. Questions a measurement settles are not questions for this document
at all; they are steps in [`ROADMAP.md`](./ROADMAP.md) with the measurement attached.

---

## 1. No remote until the platform is built

**Decision.** No GitHub repository yet. One is created once the platform stands, together with the
decision in §8 about what is published from it.

**Why.** A remote is needed for two things: enforcement and publication. Publication is gated at M6
by the roadmap's own rule. Enforcement can be had locally at a fraction of the cost, and a
repository created before there is anything to push is a repository whose first commits describe an
unfinished tree.

**Consequence.** Every gate row that read "checked by CI" reads "checked by `make pr`, run before
the commit that closes the milestone". The five workflow files under `.github/workflows/` stay where
they are and are kept in step with the Makefile, so that connecting a remote later is one push and
not a milestone. `make pr` is the gate, and it is the same set of checks CI would run.

Two files wait with the remote rather than being guessed at: `CODEOWNERS` and the issue
templates both name a GitHub account, and an account that does not exist yet cannot own a
review.

---

## 2. Authorship and commit rules

**Decision.** Authorship is strictly the owner's. No commit, tag, changelog entry or release note
ever names a tool, in any form — no `Co-Authored-By` trailer, no "generated with" footer, no mention
in a body. The commit rules of the reference project apply here unchanged, and are written into this
repository's own `AGENTS.md` rather than referenced across projects.

**Why.** Authorship in a repository's history is the owner's decision. The tool used to type the
text has nothing to do with it.

**Consequence.** `AGENTS.md` and `CLAUDE.md` exist at the root, `AGENTS.md` carrying rule zero, the
commit format, the comment rule and the index into `docs/`. The commit format is
`<type>(<scope>): <subject>`, imperative, lower case, no trailing period, at most 72 characters,
with a body that answers *why* and never lists changed files. A change that invalidates stored
artefacts states it: `BREAKING: invalidates dictionaries built before the schema change`.

---

## 3. Dead documents are untracked, not deleted

**Decision.** A document superseded by a live one leaves the index and stays on disk. It is added to
`.gitignore` and untracked with `git rm --cached`; nothing is deleted from the working tree.

**Why.** Two different needs were being confused. What must not happen is a repository full of
documents that contradict the live ones — a reader trusts a stale document exactly as much as a
current one, which is what makes it harmful. What was never needed is losing the text: it is on disk
and it is in `git log`.

**What counts as dead**, and the test is mechanical rather than a matter of taste: a document is dead
when a live document contradicts it. Today that is `docs/phases_roadmap/` — 11 files, 251 133 bytes,
a phase-by-phase plan the roadmap replaces — and the root `PLAN.md`, 57 039 bytes of session tracker
whose every line is chronology.

**Consequence.** M0 gains a step: `.gitignore` grows both paths, `git rm --cached` removes them from
the index, and the commit that does it names them so that `git log` finds them by name. The M0 gate
row changes from "absent from disk" to "absent from the index, present on disk, present in
`git log`".

---

## 4. `domains/` and `audit/` stay, and stay current

**Decision.** Both stay tracked and both are maintained. Neither is frozen, neither is untracked.
Where M5's inventory contradicts a passage in `domains/`, that passage is corrected in the same
change that produces the contradiction.

**Why.** They are not dead by the test in §3: nothing live contradicts them today. The cost that
made freezing tempting — translating 224 kilobytes twice — is paid once if they are corrected as the
platform changes rather than after it has changed.

**Consequence.** The M0 translation budget includes `domains/` and `audit/`. The mirror rule applies
to them like everything else: `docs/ru/domains/`, `docs/ru/audit/`, identical heading trees, both
sides changed in the same commit.

---

## 5. One prefix: `sting-`

**Decision.** The platform is `sting`, and every crate in it carries the `sting-` prefix, including
the ones arriving from `karman` and `sybil`. The pattern is the one the tree already uses:
`sting-<domain>-<part>`.

**Why.** `sting-abi-core`, `sting-abi-tolk` and `sting-abi-dictionary` already establish it, and 29
crates already carry the prefix against 23 that do not. `sybil`'s crates are named `common`,
`graph`, `features`, `cluster`, `ingestion`, `scorer` — names that cannot enter a shared workspace at
all: `common` is exactly the module name the code rules forbid, and a bare `graph` says nothing about
whose graph it is.

**Consequence.** The rename happens in M4, in one commit per domain, and the mapping is fixed here so
that no later change re-litigates it:

| Today | In the tree |
|---|---|
| `sting-karman-core`, `sting-karman-types`, `sting-karman-agents`, `sting-karman-scenario`, `sting-karman-data`, `sting-karman-report`, `sting-karman-verify`, `sting-karman-app`, `sting-karman-cli` | `sting-karman-*`, same suffix |
| `sting-karman-mech-curve`, `-lending`, `-perp`, `-showcase`, `-vouch` | `sting-karman-mech-*` |
| `common` | `sting-address` — the address model, which is what it mostly is (M3) |
| `graph`, `features`, `cluster`, `scorer`, `ingestion`, `realtime-api` | `sting-sybil-graph`, `-features`, `-cluster`, `-scorer`, `-ingestion`, `-api` |
| `sybil_native` | `sting-sybil-python` — an extension module, and the name says which language it is for |

---

## 6. The EVM reference does not enter the tree

**Decision.** `karman-reference` — the Morpho Blue reference executed on `revm`, with
`alloy-sol-types` behind it — is not merged. It stays in the standalone `karman` project. The corpus
it produces, `testdata/level0/morpho-blue.json`, travels with the merged crates as a fixture.

**Why.** This platform is for TON. An EVM executor in a TON workspace is a second chain arriving
without a decision, and the standalone `karman` project already exists as the place where a second
chain would belong if one is ever wanted.

**What it costs, stated plainly.** Regenerating the level-0 corpus requires the standalone project.
That is one command run rarely, against a fixture that is checked in, and the differential harness
that reads the fixture merges normally.

**Consequence.** M4 merges every `karman` crate except that one. Nothing in the tree depends on it
today — it is a leaf — so the boundary is enforced by its absence rather than by a rule. The
dependency check gains one row anyway: `revm` and `alloy-*` appear in no manifest in the workspace.

---

## 7. Infrastructure runs in Docker

**Decision.** Docker is available, and every piece of infrastructure a milestone needs runs in it:
ClickHouse for M8, a stub chain server for M3's retry tests, anything M7 needs to measure load.

**Why.** It removes the only line in the roadmap that was blocked on a purchase, and it makes the
infrastructure part of the repository rather than part of one machine: a `compose` file is reviewed,
a hand-configured host is not.

**Consequence.** M8 stops waiting on a purchase. The 18 skipped acceptance tests are
unskipped against a single-node container, and the compose file lives in the repository next to the
tests that need it. Scale, if it is ever needed, is a separate measurement with its own number.

---

## 8. Where the dataset lives — open

**What.** M6's artefact is the method-id dataset, and its home decides its licence, its issue tracker
and who is seen to own it. The same question covers what else eventually leaves this repository, and
how the space is organised when it does.

**Why it stays open.** It is settled after the platform is built, not before: the answer depends on
what the inventory in M5 says there is, and on what M6's own gate measures the dataset to be against
the foundation's catalogue.

**What the gate measured.** 47.2% of the foundation's catalogue and 20.1% of this dataset overlap,
1 122 names are only here and 317 only there — a dataset that is neither a subset of theirs nor a
replacement for it, which is the shape the 90% threshold was written to detect. The licence is
decided and is in the file: `CC0-1.0`, the whole of the answer that belongs to the artefact rather
than to its address.

**What stays open is the address, and only it.** Which account, which organisation, which issue
tracker, and what goes there after this one.

**What proceeds meanwhile.** Everything in M6 except the last step, and all of it is done: the
dataset carries its provenance, schema, version and licence, and rebuilds byte for byte
([`domains/method-dictionary.md`](./domains/method-dictionary.md) §10). Publication is the one step
that cannot be undone, and it waits for this decision.

---

## 9. What this document does not contain

It does not contain technical choices with a measurement behind them. The Tact freeze waits on a
number ([`ROADMAP.md`](./ROADMAP.md) §5), the grammar's fate waits on the compiler oracle
([`ROADMAP.md`](./ROADMAP.md) §4), and the vouching-ring verdict waits on a run whose claim is
registered before it. None of those is a question for the owner; each is a question for a command.

It does not contain reasoning that belongs in the document it governs. Where a decision above
produces an invariant, the invariant is stated in [`ARCHITECTURE.md`](./ARCHITECTURE.md) §1 and cited
from the code by number; this document keeps only why it is that way.
