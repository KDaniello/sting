# Source

Hand-authored, not vendored — minimal fixture for `sting-pipeline`'s
`id_cross_file_real_tolk.rs` integration test (`git show a7280d2:docs/security/PROJECT_REVIEW.md`
направление 5, session 2, Фаза 1). Same spirit as
`corpus/fixtures/cross-file/ac-unguarded-delegate` (see that fixture's own
`SOURCE.md`).

`Main.tolk` (has `onInternalMessage`, so `LintController::lint_paths`
attempts a real compile for it) defines `save`, writing `balance` then
`owner`. `storage.tolk` (imported by `Main.tolk`, no `onInternalMessage` of
its own — confirmed via direct experimentation that `tolk.exe` refuses to
compile a file with no `onInternalMessage` reachable at all: "fatal: the
contract has no entrypoint") defines `load`, reading the same two fields
back in the opposite order — the exact swapped-order shape `ID` exists to
catch, now split across two files.

**Key architectural fact this fixture exists to prove**: `ID`'s cross-file
gathering (`id.rs::gather_events`) doesn't need `storage.tolk` itself to
ever be compiled as an entrypoint — it only needs `Main.tolk` (which *can*
be compiled, since it has `onInternalMessage`) to transitively import
`storage.tolk`. `ProjectStore::reachable_files(main_file_id)` already
returns the full transitive closure, so `storage.tolk`'s `load` becomes a
visible cross-file candidate the moment `Main.tolk` itself is linted, with
no separate compile attempt for `storage.tolk` needed at all.

Verified directly against the real vendored binary before being committed:

```sh
TOLK_STDLIB=vendor/tolk/tolk-stdlib vendor/tolk/tolk.exe --output out.fif Main.tolk
```

produces a clean compile (exit 0).
