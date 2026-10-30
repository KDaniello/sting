# Source

Hand-authored, not vendored — minimal fixture for `sting-pipeline`'s
`uta_cross_file_real_tolk.rs` integration test (`git show a7280d2:docs/security/PROJECT_REVIEW.md`
направление 5, session 2, Фаза 2). Same spirit as
`corpus/fixtures/cross-file/ac-unguarded-delegate`/`id-swapped-cross-file`
(see their own `SOURCE.md`s).

**This fixture exists specifically to prove a real gap found and fixed this
same phase, not just the general cross-file mechanism** (already proven by
the `ac`/`id` fixtures): `helper.tolk` defines `transfer`, the function whose
own body contains the asymmetric-check pattern `UTA` looks for — but
`helper.tolk` itself defines no `onInternalMessage` at all. Before this
phase, `LintController::compile_for_cross_file_resolution` attached a
compiled `ProjectStore` only to the *same file* that triggered its own
compile (`Main.tolk`, which *does* define `onInternalMessage`) — meaning
`helper.tolk`'s own detector pass (where `UTA`'s finding must actually fire,
since that is the file `transfer` lives in) never received one at all,
silently missing this exact real shape end-to-end even though the detector
mechanism itself was already fully implemented and unit-tested.

Fixed by `LintController::compile_reachable_projects`: one compile per
`onInternalMessage`-bearing file in the whole batch, but the resulting
`ProjectStore` is now attached to **every** file it reaches
(`file_id_for_path` succeeding is proof of reachability — `tolk` only ever
lists files it actually needed to resolve the given entrypoint), not just
the entrypoint file itself.

Verified directly against the real vendored binary before being committed:

```sh
TOLK_STDLIB=vendor/tolk/tolk-stdlib vendor/tolk/tolk.exe --output out.fif Main.tolk
```

produces a clean compile (exit 0).
