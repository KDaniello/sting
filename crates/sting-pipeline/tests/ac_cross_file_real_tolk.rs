//! End-to-end proof that `AC`'s cross-file delegation activates through the
//! real `sting lint` path — not just at the unit level (`ac.rs`'s own
//! hand-built-`ProjectStore` tests prove the *mechanism*; this proves the
//! *wiring*: `LintController::lint_paths` → a real `tolk` compile →
//! `DetectorContext::with_project` → `AcDetector::check`).
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
fn ac_resolves_an_unguarded_cross_file_delegate_through_a_real_lint_paths_run() {
    let tolk_path = vendored_tolk_path();
    if !tolk_path.is_file() {
        println!(
            "SKIP: {} not present locally — fetch it from ton-blockchain/ton's tolk-X.Y.Z \
             release to run this test for real",
            tolk_path.display()
        );
        return;
    }

    let fixture_dir = repo_root().join("corpus/fixtures/cross-file/ac-unguarded-delegate");
    let paths = resolve_tolk_files(&fixture_dir).expect("fixture directory exists");
    assert_eq!(
        paths.len(),
        3,
        "expected exactly Main.tolk, helper.tolk, storage.tolk"
    );

    let registry = DetectorRegistry::builtin();
    // `LintController::new`'s own default `tolk` resolution is relative to
    // `current_exe()`, which for this test binary is some path under
    // `target/.../deps/` — never actually next to `vendor/tolk/tolk.exe` —
    // so this test supplies the real vendored path explicitly rather than
    // relying on that convention (see `with_tolk_compiler`'s own doc
    // comment).
    let outcome = LintController::new(&registry)
        .with_tolk_compiler(sting_tolkc::TolkCompiler::new(tolk_path))
        .lint_paths(&paths);
    assert!(
        outcome.failures.is_empty(),
        "every fixture file must parse cleanly, got {:?}",
        outcome.failures
    );

    let ac_findings: Vec<_> = outcome
        .findings
        .iter()
        .filter(|f| f.detector_id == "AC")
        .collect();
    assert_eq!(
        ac_findings.len(),
        1,
        "processDeposit (defined in helper.tolk) is unguarded and cross-file-resolvable \
         from Main.tolk's onInternalMessage, so it must be flagged. All AC findings: {:?}",
        outcome
            .findings
            .iter()
            .filter(|f| f.detector_id == "AC")
            .collect::<Vec<_>>()
    );
    assert!(
        ac_findings[0].message.contains("processDeposit"),
        "got: {}",
        ac_findings[0].message
    );
    assert_eq!(
        ac_findings[0]
            .location
            .file
            .file_name()
            .and_then(|n| n.to_str()),
        Some("Main.tolk"),
        "the finding must be attributed to the calling file, Main.tolk"
    );
}

/// Negative control: without a real `tolk` compile (this crate's own
/// `lint_source`, which never attaches a `ProjectStore` — see
/// `LintController`'s doc comment), the exact same cross-file call is
/// invisible, exactly as it was before cross-file resolution existed. Not gated on
/// `vendor/tolk/` at all — this must hold on every machine, always.
#[test]
fn ac_does_not_resolve_the_same_delegate_via_lint_source() {
    let fixture_dir = repo_root().join("corpus/fixtures/cross-file/ac-unguarded-delegate");
    let main_source =
        std::fs::read_to_string(fixture_dir.join("Main.tolk")).expect("fixture exists");

    let registry = DetectorRegistry::builtin();
    let findings = LintController::new(&registry)
        .lint_source("Main.tolk".into(), main_source)
        .expect("parses");
    assert!(
        !findings.iter().any(|f| f.detector_id == "AC"),
        "lint_source must never attach a ProjectStore, so processDeposit stays unresolved \
         (same-file-only), exactly like every detector before cross-file resolution"
    );
}
