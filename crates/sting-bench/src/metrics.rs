//! TP/FP/FN computation and precision/recall/FP-rate aggregation
//! (`docs/domains/precision-sources.md`, §5).

use std::collections::{BTreeMap, HashMap, HashSet};
use sting_ast::Language;

use sting_core::Finding;
use sting_detectors::DetectorRegistry;
use sting_pipeline::LintController;

use crate::DETECTOR_CODES;
use crate::corpus::{GroundTruthEntry, Project};
use crate::mutate::MutatedInstance;

/// Whether `id` belongs to the set that runs by default and asserts a
/// defect. Asks the built-in registry, so it stays true by construction when
/// a detector changes category (see
/// [`BenchReport::default_set_precision`]'s own doc comment).
fn is_in_default_set(id: &str) -> bool {
    DetectorRegistry::builtin()
        .detectors()
        .find(|d| d.id() == id)
        .is_some_and(|d| {
            d.is_default_enabled() && d.default_severity() > sting_core::Severity::Info
        })
}

/// How exactly a finding must match the label to count as a hit.
///
/// Two levels exist because the questions differ. The internal gate asks
/// whether **our** detector has drifted, and there the line is mandatory: a
/// regression that points forty lines away would otherwise pass unnoticed. A
/// public comparison asks whether a tool found the defect, and there demanding
/// a line-for-line match is unfair to everyone but us — the label takes its
/// line from **our** finding's `Finding::location`, and another tool is
/// entitled to point at a different legitimate place for the same defect.
///
/// This is not a hypothesis. `acton 1.1.0` correctly reports the missing
/// `onBouncedMessage` in `clean_acton_contracts_elector__ubm_handler_deleted`,
/// points at lines 300, 1654, 1675 and 1689, and scores 0 hits and 4 false
/// positives under a line-for-line match.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MatchGranularity {
    /// A match by line, within `LINE_TOLERANCE`. The default, and the only
    /// level the internal gate uses.
    Line,
    /// A match by fact: did the tool report the class in this unit at all.
    /// Several findings of one class in one unit collapse into one — this is
    /// detection rather than localization.
    Unit,
}

/// Where a scored unit came from — the axis
/// §3.3](../../../docs/phases_roadmap/phase-6-benchmark.md) requires the
/// published dataset to report separately.
///
/// The distinction is not bookkeeping. Aggregate recall reads as "100% of
/// defects found" while every defect it was measured on is one **we** put
/// there, on code that never had it; and aggregate precision mixes false
/// positives on untouched production contracts with false positives on
/// hand-written fixtures built to exercise a specific pattern. Merged into
/// one row, each number flatters the tool in a different direction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum UnitOrigin {
    /// A vendored real contract, untouched. Carries no ground truth, so it
    /// can only ever produce false positives — the honest measure of noise
    /// on code nobody wrote for us.
    RealCode,
    /// A vendored real contract with exactly one defect injected by the
    /// mutation engine. Real surroundings, synthetic defect.
    InjectedIntoRealCode,
    /// A hand-written golden fixture. Synthetic through and through — it
    /// proves a detector recognizes a pattern, not that it behaves on wild
    /// code.
    HandWritten,
}

impl UnitOrigin {
    /// Stable key for report tables and JSON.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::RealCode => "real-code",
            Self::InjectedIntoRealCode => "injected-into-real-code",
            Self::HandWritten => "hand-written",
        }
    }

    /// Every origin, in reporting order.
    #[must_use]
    pub fn all() -> [Self; 3] {
        [
            Self::RealCode,
            Self::InjectedIntoRealCode,
            Self::HandWritten,
        ]
    }
}

/// One scoreable item of the benchmark dataset: source to analyze, what is
/// known to be wrong with it, and where it came from.
///
/// **Public, and deliberately says nothing about who analyzes it.** This is
/// what makes the dataset usable by `sting-benchmark` to score an external
/// analyzer: the type carries the *question* (here is code, here is the
/// truth), never the answer. The internal regression gate and
/// the public benchmark share the dataset and the scoring, and share nothing
/// about which tool produced the findings.
#[derive(Debug, Clone)]
pub struct DatasetUnit {
    /// Report-facing name of the unit.
    pub name: String,
    /// Which grammar reads [`Self::source`], so the unit is parsed by the one that wrote it.
    pub language: Language,
    /// The source text to analyze.
    pub source: String,
    /// Where this unit came from — see [`UnitOrigin`].
    pub origin: UnitOrigin,
    /// Known defects in `source`, if any. Empty for [`UnitOrigin::RealCode`].
    pub ground_truth: Vec<GroundTruthEntry>,
    /// Exact `Finding::message` text to treat as already accounted for —
    /// neither a fresh true nor false positive — even if it recurs in
    /// `source`. Only ever non-empty for a [`MutatedInstance`]: see that
    /// type's `pre_existing_messages` field for why.
    ///
    /// **Counted, not a set.** A merged project can hold two or three
    /// entrypoints, and two same-named entrypoints produce byte-identical
    /// `Finding::message` text — so a set skipped the *injected* occurrence
    /// too, the ground truth was never matched, and the instance scored as a
    /// false negative. This must stay consistent with `verify_and_wrap`'s own
    /// acceptance rule, which counts occurrences for exactly the same reason:
    /// if one counts and the other does not, a mutation can be accepted at
    /// generation time and then be unscoreable.
    pub ignored_message_counts: HashMap<String, usize>,
    /// The classes the unit produced before the injection — see
    /// [`MutatedInstance::pre_existing_classes`]. Empty for everything but a
    /// mutation.
    pub pre_existing_classes: std::collections::BTreeSet<String>,
    /// The per-file layout — see [`Project::files`]. A mutation has exactly
    /// one: the engine edits merged text, and the original split into files is
    /// lost.
    pub files: Vec<(String, String)>,
}

impl DatasetUnit {
    /// A [`Project`] carries no origin of its own — the same type is both a
    /// vendored real contract and a hand-written fixture — so the caller
    /// that knows which one it is passes it in.
    #[must_use]
    pub fn from_project(project: &Project, origin: UnitOrigin) -> Self {
        Self {
            name: project.name.clone(),
            language: project.language,
            source: project.source.clone(),
            origin,
            ground_truth: project.ground_truth.clone(),
            ignored_message_counts: HashMap::new(),
            pre_existing_classes: std::collections::BTreeSet::new(),
            files: project.files.clone(),
        }
    }
}

impl From<&MutatedInstance> for DatasetUnit {
    fn from(instance: &MutatedInstance) -> Self {
        Self {
            name: instance.name.clone(),
            // The mutation engine injects Tolk defects into Tolk projects, and says so here rather
            // than inheriting a field that could quietly become wrong if it ever injects elsewhere.
            language: Language::Tolk,
            source: instance.source.clone(),
            origin: UnitOrigin::InjectedIntoRealCode,
            ground_truth: vec![instance.ground_truth.clone()],
            ignored_message_counts: instance.pre_existing_messages.iter().fold(
                HashMap::new(),
                |mut counts, message| {
                    *counts.entry(message.clone()).or_default() += 1;
                    counts
                },
            ),
            pre_existing_classes: instance.pre_existing_classes.iter().cloned().collect(),
            // The layout the mutation kept, and the merged text under one name only when there was
            // no layout to keep: a participant that resolves imports has to be handed a project.
            files: if instance.files.is_empty() {
                vec![("unit.tolk".to_string(), instance.source.clone())]
            } else {
                instance.files.clone()
            },
        }
    }
}

/// One (file, ground-truth-entry-not-matched) pair — a miss, kept for the
/// report's "what exactly was missed" list
/// (`docs/domains/precision-sources.md`) — not only the figures but a way to
/// find quickly *what exactly* went wrong.
#[derive(Debug, Clone)]
pub struct MissedDefect {
    /// Which corpus unit it was missed in.
    pub unit_name: String,
    /// The detector that should have fired.
    pub defect_class: String,
    /// The expected line.
    pub line: u32,
}

/// One finding that did not match any ground truth — a false positive, kept
/// for the same reason as [`MissedDefect`].
#[derive(Debug, Clone)]
pub struct SpuriousFinding {
    /// Which corpus unit it fired in.
    pub unit_name: String,
    /// The detector that fired.
    pub detector_id: String,
    /// Where it pointed.
    pub line: u32,
    /// The finding's own message, for report readability.
    pub message: String,
}

/// Per-detector tallies, plus the specific misses/false-positives behind
/// them.
#[derive(Debug, Clone, Default)]
pub struct DetectorMetrics {
    /// True positives: a finding matched a ground-truth entry.
    pub true_positives: u32,
    /// False positives: a finding matched no ground-truth entry.
    pub false_positives: u32,
    /// False negatives: a ground-truth entry matched no finding.
    pub false_negatives: u32,
    /// Specific misses, for the report.
    pub missed: Vec<MissedDefect>,
    /// Specific false positives, for the report.
    pub spurious: Vec<SpuriousFinding>,
}

impl DetectorMetrics {
    /// `TP / (TP + FP)`, or `None` if the detector never fired (undefined,
    /// not zero — `docs/domains/precision-sources.md` never specifies a 0-finding
    /// convention, and reporting "0%" would misleadingly suggest the
    /// detector is actively wrong rather than simply silent).
    #[must_use]
    pub fn precision(&self) -> Option<f64> {
        let denom = self.true_positives + self.false_positives;
        (denom > 0).then(|| f64::from(self.true_positives) / f64::from(denom))
    }

    /// `TP / (TP + FN)`, or `None` if there was no ground truth for this
    /// detector at all.
    #[must_use]
    pub fn recall(&self) -> Option<f64> {
        let denom = self.true_positives + self.false_negatives;
        (denom > 0).then(|| f64::from(self.true_positives) / f64::from(denom))
    }
}

/// The full benchmark result: per-detector metrics plus how large a corpus
/// produced them (`docs/domains/precision-sources.md`: the report always states
/// corpus size, exhaustive-vs-sampled is not a live question at this size).
#[derive(Debug, Clone, Default)]
pub struct BenchReport {
    /// Metrics for each of the 4 v1 detectors, keyed by detector id.
    pub by_detector: BTreeMap<String, DetectorMetrics>,
    /// The same tallies split by where the scored unit came from — see
    /// [`UnitOrigin`] for why one merged number misleads in two directions
    /// at once.
    pub by_origin: BTreeMap<UnitOrigin, BTreeMap<String, DetectorMetrics>>,
    /// The same tallies split by the language of the scored unit.
    ///
    /// A detector that reads two languages does not read them equally well, and one row averaging
    /// the two would be a number about neither. This is also what makes "the older language's
    /// figures did not move" a claim anybody can check rather than a promise.
    pub by_language: BTreeMap<Language, BTreeMap<String, DetectorMetrics>>,
    /// Number of clean-corpus projects scored.
    pub clean_project_count: usize,
    /// Number of mutation-engine instances scored, by detector.
    pub mutation_instance_count: BTreeMap<String, usize>,
    /// Number of hand-written golden-fixture instances scored, by detector.
    pub golden_fixture_count: BTreeMap<String, usize>,
}

impl BenchReport {
    /// Aggregate precision across every detector that has one defined.
    #[must_use]
    pub fn aggregate_precision(&self) -> Option<f64> {
        let (tp, fp) = self.by_detector.values().fold((0u32, 0u32), |(tp, fp), m| {
            (tp + m.true_positives, fp + m.false_positives)
        });
        (tp + fp > 0).then(|| f64::from(tp) / f64::from(tp + fp))
    }

    /// Aggregate precision over the **default set** — the detectors that
    /// actually run when nobody configures anything and that assert a defect
    /// rather than state a fact.
    ///
    /// Two kinds are left out, both by asking the registry rather than by a
    /// list kept here (a list would drift from the product the first time a
    /// detector changed category):
    ///
    /// - experimental ones, `Detector::is_default_enabled` `== false` —
    ///   `AC`, which is off by default;
    /// - reference ones reporting at `Severity::Info` — `DEPR`, and `LEP`
    ///   which reports at `Info`.
    ///
    /// [`BenchReport::aggregate_precision`] stays exactly as it was and keeps
    /// covering everything measured. The two numbers answer different
    /// questions: that one is "how noisy is everything we ship", this one is
    /// "how noisy is what a user sees by default", and the second is the one
    /// the phase gate is about.
    #[must_use]
    pub fn default_set_precision(&self) -> Option<f64> {
        let (tp, fp) = self
            .by_detector
            .iter()
            .filter(|(id, _)| is_in_default_set(id))
            .fold((0u32, 0u32), |(tp, fp), (_, m)| {
                (tp + m.true_positives, fp + m.false_positives)
            });
        (tp + fp > 0).then(|| f64::from(tp) / f64::from(tp + fp))
    }

    /// `(TP, FP, FN)` totals for one origin, across every detector.
    #[must_use]
    pub fn origin_totals(&self, origin: UnitOrigin) -> (u32, u32, u32) {
        self.by_origin
            .get(&origin)
            .map_or((0, 0, 0), |by_detector| {
                by_detector.values().fold((0, 0, 0), |(tp, fp, fn_), m| {
                    (
                        tp + m.true_positives,
                        fp + m.false_positives,
                        fn_ + m.false_negatives,
                    )
                })
            })
    }

    /// Whether this origin has any ground truth at all. `false` for
    /// [`UnitOrigin::RealCode`]: a vendored contract is scored with an empty
    /// ground truth, so **no** finding on it can ever be a true positive.
    #[must_use]
    pub fn origin_has_ground_truth(&self, origin: UnitOrigin) -> bool {
        let (tp, _, fn_) = self.origin_totals(origin);
        tp + fn_ > 0
    }

    /// Precision for one origin, or `None` when the origin has no ground
    /// truth.
    ///
    /// That `None` is deliberate and load-bearing. Without it the formula
    /// still yields a number for [`UnitOrigin::RealCode`] — `0 / (0 + FP)` =
    /// **0.0%** — which reads as "every finding on production code is wrong"
    /// when it actually means "we never labelled any defect there, so nothing
    /// could count as right". A figure that is zero by construction is worse
    /// than no figure: it looks measured.
    ///
    /// What is meaningful on that origin is the raw false-positive count and
    /// the per-project rate, both of which the report prints instead.
    #[must_use]
    pub fn origin_precision(&self, origin: UnitOrigin) -> Option<f64> {
        if !self.origin_has_ground_truth(origin) {
            return None;
        }
        let (tp, fp, _) = self.origin_totals(origin);
        (tp + fp > 0).then(|| f64::from(tp) / f64::from(tp + fp))
    }

    /// Recall for one origin. `None` for [`UnitOrigin::RealCode`] by
    /// construction, and that `None` is the point: untouched production
    /// contracts carry no ground truth, so **recall is not measured on real
    /// defects anywhere in this benchmark**. Reporting it as a number there
    /// would invent one.
    #[must_use]
    pub fn origin_recall(&self, origin: UnitOrigin) -> Option<f64> {
        let (tp, _, fn_) = self.origin_totals(origin);
        (tp + fn_ > 0).then(|| f64::from(tp) / f64::from(tp + fn_))
    }

    /// Ids excluded from [`BenchReport::default_set_precision`], sorted — so
    /// a report can name them instead of leaving the reader to infer which
    /// rows the number skipped.
    #[must_use]
    pub fn detectors_outside_default_set(&self) -> Vec<&str> {
        let mut ids: Vec<&str> = self
            .by_detector
            .keys()
            .map(String::as_str)
            .filter(|id| !is_in_default_set(id))
            .collect();
        ids.sort_unstable();
        ids
    }

    /// Aggregate recall across every detector that has one defined.
    #[must_use]
    pub fn aggregate_recall(&self) -> Option<f64> {
        let (tp, fn_) = self
            .by_detector
            .values()
            .fold((0u32, 0u32), |(tp, fn_), m| {
                (tp + m.true_positives, fn_ + m.false_negatives)
            });
        (tp + fn_ > 0).then(|| f64::from(tp) / f64::from(tp + fn_))
    }
}

/// Runs `registry` over every clean project, mutation instance, and golden
/// fixture, matching findings against ground truth by
/// `(defect_class, line)` with a small line tolerance
/// (`docs/domains/precision-sources.md`: an exact column match is not
/// required), and tallies TP/FP/FN per detector.
#[must_use]
pub fn score(
    clean_projects: &[Project],
    mutations: &[MutatedInstance],
    golden_fixtures: &[Project],
) -> BenchReport {
    let mut report = BenchReport {
        clean_project_count: clean_projects.len(),
        ..BenchReport::default()
    };

    for m in mutations {
        *report
            .mutation_instance_count
            .entry(m.ground_truth.defect_class.clone())
            .or_insert(0) += 1;
    }
    for p in golden_fixtures {
        for g in &p.ground_truth {
            *report
                .golden_fixture_count
                .entry(g.defect_class.clone())
                .or_insert(0) += 1;
        }
    }

    let units: Vec<DatasetUnit> = clean_projects
        .iter()
        .map(|p| DatasetUnit::from_project(p, UnitOrigin::RealCode))
        .chain(mutations.iter().map(DatasetUnit::from))
        .chain(
            golden_fixtures
                .iter()
                .map(|p| DatasetUnit::from_project(p, UnitOrigin::HandWritten)),
        )
        .collect();

    let registry = DetectorRegistry::builtin();
    let per_unit_findings: Vec<Vec<Finding>> = units
        .iter()
        .map(|unit| {
            LintController::new(&registry)
                .lint_source_as(unit.language, unit.name.clone().into(), unit.source.clone())
                .unwrap_or_default()
        })
        .collect();

    tally_into(
        &mut report,
        &units,
        &per_unit_findings,
        MatchGranularity::Line,
    );
    report
}

/// Scores findings **someone else produced** against the same dataset and the
/// same rules `score` uses for our own detectors.
///
/// This is the seam that lets a third party be scored. `sting-bench` stays the
/// internal regression gate and never learns how to launch an external
/// process; `sting-benchmark` launches participants, normalizes whatever they
/// emit into [`Finding`]s, and hands them here. Neither the dataset nor the
/// matching rules fork between "us" and "them" — which is the only way a
/// published comparison can claim to be even-handed.
///
/// `per_unit_findings` is parallel to `units`; a shorter slice scores the
/// missing units as producing nothing, which is the correct reading of a
/// participant that failed or declined to analyze a file.
#[must_use]
pub fn score_dataset(units: &[DatasetUnit], per_unit_findings: &[Vec<Finding>]) -> BenchReport {
    score_dataset_with(units, per_unit_findings, MatchGranularity::Line)
}

/// [`score_dataset`] with the match level chosen explicitly — see
/// [`MatchGranularity`] for why there are two and which is for what.
#[must_use]
pub fn score_dataset_with(
    units: &[DatasetUnit],
    per_unit_findings: &[Vec<Finding>],
    granularity: MatchGranularity,
) -> BenchReport {
    let mut report = BenchReport {
        clean_project_count: units
            .iter()
            .filter(|u| u.origin == UnitOrigin::RealCode)
            .count(),
        ..BenchReport::default()
    };
    for unit in units.iter().filter(|u| u.origin != UnitOrigin::RealCode) {
        let counter = if unit.origin == UnitOrigin::InjectedIntoRealCode {
            &mut report.mutation_instance_count
        } else {
            &mut report.golden_fixture_count
        };
        for entry in &unit.ground_truth {
            *counter.entry(entry.defect_class.clone()).or_insert(0) += 1;
        }
    }
    tally_into(&mut report, units, per_unit_findings, granularity);
    report
}

/// Shared tail of [`score`] and [`score_dataset`]: match findings against
/// ground truth per unit and fold the result into both the per-detector and
/// the per-origin tallies.
fn tally_into(
    report: &mut BenchReport,
    units: &[DatasetUnit],
    per_unit_findings: &[Vec<Finding>],
    granularity: MatchGranularity,
) {
    static NO_FINDINGS: &Vec<Finding> = &Vec::new();
    for (unit, findings) in units.iter().zip(
        per_unit_findings
            .iter()
            .chain(std::iter::repeat(NO_FINDINGS)),
    ) {
        let per_unit = match granularity {
            MatchGranularity::Line => score_findings(unit, findings),
            MatchGranularity::Unit => score_findings_at_unit_level(unit, findings),
        };
        merge_into(&mut report.by_detector, &per_unit);
        merge_into(report.by_origin.entry(unit.origin).or_default(), &per_unit);
        merge_into(
            report.by_language.entry(unit.language).or_default(),
            &per_unit,
        );
    }
}

/// Builds the dataset the benchmark scores against — the three halves as
/// one list, each unit tagged with [`UnitOrigin`].
#[must_use]
pub fn build_dataset(
    clean_projects: &[Project],
    mutations: &[MutatedInstance],
    golden_fixtures: &[Project],
) -> Vec<DatasetUnit> {
    clean_projects
        .iter()
        .map(|p| DatasetUnit::from_project(p, UnitOrigin::RealCode))
        .chain(mutations.iter().map(DatasetUnit::from))
        .chain(
            golden_fixtures
                .iter()
                .map(|p| DatasetUnit::from_project(p, UnitOrigin::HandWritten)),
        )
        .collect()
}

/// Line tolerance for matching a finding against a ground-truth entry
/// (`docs/domains/precision-sources.md`).
const LINE_TOLERANCE: u32 = 3;

/// Scoring at unit level: one pair of unit and class, rather than by line.
///
/// The rule is symmetric for every participant:
///
/// - the class is in the label and the participant reported it in this unit →
///   **TP**;
/// - the class is in the label and the participant was silent → **FN**;
/// - the participant reported a class the label does not carry → **FP**.
///
/// How many times it was reported does not matter: this is detection rather
/// than localization.
///
/// **A pair that cannot be scored is excluded rather than assigned to
/// somebody.** If the class was present in the project **before** the injection
/// ([`DatasetUnit::pre_existing_classes`]), a participant's answer proves
/// nothing at this level: it may have seen the old defect rather than the
/// injected one. Counting such a pair as a hit would be a gift, and counting it
/// as a miss would take away something earned, so it is not counted at all.
fn score_findings_at_unit_level(
    unit: &DatasetUnit,
    findings: &[Finding],
) -> BTreeMap<String, DetectorMetrics> {
    let mut report = BTreeMap::<String, DetectorMetrics>::new();

    let expected: std::collections::BTreeSet<&str> = unit
        .ground_truth
        .iter()
        .map(|e| e.defect_class.as_str())
        .collect();
    let reported: std::collections::BTreeSet<&str> = findings
        .iter()
        .map(|f| f.detector_id)
        .filter(|id| DETECTOR_CODES.contains(id))
        .collect();

    for class in expected.union(&reported) {
        if unit.pre_existing_classes.contains(*class) {
            continue;
        }
        let metrics = report.entry((*class).to_string()).or_default();
        match (expected.contains(class), reported.contains(class)) {
            (true, true) => metrics.true_positives += 1,
            (true, false) => {
                metrics.false_negatives += 1;
                metrics.missed.push(MissedDefect {
                    unit_name: unit.name.clone(),
                    defect_class: (*class).to_string(),
                    line: unit
                        .ground_truth
                        .iter()
                        .find(|e| e.defect_class == *class)
                        .map_or(0, |e| e.line),
                });
            }
            (false, true) => {
                metrics.false_positives += 1;
                metrics.spurious.push(SpuriousFinding {
                    unit_name: unit.name.clone(),
                    detector_id: (*class).to_string(),
                    line: findings
                        .iter()
                        .find(|f| f.detector_id == *class)
                        .map_or(0, |f| f.location.start_line),
                    message: format!(
                        "reported class {class}, which the unit's label does not carry"
                    ),
                });
            }
            (false, false) => {}
        }
    }
    report
}

/// Matches one unit's `findings` against its ground truth. Pure: takes
/// findings, never produces them — see [`score_dataset`] for why that
/// separation is the point of this module's shape.
fn score_findings(unit: &DatasetUnit, findings: &[Finding]) -> BTreeMap<String, DetectorMetrics> {
    let mut report = BTreeMap::<String, DetectorMetrics>::new();
    // A working copy: each pre-existing occurrence of a message absorbs one
    // finding, and the rest are scored normally.
    let mut ignored_remaining = unit.ignored_message_counts.clone();
    let mut matched_ground_truth: HashSet<usize> = HashSet::new();
    for finding in findings {
        // A real bug, caught by hand while adding SUA: `DEPR` (and any
        // future non-taxonomy detector) has no ground-truth concept —
        // `deprecated.rs`'s own doc comment says so directly — but before
        // this check, every one of its findings anywhere in the whole
        // corpus still flowed through this same scoring loop and, having no
        // manifest entry to ever match, counted as a false positive. 58 of
        // them were silently dragging the aggregate precision figure down
        // (measured before this fix: 71.7%; DEPR alone accounted for the
        // entire gap against the true, intended aggregate) despite
        // `DETECTOR_CODES`'s own doc comment already stating the exclusion
        // as settled, documented intent — the registry-wide `lint_source`
        // call two lines up was never actually filtered to match.
        if !DETECTOR_CODES.contains(&finding.detector_id) {
            continue;
        }
        // Ground truth is consulted **before** the pre-existing quota, not
        // after. When two same-named entrypoints yield identical messages,
        // the quota is absorbed in iteration order, and the occurrence it
        // swallows can be the injected one — leaving a pre-existing
        // occurrence at a different line to be matched against the ground
        // truth, missing it, and scoring a false negative on a defect the
        // detector actually found. A finding sitting at the injected defect's
        // own line *is* the injected one; nothing pre-existing was there.
        if let Some(index) =
            find_unmatched_entry(&unit.ground_truth, finding, &matched_ground_truth)
        {
            matched_ground_truth.insert(index);
            report
                .entry(finding.detector_id.to_string())
                .or_default()
                .true_positives += 1;
            continue;
        }
        if let Some(remaining) = ignored_remaining.get_mut(&finding.message)
            && *remaining > 0
        {
            *remaining -= 1;
            continue;
        }
        {
            let metrics = report.entry(finding.detector_id.to_string()).or_default();
            metrics.false_positives += 1;
            metrics.spurious.push(SpuriousFinding {
                unit_name: unit.name.clone(),
                detector_id: finding.detector_id.to_string(),
                line: finding.location.start_line,
                message: finding.message.clone(),
            });
        }
    }

    for (index, entry) in unit.ground_truth.iter().enumerate() {
        if matched_ground_truth.contains(&index) {
            continue;
        }
        let metrics = report.entry(entry.defect_class.clone()).or_default();
        metrics.false_negatives += 1;
        metrics.missed.push(MissedDefect {
            unit_name: unit.name.clone(),
            defect_class: entry.defect_class.clone(),
            line: entry.line,
        });
    }
    report
}

/// Adds `from`'s tallies into `into`, per detector.
fn merge_into(
    into: &mut BTreeMap<String, DetectorMetrics>,
    from: &BTreeMap<String, DetectorMetrics>,
) {
    for (detector, metrics) in from {
        let target = into.entry(detector.clone()).or_default();
        target.true_positives += metrics.true_positives;
        target.false_positives += metrics.false_positives;
        target.false_negatives += metrics.false_negatives;
        target.missed.extend(metrics.missed.iter().cloned());
        target.spurious.extend(metrics.spurious.iter().cloned());
    }
}

fn find_unmatched_entry(
    ground_truth: &[GroundTruthEntry],
    finding: &Finding,
    already_matched: &HashSet<usize>,
) -> Option<usize> {
    ground_truth.iter().enumerate().find_map(|(index, entry)| {
        let already = already_matched.contains(&index);
        let class_matches = entry.defect_class == finding.detector_id;
        let line_matches = entry.line.abs_diff(finding.location.start_line) <= LINE_TOLERANCE;
        (!already && class_matches && line_matches).then_some(index)
    })
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "docs/ARCHITECTURE.md §3.2 scopes the no-unwrap/expect policy to library code outside tests"
)]
mod tests {
    use super::{DatasetUnit, score};
    use crate::corpus::{GroundTruthEntry, Project};
    use crate::mutate::MutatedInstance;
    use sting_ast::Language;

    fn clean_project(source: &str) -> Project {
        Project {
            language: Language::Tolk,
            name: "clean-project".to_string(),
            source: source.to_string(),
            ground_truth: Vec::new(),
            file_starts: Vec::new(),
            files: Vec::new(),
        }
    }

    #[test]
    fn a_clean_project_with_no_findings_contributes_nothing() {
        let report = score(&[clean_project("fun f() {\n    return;\n}\n")], &[], &[]);
        assert!(report.by_detector.is_empty());
    }

    #[test]
    fn a_finding_on_a_clean_project_is_a_false_positive() {
        // GVR fixture shape known to fire (see corpus/fixtures/detectors/GVR).
        let source = r"
contract Wallet {
    author: test
    storage: WalletStorage
}
struct WalletStorage {
    owner: address
}
fun f() {
    var owner = 0;
}
";
        let report = score(&[clean_project(source)], &[], &[]);
        let gvr = report.by_detector.get("GVR").expect("GVR fired");
        assert_eq!(gvr.false_positives, 1);
        assert_eq!(gvr.true_positives, 0);
        assert_eq!(gvr.precision(), Some(0.0));
    }

    #[test]
    fn depr_findings_never_enter_by_detector_at_all() {
        // Regression test for a real bug: before the DETECTOR_CODES filter in
        // score_unit, this exact source's one real DEPR finding
        // (deprecated.rs's own test fixture shape) had no ground-truth entry
        // to ever match anywhere in the corpus, so every one of them across
        // the whole real corpus silently became a false positive —
        // contradicting deprecated.rs's own doc comment ("there is no
        // ground truth to calibrate precision/recall against here") and
        // dragging the aggregate precision figure down by DEPR's full
        // finding count, not by an actual detector regression.
        let source = "fun f() {\n    val h = stringCrc32(\"abc\");\n}\n";
        let report = score(&[clean_project(source)], &[], &[]);
        assert!(
            !report.by_detector.contains_key("DEPR"),
            "DEPR must never appear in by_detector, got {:?}",
            report.by_detector.get("DEPR")
        );
    }

    #[test]
    fn a_mutation_instance_whose_finding_survives_is_a_true_positive() {
        let mutation = MutatedInstance {
            files: Vec::new(),
            name: "mut-1".to_string(),
            source: "fun f(s: slice) {\n    val x = s.loadUint(32);\n}\n".to_string(),
            ground_truth: GroundTruthEntry {
                defect_class: "LEP".to_string(),
                line: 1,
                column: 1,
            },
            pre_existing_messages: Vec::new(),
            pre_existing_classes: Vec::new(),
        };
        let report = score(&[], std::slice::from_ref(&mutation), &[]);
        let lep = report.by_detector.get("LEP").expect("LEP scored");
        assert_eq!(lep.true_positives, 1);
        assert_eq!(lep.false_negatives, 0);
        assert_eq!(lep.recall(), Some(1.0));
    }

    #[test]
    fn a_ground_truth_entry_with_no_matching_finding_is_a_false_negative() {
        let mutation = MutatedInstance {
            files: Vec::new(),
            name: "mut-1".to_string(),
            // No slice parameter at all -> LEP will never fire here, but we
            // claim (falsely, on purpose) that it should.
            source: "fun f() {\n    return;\n}\n".to_string(),
            ground_truth: GroundTruthEntry {
                defect_class: "LEP".to_string(),
                line: 1,
                column: 1,
            },
            pre_existing_messages: Vec::new(),
            pre_existing_classes: Vec::new(),
        };
        let report = score(&[], std::slice::from_ref(&mutation), &[]);
        let lep = report.by_detector.get("LEP").expect("LEP scored");
        assert_eq!(lep.false_negatives, 1);
        assert_eq!(lep.true_positives, 0);
        assert_eq!(lep.recall(), Some(0.0));
    }

    #[test]
    fn line_tolerance_still_matches_a_finding_a_couple_lines_off() {
        let mutation = MutatedInstance {
            files: Vec::new(),
            name: "mut-1".to_string(),
            source: "fun f(s: slice) {\n    val x = s.loadUint(32);\n}\n".to_string(),
            ground_truth: GroundTruthEntry {
                defect_class: "LEP".to_string(),
                line: 3, // real finding lands on line 1; within tolerance
                column: 1,
            },
            pre_existing_messages: Vec::new(),
            pre_existing_classes: Vec::new(),
        };
        let report = score(&[], std::slice::from_ref(&mutation), &[]);
        let lep = report.by_detector.get("LEP").expect("LEP scored");
        assert_eq!(lep.true_positives, 1);
    }

    /// Two functions with the same name produce byte-identical `LEP`
    /// messages. One occurrence is pre-existing, the other is the injected
    /// defect. Scoring must absorb exactly one and score the other.
    ///
    /// Before the counts were compared instead of the message keys, the
    /// pre-existing entry silenced **both**, the ground truth went unmatched,
    /// and a defect the detector had actually found scored as a false
    /// negative. That shape is not hypothetical: 11 corpus projects merge two
    /// or three same-named entrypoints into one text.
    #[test]
    fn a_pre_existing_message_absorbs_one_occurrence_not_every_occurrence() {
        let source = "fun f(s: slice) {\n    val x = s.loadUint(32);\n}\n\nfun f(s: slice) {\n    val y = s.loadUint(32);\n}\n";
        let message = "function `f` reads `slice` parameter `s` but does not call `.assertEnd()` on every path — trailing data the caller attached would be silently ignored".to_string();
        let mutation = MutatedInstance {
            files: Vec::new(),
            name: "mut-1".to_string(),
            source: source.to_string(),
            ground_truth: GroundTruthEntry {
                defect_class: "LEP".to_string(),
                line: 5,
                column: 1,
            },
            pre_existing_messages: vec![message],
            pre_existing_classes: Vec::new(),
        };
        let report = score(&[], std::slice::from_ref(&mutation), &[]);
        let lep = report.by_detector.get("LEP").expect("LEP scored");
        assert_eq!(
            (lep.true_positives, lep.false_positives, lep.false_negatives),
            (1, 0, 0),
            "the second occurrence is the injected defect and must score as a true positive"
        );
    }

    /// The real and synthetic halves are counted apart, and the
    /// real half reports **no** precision/recall rather than a
    /// zero-by-construction one.
    #[test]
    fn real_code_and_synthetic_halves_are_tallied_separately() {
        use super::UnitOrigin;

        // Fires GVR: a local shadowing the storage field.
        let noisy_real = r"
contract Wallet {
    author: test
    storage: WalletStorage
}
struct WalletStorage {
    owner: address
}
fun f() {
    var owner = 0;
}
";
        let mutation = MutatedInstance {
            files: Vec::new(),
            name: "mut-1".to_string(),
            source: "fun f(s: slice) {\n    val x = s.loadUint(32);\n}\n".to_string(),
            ground_truth: GroundTruthEntry {
                defect_class: "LEP".to_string(),
                line: 1,
                column: 1,
            },
            pre_existing_messages: Vec::new(),
            pre_existing_classes: Vec::new(),
        };
        let report = score(
            &[clean_project(noisy_real)],
            std::slice::from_ref(&mutation),
            &[],
        );

        let (tp, fp, fn_) = report.origin_totals(UnitOrigin::RealCode);
        assert_eq!(
            (tp, fp, fn_),
            (0, 1, 0),
            "a vendored contract carries no ground truth, so its findings can only be noise"
        );
        assert_eq!(
            report.origin_precision(UnitOrigin::RealCode),
            None,
            "precision on unlabelled code must be undefined, not the 0.0% the formula yields"
        );
        assert_eq!(report.origin_recall(UnitOrigin::RealCode), None);

        let (tp, _, _) = report.origin_totals(UnitOrigin::InjectedIntoRealCode);
        assert_eq!(tp, 1, "the injected defect is scored on its own origin");
        assert_eq!(
            report.origin_recall(UnitOrigin::InjectedIntoRealCode),
            Some(1.0),
            "and recall exists only there"
        );

        // The split must not change the totals it splits.
        assert_eq!(report.aggregate_recall(), Some(1.0));
        assert_eq!(
            report.by_detector.get("GVR").map(|m| m.false_positives),
            Some(1)
        );
    }

    /// Unit level counts a hit even where the tool pointed at another place
    /// for the same defect, which is the whole reason it exists.
    #[test]
    fn unit_level_matching_accepts_a_finding_at_a_different_valid_line() {
        use super::{MatchGranularity, UnitOrigin, score_dataset_with};

        let unit = DatasetUnit::from_project(
            &Project {
                language: Language::Tolk,
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

        let line_level = score_dataset_with(
            std::slice::from_ref(&unit),
            &[Vec::new()],
            MatchGranularity::Line,
        );
        // Our own detector finds the defect on line 1, where the difference
        // between the levels is invisible, so it is checked on a foreign
        // finding instead.
        assert_eq!(
            line_level.by_detector.get("LEP").map(|m| m.false_negatives),
            Some(1),
            "silence is a miss at either level"
        );

        // A finding of the same class on line 40: by line that is a miss plus
        // a false positive, and at unit level it is a hit.
        let far_away = vec![vec![sting_core::Finding {
            detector_id: "LEP",
            severity: sting_core::Severity::Info,
            message: "the same defect, another place".to_string(),
            remediation: String::new(),
            location: sting_core::Location {
                file: "unit-1".into(),
                start_line: 40,
                start_column: 1,
                end_line: 40,
                end_column: 2,
            },
            confidence: None,
            dynamic_verification: None,
            suggestion: None,
        }]];

        let by_line = score_dataset_with(
            std::slice::from_ref(&unit),
            &far_away,
            MatchGranularity::Line,
        );
        let m = by_line.by_detector.get("LEP").expect("scored");
        assert_eq!(
            (m.true_positives, m.false_positives, m.false_negatives),
            (0, 1, 1),
            "by line, pointing elsewhere is penalised twice"
        );

        let by_unit = score_dataset_with(
            std::slice::from_ref(&unit),
            &far_away,
            MatchGranularity::Unit,
        );
        let m = by_unit.by_detector.get("LEP").expect("scored");
        assert_eq!(
            (m.true_positives, m.false_positives, m.false_negatives),
            (1, 0, 0),
            "at unit level this is a detection, and it counts"
        );
    }

    /// A class that was in the project **before** the injection cannot be
    /// scored at unit level, and so counts for nobody.
    #[test]
    fn unit_level_skips_a_class_the_project_already_had_before_the_mutation() {
        use super::{MatchGranularity, score_dataset_with};

        let mut unit = DatasetUnit::from(&MutatedInstance {
            files: Vec::new(),
            name: "mut-1".to_string(),
            source: "fun f(s: slice) {\n    val x = s.loadUint(32);\n}\n".to_string(),
            ground_truth: GroundTruthEntry {
                defect_class: "LEP".to_string(),
                line: 1,
                column: 1,
            },
            pre_existing_messages: Vec::new(),
            pre_existing_classes: vec!["LEP".to_string()],
        });
        unit.name = "mut-1".to_string();

        let report = score_dataset_with(
            std::slice::from_ref(&unit),
            &[Vec::new()],
            MatchGranularity::Unit,
        );
        assert!(
            !report.by_detector.contains_key("LEP"),
            "a unit-and-class pair whose class predates the injection is not counted at \
             all, for or against"
        );
    }

    #[test]
    fn precision_and_recall_are_none_when_undefined_not_zero() {
        let metrics = super::DetectorMetrics::default();
        assert_eq!(metrics.precision(), None);
        assert_eq!(metrics.recall(), None);
    }
}
