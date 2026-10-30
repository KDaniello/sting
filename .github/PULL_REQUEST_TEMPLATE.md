## What changes, and why

<!-- One paragraph. What the tree does differently now, and the reason. Not a list of files. -->

## What number says it works

<!-- The command and its output. A change without a number is a change nobody can check:
     `make pr`, a benchmark line, a corpus share, a test that failed for the right reason first. -->

## What this does not do

<!-- The limitation that survives this change, stated here rather than discovered later. -->

## Checklist

- [ ] `make pr` green
- [ ] A bug fix carries its regression test in the same commit, and the message names what found it
- [ ] Comments explain a block, an algorithm or a contract — never a line, and never the plan
- [ ] Documentation changed on both sides: `docs/` and `docs/ru/`, in this commit
- [ ] A change that invalidates a stored artefact says so: `BREAKING: …`
