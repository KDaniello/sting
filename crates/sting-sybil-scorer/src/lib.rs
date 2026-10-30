//! The scorer on the hot path: a linear model, read from a file, evaluated in microseconds.
//!
//! A request that has to reach a Python process to learn whether an address is risky is a request
//! that answers in tens of milliseconds and stops answering when that process does. What is here
//! instead is the distilled form of the full model — a bias, a weight per feature, a threshold —
//! which agrees with it closely enough that the disagreement is measured and recorded rather than
//! assumed away.
//!
//! Weights are named rather than positional. The file the training side writes is a table of
//! name to weight, and a positional vector would make the meaning of a score depend on a column
//! order nothing enforces.
//!
//! # What this crate does not do
//!
//! It trains nothing and calibrates nothing: it reads coefficients somebody else produced. It
//! computes no features — a caller hands it a set of named values. It does not decide what a
//! threshold should be; the model states one and this applies it.

use std::collections::HashMap;

/// A calibrated probability, between zero and one.
#[must_use]
pub fn sigmoid(z: f64) -> f64 {
    1.0 / (1.0 + (-z).exp())
}

/// A linear model with named weights.
#[derive(Clone, Debug, PartialEq)]
pub struct LinearScorer {
    /// Which model this is, so a score can be traced to the artefact that produced it.
    pub model_version: String,
    /// The intercept.
    pub bias: f64,
    /// A weight per feature name.
    pub weights: HashMap<String, f64>,
    /// At and above which score an address is flagged.
    pub threshold: f64,
}

#[derive(serde::Deserialize)]
struct LinearScorerConfig {
    model_version: String,
    bias: f64,
    threshold: f64,
    weights: HashMap<String, f64>,
}

impl LinearScorer {
    /// The score for a set of named features.
    ///
    /// A feature this model has no weight for contributes nothing, and that is not an error: a
    /// model retrained on a different vocabulary legitimately knows fewer features than the
    /// caller can supply, and refusing the call would make every retrain a breaking change.
    #[must_use]
    pub fn score(&self, features: &HashMap<String, f64>) -> f64 {
        let z: f64 = self.bias
            + self
                .weights
                .iter()
                .map(|(name, weight)| weight * features.get(name).copied().unwrap_or(0.0))
                .sum::<f64>();
        sigmoid(z)
    }

    /// The band a score falls in, against this model's own threshold.
    #[must_use]
    pub fn band(&self, risk: f64) -> &'static str {
        if risk >= self.threshold {
            "high"
        } else if risk >= self.threshold / 2.0 {
            "medium"
        } else {
            "low"
        }
    }

    /// Whether a score is at or above the threshold.
    #[must_use]
    pub fn is_flagged(&self, risk: f64) -> bool {
        risk >= self.threshold
    }

    /// Reads a model from the file format the training side writes.
    ///
    /// # Errors
    /// [`toml::de::Error`] when the text is not that format.
    pub fn from_toml_str(text: &str) -> Result<Self, toml::de::Error> {
        let cfg: LinearScorerConfig = toml::from_str(text)?;
        Ok(Self {
            model_version: cfg.model_version,
            bias: cfg.bias,
            weights: cfg.weights,
            threshold: cfg.threshold,
        })
    }
}

#[cfg(test)]
#[allow(
    clippy::float_cmp,
    reason = "these compare values read straight back out of a file, not the result of accumulated arithmetic"
)]
#[allow(
    clippy::expect_used,
    reason = "AGENTS.md §5 scopes the no-unwrap/expect/panic rule to library code outside tests"
)]
mod tests {
    use super::*;

    #[test]
    fn a_score_of_zero_evidence_is_a_half() {
        assert_eq!(sigmoid(0.0), 0.5);
    }

    #[test]
    fn extreme_evidence_approaches_the_bounds_without_reaching_them() {
        assert!(sigmoid(100.0) > 0.9999);
        assert!(sigmoid(-100.0) < 0.0001);
    }

    fn sample_scorer() -> LinearScorer {
        LinearScorer {
            model_version: "test.v1".into(),
            bias: -1.0,
            weights: HashMap::from([
                ("iet_cv".to_string(), -2.0),
                ("cluster_agreement".to_string(), 3.0),
            ]),
            threshold: 0.62,
        }
    }

    #[test]
    fn the_score_is_the_arithmetic_it_claims_to_be() {
        let scorer = sample_scorer();
        let features = HashMap::from([
            ("iet_cv".to_string(), 0.1),
            ("cluster_agreement".to_string(), 1.0),
        ]);
        // -1.0 + (-2.0 × 0.1) + (3.0 × 1.0) = 1.8
        let expected = sigmoid(1.8);
        assert!((scorer.score(&features) - expected).abs() < 1e-12);
    }

    #[test]
    fn a_feature_the_caller_omits_contributes_nothing() {
        let scorer = sample_scorer();
        let features = HashMap::from([("iet_cv".to_string(), 0.1)]);
        let expected = sigmoid(-1.2);
        assert!((scorer.score(&features) - expected).abs() < 1e-12);
    }

    #[test]
    fn a_feature_the_model_does_not_know_is_ignored() {
        let scorer = sample_scorer();
        let features = HashMap::from([
            ("iet_cv".to_string(), 0.1),
            ("cluster_agreement".to_string(), 1.0),
            ("totally_unrelated_field".to_string(), 999.0),
        ]);
        let expected = sigmoid(1.8);
        assert!((scorer.score(&features) - expected).abs() < 1e-12);
    }

    #[test]
    fn the_bands_sit_where_the_threshold_puts_them() {
        let scorer = sample_scorer();
        assert_eq!(scorer.band(0.9), "high");
        assert_eq!(scorer.band(0.62), "high");
        assert_eq!(scorer.band(0.4), "medium");
        assert_eq!(scorer.band(0.1), "low");
    }

    #[test]
    fn flagging_is_at_the_threshold_and_not_above_it() {
        let scorer = sample_scorer();
        assert!(scorer.is_flagged(0.62));
        assert!(!scorer.is_flagged(0.619_999));
    }

    #[test]
    fn a_model_file_reads_into_the_model_it_describes() {
        let toml_str = r#"
            model_version = "2026-07-01.v3"
            bias = -1.203
            threshold = 0.62

            [weights]
            iet_cv = -2.41
            wcc_size = 0.83
            cluster_agreement = 1.95
            novelty_ratio = 0.44
        "#;
        let scorer = LinearScorer::from_toml_str(toml_str).expect("this is the documented format");
        assert_eq!(scorer.model_version, "2026-07-01.v3");
        assert_eq!(scorer.bias, -1.203);
        assert_eq!(scorer.threshold, 0.62);
        assert_eq!(scorer.weights.len(), 4);
        assert_eq!(scorer.weights["cluster_agreement"], 1.95);
    }

    #[test]
    fn a_file_that_is_not_a_model_is_refused() {
        assert!(LinearScorer::from_toml_str("this is not valid toml {{{").is_err());
    }
}
