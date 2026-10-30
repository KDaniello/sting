//! Golden-fixture regression tests (`docs/domains/detectors.md`): every
//! `corpus/fixtures/detectors/{CODE}/positive/*.tolk` must produce at least one
//! finding from detector `{CODE}`; every
//! `.../negative/*.tolk` must produce none.
//!
//! Locations/descriptions are cross-checked by hand against each fixture's
//! sibling `.manifest.yaml` (same format as `docs/domains/precision-sources.md`
//! §2, reused as-is by the benchmark harness). This test does not parse the
//! manifests itself: the presence or absence of a finding is what the fixtures
//! commit to, and tolerance-matching an exact line and column against ground
//! truth is the `sting bench` harness's job
//! (`docs/domains/precision-sources.md`), not duplicated here.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "docs/ARCHITECTURE.md §3.2 scopes the no-unwrap/expect policy to library code outside tests; this whole file is a test"
)]

use std::fs;
use std::path::{Path, PathBuf};

use sting_ast::AstStore;
use sting_core::Finding;
use sting_detectors::{DetectorContext, DetectorRegistry};

fn fixture_dir(code: &str, category: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../corpus/fixtures/detectors")
        .join(code)
        .join(category)
}

fn tolk_files(dir: &Path) -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = fs::read_dir(dir)
        .unwrap_or_else(|e| panic!("reading {}: {e}", dir.display()))
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|ext| ext == "tolk"))
        .collect();
    files.sort();
    files
}

fn findings_for(code: &str, path: &Path) -> Vec<Finding> {
    let source =
        fs::read_to_string(path).unwrap_or_else(|e| panic!("reading {}: {e}", path.display()));
    let tree =
        sting_ast::parse(&source).unwrap_or_else(|e| panic!("parsing {}: {e}", path.display()));
    let store = AstStore::build(&tree, &source);
    let ctx = DetectorContext::new(path.to_path_buf(), tree, source);
    let registry = DetectorRegistry::builtin();
    let detector = registry
        .detectors()
        .find(|d| d.id() == code)
        .unwrap_or_else(|| panic!("{code} is not a registered detector"));
    detector.check(&store, &ctx)
}

fn assert_golden_fixtures(code: &str) {
    let positives = tolk_files(&fixture_dir(code, "positive"));
    assert!(
        positives.len() >= 3,
        "{code} must have at least 3 positive golden fixtures, found {}",
        positives.len()
    );
    for path in &positives {
        let findings = findings_for(code, path);
        assert!(
            !findings.is_empty(),
            "{code} produced no findings for positive fixture {} (expected at least one)",
            path.display()
        );
        assert!(
            findings.iter().all(|f| f.detector_id == code),
            "{code}'s own check() must only ever produce {code} findings, got {:?} for {}",
            findings.iter().map(|f| f.detector_id).collect::<Vec<_>>(),
            path.display()
        );
    }

    let negatives = tolk_files(&fixture_dir(code, "negative"));
    assert!(
        negatives.len() >= 3,
        "{code} must have at least 3 negative golden fixtures, found {}",
        negatives.len()
    );
    for path in &negatives {
        let findings = findings_for(code, path);
        assert!(
            findings.is_empty(),
            "{code} produced {} unexpected finding(s) for negative fixture {}: {:?}",
            findings.len(),
            path.display(),
            findings.iter().map(|f| &f.message).collect::<Vec<_>>()
        );
    }
}

#[test]
fn gvr_golden_fixtures_pass() {
    assert_golden_fixtures("GVR");
}

#[test]
fn tlb_golden_fixtures_pass() {
    assert_golden_fixtures("TLB");
}

#[test]
fn pl_golden_fixtures_pass() {
    assert_golden_fixtures("PL");
}

#[test]
fn br_golden_fixtures_pass() {
    assert_golden_fixtures("BR");
}

#[test]
fn ur_golden_fixtures_pass() {
    assert_golden_fixtures("UR");
}

#[test]
fn ac_golden_fixtures_pass() {
    assert_golden_fixtures("AC");
}

#[test]
fn lep_golden_fixtures_pass() {
    assert_golden_fixtures("LEP");
}

#[test]
fn id_golden_fixtures_pass() {
    assert_golden_fixtures("ID");
}

#[test]
fn ubm_golden_fixtures_pass() {
    assert_golden_fixtures("UBM");
}

#[test]
fn uta_golden_fixtures_pass() {
    assert_golden_fixtures("UTA");
}

#[test]
fn depr_golden_fixtures_pass() {
    assert_golden_fixtures("DEPR");
}

#[test]
fn sua_golden_fixtures_pass() {
    assert_golden_fixtures("SUA");
}

#[test]
fn id_does_not_fire_on_struct_based_auto_serialized_storage() {
    // Named explicitly per docs/domains/detectors.md's Definition of
    // `ID` must correctly stay silent on struct-based, auto-serialized
    // storage, and that negative case gets its own test rather than being
    // folded into the general negative
    // loop above, even though it is also covered there.
    let path = fixture_dir("ID", "negative").join("02-struct-based-load-save.tolk");
    assert!(
        path.is_file(),
        "the dedicated struct-based-storage negative fixture must exist at {}",
        path.display()
    );
    let findings = findings_for("ID", &path);
    assert!(
        findings.is_empty(),
        "ID must not fire on struct-based (auto-serialized) load/save: {:?}",
        findings.iter().map(|f| &f.message).collect::<Vec<_>>()
    );
}
