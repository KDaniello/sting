# CLAUDE.md

See [`AGENTS.md`](./AGENTS.md) — it is the single guide for working in this repository, and it
applies here in full.

Three rules that must not be missed:

**Authorship.** Every commit, pull request, issue and release note is authored by a human. Never add
a `Co-Authored-By` trailer naming an assistant, a "Generated with …" footer, or any mention of
development tooling to the git history. See [`docs/DECISIONS.md`](./docs/DECISIONS.md) §2.

**Comments.** As many as it takes for the code to be understandable, and no more. A comment explains
a block, an algorithm or a contract — never a line. Explain *why*, not *what*. Everything in
English, and never any project meta: no milestone names, no roadmap references, no "for now" or "to
be added later". See [`AGENTS.md`](./AGENTS.md).

**Silence.** A file that could not be parsed, a version that could not be identified, a check that
did not run — none of them are allowed to look like a clean result. Report it and exit non-zero.
