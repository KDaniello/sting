//! Report rendering (JSON + Markdown) and baseline regression comparison
//! (`docs/domains/precision-sources.md`, §3.4).

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::metrics::{BenchReport, UnitOrigin};

/// Errors from [`load_baseline`].
#[derive(Debug, thiserror::Error)]
pub enum BaselineError {
    /// The file could not be read.
    #[error("failed to read baseline {path}: {source}")]
    Io {
        /// The path that failed.
        path: String,
        /// Underlying I/O error.
        #[source]
        source: std::io::Error,
    },
    /// The file's contents are not valid JSON matching [`Baseline`]'s shape.
    #[error("failed to parse baseline {path}: {source}")]
    Parse {
        /// The path that failed.
        path: String,
        /// Underlying parse error.
        #[source]
        source: serde_json::Error,
    },
}

/// Loads a committed `corpus/baseline.json`.
///
/// # Errors
/// Returns [`BaselineError`] if `path` cannot be read or is not valid JSON
/// matching [`Baseline`]'s shape.
pub fn load_baseline(path: &Path) -> Result<Baseline, BaselineError> {
    let text = std::fs::read_to_string(path).map_err(|source| BaselineError::Io {
        path: path.display().to_string(),
        source,
    })?;
    serde_json::from_str(&text).map_err(|source| BaselineError::Parse {
        path: path.display().to_string(),
        source,
    })
}

/// The machine-readable snapshot committed as `corpus/baseline.json` and
/// compared on every `sting bench` run (`docs/domains/precision-sources.md`
/// §3.3) — deliberately just the aggregate counts a regression check needs,
/// not the full per-miss/per-false-positive detail (that only ever belongs
/// in the human-readable Markdown report, and would make the baseline noisy
/// to diff/review).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Baseline {
    /// Which version of this file's shape it is in.
    ///
    /// It is committed, diffed and compared against months later, so it says which shape it is
    /// rather than leaving its reader to guess. A file written before the field existed reads as
    /// the version it actually is.
    #[serde(default = "schema_version_before_the_field")]
    pub schema_version: String,
    /// Per-detector counts.
    pub detectors: BTreeMap<String, DetectorSnapshot>,
}

/// This file format's own version.
pub const BASELINE_SCHEMA_VERSION: &str = "1.0";

/// The version a baseline written before the field existed is read as.
fn schema_version_before_the_field() -> String {
    // Pinned rather than following the constant above: a file with no version field is a
    // file of the shape 1.0 describes, and it stays that shape when the format moves on.
    "1.0".to_string()
}

/// One detector's counts in a [`Baseline`].
#[derive(Debug, Clone, Copy, Serialize, Deserialize, Default)]
pub struct DetectorSnapshot {
    /// True positives.
    pub true_positives: u32,
    /// False positives.
    pub false_positives: u32,
    /// False negatives.
    pub false_negatives: u32,
}

impl From<&BenchReport> for Baseline {
    fn from(report: &BenchReport) -> Self {
        let detectors = report
            .by_detector
            .iter()
            .map(|(id, m)| {
                (
                    id.clone(),
                    DetectorSnapshot {
                        true_positives: m.true_positives,
                        false_positives: m.false_positives,
                        false_negatives: m.false_negatives,
                    },
                )
            })
            .collect();
        Self {
            schema_version: BASELINE_SCHEMA_VERSION.to_string(),
            detectors,
        }
    }
}

/// One detector's metrics moved worse than `tolerance` findings relative to
/// the baseline: on a small corpus one finding either way is noise, and the
/// exact threshold is calibrated from measurement
/// (`docs/domains/precision-sources.md`).
#[derive(Debug, Clone)]
pub struct Regression {
    /// Which detector regressed.
    pub detector_id: String,
    /// Human-readable description of the regression.
    pub description: String,
}

/// Compares `current` against `baseline`, returning every detector whose
/// true-positive count dropped, or false-positive count rose, by more than
/// `tolerance` findings. A detector present in `current` but absent from
/// `baseline` (a newly added detector) is not a regression.
#[must_use]
pub fn compare_to_baseline(
    current: &BenchReport,
    baseline: &Baseline,
    tolerance: u32,
) -> Vec<Regression> {
    let mut regressions = Vec::new();
    for (detector_id, base) in &baseline.detectors {
        let current_metrics = current.by_detector.get(detector_id);
        let (tp, fp) = current_metrics.map_or((0, 0), |m| (m.true_positives, m.false_positives));

        if tp + tolerance < base.true_positives {
            regressions.push(Regression {
                detector_id: detector_id.clone(),
                description: format!(
                    "true positives dropped from {} to {tp} (recall regression)",
                    base.true_positives
                ),
            });
        }
        if fp > base.false_positives + tolerance {
            regressions.push(Regression {
                detector_id: detector_id.clone(),
                description: format!(
                    "false positives rose from {} to {fp} (precision regression)",
                    base.false_positives
                ),
            });
        }
    }
    regressions
}

/// Renders `report` as JSON (`docs/domains/precision-sources.md`:
/// machine-readable, for the CI gate.
#[must_use]
pub fn to_json(report: &BenchReport) -> serde_json::Value {
    let by_detector: serde_json::Map<String, serde_json::Value> = report
        .by_detector
        .iter()
        .map(|(id, m)| {
            (
                id.clone(),
                serde_json::json!({
                    "true_positives": m.true_positives,
                    "false_positives": m.false_positives,
                    "false_negatives": m.false_negatives,
                    "precision": m.precision(),
                    "recall": m.recall(),
                }),
            )
        })
        .collect();
    let by_origin: serde_json::Map<String, serde_json::Value> = UnitOrigin::all()
        .into_iter()
        .map(|origin| {
            let (tp, fp, fn_) = report.origin_totals(origin);
            (
                origin.label().to_string(),
                serde_json::json!({
                    "true_positives": tp,
                    "false_positives": fp,
                    "false_negatives": fn_,
                    "precision": report.origin_precision(origin),
                    "recall": report.origin_recall(origin),
                }),
            )
        })
        .collect();
    serde_json::json!({
        "clean_project_count": report.clean_project_count,
        "mutation_instance_count": report.mutation_instance_count,
        "golden_fixture_count": report.golden_fixture_count,
        "by_detector": by_detector,
        "aggregate_precision": report.aggregate_precision(),
        "aggregate_recall": report.aggregate_recall(),
        "default_set_precision": report.default_set_precision(),
        "detectors_outside_default_set": report.detectors_outside_default_set(),
        "by_origin": by_origin,
    })
}

/// [`to_json`], serialized to pretty-printed JSON text.
#[must_use]
pub fn to_json_string(report: &BenchReport) -> String {
    serde_json::to_string_pretty(&to_json(report)).unwrap_or_default()
}

/// [`Baseline`], serialized to pretty-printed JSON text — what
/// `--update-baseline` writes to `corpus/baseline.json`.
#[must_use]
pub fn baseline_to_json_string(baseline: &Baseline) -> String {
    serde_json::to_string_pretty(baseline).unwrap_or_default()
}

/// Renders the origin breakdown — split out of [`to_markdown`] to keep that
/// function under clippy's line limit.
/// One row per detector per language, for every detector that reads more than one.
///
/// A detector that has crossed to a second language does not read both equally well, and a row
/// averaging the two describes neither. Printing them apart is also what makes "the first
/// language's figures did not move" checkable rather than asserted: the row is still there, with
/// the same numbers in it, beside the new one.
fn write_language_section(out: &mut String, report: &BenchReport) {
    let crossed: Vec<&str> = crate::DETECTOR_CODES
        .into_iter()
        .filter(|code| {
            report
                .by_language
                .values()
                .filter(|per_detector| per_detector.contains_key(*code))
                .count()
                > 1
        })
        .collect();
    if crossed.is_empty() {
        return;
    }

    let _ = write!(
        out,
        "\n### By language\n\nOnly the detectors this corpus measured in more than one. What a \
         detector has a rule for is its own to declare and a run says so; a row missing here means \
         the corpus carried nothing to score it on, which is a third thing again.\n\n"
    );
    let _ = writeln!(
        out,
        "| Detector | Language | TP | FP | FN | Precision | Recall |"
    );
    let _ = writeln!(out, "|---|---|---|---|---|---|---|");
    for (language, per_detector) in &report.by_language {
        for code in &crossed {
            let Some(m) = per_detector.get(*code) else {
                continue;
            };
            let _ = writeln!(
                out,
                "| {code} | {} | {} | {} | {} | {} | {} |",
                language.label(),
                m.true_positives,
                m.false_positives,
                m.false_negatives,
                format_percent(m.precision()),
                format_percent(m.recall()),
            );
        }
    }
    let _ = writeln!(out);
}

fn write_origin_section(out: &mut String, report: &BenchReport) {
    // The published dataset must not let one row read as three
    // different claims. Synthetic and real halves are counted apart.
    let _ = write!(out, "\n### By origin\n\n");
    let _ = write!(
        out,
        "| Origin | TP | FP | FN | Precision | Recall |\n|---|---|---|---|---|---|\n"
    );
    for origin in UnitOrigin::all() {
        let (tp, fp, fn_) = report.origin_totals(origin);
        if tp + fp + fn_ == 0 {
            continue;
        }
        // Neither precision nor recall is printed as a number where there is
        // no ground truth to compute it against — see
        // `BenchReport::origin_precision`'s own doc comment for why a
        // zero-by-construction figure is worse than none.
        let cell =
            |value: Option<f64>| value.map_or_else(|| "n/a".to_string(), |_| format_percent(value));
        let _ = writeln!(
            out,
            "| {} | {tp} | {fp} | {fn_} | {} | {} |",
            origin.label(),
            cell(report.origin_precision(origin)),
            cell(report.origin_recall(origin)),
        );
    }
    let (_, real_fp, _) = report.origin_totals(UnitOrigin::RealCode);
    let per_project = if report.clean_project_count > 0 {
        #[expect(
            clippy::cast_precision_loss,
            reason = "project counts are in the tens; f64 is exact well past that"
        )]
        {
            f64::from(real_fp) / report.clean_project_count as f64
        }
    } else {
        0.0
    };
    let _ = write!(
        out,
        "\n`real-code` is vendored production contracts, scored with **no ground truth**: \
         nothing there is labelled, so no finding can be a true positive and precision/recall \
         are undefined rather than zero. What it does measure is noise — {real_fp} findings \
         across {} projects, {per_project:.1} per project.\n\n\
         Recall is therefore measured only on defects this harness injected or hand-wrote. \
         No organic defect in untouched production code is part of any recall figure here.\n",
        report.clean_project_count,
    );
}

/// Renders `report` as a self-contained Markdown document
/// — understandable to an outside reader without explanation
/// (`docs/domains/precision-sources.md`).
#[must_use]
pub fn to_markdown(report: &BenchReport) -> String {
    let mut out = String::new();
    out.push_str("# sting bench report\n\n");
    let _ = write!(
        out,
        "Corpus: {} clean projects, {} mutation instances, {} hand-written golden fixtures.\n\n",
        report.clean_project_count,
        report.mutation_instance_count.values().sum::<usize>(),
        report.golden_fixture_count.values().sum::<usize>(),
    );

    out.push_str(
        "| Detector | TP | FP | FN | Precision | Recall | Mutations | Golden fixtures |\n",
    );
    out.push_str("|---|---|---|---|---|---|---|---|\n");
    for code in crate::DETECTOR_CODES {
        let empty = crate::metrics::DetectorMetrics::default();
        let m = report.by_detector.get(code).unwrap_or(&empty);
        let _ = writeln!(
            out,
            "| {code} | {} | {} | {} | {} | {} | {} | {} |",
            m.true_positives,
            m.false_positives,
            m.false_negatives,
            format_percent(m.precision()),
            format_percent(m.recall()),
            report
                .mutation_instance_count
                .get(code)
                .copied()
                .unwrap_or(0),
            report.golden_fixture_count.get(code).copied().unwrap_or(0),
        );
    }

    let _ = write!(
        out,
        "\n**Aggregate**: precision {}, recall {}.\n",
        format_percent(report.aggregate_precision()),
        format_percent(report.aggregate_recall()),
    );

    write_origin_section(&mut out, report);
    write_language_section(&mut out, report);

    // The aggregate above covers everything measured, including the
    // experimental and reference-category detectors. The one below covers
    // what a user actually sees without configuring anything — the number
    // the gate is stated in terms of.
    let outside = report.detectors_outside_default_set();
    if !outside.is_empty() {
        let _ = writeln!(
            out,
            "**Default set** (excludes {}): precision {}.",
            outside.join(", "),
            format_percent(report.default_set_precision()),
        );
    }

    let missed: Vec<_> = report
        .by_detector
        .values()
        .flat_map(|m| m.missed.iter())
        .collect();
    if !missed.is_empty() {
        out.push_str("\n## False negatives (missed defects)\n\n");
        for m in missed {
            let _ = writeln!(
                out,
                "- `{}` in `{}` (expected near line {})",
                m.defect_class, m.unit_name, m.line
            );
        }
    }

    let spurious: Vec<_> = report
        .by_detector
        .values()
        .flat_map(|m| m.spurious.iter())
        .collect();
    if !spurious.is_empty() {
        out.push_str("\n## False positives\n\n");
        for s in spurious {
            let _ = writeln!(
                out,
                "- `{}` in `{}`:{}: {}",
                s.detector_id, s.unit_name, s.line, s.message
            );
        }
    }

    out
}

fn format_percent(value: Option<f64>) -> String {
    value.map_or_else(|| "n/a".to_string(), |v| format!("{:.1}%", v * 100.0))
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "docs/ARCHITECTURE.md §3.2 scopes the no-unwrap/expect policy to library code outside tests"
)]
mod tests {
    use super::{
        Baseline, baseline_to_json_string, compare_to_baseline, load_baseline, to_json, to_markdown,
    };
    use crate::metrics::{BenchReport, DetectorMetrics};

    fn report_with(detector: &str, tp: u32, fp: u32, fn_: u32) -> BenchReport {
        let mut report = BenchReport::default();
        report.by_detector.insert(
            detector.to_string(),
            DetectorMetrics {
                true_positives: tp,
                false_positives: fp,
                false_negatives: fn_,
                missed: Vec::new(),
                spurious: Vec::new(),
            },
        );
        report
    }

    #[test]
    fn load_baseline_round_trips_through_a_real_file() {
        let report = report_with("UBM", 3, 0, 1);
        let baseline = Baseline::from(&report);
        let path = std::env::temp_dir().join(format!(
            "sting-bench-test-baseline-{}.json",
            std::process::id()
        ));
        std::fs::write(&path, baseline_to_json_string(&baseline)).expect("writes");
        let loaded = load_baseline(&path).expect("loads");
        std::fs::remove_file(&path).ok();
        assert_eq!(loaded.detectors["UBM"].true_positives, 3);
        assert_eq!(loaded.detectors["UBM"].false_negatives, 1);
    }

    #[test]
    fn load_baseline_errors_clearly_on_a_missing_file() {
        let missing = std::env::temp_dir().join("sting-bench-does-not-exist.json");
        assert!(load_baseline(&missing).is_err());
    }

    #[test]
    fn baseline_round_trips_through_json() {
        let report = report_with("GVR", 10, 1, 0);
        let baseline = Baseline::from(&report);
        let json = serde_json::to_string(&baseline).expect("serializes");
        let restored: Baseline = serde_json::from_str(&json).expect("deserializes");
        assert_eq!(restored.detectors["GVR"].true_positives, 10);
    }

    #[test]
    fn no_regression_when_metrics_match_baseline() {
        let report = report_with("GVR", 10, 1, 0);
        let baseline = Baseline::from(&report);
        assert!(compare_to_baseline(&report, &baseline, 0).is_empty());
    }

    #[test]
    fn dropped_true_positives_is_a_regression() {
        let baseline = Baseline::from(&report_with("GVR", 10, 0, 0));
        let current = report_with("GVR", 7, 0, 0);
        let regressions = compare_to_baseline(&current, &baseline, 0);
        assert_eq!(regressions.len(), 1);
        assert_eq!(regressions[0].detector_id, "GVR");
    }

    #[test]
    fn increased_false_positives_is_a_regression() {
        let baseline = Baseline::from(&report_with("GVR", 10, 1, 0));
        let current = report_with("GVR", 10, 5, 0);
        let regressions = compare_to_baseline(&current, &baseline, 0);
        assert_eq!(regressions.len(), 1);
    }

    #[test]
    fn small_drift_within_tolerance_is_not_a_regression() {
        let baseline = Baseline::from(&report_with("GVR", 10, 1, 0));
        let current = report_with("GVR", 9, 2, 0);
        assert!(compare_to_baseline(&current, &baseline, 1).is_empty());
    }

    #[test]
    fn a_detector_missing_from_current_but_present_in_baseline_is_a_regression() {
        let baseline = Baseline::from(&report_with("GVR", 10, 0, 0));
        let current = BenchReport::default();
        let regressions = compare_to_baseline(&current, &baseline, 0);
        assert_eq!(regressions.len(), 1);
        assert!(regressions[0].description.contains("dropped from 10 to 0"));
    }

    #[test]
    fn json_report_contains_expected_top_level_fields() {
        let report = report_with("GVR", 10, 1, 0);
        let json = to_json(&report);
        assert!(json.get("by_detector").is_some());
        assert!(json.get("aggregate_precision").is_some());
    }

    #[test]
    fn markdown_report_is_non_empty_and_contains_every_detector() {
        let report = report_with("GVR", 10, 1, 0);
        let markdown = to_markdown(&report);
        for code in crate::DETECTOR_CODES {
            assert!(markdown.contains(code), "missing {code} row");
        }
    }
}
