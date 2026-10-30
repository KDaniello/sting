# `tree-sitter-tolk` — vendored and patched, committed rather than gitignored

The published grammar is not a fork here by preference. It is a fork because the language moved and
the grammar did not: `tree-sitter-tolk` 0.0.5 was published 2026-01-17 and knows Tolk 1.2, while the
compiler this repository vendors is 1.4.2, built 2026-07-15. Five constructs of the live language
had no rule at all, and 54 of 297 corpus files parsed with an `ERROR` node because of them.

## Why this is committed, unlike `vendor/tolk/` and `vendor/libemulator/`

Those are large, platform-specific binaries fetched from `ton-blockchain/ton` releases, gitignored on
purpose. This is small, readable, MIT-licensed source that is **modified from upstream**. Committing
it is what makes the modification reviewable and the build reproducible without a regeneration step
on every fresh checkout — the same reasoning `vendor/tree-sitter-func/README.md` records.

## Provenance

Copied from [`tree-sitter-tolk` v0.0.5](https://crates.io/crates/tree-sitter-tolk) as published by
TON Core, taken from the local cargo registry rather than re-downloaded, so the starting point is
byte-for-byte the crate the workspace depended on before.

## What was changed, and why each change was needed

Every one of these came from a file the compiler reads and the grammar refused. Nothing here was
added speculatively; `cargo run -p sting-ast --example grammar_oracle` is the command that produced
the list and the command that says when it is empty.

| Change | The construct | Files it unblocked |
|---|---|---|
| `contract_declaration`, `contract_body`, `contract_field_declaration` | the Tolk 1.3 manifest block | 36 |
| `annotation_name`, dotted | `@abi.clientType(...)`, Tolk 1.4 | 4 |
| `annotation_list` on `struct_field_declaration` | an annotation on a field rather than on a declaration | 4 |
| a type as an `annotation_arguments` element | `@abi.clientType(Cell<T>?)` | 1 |
| `;` as an `enum_body` separator | real production enums | 3 |

One resolution was needed that is not a new construct: `annotation` became right-associative.
Upstream makes both the name and the arguments optional, so a bare `@` is a valid annotation and
`@name` was ambiguous between an annotation carrying a name and an annotation carrying none followed
by a field of that name. The generator refuses that conflict rather than guessing, and right
associativity is the resolution it names.

## What this does not change

No rule was removed and no existing rule's shape was altered, so a tree that parsed before parses the
same way now. The measured proof is the detector corpus: `sting bench` reports the same numbers
against `corpus/baseline.json` after the change as before it, per detector and in aggregate.

## Regenerating

`src/{parser.c,grammar.json,node-types.json}` are generated from `grammar.js` and are committed with
it. After any edit to the grammar:

```sh
cd vendor/tree-sitter-tolk
npx tree-sitter-cli@0.26.11 generate
```

Then re-run the oracle from the repository root and expect it to report nothing diverging:

```sh
cargo run -p sting-ast --example grammar_oracle
```

## Upstream

The patch is offered to
[`ton-blockchain/tree-sitter-tolk`](https://github.com/ton-blockchain/tree-sitter-tolk). Vendoring is
not a decision to keep a fork: the oracle keeps any divergence between this grammar and the compiler
visible whether or not upstream takes it, which is what makes the fork answerable rather than
permanent.
