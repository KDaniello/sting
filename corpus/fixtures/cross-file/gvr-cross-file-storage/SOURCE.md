# Source

Hand-authored, not vendored — minimal fixture for `sting-pipeline`'s
`gvr_cross_file_real_tolk.rs` integration test (`git show a7280d2:docs/security/PROJECT_REVIEW.md`
направление 5, session 2, Фаза C1). Same spirit as
`corpus/fixtures/cross-file/ac-unguarded-delegate`: the smallest real,
`tolk`-compilable shape that demonstrates one specific gap, not lifted
from a real audited contract — though the shape itself (storage struct in
its own file, separate from the contract header/handlers) is extremely
common in this project's own corpus (`corpus/clean/tolk-bench/*/storage.tolk`,
`corpus/clean/acton-contracts/*/storage.tolk`, one `storage.tolk` per
project, every single one), unlike `ac-unguarded-delegate`'s more
narrowly-motivated shape.

`Main.tolk` declares `contract Wallet { storage: Storage }` and
`onInternalMessage` with a local `var balance` — shadowing `Storage`'s own
`balance` field — but `Storage` itself is declared in a *different* file
(`storage.tolk`), exactly the shape `gvr.rs`'s own module doc comment
named as structurally invisible to `GVR` before Фаза C1 (`AstStore` is
single-file, `struct_by_name` only ever saw `Main.tolk`'s own text).

**Confirmed to compile cleanly with the real vendored `tolk.exe`**
(`vendor/tolk/tolk.exe`, gitignored):

```sh
TOLK_STDLIB=vendor/tolk/tolk-stdlib vendor/tolk/tolk.exe --output out.fif Main.tolk
```

produces a clean compile whose own `out.symbolTypes.json` attributes
`Storage` (with its one field, `balance`) to `storage.tolk`'s own
`file_id`, distinct from `Main.tolk`'s — the real cross-file shape the
integration test relies on, not assumed.

This fixture is intentionally **not** used by `sting bench`'s own
precision/recall scoring (no `.manifest.yaml`, unlike
`corpus/fixtures/detectors/GVR/`) — that harness scores via
`LintController::lint_source` over textually merged projects, which never
attaches a compiled `ProjectStore` at all (see `LintController`'s own doc
comment) and would not exercise this code path regardless.
