# `vendor/tact/` — pinned `@tact-lang/compiler` install for `TactExtractor`

> `node_modules/` and `package-lock.json` are `.gitignore`d (see root
> `.gitignore`'s comment) — same reason as `vendor/tolk/`/`vendor/fift/`/
> `vendor/libemulator/`: large, environment-fetched, never committed.
> `package.json` (this directory's only committed file besides this
> README) pins the exact version.

## Setup

```sh
cd vendor/tact
npm install
```

Requires Node.js in `PATH` (confirmed working under Node 20.20.2, 2026-07-21,
despite the package's own `package.json` declaring
`"engines": {"node": ">=22.0.0"}` — `npm install` only warns
(`EBADENGINE`), it still installs and runs correctly).

## Why pinned to `1.6.13` specifically

`@tact-lang/compiler`'s own install-time deprecation notice (confirmed
verbatim): `Tact is deprecated. Please learn the Tolk language
https://docs.ton.org/tolk/overview instead.` — no further releases should
be assumed forthcoming, so this pins the exact version this crate's
extraction logic and golden fixtures (`crates/sting-abi-tact/tests/`) were
built and reviewed against, the same reasoning
`crates/sting-abi-tact/tests/fixtures/SOURCE.md` (Block 00 risk spike B)
already recorded.

## What `TactExtractor` actually invokes

`node vendor/tact/node_modules/@tact-lang/compiler/bin/tact.js <file.tact>`
— no `tact.config.json` needed, a bare file path is a first-class
alternative to `--config` (confirmed via `tact --help`'s own usage line).
Output artifacts (including the `.abi` JSON this crate reads) land next to
the source file, named `<source-file-name>_<ContractName>.<ext>` — see
`crates/sting-abi-tact/tests/fixtures/SOURCE.md` for the full confirmed
CLI contract, or `crates/sting-abi-tact/src/lib.rs` for how
`TactCompiler`/`TactExtractor` resolve the script path
(`STING_TACT_JS_PATH` env override, default next to this README).
