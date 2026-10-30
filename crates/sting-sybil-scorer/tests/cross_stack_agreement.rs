//! How far the fast scorer and the full one disagree, read off the artefact that measured it.
//!
//! The distillation optimises for agreement with the full model rather than being trained
//! independently, and the training run records the mean absolute difference it achieved. Nothing
//! is gained by recomputing that here over invented feature vectors — it would be a weaker claim,
//! about numbers this test made up rather than about the model that was actually produced. What
//! is checked instead is that the artefact carries the figure and that the figure is inside the
//! bound the platform states.
//!
//! Read with plain JSON and no Python involved, because a test that needed an interpreter to
//! check a committed file would be testing the interpreter.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "AGENTS.md §5 scopes the no-unwrap/expect/panic rule to library code outside tests"
)]

fn fixture_json() -> String {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../corpus/sybil/fixtures/model_v1.json"
    );
    std::fs::read_to_string(path).expect("the training artefact is checked in")
}

#[test]
fn the_disagreement_is_measured_and_inside_its_bound() {
    let doc: serde_json::Value = serde_json::from_str(&fixture_json()).unwrap();

    let diff = doc["metrics"]["rt_batch_mean_abs_diff"]
        .as_f64()
        .expect("the training run records the difference it achieved");

    assert!(
        diff.is_finite() && diff >= 0.0,
        "the difference is a real non-negative number, and is {diff}"
    );
    assert!(
        diff < 0.2,
        "mean absolute difference {diff} between the fast and the full scorer. If a distillation \
         really drifted this far, the bound is what needs re-deciding — deliberately, and not by \
         loosening this line"
    );
}

/// The two files are two halves of one artefact, and a version that disagrees means somebody
/// regenerated one of them alone.
#[test]
fn both_halves_of_the_artefact_name_the_same_model() {
    let model_json: serde_json::Value = serde_json::from_str(&fixture_json()).unwrap();
    let toml_path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../corpus/sybil/fixtures/linear_scorer_v1.toml"
    );
    let toml_str = std::fs::read_to_string(toml_path).unwrap();
    let linear_scorer = sting_sybil_scorer::LinearScorer::from_toml_str(&toml_str).unwrap();

    assert_eq!(
        model_json["model_version"].as_str().unwrap(),
        linear_scorer.model_version
    );
}
