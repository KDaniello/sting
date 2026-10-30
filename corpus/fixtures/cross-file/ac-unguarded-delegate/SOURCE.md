# Source

Hand-authored, not vendored — minimal fixture for `sting-pipeline`'s
`ac_cross_file_real_tolk.rs` integration test (`git show a7280d2:docs/security/PROJECT_REVIEW.md`
направление 5, Фаза 5c). Same spirit as `corpus/fixtures/bridge`'s own
hand-authored patterns: the smallest real, `tolk`-compilable shape that
demonstrates one specific gap, not lifted from a real audited contract.

`Main.tolk`'s `onInternalMessage` calls `processDeposit`, a bare free
function defined in a *different* file (`helper.tolk`), which itself calls
`storage.save()` with no sender/payment/signature check on the path —
exactly the shape `ac.rs`'s own module doc comment names as structurally
invisible to `AC` before Фаза 5c (same-file-only `classify_function`), and
the shape Фаза 5c's `ProjectStore`-backed cross-file resolution exists to
close.

**Deliberately minimal, not modeled on a specific real contract**: no
`contract{}` header, no message-type parsing, no sender check anywhere —
confirmed to still compile cleanly with the real vendored `tolk.exe`
(`vendor/tolk/tolk.exe`, gitignored — see this crate's own note on
`corpus/clean/acton-contracts/config/SOURCE.md` for the general pattern).
Verified directly against the real binary before being committed:

```sh
TOLK_STDLIB=vendor/tolk/tolk-stdlib vendor/tolk/tolk.exe --output out.fif Main.tolk
```

produces a clean compile whose own `out.symbolTypes.json` attributes
`onInternalMessage` to `Main.tolk` and `processDeposit` to `helper.tolk` (two
distinct `file_id`s, `Main.tolk`'s own `imports` listing both `helper.tolk`
and `storage.tolk`) — the real cross-file shape the integration test relies
on, not assumed.

This fixture is intentionally **not** used by `sting bench`'s own
precision/recall scoring (no `.manifest.yaml`, unlike
`corpus/fixtures/detectors/AC/`) — that harness scores via
`LintController::lint_source` over textually merged projects, which never
attaches a compiled `ProjectStore` at all (see `LintController`'s own doc
comment) and would not exercise this code path regardless.
