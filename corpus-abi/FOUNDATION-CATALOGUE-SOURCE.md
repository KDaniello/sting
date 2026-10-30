# The foundation's catalogue, as read

`foundation-catalogue.json` is what `ton-blockchain/abis` declares at commit
`90c4c6bca79db19ad803f41ff8560f920305002c`, committed 2026-08-10 and read on 2026-08-23: 600
get-method names and 1 296 message opcodes, taken from the 285 type files among its 1 034 Tolk
files.

The other 749 files are tests and helpers. Their test cases are written as get-methods, and counting
those would credit the catalogue with names no contract on chain answers to — inflating the side
this repository is measured against, which is as dishonest as deflating it.

It is here as a fixture rather than as a submodule because what the comparison needs is two lists,
not a build of somebody else's repository, and because a comparison whose input can change under it
is a comparison nobody can reproduce. `abi-layer catalogue-fetch --checkout <clone> --read-at <date>`
rebuilds the file from a clone: it reads the commit out of the checkout rather than taking it on
trust, states the date instead of reading a clock, and produces the same bytes from the same
checkout.

## What it is not

It is not their dataset. Their artefact is a catalogue of described contracts — types, storage
layouts, opcodes, written by hand for the projects worth describing — and these two lists are the
only part of it this repository compares itself against. Nothing here reproduces their work or
stands in for it.

It is not a judgement of coverage either way. The overlap and the two differences are measured by
`abi-layer catalogue-diff`, which states both numbers including the unflattering one.
