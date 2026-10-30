//! End-to-end proof that `UTA`'s cross-file caller lookup activates through
//! the real `sting lint` path — and specifically proof of the real gap found
//! and fixed alongside it: the file whose own body contains
//! the finding (`helper.tolk`'s `transfer`) never defines `onInternalMessage`
//! itself, so it only gets a `ProjectStore` at all because
//! `LintController::compile_reachable_projects` now attaches one compiled
//! project to *every* file it reaches, not just the entrypoint file that
//! triggered the compile (`Main.tolk`) — see the fixture's own `SOURCE.md`.
//!
//! Gated on a real, vendored `tolk` binary being present
//! (`vendor/tolk/tolk.exe`, gitignored), same pattern as this crate's other
//! `*_cross_file_real_tolk.rs` tests.

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
fn uta_resolves_a_caller_in_a_file_with_no_entrypoint_of_its_own() {
    let tolk_path = vendored_tolk_path();
    if !tolk_path.is_file() {
        println!(
            "SKIP: {} not present locally — fetch it from ton-blockchain/ton's tolk-X.Y.Z \
             release to run this test for real",
            tolk_path.display()
        );
        return;
    }

    let fixture_dir = repo_root().join("corpus/fixtures/cross-file/uta-cross-file-no-entrypoint");
    let paths = resolve_tolk_files(&fixture_dir).expect("fixture directory exists");
    assert_eq!(paths.len(), 2, "expected exactly Main.tolk, helper.tolk");

    let registry = DetectorRegistry::builtin();
    let outcome = LintController::new(&registry)
        .with_tolk_compiler(sting_tolkc::TolkCompiler::new(tolk_path))
        .lint_paths(&paths);
    assert!(
        outcome.failures.is_empty(),
        "every fixture file must parse cleanly, got {:?}",
        outcome.failures
    );

    let uta_findings: Vec<_> = outcome
        .findings
        .iter()
        .filter(|f| f.detector_id == "UTA")
        .collect();
    assert_eq!(
        uta_findings.len(),
        1,
        "transfer (defined in helper.tolk, which has no onInternalMessage of its own) is \
         called from Main.tolk's onInternalMessage with a signed .loadInt argument — must be \
         flagged now that helper.tolk itself receives Main.tolk's compiled ProjectStore. \
         All findings: {:?}",
        outcome.findings
    );
    assert_eq!(
        uta_findings[0]
            .location
            .file
            .file_name()
            .and_then(|n| n.to_str()),
        Some("helper.tolk"),
        "the finding must be attributed to helper.tolk, where transfer's own body is — not \
         Main.tolk, which merely calls it"
    );
}

/// Negative control, not gated on `vendor/tolk/` — through `lint_source`
/// (never attaches a `ProjectStore`), the same cross-file caller stays
/// invisible.
#[test]
fn uta_does_not_resolve_the_same_caller_via_lint_source() {
    let fixture_dir = repo_root().join("corpus/fixtures/cross-file/uta-cross-file-no-entrypoint");
    let helper_source =
        std::fs::read_to_string(fixture_dir.join("helper.tolk")).expect("fixture exists");

    let registry = DetectorRegistry::builtin();
    let findings = LintController::new(&registry)
        .lint_source("helper.tolk".into(), helper_source)
        .expect("parses");
    assert!(
        !findings.iter().any(|f| f.detector_id == "UTA"),
        "lint_source must never attach a ProjectStore, so the cross-file caller stays unresolved"
    );
}
