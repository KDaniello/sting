//! Real end-to-end validation of `BridgeController`'s `UBM` strategy against
//! the vendored `libemulator`: at least one defect class reproduced end to
//! end (`docs/domains/emulator.md` §6).
//!
//! Follows the same gating pattern as
//! `crates/sting-emulator/tests/integration.rs`: `vendor/libemulator/` is
//! `.gitignore`d, so a fresh checkout without it present is expected and not
//! a failure — every test here returns early (a trivial pass) when the
//! library is absent. `.github/workflows/emulator.yml` fetches it before
//! running crates whose tests need it, including this one.
//!
//! Walks the full §7.4 chain against `corpus/fixtures/bridge/UbmMintPattern.*`
//! (see that directory's `SOURCE.md` for why this fixture, not the real
//! `JettonMinter.tolk`, is used — the confirmed `INMSG_SRC` limitation):
//! 1. `sting-detectors`' real `UbmDetector` finds the pattern in the source.
//! 2. The fixture's code+data `BoC` stand in for "deployed" — the real
//!    emulator executes the exact bytes a testnet deployment would hold; no
//!    literal testnet upload is performed (see this file's module doc for
//!    why that is the same scope call already documented in
//!    `corpus/fixtures/bridge/SOURCE.md`).
//! 3. `BridgeController` synthesizes the mint scenario via `Hypothesis::Ubm`.
//! 4. The real emulator confirms storage committed and an action was queued.
//! 5. `ScoringController` assigns a high confidence; the `PoC` is persisted.
//! 6. `sting-report` renders a SARIF result carrying real (not stub)
//!    `confidence`/`dynamicVerification` properties.
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

/// Loads the real vendored library, or prints a message and returns `None`
/// if it is not present locally — see this module's doc comment.
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

const OP_MINT: u32 = 0x4d49_4e54; // "MINT", corpus/fixtures/bridge/UbmMintPattern.tolk
const MINT_AMOUNT: u64 = 1_000_000_000;

#[test]
fn ubm_mint_pattern_is_found_statically_and_confirmed_dynamically_end_to_end() {
    let Some(lib) = load_lib_or_skip() else {
        return;
    };

    // --- Step 1: the real static UBM detector finds the pattern. ---
    let source = read_fixture("UbmMintPattern.tolk");
    let registry = DetectorRegistry::builtin();
    let lint = LintController::new(&registry);
    let findings = lint
        .lint_source(PathBuf::from("UbmMintPattern.tolk"), source)
        .expect("fixture parses cleanly");
    let mut finding = findings
        .into_iter()
        .find(|f| f.detector_id == "UBM")
        .expect("the static UBM detector must flag corpus/fixtures/bridge/UbmMintPattern.tolk");

    // --- Step 2: fixture code+data BoC stand in for a deployed instance. ---
    let target = EmulationTarget {
        code_boc_base64: read_fixture("UbmMintPattern.code.boc.base64"),
        data_boc_base64: read_fixture("UbmMintPattern.data.boc.base64"),
        address: "0:0000000000000000000000000000000000000000000000000000000000000000".to_string(),
    };

    // --- Step 3+4: BridgeController drives the real emulator. ---
    let pool = EmulationPoolController::new(&lib);
    let bridge = BridgeController::new(&pool);
    let hypothesis = Hypothesis::Ubm {
        mint_op: OP_MINT,
        mint_amount: MINT_AMOUNT,
    };
    let verification = bridge.verify(&hypothesis, &target);
    let DynamicVerification::Confirmed(poc) = &verification else {
        panic!("expected the mint scenario to be dynamically Confirmed, got {verification:?}");
    };
    assert!(
        poc.actual.contains("outbound action"),
        "PoC should describe the queued outbound action: {poc:?}"
    );
    finding.dynamic_verification = Some(verification.clone());

    // --- Step 5: ScoringController assigns a high confidence; PoC persisted. ---
    // Real corpus/baseline.json UBM precision (13/16 ≈ 0.8125,
    // docs/domains/precision-sources.md) — the same calibration input
    // crates/sting-pipeline/src/controllers/scoring.rs's own tests use, not
    // an invented number.
    let stats =
        DetectorStatsSnapshot::new([("UBM".to_string(), 13.0 / 16.0)].into_iter().collect());
    let scoring = ScoringController::new(stats);
    let confidence = scoring.score(&finding);
    assert!(
        confidence.value() > 0.8,
        "a Confirmed UBM finding should score high confidence, got {}",
        confidence.value()
    );
    finding.confidence = Some(confidence);

    let poc_artifact = serde_json::json!({
        "defect_class": "UBM",
        "fixture": "corpus/fixtures/bridge/UbmMintPattern.tolk",
        "expected": poc.expected,
        "actual": poc.actual,
        "steps": poc.steps,
        "confidence": confidence.value(),
    });
    let artifact_path = std::env::temp_dir().join("sting-ubm-poc.json");
    std::fs::write(
        &artifact_path,
        serde_json::to_string_pretty(&poc_artifact).expect("PoC artifact serializes"),
    )
    .expect("PoC artifact is written to disk as a reproducible record of this run");
    let reread = std::fs::read_to_string(&artifact_path).expect("PoC artifact is readable back");
    let reread: serde_json::Value =
        serde_json::from_str(&reread).expect("persisted PoC artifact is valid JSON");
    assert_eq!(reread["defect_class"], "UBM");
    assert_eq!(reread["actual"], poc.actual);
    println!("PoC artifact written to {}", artifact_path.display());

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
