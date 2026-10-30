//! The linter's own output, recorded.
//!
//! The report crate already validates what it produces against the real SARIF 2.1.0 schema, which
//! says the shape is right. What that cannot say is whether the content is: a rule id that changed,
//! a message that lost its subject, a level that moved from warning to note. A recorded run over a
//! fixed contract catches those, and catches them as a diff rather than as an argument.
//!
//! The file is a fixture chosen for having findings in it: recording a clean contract would record
//! an empty report, which is the one report that cannot regress.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "AGENTS.md §5 scopes the no-unwrap/expect/panic rule to library code outside tests"
)]

use std::process::Command;

/// A fixture with a known unchecked-bounce finding in it.
const FIXTURE: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../corpus/fixtures/detectors/UBM/positive/01-no-bounce-handler-at-all.tolk"
);

#[test]
fn the_sarif_report_matches_its_recorded_shape() {
    assert!(
        std::path::Path::new(FIXTURE).is_file(),
        "the corpus fixtures are part of the checkout: {FIXTURE}"
    );
    let target = FIXTURE;
    let output = Command::new(env!("CARGO_BIN_EXE_sting"))
        .args(["lint", target, "--format", "sarif"])
        .output()
        .expect("the binary runs");
    let text = String::from_utf8(output.stdout).expect("the report is text");
    let report: serde_json::Value = serde_json::from_str(&text).expect("the report is JSON");

    // Three things in a report belong to the machine rather than to the finding: the path of the
    // file that was linted, the same path inside the stable identifier, and the tool's own version,
    // which moves with a release. All three are replaced before recording, so what is recorded is
    // the finding.
    let mut recorded = report.clone();
    let runs = recorded["runs"].as_array_mut().expect("one run");
    for run in runs.iter_mut() {
        run["tool"]["driver"]["version"] = serde_json::Value::Null;
        if let Some(results) = run["results"].as_array_mut() {
            for result in results.iter_mut() {
                if let Some(prints) = result["partialFingerprints"].as_object_mut() {
                    for (_, value) in prints.iter_mut() {
                        let text = value.as_str().unwrap_or_default();
                        let stable = text
                            .rsplit(['/', std::path::MAIN_SEPARATOR])
                            .next()
                            .unwrap_or(text);
                        *value = serde_json::Value::String(format!("<path>/{stable}"));
                    }
                }
                if let Some(locations) = result["locations"].as_array_mut() {
                    for location in locations.iter_mut() {
                        location["physicalLocation"]["artifactLocation"]["uri"] =
                            serde_json::Value::String("<the file that was linted>".to_string());
                    }
                }
            }
        }
    }

    insta::assert_json_snapshot!(recorded);
}
