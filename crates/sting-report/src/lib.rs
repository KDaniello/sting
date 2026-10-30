//! Finding aggregation and SARIF 2.1.0 serialization
//! (`docs/ARCHITECTURE.md` §6).

use std::collections::{BTreeMap, BTreeSet};

use serde_sarif::sarif::{
    ArtifactLocation, Location, Message, MultiformatMessageString, PhysicalLocation, PropertyBag,
    Region, ReportingDescriptor, Result as SarifResult, ResultLevel, Run, Sarif, Tool,
    ToolComponent,
};
use sting_core::{DynamicVerification, Finding, Severity};

/// This tool's name in every SARIF report it produces (`tool.driver.name`).
const TOOL_NAME: &str = "sting";

/// Builds a full SARIF 2.1.0 log from `findings`: one `run`, covering a
/// single `sting lint` invocation (multiple `runs` in one log are for
/// aggregating separate tool invocations, which sting has no use for).
#[must_use]
pub fn to_sarif(findings: &[Finding]) -> Sarif {
    let driver = ToolComponent::builder()
        .name(TOOL_NAME.to_string())
        .rules(distinct_rules(findings))
        .build();
    let run = Run::builder()
        .tool(Tool::builder().driver(driver).build())
        .results(findings.iter().map(finding_to_result).collect::<Vec<_>>())
        .build();
    Sarif::builder()
        .version(serde_json::json!("2.1.0"))
        .runs(vec![run])
        .build()
}

/// [`to_sarif`], serialized to pretty-printed JSON text — what `sting lint
/// --format sarif` actually writes out.
///
/// # Errors
/// Returns [`ReportError::Serialize`] if `serde_json` fails to serialize the
/// built [`Sarif`] value. Not expected in practice (nothing in [`to_sarif`]'s
/// output can contain non-UTF8 keys or cyclic references), but the
/// underlying call is fallible, so this stays a `Result` rather than
/// asserting internally.
pub fn to_json_string(findings: &[Finding]) -> Result<String, ReportError> {
    Ok(serde_json::to_string_pretty(&to_sarif(findings))?)
}

/// Errors from this crate.
#[derive(Debug, thiserror::Error)]
pub enum ReportError {
    /// Serializing the built SARIF document to JSON failed.
    #[error("failed to serialize SARIF report: {0}")]
    Serialize(#[from] serde_json::Error),
}

fn finding_to_result(finding: &Finding) -> SarifResult {
    let region = Region::builder()
        .start_line(i64::from(finding.location.start_line))
        .start_column(i64::from(finding.location.start_column))
        .end_line(i64::from(finding.location.end_line))
        .end_column(i64::from(finding.location.end_column))
        .build();
    let artifact_location = ArtifactLocation::builder()
        .uri(normalize_uri(&finding.location.file))
        .build();
    let physical_location = PhysicalLocation::builder()
        .artifact_location(artifact_location)
        .region(region)
        .build();
    let location = Location::builder()
        .physical_location(physical_location)
        .build();
    let message = Message::builder().text(finding.message.clone()).build();

    let mut fingerprints = BTreeMap::new();
    fingerprints.insert("stingStableId/v1".to_string(), finding.stable_id());

    SarifResult::builder()
        .rule_id(rule_id(finding.detector_id))
        .level(severity_to_level(effective_severity(finding)))
        .message(message)
        .locations(vec![location])
        .partial_fingerprints(fingerprints)
        .properties(finding_properties(finding))
        .build()
}

/// `properties.confidence`/`properties.dynamicVerification`/
/// `properties.remediation` (`docs/ARCHITECTURE.md` §6,
/// `docs/domains/emulator.md`): `confidence` is present only
/// once a [`Finding`] actually carries one (still `None` for a purely static
/// finding — `BridgeController`/`ScoringController` were never run against
/// it, e.g. `sting lint` with no on-chain deployment available).
/// `dynamicVerification` reflects the real [`DynamicVerification`] outcome
/// when present, `"not-attempted"` only when the bridge genuinely never ran
/// for this finding — not a permanent placeholder anymore. `remediation` is
/// always present (every [`Finding`] carries one, unconditionally) — a
/// SARIF consumer that only reads `result.message` never sees it otherwise,
/// since `Result` itself has no dedicated "how to fix" field (only
/// `ReportingDescriptor.help`, which is rule-level and therefore too coarse
/// for the instance-specific text this crate writes — see [`distinct_rules`]
/// for the rule-level counterpart).
fn finding_properties(finding: &Finding) -> PropertyBag {
    let mut properties = BTreeMap::new();
    if let Some(confidence) = finding.confidence {
        properties.insert(
            "confidence".to_string(),
            serde_json::json!(confidence.value()),
        );
    }
    properties.insert(
        "dynamicVerification".to_string(),
        serde_json::json!(dynamic_verification_label(
            finding.dynamic_verification.as_ref()
        )),
    );
    properties.insert(
        "remediation".to_string(),
        serde_json::json!(finding.remediation),
    );
    PropertyBag::builder()
        .additional_properties(properties)
        .build()
}

/// The `dynamicVerification` property's string value. `Confirmed`/`Refuted`
/// match the block doc's own vocabulary (§7.2) exactly, lowercased for SARIF
/// property-value convention; `Inconclusive` keeps its reason inline
/// (`"inconclusive: <reason>"`) rather than a second property key, since a
/// SARIF consumer filtering/displaying this property wants the reason right
/// there, not in a separate lookup.
fn dynamic_verification_label(verification: Option<&DynamicVerification>) -> String {
    match verification {
        None => "not-attempted".to_string(),
        Some(DynamicVerification::Confirmed(_)) => "confirmed".to_string(),
        Some(DynamicVerification::Refuted) => "refuted".to_string(),
        Some(DynamicVerification::Inconclusive(reason)) => format!("inconclusive: {reason}"),
    }
}

/// The severity actually rendered in the report: `finding.severity` as
/// determined by the static detector, **except** when dynamic verification
/// [`Refuted`](DynamicVerification::Refuted) it — the static pattern still
/// matched (worth a human's attention on manual review) but the runtime
/// behavior was confirmed safe, so it must not be rendered at the same
/// severity as an unverified or confirmed finding
/// (`docs/domains/emulator.md` §6: a refutation lowers the severity a report
/// renders and never removes the finding). `finding.severity`
/// itself is left untouched — this is a rendering-time decision, not a
/// mutation of the finding's own historical record.
fn effective_severity(finding: &Finding) -> Severity {
    if matches!(
        finding.dynamic_verification,
        Some(DynamicVerification::Refuted)
    ) {
        Severity::Info
    } else {
        finding.severity
    }
}

/// `sting/<CODE>` — the `ruleId` namespace (`docs/ARCHITECTURE.md` §6).
fn rule_id(detector_id: sting_core::DetectorId) -> String {
    format!("sting/{detector_id}")
}

/// `Critical`/`High` → `error`, `Medium` → `warning`, `Low`/`Info` → `note`
/// (`docs/ARCHITECTURE.md` §6's exact mapping).
const fn severity_to_level(severity: Severity) -> ResultLevel {
    match severity {
        Severity::Critical | Severity::High => ResultLevel::Error,
        Severity::Medium => ResultLevel::Warning,
        Severity::Low | Severity::Info => ResultLevel::Note,
    }
}

/// Forward-slash `uri` for a [`sting_core::Location::file`] path, so a SARIF
/// report produced on Windows (this project's primary dev platform,
/// `docs/ARCHITECTURE.md` §2) still reads as a valid `file`-scheme-relative
/// URI on any consumer, not a Windows-only backslash path.
fn normalize_uri(path: &std::path::Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

/// One [`ReportingDescriptor`] per distinct `detector_id` present in
/// `findings`, in first-seen order — `driver.rules` metadata SARIF
/// consumers (e.g. GitHub code scanning) use to resolve a `result`'s
/// `ruleId` to a human-readable rule.
///
/// `.help` is seeded from that first-seen finding's own `remediation` text —
/// not a separate, hand-maintained per-`DetectorId` lookup table, which
/// would just be a second copy of the same guidance every detector already
/// writes at its own `Finding` call site, free to drift out of sync with it.
/// A SARIF consumer that renders rule-level help (rather than, or in
/// addition to, each result's own `properties.remediation`) sees whichever
/// instance's phrasing happened to be first — every detector in this
/// workspace writes remediation text specific enough to the *pattern*, not
/// just the one instance, that this is a reasonable rule-level summary, not
/// a misleading one.
fn distinct_rules(findings: &[Finding]) -> Vec<ReportingDescriptor> {
    let mut seen = BTreeSet::new();
    findings
        .iter()
        .filter(|f| seen.insert(f.detector_id))
        .map(|f| {
            ReportingDescriptor::builder()
                .id(rule_id(f.detector_id))
                .help(
                    MultiformatMessageString::builder()
                        .text(f.remediation.clone())
                        .build(),
                )
                .build()
        })
        .collect()
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "docs/ARCHITECTURE.md §3.2 scopes the no-unwrap/expect policy to library code outside tests"
)]
mod tests {
    use std::path::PathBuf;

    use sting_core::{Confidence, Location};

    use super::{Finding, to_sarif};

    fn finding(detector_id: &'static str, severity: sting_core::Severity) -> Finding {
        Finding {
            detector_id,
            severity,
            message: format!("{detector_id} triggered"),
            remediation: format!("fix the {detector_id} issue"),
            location: Location {
                file: PathBuf::from("contract.tolk"),
                start_line: 3,
                start_column: 5,
                end_line: 3,
                end_column: 12,
            },
            confidence: None,
            dynamic_verification: None,
            suggestion: None,
        }
    }

    #[test]
    fn produces_exactly_one_result_per_finding() {
        let findings = vec![
            finding("GVR", sting_core::Severity::High),
            finding("UBM", sting_core::Severity::Critical),
        ];
        let sarif = to_sarif(&findings);
        assert_eq!(
            sarif.runs[0].results.as_ref().expect("results set").len(),
            2
        );
    }

    #[test]
    fn every_result_has_a_fully_populated_physical_location_region() {
        let findings = vec![finding("LEP", sting_core::Severity::Medium)];
        let sarif = to_sarif(&findings);
        let results = sarif.runs[0].results.as_ref().expect("results set");
        let region = results[0].locations.as_ref().expect("locations set")[0]
            .physical_location
            .as_ref()
            .expect("physical_location set")
            .region
            .as_ref()
            .expect("region set");
        assert_eq!(region.start_line, Some(3));
        assert_eq!(region.start_column, Some(5));
        assert_eq!(region.end_line, Some(3));
        assert_eq!(region.end_column, Some(12));
    }

    #[test]
    fn rule_id_is_namespaced_under_sting() {
        let findings = vec![finding("ID", sting_core::Severity::High)];
        let sarif = to_sarif(&findings);
        let results = sarif.runs[0].results.as_ref().expect("results set");
        assert_eq!(results[0].rule_id.as_deref(), Some("sting/ID"));
    }

    #[test]
    fn severity_maps_to_the_documented_sarif_levels() {
        use serde_sarif::sarif::ResultLevel;
        let cases = [
            (sting_core::Severity::Critical, ResultLevel::Error),
            (sting_core::Severity::High, ResultLevel::Error),
            (sting_core::Severity::Medium, ResultLevel::Warning),
            (sting_core::Severity::Low, ResultLevel::Note),
            (sting_core::Severity::Info, ResultLevel::Note),
        ];
        for (severity, expected) in cases {
            let sarif = to_sarif(&[finding("GVR", severity)]);
            let results = sarif.runs[0].results.as_ref().expect("results set");
            assert_eq!(
                results[0].level,
                Some(expected),
                "{severity:?} should map to {expected:?}"
            );
        }
    }

    #[test]
    fn confidence_property_is_absent_when_the_finding_has_none() {
        let sarif = to_sarif(&[finding("GVR", sting_core::Severity::High)]);
        let results = sarif.runs[0].results.as_ref().expect("results set");
        let props = &results[0]
            .properties
            .as_ref()
            .expect("properties set")
            .additional_properties;
        assert!(!props.contains_key("confidence"));
        assert_eq!(
            props.get("dynamicVerification").and_then(|v| v.as_str()),
            Some("not-attempted")
        );
    }

    #[test]
    fn confidence_property_is_present_when_the_finding_has_one() {
        let mut f = finding("GVR", sting_core::Severity::High);
        f.confidence = Some(Confidence::new(0.75).expect("valid confidence"));
        let sarif = to_sarif(&[f]);
        let results = sarif.runs[0].results.as_ref().expect("results set");
        let props = &results[0]
            .properties
            .as_ref()
            .expect("properties set")
            .additional_properties;
        assert_eq!(
            props.get("confidence").and_then(serde_json::Value::as_f64),
            Some(0.75)
        );
    }

    #[test]
    fn remediation_property_is_always_present() {
        let f = finding("GVR", sting_core::Severity::High);
        let sarif = to_sarif(&[f]);
        let results = sarif.runs[0].results.as_ref().expect("results set");
        let props = &results[0]
            .properties
            .as_ref()
            .expect("properties set")
            .additional_properties;
        assert_eq!(
            props.get("remediation").and_then(|v| v.as_str()),
            Some("fix the GVR issue")
        );
    }

    #[test]
    fn rule_help_is_seeded_from_the_first_seen_findings_remediation() {
        let mut second = finding("GVR", sting_core::Severity::High);
        second.remediation = "a different remediation string".to_string();
        let sarif = to_sarif(&[finding("GVR", sting_core::Severity::High), second]);
        let rules = &sarif.runs[0].tool.driver.rules;
        let rules = rules.as_ref().expect("rules set");
        assert_eq!(rules.len(), 1, "GVR must be deduplicated to one rule");
        let help = rules[0].help.as_ref().expect("help set").text.as_str();
        assert_eq!(
            help, "fix the GVR issue",
            "help must come from the first-seen finding, not the second"
        );
    }

    #[test]
    fn dynamic_verification_property_reflects_confirmed() {
        let mut f = finding("UBM", sting_core::Severity::Critical);
        f.dynamic_verification = Some(sting_core::DynamicVerification::Confirmed(
            sting_core::ProofOfConcept {
                expected: "storage commits before an unrevertable bounceable send".to_string(),
                actual: "totalSupply persisted at 1_000_000_000 with no onBouncedMessage"
                    .to_string(),
                steps: vec!["sent MINT op with amount=1_000_000_000".to_string()],
            },
        ));
        let sarif = to_sarif(&[f]);
        let results = sarif.runs[0].results.as_ref().expect("results set");
        let props = &results[0]
            .properties
            .as_ref()
            .expect("properties set")
            .additional_properties;
        assert_eq!(
            props.get("dynamicVerification").and_then(|v| v.as_str()),
            Some("confirmed")
        );
    }

    #[test]
    fn dynamic_verification_property_reflects_inconclusive_with_its_reason() {
        let mut f = finding("GVR", sting_core::Severity::High);
        f.dynamic_verification = Some(sting_core::DynamicVerification::Inconclusive(
            "no on-chain deployment available".to_string(),
        ));
        let sarif = to_sarif(&[f]);
        let results = sarif.runs[0].results.as_ref().expect("results set");
        let props = &results[0]
            .properties
            .as_ref()
            .expect("properties set")
            .additional_properties;
        assert_eq!(
            props.get("dynamicVerification").and_then(|v| v.as_str()),
            Some("inconclusive: no on-chain deployment available")
        );
    }

    #[test]
    fn refuted_finding_is_downgraded_to_note_level_but_still_present() {
        let mut f = finding("ID", sting_core::Severity::High);
        f.dynamic_verification = Some(sting_core::DynamicVerification::Refuted);
        let sarif = to_sarif(&[f]);
        let results = sarif.runs[0].results.as_ref().expect("results set");
        assert_eq!(results.len(), 1, "Refuted must not delete the finding");
        assert_eq!(
            results[0].level,
            Some(serde_sarif::sarif::ResultLevel::Note)
        );
        let props = &results[0]
            .properties
            .as_ref()
            .expect("properties set")
            .additional_properties;
        assert_eq!(
            props.get("dynamicVerification").and_then(|v| v.as_str()),
            Some("refuted")
        );
    }

    #[test]
    fn a_confirmed_finding_keeps_its_original_severity_level() {
        let mut f = finding("UBM", sting_core::Severity::Critical);
        f.dynamic_verification = Some(sting_core::DynamicVerification::Confirmed(
            sting_core::ProofOfConcept {
                expected: "x".to_string(),
                actual: "x".to_string(),
                steps: vec![],
            },
        ));
        let sarif = to_sarif(&[f]);
        let results = sarif.runs[0].results.as_ref().expect("results set");
        assert_eq!(
            results[0].level,
            Some(serde_sarif::sarif::ResultLevel::Error)
        );
    }

    #[test]
    fn distinct_rules_are_deduplicated_and_namespaced() {
        let findings = vec![
            finding("GVR", sting_core::Severity::High),
            finding("GVR", sting_core::Severity::High),
            finding("UBM", sting_core::Severity::Critical),
        ];
        let sarif = to_sarif(&findings);
        let rules = sarif.runs[0].tool.driver.rules.as_ref().expect("rules set");
        let ids: Vec<_> = rules.iter().map(|r| r.id.as_str()).collect();
        assert_eq!(ids, vec!["sting/GVR", "sting/UBM"]);
    }

    #[test]
    fn an_empty_findings_list_still_produces_a_valid_zero_result_run() {
        let sarif = to_sarif(&[]);
        assert!(
            sarif.runs[0]
                .results
                .as_ref()
                .expect("results set")
                .is_empty()
        );
    }

    #[test]
    fn to_json_string_round_trips_through_serde_json() {
        let findings = vec![finding("GVR", sting_core::Severity::High)];
        let text = super::to_json_string(&findings).expect("serializes");
        let value: serde_json::Value = serde_json::from_str(&text).expect("valid json");
        assert_eq!(value["version"], "2.1.0");
    }
}
