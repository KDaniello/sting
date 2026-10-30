//! End-to-end proof that `GVR`'s cross-file storage resolution activates
//! through the real `sting lint` path — not just at the unit
//! level (`gvr.rs`'s own hand-built-`ProjectStore` test proves the
//! *mechanism*; this proves the *wiring*: `LintController::lint_paths` → a
//! real `tolk` compile → `DetectorContext::with_project` →
//! `GvrDetector::check`).
//!
//! Gated on a real, vendored `tolk` binary being present
//! (`vendor/tolk/tolk.exe`, gitignored) exactly like `sting-tolkc`'s own
//! `tests/real_compile.rs` — a fresh checkout without it is expected and not
//! a failure, so this test skips gracefully rather than failing.

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
fn gvr_resolves_a_cross_file_storage_struct_through_a_real_lint_paths_run() {
    let tolk_path = vendored_tolk_path();
    if !tolk_path.is_file() {
        println!(
            "SKIP: {} not present locally — fetch it from ton-blockchain/ton's tolk-X.Y.Z \
             release to run this test for real",
            tolk_path.display()
        );
        return;
    }

    let fixture_dir = repo_root().join("corpus/fixtures/cross-file/gvr-cross-file-storage");
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

    let gvr_findings: Vec<_> = outcome
        .findings
        .iter()
        .filter(|f| f.detector_id == "GVR")
        .collect();
    assert_eq!(
        gvr_findings.len(),
        1,
        "Main.tolk's local `var balance` shadows Storage's own `balance` field, and Storage is \
         cross-file-resolvable from storage.tolk, so it must be flagged. All GVR findings: \
         {gvr_findings:?}"
    );
    assert!(
        gvr_findings[0].message.contains("balance"),
        "got: {}",
        gvr_findings[0].message
    );
    assert!(
        gvr_findings[0].message.contains("Storage"),
        "got: {}",
        gvr_findings[0].message
    );
    assert_eq!(
        gvr_findings[0]
            .location
            .file
            .file_name()
            .and_then(|n| n.to_str()),
        Some("Main.tolk"),
        "the finding must be attributed to the file the shadowing var lives in, Main.tolk"
    );
}

/// Negative control: without a real `tolk` compile (this crate's own
/// `lint_source`, which never attaches a `ProjectStore` — see
/// `LintController`'s doc comment), the exact same cross-file struct is
/// invisible, exactly as it was before cross-file resolution existed. Not gated on
/// `vendor/tolk/` at all — this must hold on every machine, always.
#[test]
fn gvr_does_not_resolve_the_same_storage_struct_via_lint_source() {
    let fixture_dir = repo_root().join("corpus/fixtures/cross-file/gvr-cross-file-storage");
    let main_source =
        std::fs::read_to_string(fixture_dir.join("Main.tolk")).expect("fixture exists");

    let registry = DetectorRegistry::builtin();
    let findings = LintController::new(&registry)
        .lint_source("Main.tolk".into(), main_source)
        .expect("parses");
    assert!(
        !findings.iter().any(|f| f.detector_id == "GVR"),
        "lint_source must never attach a ProjectStore, so Storage stays unresolved \
         (same-file-only), exactly like every detector before cross-file resolution"
    );
}
