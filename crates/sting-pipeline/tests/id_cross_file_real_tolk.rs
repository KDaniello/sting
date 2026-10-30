//! End-to-end proof that `ID`'s cross-file store and load pairing activates
//! through the real `sting lint` path — not just at the unit level (`id.rs`'s
//! own hand-built-`ProjectStore` tests prove the *mechanism*; this proves the
//! *wiring*, and specifically that `storage.tolk` itself never needs to be
//! compiled as its own entrypoint — see the fixture's own `SOURCE.md`).
//!
//! Gated on a real, vendored `tolk` binary being present
//! (`vendor/tolk/tolk.exe`, gitignored), same pattern as `sting-tolkc`'s own
//! `tests/real_compile.rs` and `ac_cross_file_real_tolk.rs`.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "docs/ARCHITECTURE.md §3.2 scopes the no-unwrap/expect/panic policy to library code outside tests"
)]

use std::path::{Path, PathBuf};

use sting_detectors::DetectorRegistry;
use sting_pipeline::{LintController, resolve_tolk_files};

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn vendored_tolk_path() -> PathBuf {
    repo_root()
        .join("vendor/tolk")
        .join(sting_tolkc::platform_executable_filename())
}

#[test]
fn id_resolves_a_cross_file_swapped_pair_through_a_real_lint_paths_run() {
    let tolk_path = vendored_tolk_path();
    if !tolk_path.is_file() {
        println!(
            "SKIP: {} not present locally — fetch it from ton-blockchain/ton's tolk-X.Y.Z \
             release to run this test for real",
            tolk_path.display()
        );
        return;
    }

    let fixture_dir = repo_root().join("corpus/fixtures/cross-file/id-swapped-cross-file");
    let paths = resolve_tolk_files(&fixture_dir).expect("fixture directory exists");
    assert_eq!(paths.len(), 2, "expected exactly Main.tolk, storage.tolk");

    let registry = DetectorRegistry::builtin();
    let outcome = LintController::new(&registry)
        .with_tolk_compiler(sting_tolkc::TolkCompiler::new(tolk_path))
        .lint_paths(&paths);
    assert!(
        outcome.failures.is_empty(),
        "every fixture file must parse cleanly, got {:?}",
        outcome.failures
    );

    let id_findings: Vec<_> = outcome
        .findings
        .iter()
        .filter(|f| f.detector_id == "ID")
        .collect();
    assert_eq!(
        id_findings.len(),
        1,
        "storage.tolk's load reads balance/owner in the opposite order Main.tolk's save \
         writes them — must be flagged now that storage.tolk never needs its own compile. \
         All findings: {:?}",
        outcome.findings
    );
    assert!(id_findings[0].message.contains("save"));
    assert!(id_findings[0].message.contains("load"));
}

/// Negative control, not gated on `vendor/tolk/` — through `lint_source`
/// (never attaches a `ProjectStore`), the same cross-file pair stays
/// invisible, exactly like every detector's pre-session-2 behavior.
#[test]
fn id_does_not_resolve_the_same_pair_via_lint_source() {
    let fixture_dir = repo_root().join("corpus/fixtures/cross-file/id-swapped-cross-file");
    let main_source =
        std::fs::read_to_string(fixture_dir.join("Main.tolk")).expect("fixture exists");

    let registry = DetectorRegistry::builtin();
    let findings = LintController::new(&registry)
        .lint_source("Main.tolk".into(), main_source)
        .expect("parses");
    assert!(
        !findings.iter().any(|f| f.detector_id == "ID"),
        "lint_source must never attach a ProjectStore, so storage.tolk's load stays \
         unresolved (same-file-only)"
    );
}
