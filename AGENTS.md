# Working in this repository

Guide for automated tooling and for anyone arriving without context. It is a condensed index into
[`docs/`](./docs/README.md), not a replacement for it.

---

## Rule zero: authorship

**Every action on this repository is performed as the developer. The only author is a human.**

- **Never** add a `Co-Authored-By` trailer naming an assistant, model or tool to a commit message.
- **Never** add a "Generated with …", "Created by …" or similar footer to a commit, pull request,
  issue or release note.
- **Never** mention development tooling in the git history, tags, changelog or release notes, in any
  form.

Authorship in a repository's history is the owner's decision, and this project's history reads as the
work of its team. The tool used to type the text has nothing to do with authorship.

Full statement: [`docs/DECISIONS.md`](./docs/DECISIONS.md) §2.

---

## What this project is

`sting` is a platform for the TON chain. It reads a Tolk contract statically and confirms what it
found by executing the real bytecode in a TVM emulator. It extracts a normalised schema from Tolk,
Tact and FunC, and answers "what operation was that" for an arbitrary on-chain message. It scores
risk on jettons and gifts. As the merge in [`docs/ROADMAP.md`](./docs/ROADMAP.md) §7 completes, it
clusters addresses by funding and runs economic mechanisms against adversarial agents.

The single fact everything follows from: **the chain shows bytes, and nothing in the protocol says
what those bytes meant.** Read [`docs/POSITION.md`](./docs/POSITION.md) §3 before touching the
abi-layer.

## Layers

Dependency runs downward only. A crate of layer N depends on layers below N; an upward dependency is
an architectural error and does not pass review.

The table names one or two crates per layer rather than all 29; the full graph is
[`docs/ARCHITECTURE.md`](./docs/ARCHITECTURE.md) §3.

| Layer | Crates | Rule |
|---|---|---|
| 5 · products | `sting-cli`, `sting-abi-service`, `sting-abi-cli` | wiring only; no analysis lives here |
| 4 · subject | `sting-detectors`, `sting-scamcheck` | one subject each, no cross-imports |
| 3 · analysis | `sting-semantic`, `sting-dataflow` | pure over an AST; no I/O |
| 2 · parsing | `sting-ast`, `sting-abi-core`, `sting-abi-*` | a parse failure is reported, never swallowed |
| 1 · foundation | `sting-core`, `sting-corpus`, `sting-emulator`, `sting-emulator-sys` | the only place with `unsafe` and the only place that talks to a host |

## Commands

```bash
make test-fast    # library tests, the inner loop
make test         # everything this host can run offline
make test-live    # the tests that need the network, a container or a key
make lint         # fmt, clippy, deny, typos, rustdoc, the documentation rules, the lint tables
make pr           # the full gate; green before a commit closes a milestone
```

`make docs-check` alone runs the documentation rules and prints one line per rule with its count and
its threshold, so the output is a work list rather than a verdict.

## Rules that will get a change rejected

1. **A file that is not parsed never looks like a file without findings.** Any command that reads
   source reports `ERROR`/`MISSING` nodes with a position and exits non-zero. Silence means "read and
   clean", and it may not mean anything else.
2. **A claim carries its number, and the number comes from a command.** No "significantly faster", no
   "high precision". A number in a document that no script reproduces is a defect
   ([`docs/ROADMAP.md`](./docs/ROADMAP.md) §2).
3. **A threshold is registered before the run it judges.** A threshold named afterwards is not a
   threshold, and a negative result is published in the same table as a positive one.
4. **No `unsafe` outside `sting-emulator-sys` and `sting-emulator`.** The workspace forbids it; the
   FFI boundary and its safe wrapper are the two documented exceptions
   ([`docs/ARCHITECTURE.md`](./docs/ARCHITECTURE.md) §1.3), no public function in the wrapper is an
   `unsafe fn`, and every block carries a `// SAFETY:` comment naming the invariant the caller must
   uphold. `make lints-check` refuses a third.
5. **No `unwrap`, `expect` or `panic!` in library code.** The workspace denies all three. An error
   crosses a boundary as a typed `Result`.
6. **Errors are typed per crate.** `thiserror` in libraries; `anyhow` only at the top of a binary —
   `sting-cli`, `sting-abi-cli`, `sting-scamcheck`, and nowhere else.
7. **Newtypes for identifiers, units in names.** Two `u64` parameters that can be swapped will be
   swapped. `delay_ms`, `size_bytes`, `budget_runs` — never bare `delay`, `size`, `budget`.
8. **No upward dependency between layers**, and no `utils`/`helpers`/`common` module: those names
   accumulate unrelated code and say nothing about what is inside.
9. **English everywhere** — code, comments, doc comments, error text, commits, issues. `docs/ru/` is a
   mirror, not a source.
10. **No project meta in code**: no milestone names, no roadmap references, no "for now", no "later".

## Comments

**As many as it takes for the code to be understandable, and no more.** A comment on every line is
harmful: it doubles the reading volume, diverges from the code at the first edit, and lies from then
on. A lying comment is worse than a missing one, because it is believed.

**A comment explains a block, an algorithm or a contract — never a line. Explain *why*, not *what*.**
If the "what" is unclear, fix the names and the structure, not the prose.

| Mandatory | Why |
|---|---|
| `///` on every public item | `missing_docs` is a warning workspace-wide |
| a line or two on any non-trivial algorithm | the structure alone does not carry the idea |
| an invariant the type system cannot state | nothing else records it |
| `// SAFETY:` on every `unsafe` block | it names what the caller must uphold |
| a workaround for a chain, compiler or libc quirk, **with a link** | otherwise it reads as a mistake |
| anything that exists to keep a result reproducible | otherwise it looks redundant and a cleanup deletes it |

| Forbidden | Why |
|---|---|
| restating the code | it doubles the reading and diverges at the first edit |
| commented-out code, and a quotation of code in another language | nothing recompiles either, and both age the same way |
| a `TODO` without an owner | it belongs to nobody and is done by nobody |
| a decorative separator bar | it separates nothing the language does not |
| a doc comment on an obvious accessor | the signature already says it |
| text copied out of `docs/` rather than linked | two copies drift, and the code's copy drifts first |
| a number a command reproduces | it belongs in that command's output |
| **project meta of any kind** | it dates the comment the moment it is written |

Project meta is wider than a milestone's name: a session, a risk spike, a definition of done and a
development block all date a comment the same way. A comment describes the code as it is. State a
real limitation and its consequence, never when it is expected to end.

```rust
// Tolk's grammar has no rule for `contract { … }`, so the block arrives as one opaque ERROR node
// and the storage binding has to be recovered from its text. Delete this the moment the grammar
// gains the rule; do not build further logic on top of it.
fn parse_contract_headers_text(text: &str) -> Vec<ParsedHeaderBlock> {
```

## Commits

Small, and the message is about the important part.

```
<type>(<scope>): <short description>
```

Types: `feat`, `fix`, `perf`, `refactor`, `docs`, `test`, `chore`.

Scopes: `core`, `ast`, `semantic`, `dataflow`, `detectors`, `report`, `emulator`, `tolkc`, `fift`,
`corpus`, `pipeline`, `bench`, `fuzz`, `abi`, `dictionary`, `coverage`, `profiler`, `service`,
`scamcheck`, `chain`, `address`, `karman`, `sybil`, `cli`, `docs`, `ci`.

Subject: imperative, lower case, no trailing period, at most 72 characters. Body optional, one to
three sentences, answering *why*. Never a list of changed files. Never "comprehensive", "robust",
"enhance", "leverage".

A change that invalidates a stored artefact states it:
`BREAKING: invalidates dictionaries built before the schema change`.

```
fix(ast): report ERROR nodes instead of returning an empty finding list

A file the grammar cannot parse produced the same empty SARIF as a clean one,
so 54 of 297 corpus files read as analysed when nothing had looked at them.
```

## Tests

- Every bug fix carries its regression test **in the same commit**, and the message names the fixture
  or the corpus file that found it.
- Anything in [`docs/API.md`](./docs/API.md) is written test-first — those are promises.
- Verify a new test fails for the **right reason** before making it pass. A detector test that passes
  because the file never parsed is worse than no test.
- A detector change re-runs the corpus and records the difference in findings, true and false
  positives classified by hand.
- **No retry annotations. Ever.** A flaky test is a finding, not a nuisance.

## Documentation

`docs/` is English and canonical. `docs/ru/` is an identical Russian mirror with the same heading
structure; `make docs-check` compares the heading trees. **Both change in the same commit.**

Cross-references use relative links with a section number, and a section number is stable:

```
[`API.md`](./API.md) §2.3
```

Renumbering a section means fixing every reference to it in the same change.

A document superseded by a live one is added to `.gitignore` and removed from the index with
`git rm --cached`. It stays on disk and in `git log`; it stops being something the repository claims
is true ([`docs/DECISIONS.md`](./docs/DECISIONS.md) §3).

## Where to look first

| Question | Document |
|---|---|
| what is being built and in what order | [`docs/ROADMAP.md`](./docs/ROADMAP.md) |
| which gates exist and why | [`docs/PLAN.md`](./docs/PLAN.md) |
| why is it built this way | [`docs/ARCHITECTURE.md`](./docs/ARCHITECTURE.md) |
| what exactly crosses this boundary | [`docs/API.md`](./docs/API.md) |
| what proves it | [`docs/TESTING.md`](./docs/TESTING.md) |
| what is settled and what is still open | [`docs/DECISIONS.md`](./docs/DECISIONS.md) |
| who else is in this niche | [`docs/POSITION.md`](./docs/POSITION.md) |
