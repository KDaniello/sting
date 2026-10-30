//! A participant that is a separate process
//! ([§3.2](../../../docs/phases_roadmap/phase-6-benchmark.md)).
//!
//! Three things are declared **before** the run and published with the result,
//! or the result cannot be defended:
//!
//! 1. **The command** and how machine-readable output is obtained.
//! 2. **The mapping from their rule to a benchmark class** — [`RuleMapping`].
//!    Choosing it after seeing the numbers is fitting the comparison to them,
//!    which is why it is data rather than code and sits next to the report.
//! 3. **The participant's version and configuration.** A rule they disable by
//!    default, run as enabled, is a measurement error rather than rigour.
//!
//! A class a participant does not have at all simply does not enter the
//! mapping, and so never appears in the comparison as a zero. "There is no
//! rule" and "the rule exists and is wrong" are different claims.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::process::Command;

use serde::{Deserialize, Serialize};
use sting_bench::DatasetUnit;
use sting_core::{Finding, Location, Severity};

use crate::{Analyzer, AnalyzerError};

/// The declared correspondence between a participant's rules and the
/// benchmark's defect classes.
///
/// The key is the participant's stable rule code (`E007` for acton) and the
/// value is our class (`UBM`). A missing key means "the participant does not
/// have this class", never "zero findings".
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RuleMapping {
    /// The participant's rule code to the benchmark's class code.
    pub codes: BTreeMap<String, String>,
}

impl RuleMapping {
    /// The benchmark class for a participant's rule, where one is declared.
    #[must_use]
    pub fn class_for(&self, participant_code: &str) -> Option<&str> {
        self.codes.get(participant_code).map(String::as_str)
    }
}

/// How a participant prints its result.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum OutputFormat {
    /// The `acton check --output-format json` shape, described by their own
    /// `lint-report.schema.json`.
    ActonLintJson,
}

/// The full, publishable description of a process participant.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExternalParticipant {
    /// The name in the report.
    pub name: String,
    /// The version is part of the result rather than a footnote to it.
    pub version: String,
    /// The program and its arguments. The substring `{file}` in any argument
    /// is replaced with the path of the file being analysed.
    pub command: Vec<String>,
    /// The working directory, where a participant needs project context.
    pub working_dir: Option<PathBuf>,
    /// The output format.
    pub format: OutputFormat,
    /// The rule mapping, declared in advance.
    pub mapping: RuleMapping,
    /// What exactly was enabled or disabled in the participant's
    /// configuration — free text, published verbatim. It exists because "run
    /// with the defaults" and "a disabled rule turned on" give different
    /// numbers, and the reader has to see which of the two happened.
    pub configuration_note: String,
}

/// A process participant: writes the source to a temporary file, runs the
/// command, parses the output, and maps rules onto benchmark classes.
pub struct ExternalAnalyzer {
    participant: ExternalParticipant,
    workdir: PathBuf,
}

impl ExternalAnalyzer {
    /// `workdir` is where the temporary files for analysis are written.
    #[must_use]
    pub fn new(participant: ExternalParticipant, workdir: PathBuf) -> Self {
        Self {
            participant,
            workdir,
        }
    }

    /// The participant's description, published with the numbers.
    #[must_use]
    pub fn participant(&self) -> &ExternalParticipant {
        &self.participant
    }

    fn failure(&self, unit: &DatasetUnit, reason: impl Into<String>) -> AnalyzerError {
        AnalyzerError::Failed {
            analyzer: self.participant.name.clone(),
            unit: unit.name.clone(),
            reason: reason.into(),
        }
    }
}

impl Analyzer for ExternalAnalyzer {
    fn name(&self) -> &'static str {
        // The participant's name comes from data while the trait wants a
        // `&'static str`, so a constant for the role stands here; the real name
        // is `participant().name` and reaches the report from there.
        "external"
    }

    fn version(&self) -> &str {
        &self.participant.version
    }

    fn analyze(&self, unit: &DatasetUnit) -> Result<Vec<Finding>, AnalyzerError> {
        let safe_name: String = unit
            .name
            .chars()
            .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
            .collect();
        let path = self.workdir.join(format!("{safe_name}.tolk"));
        std::fs::create_dir_all(&self.workdir)
            .and_then(|()| std::fs::write(&path, &unit.source))
            .map_err(|e| self.failure(unit, format!("writing {}: {e}", path.display())))?;

        let (program, args) = self
            .participant
            .command
            .split_first()
            .ok_or_else(|| self.failure(unit, "empty command"))?;
        let args: Vec<String> = args
            .iter()
            .map(|a| a.replace("{file}", &path.display().to_string()))
            .collect();

        let mut command = Command::new(program);
        command.args(&args);
        if let Some(dir) = &self.participant.working_dir {
            command.current_dir(dir);
        }
        let output = command
            .output()
            .map_err(|e| self.failure(unit, format!("spawning {program}: {e}")))?;

        // A non-zero exit code from a linter is normal — it found something —
        // so it is not a refusal. A refusal is output that cannot be parsed.
        let stdout = String::from_utf8_lossy(&output.stdout);
        match self.participant.format {
            OutputFormat::ActonLintJson => parse_acton_json(&stdout, &self.participant.mapping)
                .map_err(|e| self.failure(unit, e)),
        }
    }
}

/// Parses an `acton check --output-format json` report against their own
/// `lint-report.schema.json`.
///
/// Their positions are **0-based** and ours are 1-based: an off-by-one here
/// would shift every finding and ruin the match against the labels.
///
/// # Errors
/// Returns a description of the problem when the output cannot be parsed as
/// JSON of the expected shape.
pub fn parse_acton_json(stdout: &str, mapping: &RuleMapping) -> Result<Vec<Finding>, String> {
    // A participant may print progress before the JSON, so it is taken from
    // the first `{`.
    let start = stdout
        .find('{')
        .ok_or_else(|| "no JSON object in output".to_string())?;
    let report: ActonReport =
        serde_json::from_str(&stdout[start..]).map_err(|e| format!("parsing JSON report: {e}"))?;

    let mut findings = Vec::new();
    for diagnostic in report.diagnostics {
        let code = diagnostic.code.as_deref().unwrap_or(&diagnostic.name);
        // A rule outside the declared mapping is skipped silently and
        // deliberately: a participant may have classes the benchmark does not,
        // and counting them against it would measure the wrong thing.
        let Some(class) = mapping.class_for(code) else {
            continue;
        };
        let line = diagnostic
            .annotations
            .iter()
            .find(|a| a.is_primary)
            .or_else(|| diagnostic.annotations.first())
            .map_or(1, |a| a.range.start.line + 1);
        let column = diagnostic
            .annotations
            .iter()
            .find(|a| a.is_primary)
            .or_else(|| diagnostic.annotations.first())
            .map_or(1, |a| a.range.start.character + 1);

        findings.push(Finding {
            detector_id: Box::leak(class.to_string().into_boxed_str()),
            severity: match diagnostic.severity.as_str() {
                "error" => Severity::High,
                "info" => Severity::Info,
                _ => Severity::Medium,
            },
            message: diagnostic.message,
            remediation: diagnostic.help.unwrap_or_default(),
            location: Location {
                file: PathBuf::from(diagnostic.file),
                start_line: line,
                start_column: column,
                end_line: line,
                end_column: column + 1,
            },
            confidence: None,
            dynamic_verification: None,
            suggestion: None,
        });
    }
    Ok(findings)
}

#[derive(Deserialize)]
struct ActonReport {
    diagnostics: Vec<ActonDiagnostic>,
}

#[derive(Deserialize)]
struct ActonDiagnostic {
    name: String,
    #[serde(default)]
    code: Option<String>,
    severity: String,
    file: String,
    message: String,
    #[serde(default)]
    help: Option<String>,
    #[serde(default)]
    annotations: Vec<ActonAnnotation>,
}

#[derive(Deserialize)]
struct ActonAnnotation {
    is_primary: bool,
    range: ActonRange,
}

#[derive(Deserialize)]
struct ActonRange {
    start: ActonPosition,
}

#[derive(Deserialize)]
struct ActonPosition {
    line: u32,
    character: u32,
}

/// The mapping from `acton`'s rules onto this benchmark's classes, fixed
/// **before** the first run.
///
/// Five classes, and exactly five: those where the comparison is literal. The
/// other 33 acton rules — style, naming, formatting — have no counterpart here,
/// and including them would measure the wrong thing.
///
/// | acton | code | our class |
/// |---|---|---|
/// | `deprecated_symbol_use` | `E003` | `DEPR` |
/// | `no_bounce_handler` | `E007` | `UBM` |
/// | `unauthorized_access` | `E013` | `AC` |
/// | `random_requires_initialization` | `E018` | `EPS` |
/// | `divide_before_multiply` | `E019` | `PL` |
#[must_use]
pub fn acton_rule_mapping() -> RuleMapping {
    RuleMapping {
        codes: [
            ("E003", "DEPR"),
            ("E007", "UBM"),
            ("E013", "AC"),
            ("E018", "EPS"),
            ("E019", "PL"),
        ]
        .into_iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect(),
    }
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "docs/ARCHITECTURE.md §3.2 scopes the no-unwrap/expect policy to library code outside tests"
)]
mod tests {
    use super::{acton_rule_mapping, parse_acton_json};

    const REPORT: &str = r#"{
      "success": false,
      "diagnostics": [
        {
          "name": "no_bounce_handler",
          "code": "E007",
          "severity": "warning",
          "file": "C:/tmp/unit.tolk",
          "message": "contract sends a message that may bounce",
          "source": "lint",
          "fixes": [],
          "annotations": [
            {"is_primary": true, "range": {"start": {"line": 41, "character": 4},
                                            "end": {"line": 41, "character": 8}}}
          ]
        },
        {
          "name": "unused_import",
          "code": "E024",
          "severity": "warning",
          "file": "C:/tmp/unit.tolk",
          "message": "unused import",
          "source": "lint",
          "fixes": [],
          "annotations": []
        }
      ]
    }"#;

    #[test]
    fn maps_a_declared_rule_and_converts_the_line_to_one_based() {
        let findings = parse_acton_json(REPORT, &acton_rule_mapping()).expect("parses");
        assert_eq!(findings.len(), 1, "only the declared rule is mapped");
        assert_eq!(findings[0].detector_id, "UBM");
        assert_eq!(
            findings[0].location.start_line, 42,
            "acton reports 0-based lines; off by one here would shift every finding"
        );
    }

    /// A participant's rule outside the mapping counts neither as a finding
    /// nor as an error: it has classes the benchmark does not.
    #[test]
    fn an_undeclared_rule_is_skipped_not_counted_against_the_participant() {
        let findings = parse_acton_json(REPORT, &acton_rule_mapping()).expect("parses");
        assert!(
            findings.iter().all(|f| f.detector_id != "E024"),
            "unused_import has no counterpart here and must not enter the comparison"
        );
    }

    #[test]
    fn output_with_leading_noise_before_the_json_still_parses() {
        let noisy = format!("compiling...\n{REPORT}");
        assert_eq!(
            parse_acton_json(&noisy, &acton_rule_mapping())
                .expect("parses")
                .len(),
            1
        );
    }

    #[test]
    fn unparseable_output_is_an_error_not_an_empty_finding_list() {
        assert!(
            parse_acton_json("total garbage", &acton_rule_mapping()).is_err(),
            "silently returning zero findings would score a broken run as a clean one"
        );
    }

    /// The mapping is fixed before the run, and this test catches it being
    /// changed quietly afterwards.
    #[test]
    fn the_declared_mapping_covers_exactly_the_five_overlapping_classes() {
        let mapping = acton_rule_mapping();
        let mut classes: Vec<&str> = mapping.codes.values().map(String::as_str).collect();
        classes.sort_unstable();
        assert_eq!(classes, vec!["AC", "DEPR", "EPS", "PL", "UBM"]);
    }
}
