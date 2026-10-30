//! Report rendering + baseline regression gate — mirrors the security
//! branch's `sting-bench::report` shape (`Baseline`/`compare_to_baseline`,
//! Markdown + JSON renderers), adapted for two structurally different
//! metrics instead of one (`docs/domains/precision-sources.md`):
//! Tier 1 structural completeness (`matched`/`total` per category,
//! aggregated across the whole corpus) and Tier 2 precision/recall
//! (`TP`/`FP`/`FN` per category, same shape `sting-bench` already uses).

use std::collections::BTreeMap;

use crate::throughput::ThroughputResult;
use crate::tier1::{CategoryScore, Tier1Result};
use crate::tier2::{PrecisionRecall, Tier2Result};

/// This report's own format version.
///
/// A report is written to a file, compared against a committed baseline and read months later, so
/// it says which shape it is in. A file with no version is a file whose reader has to guess.
pub const REPORT_SCHEMA_VERSION: &str = "1.0";

/// Everything one `abi-layer bench` run produces.
#[derive(Debug, Clone, serde::Serialize)]
pub struct AccuracyReport {
    /// Which version of this report's shape it is in.
    ///
    /// Written and never read back by this tree, so it carries no reading default: the report is
    /// consumed by whoever opens the file, and the version is there for them.
    pub schema_version: String,
    /// One entry per Tier 1 (Tolk/Tact) contract scored.
    pub tier1: Vec<Tier1Result>,
    /// One entry per Tier 2 (`FunC`) contract scored.
    pub tier2: Vec<Tier2Result>,
    /// Named throughput measurements (e.g. `"tolk_tact_batch"`,
    /// `"func_batch"`).
    pub throughput: BTreeMap<String, ThroughputResult>,
}

impl Default for AccuracyReport {
    fn default() -> Self {
        Self {
            schema_version: REPORT_SCHEMA_VERSION.to_string(),
            tier1: Vec::new(),
            tier2: Vec::new(),
            throughput: BTreeMap::new(),
        }
    }
}

/// Corpus-wide aggregate of every Tier 1 category, across every contract
/// scored — the unit [`Baseline`] snapshots, since per-contract counts
/// would make the baseline file noisy/unstable as the corpus grows (same
/// reasoning `sting-bench::report`'s own `Baseline` doc comment gives for
/// keeping only `TP`/`FP`/`FN`, not the full miss/spurious detail).
fn aggregate_tier1(results: &[Tier1Result]) -> BTreeMap<String, CategoryScore> {
    let mut totals: BTreeMap<String, CategoryScore> = BTreeMap::new();
    for result in results {
        for (name, score) in [
            ("storage", result.storage),
            ("get_methods", result.get_methods),
            ("errors", result.errors),
            ("incoming_internal", result.incoming_internal),
            ("incoming_external", result.incoming_external),
            ("outgoing", result.outgoing),
        ] {
            let entry = totals.entry(name.to_string()).or_default();
            entry.matched += score.matched;
            entry.total += score.total;
        }
    }
    totals
}

fn aggregate_tier2(results: &[Tier2Result]) -> BTreeMap<String, PrecisionRecall> {
    let mut totals: BTreeMap<String, PrecisionRecall> = BTreeMap::new();
    for result in results {
        for (name, score) in [
            ("get_methods", result.get_methods),
            ("storage_fields", result.storage_fields),
        ] {
            let entry = totals.entry(name.to_string()).or_default();
            entry.true_positives += score.true_positives;
            entry.false_positives += score.false_positives;
            entry.false_negatives += score.false_negatives;
        }
    }
    totals
}

/// `matched`/`total` snapshot for one Tier 1 category, corpus-wide.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Tier1Snapshot {
    /// Total matched across the whole corpus.
    pub matched: u32,
    /// Total possible across the whole corpus.
    pub total: u32,
}

/// `TP`/`FP`/`FN` snapshot for one Tier 2 category, corpus-wide.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Tier2Snapshot {
    /// True positives, summed across the corpus.
    pub true_positives: u32,
    /// False positives, summed across the corpus.
    pub false_positives: u32,
    /// False negatives, summed across the corpus.
    pub false_negatives: u32,
}

/// The committed regression-gate file (`corpus-abi/baseline.json`) — same
/// role as the security branch's `corpus/baseline.json`.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct Baseline {
    /// Tier 1 categories, keyed by category name.
    pub tier1: BTreeMap<String, Tier1Snapshot>,
    /// Tier 2 categories, keyed by category name.
    pub tier2: BTreeMap<String, Tier2Snapshot>,
}

impl From<&AccuracyReport> for Baseline {
    fn from(report: &AccuracyReport) -> Self {
        Self {
            tier1: aggregate_tier1(&report.tier1)
                .into_iter()
                .map(|(k, v)| {
                    (
                        k,
                        Tier1Snapshot {
                            matched: v.matched,
                            total: v.total,
                        },
                    )
                })
                .collect(),
            tier2: aggregate_tier2(&report.tier2)
                .into_iter()
                .map(|(k, v)| {
                    (
                        k,
                        Tier2Snapshot {
                            true_positives: v.true_positives,
                            false_positives: v.false_positives,
                            false_negatives: v.false_negatives,
                        },
                    )
                })
                .collect(),
        }
    }
}

/// One detected regression, human-readable.
#[derive(Debug, Clone)]
pub struct Regression {
    /// e.g. `"tier1.get_methods"` / `"tier2.storage_fields"`.
    pub category: String,
    /// What happened.
    pub message: String,
}

/// Compares `current` against `baseline`, allowing up to `tolerance`
/// absolute count wobble on any single number (same "small-corpus
/// statistical wobble" reasoning as `sting-cli`'s own `bench()` — a ±1
/// tolerance there). A Tier 1 category that regresses below 100%
/// completeness when the baseline was 100% is always flagged regardless of
/// `tolerance`: any deviation from 100% there is an open defect
/// (`docs/domains/precision-sources.md`), unlike Tier 2's inherently noisier
/// precision and recall.
///
/// **A tier that wasn't run this pass is skipped entirely, not treated as
/// "everything dropped to zero."** Real bug found by independent review:
/// `current.tier1`/`current.tier2` are empty maps whenever `abi-layer bench
/// --tier tier2` (or `--tier tier1`) only ran one tier — comparing an empty
/// map against a committed both-tiers baseline previously flagged every
/// category of the *unrun* tier as a full-count regression (`tier2.
/// get_methods: true positives dropped from 6 to 0`), even though nothing
/// was actually measured. The CLI's own `--tier`/`--help` text recommends
/// exactly the single-tier invocation that triggered this, so it wasn't a
/// contrived misuse.
#[must_use]
pub fn compare_to_baseline(
    current: &Baseline,
    baseline: &Baseline,
    tolerance: u32,
) -> Vec<Regression> {
    let mut regressions = Vec::new();
    if !current.tier1.is_empty() {
        regressions.extend(tier1_regressions(current, baseline, tolerance));
    }
    if !current.tier2.is_empty() {
        regressions.extend(tier2_regressions(current, baseline, tolerance));
    }
    regressions
}

fn tier1_regressions(current: &Baseline, baseline: &Baseline, tolerance: u32) -> Vec<Regression> {
    let mut regressions = Vec::new();
    for (category, base) in &baseline.tier1 {
        let current_score = current.tier1.get(category).copied().unwrap_or_default();
        let was_complete = base.total > 0 && base.matched == base.total;
        let now_incomplete = current_score.total > 0 && current_score.matched < current_score.total;
        if was_complete && now_incomplete {
            regressions.push(Regression {
                category: format!("tier1.{category}"),
                message: format!(
                    "was 100% complete ({}/{}), now {}/{}",
                    base.matched, base.total, current_score.matched, current_score.total
                ),
            });
        } else if current_score.matched + tolerance < base.matched {
            regressions.push(Regression {
                category: format!("tier1.{category}"),
                message: format!(
                    "matched count dropped from {} to {}",
                    base.matched, current_score.matched
                ),
            });
        }
    }
    regressions
}

fn tier2_regressions(current: &Baseline, baseline: &Baseline, tolerance: u32) -> Vec<Regression> {
    let mut regressions = Vec::new();
    for (category, base) in &baseline.tier2 {
        let current_score = current.tier2.get(category).copied().unwrap_or_default();
        if current_score.true_positives + tolerance < base.true_positives {
            regressions.push(Regression {
                category: format!("tier2.{category}"),
                message: format!(
                    "true positives dropped from {} to {}",
                    base.true_positives, current_score.true_positives
                ),
            });
        }
        if current_score.false_positives > base.false_positives + tolerance {
            regressions.push(Regression {
                category: format!("tier2.{category}"),
                message: format!(
                    "false positives rose from {} to {}",
                    base.false_positives, current_score.false_positives
                ),
            });
        }
    }

    regressions
}

fn format_percent(ratio: Option<f64>) -> String {
    match ratio {
        Some(r) => format!("{:.1}%", r * 100.0),
        None => "n/a".to_string(),
    }
}

/// Renders `report` as a Markdown document — same section shape as
/// `sting-bench::report::to_markdown`, split into a Tier 1 table and a Tier
/// 2 table, since two different kinds of accuracy do not belong in one table,
/// plus throughput.
#[must_use]
#[allow(
    clippy::unwrap_used,
    reason = "docs/ARCHITECTURE.md §3.2: writes into a String buffer via `write!` cannot fail (no I/O), so `unwrap()` here can never actually panic — the same guarantee `std::fmt::Write for String` documents"
)]
pub fn to_markdown(report: &AccuracyReport) -> String {
    use std::fmt::Write as _;

    let mut out = String::new();
    out.push_str("# abi-layer bench report\n\n");
    writeln!(
        out,
        "Tier 1: {} contracts scored. Tier 2: {} contracts scored.\n",
        report.tier1.len(),
        report.tier2.len()
    )
    .unwrap();

    out.push_str("## Tier 1 — structural completeness\n\n");
    out.push_str("| Category | Matched | Total | Completeness |\n|---|---|---|---|\n");
    for (category, score) in aggregate_tier1(&report.tier1) {
        writeln!(
            out,
            "| {category} | {} | {} | {} |",
            score.matched,
            score.total,
            format_percent(score.ratio())
        )
        .unwrap();
    }

    out.push_str("\n## Tier 2 — FunC precision/recall\n\n");
    out.push_str("| Category | TP | FP | FN | Precision | Recall |\n|---|---|---|---|---|---|\n");
    for (category, score) in aggregate_tier2(&report.tier2) {
        writeln!(
            out,
            "| {category} | {} | {} | {} | {} | {} |",
            score.true_positives,
            score.false_positives,
            score.false_negatives,
            format_percent(score.precision()),
            format_percent(score.recall())
        )
        .unwrap();
    }

    out.push_str("\n## Throughput\n\n");
    out.push_str("| Batch | Contracts | Succeeded | Elapsed (s) | Contracts/sec | RSS delta |\n|---|---|---|---|---|---|\n");
    for (name, t) in &report.throughput {
        let rss = t.rss_delta_bytes.map_or("n/a".to_string(), |b| {
            // `b` is a process RSS delta — nowhere near 2^52 bytes (4
            // petabytes) in practice, so the precision loss this cast could
            // theoretically incur is not a reachable concern here.
            #[allow(clippy::cast_precision_loss)]
            let mib = b as f64 / (1024.0 * 1024.0);
            format!("{mib:.1} MiB")
        });
        writeln!(
            out,
            "| {name} | {} | {} | {:.2} | {:.2} | {rss} |",
            t.contract_count, t.success_count, t.elapsed_secs, t.contracts_per_sec
        )
        .unwrap();
    }

    out.push_str("\n## Open gaps (< 100% Tier 1 completeness / Tier 2 below target)\n\n");
    let mut any_gap = false;
    for result in &report.tier1 {
        for gap in &result.gaps {
            any_gap = true;
            writeln!(out, "- `{}`: {gap}", result.contract).unwrap();
        }
    }
    if !any_gap {
        out.push_str("(none)\n");
    }

    out
}

/// Renders `report` as pretty JSON.
///
/// # Errors
/// Returns a `serde_json::Error` on the same conditions
/// `serde_json::to_string_pretty` would.
pub fn to_json_string(report: &AccuracyReport) -> Result<String, serde_json::Error> {
    serde_json::to_string_pretty(report)
}

/// Renders `baseline` as pretty JSON — the exact bytes written to
/// `corpus-abi/baseline.json`.
///
/// # Errors
/// Returns a `serde_json::Error` on the same conditions
/// `serde_json::to_string_pretty` would.
pub fn baseline_to_json_string(baseline: &Baseline) -> Result<String, serde_json::Error> {
    serde_json::to_string_pretty(baseline)
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "docs/ARCHITECTURE.md §3.2 scopes the no-unwrap/expect policy to library code outside tests"
)]
mod tests {
    use super::{Baseline, Tier1Snapshot, Tier2Snapshot, compare_to_baseline};

    #[test]
    fn a_tier1_category_dropping_below_100_percent_is_always_flagged() {
        let mut baseline = Baseline::default();
        baseline.tier1.insert(
            "get_methods".to_string(),
            Tier1Snapshot {
                matched: 10,
                total: 10,
            },
        );
        let mut current = Baseline::default();
        current.tier1.insert(
            "get_methods".to_string(),
            Tier1Snapshot {
                matched: 9,
                total: 10,
            },
        );

        // Even with a generous tolerance, "was 100%, now isn't" must still
        // be flagged — that specific regression has zero acceptable slack.
        let regressions = compare_to_baseline(&current, &baseline, 100);
        assert_eq!(regressions.len(), 1);
        assert_eq!(regressions[0].category, "tier1.get_methods");
    }

    #[test]
    fn a_tier2_recall_drop_beyond_tolerance_is_flagged() {
        let mut baseline = Baseline::default();
        baseline.tier2.insert(
            "get_methods".to_string(),
            Tier2Snapshot {
                true_positives: 20,
                false_positives: 0,
                false_negatives: 0,
            },
        );
        let mut current = Baseline::default();
        current.tier2.insert(
            "get_methods".to_string(),
            Tier2Snapshot {
                true_positives: 15,
                false_positives: 0,
                false_negatives: 5,
            },
        );

        let regressions = compare_to_baseline(&current, &baseline, 1);
        assert_eq!(regressions.len(), 1);
        assert!(regressions[0].message.contains("true positives"));
    }

    #[test]
    fn small_wobble_within_tolerance_is_not_flagged() {
        let mut baseline = Baseline::default();
        baseline.tier2.insert(
            "get_methods".to_string(),
            Tier2Snapshot {
                true_positives: 20,
                false_positives: 2,
                false_negatives: 0,
            },
        );
        let mut current = Baseline::default();
        current.tier2.insert(
            "get_methods".to_string(),
            Tier2Snapshot {
                true_positives: 20,
                false_positives: 3,
                false_negatives: 0,
            },
        );

        assert!(compare_to_baseline(&current, &baseline, 1).is_empty());
    }

    #[test]
    fn a_new_category_absent_from_the_baseline_is_not_penalized() {
        let baseline = Baseline::default();
        let mut current = Baseline::default();
        current.tier2.insert(
            "brand_new_category".to_string(),
            Tier2Snapshot {
                true_positives: 5,
                false_positives: 5,
                false_negatives: 5,
            },
        );
        assert!(compare_to_baseline(&current, &baseline, 0).is_empty());
    }

    /// Regression test for a real bug found by independent review:
    /// `abi-layer bench --tier tier2` only populates `current.tier2`,
    /// leaving `current.tier1` empty — comparing that empty map against a
    /// committed both-tiers baseline used to flag every Tier 1 category as
    /// "dropped to zero," a false regression on a tier that was never run.
    #[test]
    fn running_only_tier2_does_not_falsely_flag_tier1_as_regressed() {
        let mut baseline = Baseline::default();
        baseline.tier1.insert(
            "get_methods".to_string(),
            Tier1Snapshot {
                matched: 138,
                total: 154,
            },
        );
        baseline.tier2.insert(
            "get_methods".to_string(),
            Tier2Snapshot {
                true_positives: 6,
                false_positives: 0,
                false_negatives: 0,
            },
        );

        // Simulates `--tier tier2`: only tier2 was measured this run.
        let mut current = Baseline::default();
        current.tier2.insert(
            "get_methods".to_string(),
            Tier2Snapshot {
                true_positives: 6,
                false_positives: 0,
                false_negatives: 0,
            },
        );

        assert!(
            compare_to_baseline(&current, &baseline, 1).is_empty(),
            "tier1 was not run this pass — it must not be compared at all, not treated as a full drop to zero"
        );
    }

    /// Same fix, other direction: `--tier tier1` must not falsely flag
    /// tier2 as regressed.
    #[test]
    fn running_only_tier1_does_not_falsely_flag_tier2_as_regressed() {
        let mut baseline = Baseline::default();
        baseline.tier1.insert(
            "get_methods".to_string(),
            Tier1Snapshot {
                matched: 138,
                total: 154,
            },
        );
        baseline.tier2.insert(
            "get_methods".to_string(),
            Tier2Snapshot {
                true_positives: 6,
                false_positives: 0,
                false_negatives: 0,
            },
        );

        let mut current = Baseline::default();
        current.tier1.insert(
            "get_methods".to_string(),
            Tier1Snapshot {
                matched: 138,
                total: 154,
            },
        );

        assert!(compare_to_baseline(&current, &baseline, 1).is_empty());
    }
}
