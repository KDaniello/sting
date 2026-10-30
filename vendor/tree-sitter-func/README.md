# `tree-sitter-func` — vendored, not gitignored (unlike this repo's other `vendor/*` entries)

> Written for `docs/abi-layer/blocks/00-bootstrap-shared-core.md` (в истории: `git show cf52463:docs/abi-layer/blocks/00-bootstrap-shared-core.md`) §0.3, risk spike A.

## Why this is committed, unlike `vendor/libemulator/`, `vendor/tolk/`, `vendor/fift/`

Those three are large platform-specific binaries fetched from `ton-blockchain/ton` GitHub releases — gitignored on purpose, fetched by each developer/CI run. This is different: small, readable, MIT-licensed source, and **modified from upstream** (see below) — committing it is what makes the modification reviewable and the build reproducible without a regeneration step on every fresh checkout.

## Provenance

- Forked from [`tree-sitter-grammars/tree-sitter-func`](https://github.com/tree-sitter-grammars/tree-sitter-func), commit `f780ca55e65e7d7360d0229331763e16c452fc98` (2024-04-27, `master`) — itself a fork of [`akifoq/tree-sitter-func`](https://github.com/akifoq/tree-sitter-func), the crate published to crates.io as [`tree-sitter-func` v1.0.0](https://crates.io/crates/tree-sitter-func).
- **Why not just depend on the crates.io crate directly**: confirmed empirically (2026-07-21) that `tree-sitter-func` v1.0.0 pins `tree-sitter ~0.20.10` (its own `Cargo.toml`), while this workspace uses `tree-sitter = "0.26.11"` ([`Cargo.toml`](../../Cargo.toml)). The two versions' `Language` types are structurally identical (`*const ffi::TSLanguage`) but nominally distinct — `Parser::set_language` from 0.26 rejects a `Language` produced by 0.20's `tree_sitter_func::language()` with a hard type error (`E0308`, confirmed by actually compiling both together in a throwaway crate). Upstream has not published a new version since 2023-05-13, so this mismatch does not self-resolve.

## What was changed in the grammar itself, and why each change was needed

The three edits below came from files real contracts ship and this grammar refused. Nothing was
added speculatively; `cargo run -p sting-ast --example func_oracle -- <directory>` is the command
that produced the list and the command that says when it is empty.

| Change | The construct | Files it unblocked |
|---|---|---|
| `catch` accepts `underscore` beside `identifier` | `try { … } catch (_, _) { … }` | 4 |
| `!` added to `identifier` | `storage.upgrade_storage:get!("meta"H)` | 5 |
| `!` added to `function_name` | `slice upgrade_storage:get!(cell storage, int h)` | 1 |

The first is not a missing rule but a rule that named the wrong node. `try_statement` exists
upstream and its catch clause accepts `commaSep($.identifier)`; a bare `_` is not an identifier
here, because the identifier rule needs two characters once it starts with an underscore, and the
grammar gives `_` a node of its own. Every contract in the sample catches into `_` at least once.

The second and third are one change stated twice: a declaration and a call site have to agree on
what a name may contain, or a function the grammar reads cannot be defined in it. `?` and `:` were
already allowed in both; `!` is the same kind of suffix and appears in production for a throwing
accessor.

Measured after the three: 199 of 199 `FunC` files across eight published bug bounty scope targets
parse with no `ERROR` node, against 189 before. Nothing else moved — the detector corpus reports
82.4% aggregate precision and 96.4% over the default set, the same figures as before the edit.

## What this does not change

No rule was removed and no existing rule's shape was altered, so a tree that parsed before parses
the same way now.

## Regenerating

`grammar/` is vendored beside `grammar.js` because `grammar.js` requires it and cannot be
regenerated without it — the first version of this vendoring copied only `grammar.js`, which made
the fork unbuildable from its own sources. After any edit:

```sh
cd vendor/tree-sitter-func
npx tree-sitter-cli@0.26.11 generate
```

Then re-run the oracle over a directory of real `FunC` and expect it to report nothing unread.

## What was actually changed from upstream (commit above)

1. **`src/{parser.c,grammar.json,node-types.json}` regenerated** with `tree-sitter-cli` 0.26.11 (`npx tree-sitter-cli@0.26.11 generate`, run from the upstream checkout) — the grammar itself (`grammar.js`, copied unmodified) did not need any edit, only regeneration against a current CLI. ABI 14 (not the newer ABI 15) — no `tree-sitter.json` manifest exists upstream to opt into ABI 15; not needed for our use (parsing only, no external scanner, no wasm).
2. **`bindings/rust/lib.rs` and `Cargo.toml` rewritten** from the old raw-FFI shape (`extern "C" fn tree_sitter_func() -> Language` returning the crate's *own* `tree_sitter::Language`, requiring callers to depend on the exact same `tree-sitter` version) to the modern `tree-sitter-language`-mediated shape:
   ```rust
   extern "C" { fn tree_sitter_func() -> *const (); }
   pub const LANGUAGE: LanguageFn = unsafe { LanguageFn::from_raw(tree_sitter_func) };
   ```
   This is not a novel pattern invented here — it is copied from how `tree-sitter-tolk` (`crates/sting-ast`'s existing dependency, [`crates.io`](https://crates.io/crates/tree-sitter-tolk) v0.0.5, published by TON Core) already does it, confirmed by reading that crate's vendored source under `~/.cargo/registry`. `tree-sitter-language = "0.1"` is a tiny, stable-ABI bridge crate that both this grammar and any consumer's own `tree-sitter` version (0.20 or 0.26, doesn't matter) can agree on independently — decouples the grammar crate's version from the parser crate's version, which is exactly the problem point 1 above ran into.
3. Dropped everything not needed to compile the Rust binding: `bindings/node/`, `bindings/swift/`, `queries/`, `examples/`, `script/`, Node tooling (`package.json`, `binding.gyp`, `.eslintrc.js`). Kept: `grammar.js` (documents what the parser actually accepts, even though the Rust build doesn't read it directly), `src/{parser.c,grammar.json,node-types.json,tree_sitter/{alloc,array,parser}.h}`, `bindings/rust/{lib.rs,build.rs}`, `LICENSE` (MIT, unmodified, copyright retained).

## Regenerating (if `grammar.js` ever needs a real edit)

Not expected during Block 00/04 (the grammar parsed every real FunC sample tried without a single `ERROR` node — see risk spike A's findings in `docs/abi-layer/ARCHITECTURE.md` §9). If a future FunC syntax gap is found (the same shape of gap `contract{}` was for `tree-sitter-tolk`, see `docs/security/blocks/00-bootstrap.md` §0.3):

```sh
git clone https://github.com/tree-sitter-grammars/tree-sitter-func <scratch-dir>
cd <scratch-dir>
# edit grammar.js
npx tree-sitter-cli@0.26.11 generate   # or whatever this workspace's tree-sitter version has become
# copy src/{parser.c,grammar.json,node-types.json} back over this directory's src/
```

`bindings/rust/{lib.rs,build.rs}` do not need to be regenerated by this process — they are hand-maintained (see point 2 above), only `src/*` is CLI-generated output.

## Consumed by

`crates/sting-abi-func` (dev-dependency only as of Block 00 — this is a risk-spike proof, not yet the real Tier 2 extractor; see [`../../crates/sting-abi-func/tests/parse_real_func_samples.rs`](../../crates/sting-abi-func/tests/parse_real_func_samples.rs)). Deliberately **not** a `[workspace]` member and **not** given `[lints] workspace = true` — it necessarily contains `unsafe`/FFI (the `extern "C"` grammar-loading thunk every tree-sitter binding needs, same as `tree-sitter-tolk`), and this workspace's `unsafe_code = "forbid"` lint is scoped to crates we author ourselves ([`docs/ARCHITECTURE.md` §3.1](../../docs/ARCHITECTURE.md#31-unsafe-код--только-в-двух-крейтах-с-разной-ролью)) — this crate is vendored third-party source, treated the same as any other external dependency with internal unsafe (e.g. `tree-sitter-tolk` itself, or `serde`), not as sting's own code.
