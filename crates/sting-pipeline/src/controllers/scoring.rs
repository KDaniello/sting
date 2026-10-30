//! `ScoringController` — `docs/ARCHITECTURE.md`,
//! `docs/domains/emulator.md`
//!
//! ```text
//! confidence(finding) = w1 * detector_precision(defect_class)
//!                      + w2 * dynamic_result_weight(verification)
//!                      + w3 * severity_weight(severity)
//! ```
//!
//! **Not** the same thing `sting-bench` computes
//! (`docs/ARCHITECTURE.md`'s own explicit warning): `sting-bench`
//! measures a *detector's* precision/recall averaged over a whole corpus;
//! this scores *one finding*. `detector_precision` below is the one place
//! the two meet — this controller consumes `sting-bench`'s output as a
//! calibration input, it does not recompute it.

use std::collections::BTreeMap;

use sting_core::{Confidence, DynamicVerification, Finding, Severity};

/// `w1`: weight on the detector's own historical precision
/// (`docs/domains/precision-sources.md`'s corpus). The single largest
/// weight — a detector's track record across dozens of real/converted/
/// synthetic contracts is the most information-dense signal available,
/// independent of any one finding's specifics.
pub const WEIGHT_DETECTOR_PRECISION: f32 = 0.4;
/// `w2`: weight on this specific finding's dynamic verification outcome.
/// Equal to `w1` deliberately: direct runtime evidence about *this*
/// instance is exactly as informative as the detector's aggregate history —
/// neither should dominate the other by construction.
pub const WEIGHT_DYNAMIC_RESULT: f32 = 0.4;
/// `w3`: weight on the finding's own severity. Smallest deliberately:
/// severity is about *prioritization* (which finding to look at first), not
/// evidence about whether the finding is *real* — conflating the two would
/// let a `Critical`-severity finding outscore a `Confirmed` one just by
/// being scarier-sounding.
pub const WEIGHT_SEVERITY: f32 = 0.2;

const _: () = assert!(
    WEIGHT_DETECTOR_PRECISION + WEIGHT_DYNAMIC_RESULT + WEIGHT_SEVERITY == 1.0,
    "weights must sum to 1.0 so confidence stays in [0.0, 1.0] given each term is itself already bounded there"
);

/// `dynamic_result_weight` (`docs/domains/emulator.md`:
/// "Confirmed=1.0, Inconclusive=0.5, Refuted=0.0"). `None` (bridge never
/// ran — e.g. `sting lint` with no on-chain deployment available) is scored
/// the same as `Inconclusive`: no dynamic evidence either way, the same
/// neutral prior, not silently treated as either confirmation or refutation.
#[must_use]
pub fn dynamic_result_weight(verification: Option<&DynamicVerification>) -> f32 {
    match verification {
        Some(DynamicVerification::Confirmed(_)) => 1.0,
        Some(DynamicVerification::Inconclusive(_)) | None => 0.5,
        Some(DynamicVerification::Refuted) => 0.0,
    }
}

/// `severity_weight`: a linear 5-point scale, `Info = 0.0` through
/// `Critical = 1.0` in even steps — no corpus-derived reason to weight the
/// steps unevenly, and an even scale is the simplest choice that does not
/// silently claim more precision than is actually justified.
#[must_use]
pub const fn severity_weight(severity: Severity) -> f32 {
    match severity {
        Severity::Info => 0.0,
        Severity::Low => 0.25,
        Severity::Medium => 0.5,
        Severity::High => 0.75,
        Severity::Critical => 1.0,
    }
}

/// The severity `severity_weight` actually scores: `finding.severity`,
/// **except** a [`DynamicVerification::Refuted`] finding scores as
/// [`Severity::Info`] — mirroring `sting-report`'s own rendering-time
/// downgrade (`docs/domains/emulator.md`) so a refuted
/// finding's confidence is not inflated by a severity label the report
/// itself no longer displays at face value. Duplicated in `sting-pipeline`
/// rather than shared with `sting-report`'s own (private) equivalent
/// deliberately: `sting-report` has no reason to depend on `sting-pipeline`
/// (the dependency graph runs the other way, `docs/ARCHITECTURE.md` §1.1),
/// so this is the same three-line reasoning applied independently, not a
/// missing shared helper.
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

/// Per-detector precision, calibrated from `sting-bench`'s committed
/// baseline (`corpus/baseline.json`, `docs/domains/precision-sources.md`)
/// — real measured numbers, not invented. Built by the caller (`sting-cli`)
/// from `sting_bench::Baseline`; `sting-pipeline` does not depend on
/// `sting-bench` directly (that dependency already runs the other way:
/// `sting-bench` depends on `sting-pipeline`'s `LintController`, see
/// `docs/ARCHITECTURE.md`), so this type is the deliberately
/// narrow, one-way conversion target instead of a shared type.
#[derive(Debug, Clone, Default)]
pub struct DetectorStatsSnapshot {
    precision_by_detector: BTreeMap<String, f32>,
}

impl DetectorStatsSnapshot {
    /// Builds a snapshot from `(detector_id, precision)` pairs — precision
    /// as a plain `[0.0, 1.0]` fraction (`true_positives / (true_positives +
    /// false_positives)`), already computed by the caller.
    #[must_use]
    pub fn new(precision_by_detector: BTreeMap<String, f32>) -> Self {
        Self {
            precision_by_detector,
        }
    }

    /// This detector's known precision, if present in the snapshot.
    #[must_use]
    pub fn precision(&self, detector_id: &str) -> Option<f32> {
        self.precision_by_detector.get(detector_id).copied()
    }
}

/// A detector with no entry in the snapshot (unmeasured, or a
/// non-taxonomy/framework-validation detector — `sting_core::DetectorId`'s
/// doc comment) scores as if its precision were exactly this — a neutral
/// prior: neither `0.0` (which would unfairly zero out a real but
/// as-yet-unbenchmarked detector) nor `1.0` (which would over-trust one with
/// no track record at all).
const UNKNOWN_DETECTOR_PRECISION: f32 = 0.5;

/// Computes [`Finding::confidence`] from a [`Finding`]'s detector's known
/// precision, its own dynamic verification outcome, and its severity
/// (`docs/ARCHITECTURE.md`, this module's doc comment for the
/// formula).
pub struct ScoringController {
    detector_stats: DetectorStatsSnapshot,
}

impl ScoringController {
    /// Builds a controller against `detector_stats` (see
    /// [`DetectorStatsSnapshot`] for where that comes from).
    #[must_use]
    pub fn new(detector_stats: DetectorStatsSnapshot) -> Self {
        Self { detector_stats }
    }

    /// Scores `finding`. Pure and total: always returns a valid
    /// [`Confidence`], never fails — every input to the formula is already
    /// bounded, so [`Confidence::saturating`] is a defensive clamp, not a
    /// real error path (see that constructor's own doc comment).
    #[must_use]
    pub fn score(&self, finding: &Finding) -> Confidence {
        let detector_precision = finding
            .defect_class()
            .and_then(|class| self.detector_stats.precision(class.code()))
            .unwrap_or(UNKNOWN_DETECTOR_PRECISION);
        let raw = WEIGHT_DETECTOR_PRECISION * detector_precision
            + WEIGHT_DYNAMIC_RESULT * dynamic_result_weight(finding.dynamic_verification.as_ref())
            + WEIGHT_SEVERITY * severity_weight(effective_severity(finding));
        Confidence::saturating(raw)
    }
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "docs/ARCHITECTURE.md §3.2 scopes the no-unwrap/expect policy to library code outside tests"
)]
mod tests {
    use std::path::PathBuf;

    use sting_core::{DynamicVerification, Finding, Location, ProofOfConcept, Severity};

    use super::{DetectorStatsSnapshot, ScoringController};

    /// The real `corpus/baseline.json` precision numbers
    /// (`docs/domains/precision-sources.md`) — GVR
    /// 20/29≈0.6897, LEP 15/39≈0.3846, ID 13/13=1.0, UBM 13/16=0.8125. Used
    /// as this module's calibration/worked-example data, not invented
    /// numbers: the scores are calibrated on the labelled corpus
    /// (`docs/domains/emulator.md`).
    fn real_baseline_stats() -> DetectorStatsSnapshot {
        DetectorStatsSnapshot::new(
            [
                ("GVR".to_string(), 20.0 / 29.0),
                ("LEP".to_string(), 15.0 / 39.0),
                ("ID".to_string(), 1.0),
                ("UBM".to_string(), 13.0 / 16.0),
            ]
            .into_iter()
            .collect(),
        )
    }

    fn finding(detector_id: &'static str, severity: Severity) -> Finding {
        Finding {
            detector_id,
            severity,
            message: "test finding".to_string(),
            remediation: "fix it".to_string(),
            location: Location {
                file: PathBuf::from("contract.tolk"),
                start_line: 1,
                start_column: 1,
                end_line: 1,
                end_column: 1,
            },
            confidence: None,
            dynamic_verification: None,
            suggestion: None,
        }
    }

    fn assert_close(actual: f32, expected: f32) {
        assert!(
            (actual - expected).abs() < 1e-4,
            "expected {expected}, got {actual}"
        );
    }

    #[test]
    fn a_confirmed_critical_ubm_finding_scores_high() {
        // 0.4 * 0.8125 + 0.4 * 1.0 (Confirmed) + 0.2 * 1.0 (Critical) = 0.925
        let mut f = finding("UBM", Severity::Critical);
        f.dynamic_verification = Some(DynamicVerification::Confirmed(ProofOfConcept {
            expected: "x".to_string(),
            actual: "x".to_string(),
            steps: vec![],
        }));
        let controller = ScoringController::new(real_baseline_stats());
        assert_close(controller.score(&f).value(), 0.925);
    }

    #[test]
    fn a_refuted_high_severity_id_finding_scores_moderate_low() {
        // effective severity for Refuted is Info (0.0), not the finding's own High:
        // 0.4 * 1.0 + 0.4 * 0.0 (Refuted) + 0.2 * 0.0 (Info, downgraded) = 0.4
        let mut f = finding("ID", Severity::High);
        f.dynamic_verification = Some(DynamicVerification::Refuted);
        let controller = ScoringController::new(real_baseline_stats());
        assert_close(controller.score(&f).value(), 0.4);
    }

    #[test]
    fn a_static_only_lep_finding_with_no_bridge_run_scores_moderate() {
        // 0.4 * 0.3846... + 0.4 * 0.5 (None, neutral) + 0.2 * 0.5 (Medium) ≈ 0.4538
        let f = finding("LEP", Severity::Medium);
        let controller = ScoringController::new(real_baseline_stats());
        assert_close(controller.score(&f).value(), 0.4538);
    }

    #[test]
    fn an_inconclusive_gvr_finding_scores_the_same_as_no_bridge_run() {
        let mut f = finding("GVR", Severity::High);
        f.dynamic_verification = Some(DynamicVerification::Inconclusive("reason".to_string()));
        let controller = ScoringController::new(real_baseline_stats());
        // 0.4 * (20/29) + 0.4 * 0.5 + 0.2 * 0.75 (High)
        assert_close(
            controller.score(&f).value(),
            0.4 * (20.0 / 29.0) + 0.2 + 0.15,
        );
    }

    #[test]
    fn an_unmeasured_detector_uses_the_neutral_precision_prior() {
        let f = finding("DEMO-LONG-FN", Severity::Info);
        let controller = ScoringController::new(DetectorStatsSnapshot::default());
        // 0.4 * 0.5 (unknown) + 0.4 * 0.5 (None) + 0.2 * 0.0 (Info) = 0.4
        assert_close(controller.score(&f).value(), 0.4);
    }

    #[test]
    fn confidence_is_always_within_bounds_regardless_of_input() {
        for severity in [
            Severity::Info,
            Severity::Low,
            Severity::Medium,
            Severity::High,
            Severity::Critical,
        ] {
            for verification in [
                None,
                Some(DynamicVerification::Refuted),
                Some(DynamicVerification::Inconclusive("x".to_string())),
                Some(DynamicVerification::Confirmed(ProofOfConcept {
                    expected: "x".to_string(),
                    actual: "x".to_string(),
                    steps: vec![],
                })),
            ] {
                let mut f = finding("UBM", severity);
                f.dynamic_verification = verification;
                let controller = ScoringController::new(real_baseline_stats());
                let value = controller.score(&f).value();
                assert!(
                    (0.0..=1.0).contains(&value),
                    "confidence {value} out of range"
                );
            }
        }
    }
}
