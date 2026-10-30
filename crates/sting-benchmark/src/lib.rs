//! A public TON security benchmark: **several** analysers over one dataset,
//! with their numbers compared.
//!
//! # Why a separate crate rather than a flag on `sting-bench`
//!
//! `sting-bench` is the CI regression gate: it checks `corpus/baseline.json` on
//! every run and has to be deterministic. The public benchmark runs **somebody
//! else's binaries** — which may not be on the machine, whose version moves
//! without our involvement, and which are entitled to crash. Merging them into
//! one crate would make every pull request's greenness depend on another
//! project's release.
//!
//! The dependency therefore runs one way only: `sting-benchmark` →
//! `sting-bench`. The reverse edge is forbidden, and a test checks it
//! `the_internal_gate_does_not_depend_on_this_crate`.
//!
//! # What is shared and what is not
//!
//! **The dataset and the scoring rules** are shared —
//! [`sting_bench::load_dataset`] and [`sting_bench::score_dataset`]. Were a
//! participant and ourselves measured over different data or by different
//! rules, the comparison could not be defended against any reasonable
//! objection.
//!
//! What is not shared is how findings are obtained: for us the built-in
//! detector registry, for a participant a process launched and its output
//! parsed. That is exactly
//! [`Analyzer`].

pub mod external;

use std::collections::BTreeMap;

use sting_bench::{BenchReport, DatasetUnit};
use sting_core::Finding;

/// One participant's failure on one dataset unit.
#[derive(Debug, thiserror::Error)]
pub enum AnalyzerError {
    /// The participant could not analyse this unit.
    #[error("analyzer {analyzer} failed on {unit}: {reason}")]
    Failed {
        /// The participant's name.
        analyzer: String,
        /// The dataset unit's name.
        unit: String,
        /// What exactly went wrong.
        reason: String,
    },
}

/// One participant in the comparison: anything that can produce findings from
/// source.
///
/// A deliberately narrow trait, knowing nothing about the labels or the
/// scoring. A participant answers "what do you see in this code", and
/// [`sting_bench::score_dataset`] judges — the same one for everybody,
/// ourselves included.
pub trait Analyzer {
    /// The participant's name in the report, for instance `sting` or
    /// `acton check`.
    fn name(&self) -> &'static str;

    /// The version and the run's date are part of the result rather than a
    /// footnote to it: a number about an analyser without its version cannot be
    /// checked.
    fn version(&self) -> &str;

    /// The findings on one dataset unit.
    ///
    /// # Errors
    /// [`AnalyzerError`] if the participant crashed or refused the input. A
    /// crash on one unit does not stop the run: it counts as "nothing found",
    /// which is the honest reading of a refusal.
    fn analyze(&self, unit: &DatasetUnit) -> Result<Vec<Finding>, AnalyzerError>;
}

/// One participant's result, plus what is needed to dispute it.
#[derive(Debug)]
pub struct ParticipantRun {
    /// The participant's name.
    pub name: String,
    /// The participant's version at the time of the run.
    pub version: String,
    /// The units the participant crashed on, published alongside the numbers.
    /// A silent "zero findings" and "the tool did not start" are different
    /// claims, and conflating them lies in favour of whoever crashes more.
    pub failures: Vec<String>,
    /// The numbers, computed by the same code as our own.
    pub report: BenchReport,
}

/// Runs one participant over the whole dataset.
#[must_use]
pub fn run_participant(analyzer: &dyn Analyzer, dataset: &[DatasetUnit]) -> ParticipantRun {
    let mut failures = Vec::new();
    let mut per_unit = Vec::with_capacity(dataset.len());

    for unit in dataset {
        match analyzer.analyze(unit) {
            Ok(findings) => per_unit.push(findings),
            Err(error) => {
                failures.push(format!("{error}"));
                per_unit.push(Vec::new());
            }
        }
    }

    ParticipantRun {
        name: analyzer.name().to_string(),
        version: analyzer.version().to_string(),
        failures,
        report: sting_bench::score_dataset(dataset, &per_unit),
    }
}

/// Runs every participant over the same dataset.
#[must_use]
pub fn run_all(analyzers: &[&dyn Analyzer], dataset: &[DatasetUnit]) -> Vec<ParticipantRun> {
    analyzers
        .iter()
        .map(|a| run_participant(*a, dataset))
        .collect()
}

/// The comparison table: a row per detector, a column per participant.
///
/// A class a participant does not have at all stays `None` and prints as "not
/// covered" rather than as zero. The difference is fundamental: "there is no
/// rule" and "the rule exists and is wrong" are different claims, and
/// conflating them lies in favour of whoever has fewer rules
/// ([§3.2](../../../docs/phases_roadmap/phase-6-benchmark.md)).
#[must_use]
pub fn comparison_table(runs: &[ParticipantRun]) -> BTreeMap<String, Vec<Option<f64>>> {
    let mut classes: Vec<String> = runs
        .iter()
        .flat_map(|r| r.report.by_detector.keys().cloned())
        .collect();
    classes.sort_unstable();
    classes.dedup();

    classes
        .into_iter()
        .map(|class| {
            let row = runs
                .iter()
                .map(|run| {
                    run.report
                        .by_detector
                        .get(&class)
                        .and_then(sting_bench::DetectorMetrics::precision)
                })
                .collect();
            (class, row)
        })
        .collect()
}

/// Our own analyser as an ordinary participant, with no privileges.
///
/// It exists not for convenience but as proof that the path from findings to a
/// score is the same for us as for anybody else's tool. Were our numbers
/// computed by a separate branch of code, any divergence could be blamed on
/// it.
pub struct StingAnalyzer {
    version: String,
}

impl StingAnalyzer {
    /// The participant that runs the built-in detector registry.
    #[must_use]
    pub fn new(version: impl Into<String>) -> Self {
        Self {
            version: version.into(),
        }
    }
}

impl Analyzer for StingAnalyzer {
    fn name(&self) -> &'static str {
        "sting"
    }

    fn version(&self) -> &str {
        &self.version
    }

    fn analyze(&self, unit: &DatasetUnit) -> Result<Vec<Finding>, AnalyzerError> {
        let registry = sting_detectors::DetectorRegistry::builtin();
        sting_pipeline::LintController::new(&registry)
            .lint_source(unit.name.clone().into(), unit.source.clone())
            .map_err(|error| AnalyzerError::Failed {
                analyzer: self.name().to_string(),
                unit: unit.name.clone(),
                reason: error.to_string(),
            })
    }
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "docs/ARCHITECTURE.md §3.2 scopes the no-unwrap/expect policy to library code outside tests"
)]
mod tests {
    use sting_bench::{DatasetUnit, GroundTruthEntry, Project, UnitOrigin};
    use sting_core::{Finding, Location, Severity};

    use super::{Analyzer, AnalyzerError, StingAnalyzer, comparison_table, run_all};

    /// A participant that reports whatever it was told to, including nothing.
    struct Canned {
        findings: Vec<Finding>,
        fail: bool,
        version: String,
    }

    impl Canned {
        fn new(findings: Vec<Finding>, fail: bool) -> Self {
            Self {
                findings,
                fail,
                version: "0.0.0-test".to_string(),
            }
        }
    }

    impl Analyzer for Canned {
        fn name(&self) -> &'static str {
            "canned"
        }
        fn version(&self) -> &str {
            &self.version
        }
        fn analyze(&self, unit: &DatasetUnit) -> Result<Vec<Finding>, AnalyzerError> {
            if self.fail {
                return Err(AnalyzerError::Failed {
                    analyzer: self.name().to_string(),
                    unit: unit.name.clone(),
                    reason: "deliberate".to_string(),
                });
            }
            Ok(self.findings.clone())
        }
    }

    fn unit_with_known_defect() -> DatasetUnit {
        let mut unit = DatasetUnit::from_project(
            &Project {
                language: sting_ast::Language::Tolk,
                name: "unit-1".to_string(),
                file_starts: Vec::new(),
                source: "fun f(s: slice) {\n    val x = s.loadUint(32);\n}\n".to_string(),
                ground_truth: vec![GroundTruthEntry {
                    defect_class: "LEP".to_string(),
                    line: 1,
                    column: 1,
                }],
                files: Vec::new(),
            },
            UnitOrigin::HandWritten,
        );
        unit.name = "unit-1".to_string();
        unit
    }

    fn finding_at(detector: &str, line: u32) -> Finding {
        Finding {
            detector_id: Box::leak(detector.to_string().into_boxed_str()),
            severity: Severity::Medium,
            message: format!("{detector} says so"),
            remediation: String::new(),
            location: Location {
                file: "unit-1".into(),
                start_line: line,
                start_column: 1,
                end_line: line,
                end_column: 2,
            },
            confidence: None,
            dynamic_verification: None,
            suggestion: None,
        }
    }

    /// The point of the split: findings that never went through our detector
    /// registry are scored by the same code and the same rules.
    #[test]
    fn findings_from_an_external_participant_are_scored_by_the_same_rules() {
        let dataset = vec![unit_with_known_defect()];
        let good = Canned::new(vec![finding_at("LEP", 1)], false);
        let silent = Canned::new(Vec::new(), false);

        let runs = run_all(&[&good, &silent], &dataset);
        let lep_hit = runs[0].report.by_detector.get("LEP").expect("scored");
        assert_eq!(
            (lep_hit.true_positives, lep_hit.false_negatives),
            (1, 0),
            "a participant that finds the labelled defect scores a true positive"
        );

        let lep_miss = runs[1].report.by_detector.get("LEP").expect("scored");
        assert_eq!(
            (lep_miss.true_positives, lep_miss.false_negatives),
            (0, 1),
            "a participant that stays silent misses it — not counted as correct"
        );
    }

    #[test]
    fn a_failed_participant_is_recorded_not_silently_zeroed() {
        let dataset = vec![unit_with_known_defect()];
        let broken = Canned::new(Vec::new(), true);
        let runs = run_all(&[&broken], &dataset);
        assert_eq!(
            runs[0].failures.len(),
            1,
            "the failure must survive into the published run, not vanish into a zero"
        );
    }

    #[test]
    fn a_class_no_participant_covers_is_absent_rather_than_zero() {
        let dataset = vec![unit_with_known_defect()];
        let only_lep = Canned::new(vec![finding_at("LEP", 1)], false);
        let runs = run_all(&[&only_lep], &dataset);
        let table = comparison_table(&runs);
        assert!(
            !table.contains_key("GVR"),
            "a class nobody reported on must not appear as a 0% row"
        );
    }

    /// `sting` itself goes through the participant path, with no shortcut.
    #[test]
    fn our_own_analyzer_is_just_another_participant() {
        let dataset = vec![unit_with_known_defect()];
        let runs = run_all(&[&StingAnalyzer::new("test")], &dataset);
        assert_eq!(runs[0].name, "sting");
        let lep = runs[0].report.by_detector.get("LEP").expect("LEP scored");
        assert_eq!(lep.true_positives, 1);
    }

    /// The actual constraint: the CI gate must not gain a path to
    /// anything that launches an external process.
    #[test]
    fn the_internal_gate_does_not_depend_on_this_crate() {
        let manifest = include_str!("../../sting-bench/Cargo.toml");
        assert!(
            !manifest.contains("sting-benchmark"),
            "sting-bench is the CI regression gate and must never depend on the public \
             benchmark, which runs third-party binaries"
        );
    }
}
