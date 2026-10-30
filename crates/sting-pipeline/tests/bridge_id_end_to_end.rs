//! Real end-to-end validation of `BridgeController`'s `ID` strategy against
//! the vendored `libemulator` — the same chain
//! `bridge_ubm_end_to_end.rs` proves for `UBM`, filling a gap found while
//! wiring `sting verify` ('s direction 3):
//! `corpus/fixtures/bridge/IdSwappedPattern.*` already existed (prepared
//! alongside `UbmMintPattern.*` for this exact purpose) but had no test
//! actually exercising it — `ID`'s strategy, specifically
//! [`sting_pipeline::decode_stack_ints`] via `verify_id`, had *zero*
//! coverage against real emulator output before this file, only against
//! hand-scripted mocks (which, per the controller's own development, turned out
//! to need a subtly different cell shape than a first hand-built mock
//! guessed — see the unit tests in `crates/sting-pipeline/src/controllers/bridge.rs`).
//!
//! Same gating pattern as `bridge_ubm_end_to_end.rs`: returns early (a
//! trivial pass) when `vendor/libemulator/` is absent locally.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "docs/ARCHITECTURE.md §3.2 scopes the no-unwrap/expect/panic policy to library code outside tests"
)]

use std::path::{Path, PathBuf};

use sting_core::DynamicVerification;
use sting_detectors::DetectorRegistry;
use sting_emulator_sys::LibEmulator;
use sting_pipeline::{
    BridgeController, DetectorStatsSnapshot, EmulationPoolController, EmulationTarget, Hypothesis,
    LintController, ReportController, ReportFormat, ScoringController,
};

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn load_lib_or_skip() -> Option<LibEmulator> {
    match sting_emulator::load_default_library() {
        Ok(lib) => Some(lib),
        // A test that cannot load the library says so and passes; a silent
        // skip and a real run must never look alike.
        Err(error) => {
            println!("SKIP: {error} — see docs/domains/emulator.md for how to fetch it");
            None
        }
    }
}

fn read_fixture(name: &str) -> String {
    std::fs::read_to_string(repo_root().join("corpus/fixtures/bridge").join(name))
        .unwrap_or_else(|e| panic!("failed to read fixture {name}: {e}"))
        .trim()
        .to_string()
}

const OP_SAVE_FIELDS: u32 = 0x5341_5645; // "SAVE", corpus/fixtures/bridge/IdSwappedPattern.tolk
const FIELD_A: u32 = 100;
const FIELD_B: u32 = 200;

#[test]
fn id_swapped_pattern_is_found_statically_and_confirmed_dynamically_end_to_end() {
    let Some(lib) = load_lib_or_skip() else {
        return;
    };

    // --- Step 1: the real static ID detector finds the pattern. ---
    let source = read_fixture("IdSwappedPattern.tolk");
    let registry = DetectorRegistry::builtin();
    let lint = LintController::new(&registry);
    let findings = lint
        .lint_source(PathBuf::from("IdSwappedPattern.tolk"), source)
        .expect("fixture parses cleanly");
    let mut finding = findings
        .into_iter()
        .find(|f| f.detector_id == "ID")
        .expect("the static ID detector must flag corpus/fixtures/bridge/IdSwappedPattern.tolk");

    // --- Step 2: fixture code+data BoC stand in for a deployed instance. ---
    let target = EmulationTarget {
        code_boc_base64: read_fixture("IdSwappedPattern.code.boc.base64"),
        data_boc_base64: read_fixture("IdSwappedPattern.data.boc.base64"),
        address: "0:0000000000000000000000000000000000000000000000000000000000000000".to_string(),
    };

    // --- Step 3+4: BridgeController drives the real emulator. ---
    let pool = EmulationPoolController::new(&lib);
    let bridge = BridgeController::new(&pool);
    let hypothesis = Hypothesis::Id {
        save_op: OP_SAVE_FIELDS,
        field_a: FIELD_A,
        field_b: FIELD_B,
        get_method: "getFields".to_string(),
    };
    let verification = bridge.verify(&hypothesis, &target);
    let DynamicVerification::Confirmed(poc) = &verification else {
        panic!(
            "expected the swapped-field scenario to be dynamically Confirmed, got {verification:?}"
        );
    };
    assert!(
        poc.actual.contains(&FIELD_A.to_string()) && poc.actual.contains(&FIELD_B.to_string()),
        "PoC should describe the actually-read-back field values: {poc:?}"
    );
    finding.dynamic_verification = Some(verification.clone());

    // --- Step 5: ScoringController assigns a high confidence. ---
    // Real corpus/baseline.json ID precision (100%, docs/domains/precision-sources.md).
    let stats = DetectorStatsSnapshot::new([("ID".to_string(), 1.0)].into_iter().collect());
    let scoring = ScoringController::new(stats);
    let confidence = scoring.score(&finding);
    assert!(
        confidence.value() > 0.8,
        "a Confirmed ID finding should score high confidence, got {}",
        confidence.value()
    );
    finding.confidence = Some(confidence);

    // --- Step 6: SARIF carries real confidence/dynamicVerification. ---
    let report = ReportController;
    let sarif = report
        .render(&[finding], ReportFormat::Sarif, false)
        .expect("SARIF renders");
    assert!(
        sarif.contains("\"confidence\""),
        "SARIF must carry properties.confidence"
    );
    assert!(
        sarif.contains("\"dynamicVerification\""),
        "SARIF must carry properties.dynamicVerification"
    );
    assert!(
        sarif.contains("confirmed"),
        "SARIF's dynamicVerification value must reflect the real Confirmed result"
    );
}
