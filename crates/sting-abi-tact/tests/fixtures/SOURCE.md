# Source

Real, public Tact source files + their real, unmodified compiler output,
fetched/produced 2026-07-21 for
`git show 15b2fe0:docs/abi-layer/blocks/00-bootstrap-shared-core.md` risk spike B ("confirm
the real, current CLI path to a `ContractABI` JSON, compile ≥2 real
contracts, save the output byte-for-byte"). All three are real end-to-end
test contracts from the Tact compiler's own test suite — not hand-written
for this repo — chosen because each exercises a different part of the ABI
shape: `getters.tact` has typed getters (including several method-id forms),
`message.tact` has string-literal receivers and no typed messages,
`opcodes.tact` has explicit numeric-literal message opcodes (binary/octal/
decimal/hex).

| File | Repository | Path | Commit |
|---|---|---|---|
| `getters.tact` | `tact-lang/tact` | `src/test/e2e-emulated/contract-getters/getters.tact` | `4529fa7deef0345bfdfc05e57da829f31f90c7b7` |
| `message.tact` | `tact-lang/tact` | `src/test/e2e-emulated/message-sending/message.tact` | `4529fa7deef0345bfdfc05e57da829f31f90c7b7` |
| `opcodes.tact` | `tact-lang/tact` | `src/test/e2e-emulated/message-sending/opcodes.tact` | `b1ed29d36680fd520a1ae3e66ec5ad566a502a24` |

The matching `*.abi` file next to each is the real, byte-for-byte,
unmodified `stdout`-to-disk output of compiling that exact `.tact` file (see
below) — not hand-edited, not reformatted.

## Confirmed CLI path (risk spike B's actual deliverable)

- **Package**: `@tact-lang/compiler` v1.6.13 (`npm view @tact-lang/compiler version`, 2026-07-21) — the exact version pinned here since, per that same package's own install-time deprecation notice (confirmed verbatim: `npm warn deprecated @tact-lang/compiler@1.6.13: Tact is deprecated. Please learn the Tolk language https://docs.ton.org/tolk/overview instead.`), no further releases should be assumed forthcoming. `package.json` declares `"engines": {"node": ">=22.0.0"}`; installing and running it under Node 20.20.2 (this machine, 2026-07-21) only prints an `EBADENGINE` warning — it still installs and runs correctly.
- **Binary**: `node_modules/@tact-lang/compiler/bin/tact.js` (the `tact` bin entry in its `package.json`).
- **No `tact.config.json`/`package.json` needed next to the source** — confirmed by `tact --help`'s own usage line, `tact [...flags] (--config CONFIG | FILE)`: a bare `.tact` file path is a first-class alternative to `--config`, not a fallback. Every fixture here was compiled with exactly:
  ```sh
  node node_modules/@tact-lang/compiler/bin/tact.js path/to/contract.tact
  ```
  run from this repo's/the scratch project's root — no config file anywhere.
- **Output location — next to the source, not the working directory**: with no `-o` flag, every artifact (`.abi`, `.code.boc`, `.fc`, `.fif`, `.md`, `.pkg`, `.ts`) lands in the **same directory as the source file**, named `<source-file-name-with-extension>_<ContractName>.<ext>` — e.g. `getters.tact` declaring `contract Test` produces `getters.tact_Test.abi` right beside `getters.tact` itself. This is the form `TactExtractor` (Block 01) should rely on: run the compiler with the source's own directory as the natural output location (or pass `-o` pointed at a scratch directory — confirmed separately that `-o DIR` resolves `DIR` relative to the *source file's* directory, not the process's working directory, a real gotcha worth the extractor getting right the first time), then read back `<dir>/<source-file-name>_<ContractName>.abi`. Multiple contracts in one file each get their own `_<ContractName>.abi`.
- **No fixed contract-name-to-filename relationship** — the `<ContractName>` in the output filename comes from the `contract Name { ... }` declaration, not the source filename (`getters.tact` → `contract Test` → `getters.tact_Test.abi`), confirmed necessary knowledge for `TactExtractor` to locate its own output without guessing.

## Real, confirmed shape of `ContractABI` (top-level keys, from `getters.tact_Test.abi`)

`name`, `types` (array of `{name, header: <u32 opcode>|null, fields: [{name, type: {kind, type, optional, format?}}]}` — covers both user structs/messages *and* compiler-injected stdlib types like `StateInit`/`Context`/`SendParameters`), `receivers` (array of `{receiver: "internal", message: {kind: "typed"|"empty"|"text", type?}}`), `getters` (array of `{name, methodId, arguments: [{name, type}], returnType}` — `methodId` **always present**, unlike the `git show 6a77438:docs/abi-layer/ARCHITECTURE.md` §2 sketch's "methodId opportional" phrasing, which was reasoning from the OTP-002 spec text, not yet-observed real output; corrected there), `errors` (object keyed by numeric-string exit code → `{message}` — the standard Tact/TVM exit codes plus none custom in these three fixtures), `interfaces` (array of TEP-like dotted strings, e.g. `"org.ton.introspection.v0"` — not yet accounted for anywhere in the normalized IR sketch, open gap for Block 01 to resolve, not invented here).

## Block 01 additions (2026-07-21) — 2 more fixtures, 1 more real ABI shape found

`git show b109a7d:docs/abi-layer/blocks/01-tolk-tact-extractors.md` §1.4 wants ≥5 golden fixtures per language; the 3 above (Block 00) cover getters/text-receivers/opcodes but never exercise a `map<K, V>` field or getter argument. Two more real contracts, same source (Tact compiler's own test suite), same non-fabricated principle:

| File | Repository | Path | Commit |
|---|---|---|---|
| `nested_struct.tact` | `tact-lang/tact` | `src/test/e2e-emulated/contract-state/contract-nested-struct.tact` | `dec5db0c8a2445c8248ba196a981d5f0a2cf4e3b` |
| `map_comparison.tact` | `tact-lang/tact` | `src/test/e2e-emulated/maps/map-comparison.tact` | `4529fa7deef0345bfdfc05e57da829f31f90c7b7` |

**`map_comparison.tact` caught a real bug before it shipped**: a `map<K, V>` type reference serializes as `{"kind":"dict","key":"uint","keyFormat":8,"value":"bool"}` — no `"type"`/`"optional"`/`"format"` keys at all, a completely different shape from every `"kind":"simple"` reference the 3 Block 00 fixtures ever showed. `crates/sting-abi-tact/src/abi_json.rs`'s `AbiTypeRef` was originally one flat struct assuming the `"simple"` shape was the only one — would have hard-failed deserializing *any* contract with a map in a getter argument or message field, which is common. Fixed by making `AbiTypeRef` a real `{tag = "kind"}` enum (`Simple`/`Dict`/`Other`) before this ever reached a committed golden fixture — see that module's own doc comment for the full story. No committed `*.abi` file for these two (unlike the Block 00 three) — Block 01's golden tests always live-compile via `TactCompiler`, never read a pre-saved artifact, so there was no reason to also commit the intermediate JSON.

Also caught by broadening past 3 fixtures: `sting_abi_core::GetMethod.tvm_method_id` was `Option<u32>` — `getters.tact`'s own `get(-pow2(18))` getter (`methodIdMin`) produces a **negative** `methodId: -262144`, already present in the Block 00 fixture set but only actually exercised once the mapping code was written against it in Block 01. TON get-method ids are 19-bit *signed* integers, not unsigned-only the way Tolk's own CRC16-derived defaults happen to always be. Fixed to `Option<i64>` — see `crates/sting-abi-core/src/method.rs`'s own doc comment.

## Post-implementation review additions (2026-07-21) — `nested_project/`

`nested_project/contracts/main.tact` is a plain **copy** of `opcodes.tact` above (not a new external download — same real provenance, just placed one directory deeper) — exists solely so `tests/golden.rs`'s `entrypoint_in_a_subdirectory_extracts_correctly` test has a real fixture laid out the way the standard Blueprint/Tact project scaffold actually looks (`contracts/*.tact`, not a flat file at the project root). An independent review pass found `TactCompiler`'s artifact search only checked the scratch copy's root directory — every fixture up to that point happened to be flat, so nothing caught it. Fixed in `crates/sting-abi-tact/src/compiler.rs`; this fixture keeps it caught.
