//! A campaign measured after the fact: what this platform flagged against what the campaign
//! excluded.
//!
//! The whole value of a retrospective measurement is in the order of two events. The threshold is
//! registered in the repository first and the comparison is run second, so what a result cites is
//! the commit the threshold was registered in — a number named after a run describes the run rather
//! than judging it.
//!
//! **This measures agreement, not truth.** An exclusion list is somebody else's method, applied by
//! somebody with their own information. An owner this platform flags and the campaign did not may
//! be a farm the campaign missed, and the arithmetic below cannot tell that case from a false
//! positive. What it reports is therefore precision and recall against a named list.
//!
//! **What this does not do.** It scores nothing: scores arrive already computed, because the
//! pipeline that computes them is the pipeline being judged and a measurement that also produced its
//! own inputs would be marking its own work. It does not decide who is one person — a cluster is
//! coordination, and ownership is a claim about people the graph does not support.

use std::collections::BTreeMap;
use std::path::Path;
use std::process::Command;

use sting_sybil_types::TonAddress;

use crate::{ClusterError, Result};

/// What was registered before the campaign ran.
#[derive(Debug, Clone, PartialEq, serde::Deserialize)]
pub struct RegisteredThreshold {
    /// Which campaign this was registered for.
    pub campaign: String,
    /// When it was registered, as the file states it.
    pub registered: String,
    /// Of the owners flagged, the share the campaign also excluded.
    pub min_precision: f64,
    /// Of the owners the campaign excluded, the share flagged.
    pub min_recall: f64,
    /// How many owners each side has to name before the comparison means anything.
    pub min_owners: usize,
    /// The score at and above which an owner counts as flagged.
    pub score_threshold: f64,
}

/// What the comparison found.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct RetroResult {
    /// Which version of this result's shape it is in.
    pub schema_version: String,
    /// Which campaign was measured.
    pub campaign: String,
    /// The commit the threshold was registered in — what makes the registration mean anything.
    pub threshold_commit: String,
    /// The score threshold applied.
    pub score_threshold: f64,
    /// How many owners carried a score.
    pub scored_owners: usize,
    /// How many owners the campaign excluded.
    pub excluded_owners: usize,
    /// Flagged and excluded.
    pub true_positives: usize,
    /// Flagged and not excluded.
    pub false_positives: usize,
    /// Excluded and not flagged.
    pub false_negatives: usize,
    /// Of the owners flagged, the share the campaign also excluded.
    pub precision: f64,
    /// Of the owners the campaign excluded, the share flagged.
    pub recall: f64,
    /// Whether the registered thresholds were met, all of them.
    pub passed: bool,
    /// Which of them were not, named one by one.
    pub failures: Vec<String>,
}

/// This result's own format version.
pub const RETRO_SCHEMA_VERSION: &str = "1.0";

/// Reads the threshold that was registered.
///
/// # Errors
/// When the file is absent or is not the shape the registration states.
pub fn read_threshold(path: &Path) -> Result<RegisteredThreshold> {
    let text = std::fs::read_to_string(path)
        .map_err(|source| ClusterError::Retro(format!("reading {}: {source}", path.display())))?;
    toml::from_str(&text).map_err(|source| {
        ClusterError::Retro(format!(
            "{} is not a registration: {source}",
            path.display()
        ))
    })
}

/// The commit a file was last changed in, which is the commit it was registered in.
///
/// Read from the repository rather than written into the file by hand: a hash a human typed is a
/// hash nobody checked, and the point of citing one is that a reader can go and look.
///
/// # Errors
/// When git cannot be run, or when the file is not committed — an uncommitted threshold is not a
/// registered one, and a result that cited nothing would be a result nobody can date.
pub fn registration_commit(path: &Path) -> Result<String> {
    let output = Command::new("git")
        .args(["log", "-1", "--format=%H", "--"])
        .arg(path)
        .output()
        .map_err(|source| ClusterError::Retro(format!("running git: {source}")))?;
    if !output.status.success() {
        return Err(ClusterError::Retro(format!(
            "git refused to say when {} was registered: {}",
            path.display(),
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }
    let commit = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if commit.is_empty() {
        return Err(ClusterError::Retro(format!(
            "{} is not committed, so nothing registered it",
            path.display()
        )));
    }
    Ok(commit)
}

/// Reads a list of addresses into the one form both sides are compared in.
///
/// An exclusion list is published by whoever ran the campaign and the scores come from this
/// platform, so the two arrive in whatever form each side happened to use — one account has four
/// valid friendly spellings and one raw one. Compared as text they would simply fail to meet, and
/// a recall of zero for that reason looks exactly like a detector that found nothing.
///
/// What could not be read as an address is returned rather than dropped: a list a quarter of which
/// this tool could not parse is not a list to measure against silently.
#[must_use]
pub fn normalise(addresses: impl IntoIterator<Item = String>) -> (Vec<String>, Vec<String>) {
    let mut read = Vec::new();
    let mut unreadable = Vec::new();
    for address in addresses {
        match TonAddress::parse(&address) {
            Ok(parsed) => read.push(parsed.to_raw_string()),
            Err(_) => unreadable.push(address),
        }
    }
    (read, unreadable)
}

/// Compares scores against a campaign's exclusion list at a registered threshold.
///
/// `scores` maps an owner to the risk this platform gave it; `excluded` is the campaign's own list.
/// Owners the campaign names and no score covers count as missed rather than being dropped — a
/// recall that quietly excluded whatever was not scored would be a recall over the easy half.
#[must_use]
pub fn compare(
    scores: &BTreeMap<String, f64>,
    excluded: &[String],
    threshold: &RegisteredThreshold,
    threshold_commit: &str,
) -> RetroResult {
    let excluded_set: std::collections::BTreeSet<&str> =
        excluded.iter().map(String::as_str).collect();

    let mut true_positives = 0usize;
    let mut false_positives = 0usize;
    for (owner, risk) in scores {
        if *risk < threshold.score_threshold {
            continue;
        }
        if excluded_set.contains(owner.as_str()) {
            true_positives += 1;
        } else {
            false_positives += 1;
        }
    }
    let false_negatives = excluded_set.len() - true_positives;

    let precision = share(true_positives, true_positives + false_positives);
    let recall = share(true_positives, excluded_set.len());

    let mut failures = Vec::new();
    if precision < threshold.min_precision {
        failures.push(format!(
            "precision {precision:.3} is below the registered {:.3}",
            threshold.min_precision
        ));
    }
    if recall < threshold.min_recall {
        failures.push(format!(
            "recall {recall:.3} is below the registered {:.3}",
            threshold.min_recall
        ));
    }
    if scores.len() < threshold.min_owners || excluded_set.len() < threshold.min_owners {
        failures.push(format!(
            "the comparison covers {} scored and {} excluded owner(s), and {} on each side was registered",
            scores.len(),
            excluded_set.len(),
            threshold.min_owners
        ));
    }

    RetroResult {
        schema_version: RETRO_SCHEMA_VERSION.to_string(),
        campaign: threshold.campaign.clone(),
        threshold_commit: threshold_commit.to_string(),
        score_threshold: threshold.score_threshold,
        scored_owners: scores.len(),
        excluded_owners: excluded_set.len(),
        true_positives,
        false_positives,
        false_negatives,
        precision,
        recall,
        passed: failures.is_empty(),
        failures,
    }
}

/// A share, and zero where there is nothing to take a share of.
///
/// Zero rather than one for an empty numerator and denominator: a detector that flagged nothing has
/// not achieved perfect precision, it has not been measured.
///
/// Both counts are owner counts, so the precision `as f64` loses above 2^52 is a bound no campaign
/// reaches.
#[allow(clippy::cast_precision_loss)]
fn share(part: usize, whole: usize) -> f64 {
    if whole == 0 {
        return 0.0;
    }
    part as f64 / whole as f64
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "docs/ARCHITECTURE.md §3.2 scopes the no-unwrap/expect policy to library code outside tests"
)]
mod tests {
    use std::collections::BTreeMap;

    use super::{RegisteredThreshold, compare};

    fn registered() -> RegisteredThreshold {
        RegisteredThreshold {
            campaign: "a campaign".to_string(),
            registered: "2026-08-23".to_string(),
            min_precision: 0.9,
            min_recall: 0.5,
            min_owners: 2,
            score_threshold: 0.5,
        }
    }

    fn scores(pairs: &[(&str, f64)]) -> BTreeMap<String, f64> {
        pairs
            .iter()
            .map(|(owner, risk)| ((*owner).to_string(), *risk))
            .collect()
    }

    #[test]
    fn an_owner_the_campaign_excluded_and_nothing_scored_counts_as_missed() {
        let result = compare(
            &scores(&[("a", 0.9), ("b", 0.1)]),
            &["a".to_string(), "c".to_string()],
            &registered(),
            "0",
        );

        assert_eq!(result.true_positives, 1);
        assert_eq!(result.false_positives, 0);
        assert_eq!(
            result.false_negatives, 1,
            "c was excluded and never scored, which is a miss rather than a row to drop"
        );
        assert!((result.recall - 0.5).abs() < 1e-9);
    }

    #[test]
    fn a_run_that_flags_nothing_is_unmeasured_rather_than_perfect() {
        let result = compare(
            &scores(&[("a", 0.1), ("b", 0.2)]),
            &["a".to_string(), "b".to_string()],
            &registered(),
            "0",
        );

        assert!((result.precision - 0.0).abs() < f64::EPSILON);
        assert!(!result.passed);
    }

    /// The two sides arrive in two forms, and one account has four friendly spellings.
    #[test]
    fn a_friendly_address_and_a_raw_one_are_the_same_owner() {
        let raw = "0:e4d954ef9f4e1250a26b5bb31d1a5b5d1e5b1d5a1d5b1e5a1d5b1e5a1d5b1e5a";
        let (read, unreadable) =
            super::normalise(vec![raw.to_string(), "not an address".to_string()]);

        assert_eq!(read, vec![raw.to_string()]);
        assert_eq!(
            unreadable,
            vec!["not an address".to_string()],
            "what could not be read is named rather than dropped"
        );
    }

    #[test]
    fn a_comparison_smaller_than_registered_fails_on_its_size_and_says_so() {
        let result = compare(
            &scores(&[("a", 0.9)]),
            &["a".to_string()],
            &registered(),
            "0",
        );

        assert!((result.precision - 1.0).abs() < f64::EPSILON);
        assert!((result.recall - 1.0).abs() < f64::EPSILON);
        assert!(!result.passed);
        assert_eq!(result.failures.len(), 1);
        assert!(
            result.failures[0].contains("on each side was registered"),
            "{:?}",
            result.failures
        );
    }
}
