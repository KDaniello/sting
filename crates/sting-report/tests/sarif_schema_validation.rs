//! Validates `sting-report`'s SARIF output against the real, vendored
//! SARIF 2.1.0 JSON Schema. Output that claims to be SARIF is validated
//! against the schema rather than trusted to be right
//! (`docs/ARCHITECTURE.md` §4).
//!
//! See `tests/fixtures/SOURCE.md` for the schema's provenance.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "docs/ARCHITECTURE.md §3.2 scopes the no-unwrap/expect policy to library code outside tests; this whole file is a test"
)]

use std::path::PathBuf;
use std::sync::LazyLock;

use sting_core::{Finding, Location, Severity};
use sting_report::to_sarif;

static SCHEMA: LazyLock<serde_json::Value> = LazyLock::new(|| {
    let path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/sarif-schema-2.1.0.json");
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("reading vendored SARIF schema at {}: {e}", path.display()));
    serde_json::from_str(&text).expect("vendored SARIF schema is valid JSON")
});

fn validate(instance: &serde_json::Value) {
    let validator = jsonschema::validator_for(&SCHEMA).expect("vendored schema itself compiles");
    let errors: Vec<String> = validator
        .iter_errors(instance)
        .map(|e| format!("{e} (at {})", e.instance_path()))
        .collect();
    assert!(
        errors.is_empty(),
        "SARIF output failed schema validation:\n{}",
        errors.join("\n")
    );
}

fn sample_finding(detector_id: &'static str, severity: Severity) -> Finding {
    Finding {
        detector_id,
        severity,
        message: format!("{detector_id} triggered on this line"),
        remediation: format!("fix the {detector_id} issue"),
        location: Location {
            file: PathBuf::from("contracts/wallet.tolk"),
            start_line: 12,
            start_column: 9,
            end_line: 12,
            end_column: 14,
        },
        confidence: None,
        dynamic_verification: None,
        suggestion: None,
    }
}

#[test]
fn a_single_finding_report_passes_sarif_schema_validation() {
    let findings = vec![sample_finding("GVR", Severity::High)];
    let sarif = to_sarif(&findings);
    let instance = serde_json::to_value(&sarif).expect("Sarif serializes");
    validate(&instance);
}

#[test]
fn an_empty_findings_report_passes_sarif_schema_validation() {
    let sarif = to_sarif(&[]);
    let instance = serde_json::to_value(&sarif).expect("Sarif serializes");
    validate(&instance);
}

#[test]
fn a_report_covering_all_four_v1_detectors_and_severities_passes_sarif_schema_validation() {
    let findings = vec![
        sample_finding("GVR", Severity::High),
        sample_finding("LEP", Severity::Medium),
        sample_finding("ID", Severity::High),
        sample_finding("UBM", Severity::Critical),
    ];
    let sarif = to_sarif(&findings);
    let instance = serde_json::to_value(&sarif).expect("Sarif serializes");
    validate(&instance);
}

#[test]
fn a_finding_with_confidence_set_passes_sarif_schema_validation() {
    let mut finding = sample_finding("ID", Severity::High);
    finding.confidence =
        Some(sting_core::Confidence::new(0.42).expect("0.42 is a valid confidence"));
    let sarif = to_sarif(&[finding]);
    let instance = serde_json::to_value(&sarif).expect("Sarif serializes");
    validate(&instance);
}

#[test]
fn every_result_in_a_valid_report_carries_a_physical_location_region() {
    // Every finding carries a region. Schema validity alone would not catch a
    // report that simply omitted it, since the schema makes it optional, so
    // this is checked against the parsed JSON rather than inferred from the
    // schema passing.
    let findings = vec![
        sample_finding("GVR", Severity::High),
        sample_finding("LEP", Severity::Medium),
    ];
    let sarif = to_sarif(&findings);
    let instance = serde_json::to_value(&sarif).expect("Sarif serializes");
    let results = instance["runs"][0]["results"]
        .as_array()
        .expect("results is an array");
    assert_eq!(results.len(), findings.len());
    for result in results {
        let region = &result["locations"][0]["physicalLocation"]["region"];
        assert!(
            region.get("startLine").is_some(),
            "every result must carry physicalLocation.region.startLine: {result}"
        );
        assert!(
            region.get("startColumn").is_some(),
            "every result must carry physicalLocation.region.startColumn: {result}"
        );
    }
}
