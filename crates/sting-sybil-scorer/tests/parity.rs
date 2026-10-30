//! Whether this side reads what the training side wrote, and computes what it computed.
//!
//! A test that the formula is the same in both languages proves the formula and nothing else. The
//! question here is narrower and is the one that actually breaks: whether the file one library
//! wrote parses in the other, and whether the numbers that come out of it match what the first
//! side calculated on the same coefficients. The fixture is a real training artefact rather than
//! a handwritten file, and the expected scores below were computed on the other side of the
//! stack.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "AGENTS.md §5 scopes the no-unwrap/expect/panic rule to library code outside tests"
)]

use sting_sybil_scorer::LinearScorer;

fn fixture_toml() -> String {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../corpus/sybil/fixtures/linear_scorer_v1.toml"
    );
    std::fs::read_to_string(path).expect("the training artefact is checked in")
}

fn fixture_scorer() -> LinearScorer {
    LinearScorer::from_toml_str(&fixture_toml())
        .expect("a file the training side wrote must read here")
}

#[test]
fn the_artefact_reads_with_the_model_it_names() {
    let scorer = fixture_scorer();
    assert_eq!(scorer.model_version, "fixture.v1");
    assert!(scorer.weights.contains_key("iet_cv"));
    // The other features are constant in the synthetic scenario this was trained on, so their
    // weights are legitimately near zero — but the keys are there, because the file carries the
    // whole vocabulary rather than only the features that turned out to matter.
    assert!(scorer.weights.contains_key("wcc_size"));
    assert!(scorer.weights.contains_key("cluster_agreement"));
}

/// With no features at all, the score is the sigmoid of the bias. The expected value was computed
/// on the training side from this same file.
#[test]
fn no_features_scores_the_bias_alone() {
    let scorer = fixture_scorer();
    let features = std::collections::HashMap::new();
    let score = scorer.score(&features);
    assert!(
        (score - 0.269_707_107_612_056_2).abs() < 1e-9,
        "score={score}"
    );
}

/// One feature, against the same arithmetic performed on the other side.
#[test]
fn one_feature_scores_what_the_other_side_computed() {
    let scorer = fixture_scorer();
    let features = std::collections::HashMap::from([("iet_cv".to_string(), 0.1)]);
    let score = scorer.score(&features);
    assert!(
        (score - 0.207_369_837_979_299_25).abs() < 1e-9,
        "score={score}"
    );
}

/// A feature whose weight is near zero must not move the score, which is both a fact about this
/// model and a check that an extra feature does not disturb the sum.
#[test]
fn a_feature_with_no_weight_does_not_move_the_score() {
    let scorer = fixture_scorer();
    let features = std::collections::HashMap::from([
        ("iet_cv".to_string(), 0.9),
        ("cluster_agreement".to_string(), 1.0),
    ]);
    let score = scorer.score(&features);
    assert!(
        (score - 0.016_321_703_777_349_62).abs() < 1e-9,
        "score={score}"
    );
}
