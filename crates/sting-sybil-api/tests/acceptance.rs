//! What the request path promises: an answer inside its budget whether or not the address is
//! known, a reason attached to every score, a refusal for a request with no key or too many, and
//! counters that say what the service has been doing.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "AGENTS.md §5 scopes the no-unwrap/expect/panic rule to library code outside tests"
)]

use std::collections::HashMap;
use std::sync::Arc;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use sting_sybil_api::auth::{ApiKeyStore, AuthState, RateLimiter};
use sting_sybil_api::metrics::Metrics;
use sting_sybil_api::{AppState, router, router_with_auth};
use sting_sybil_scorer::LinearScorer;
use tower::ServiceExt;

fn sample_state() -> Arc<AppState> {
    let scorer = LinearScorer {
        model_version: "test.v1".into(),
        bias: -1.0,
        weights: HashMap::from([
            ("cluster_agreement".to_string(), 3.0),
            ("iet_cv".to_string(), -2.0),
        ]),
        threshold: 0.5,
    };
    let mut feature_cache = HashMap::new();
    feature_cache.insert(
        "0:known".to_string(),
        HashMap::from([
            ("cluster_agreement".to_string(), 1.0),
            ("iet_cv".to_string(), 0.05),
        ]),
    );
    Arc::new(AppState {
        scorer,
        feature_cache,
        metrics: Metrics::new(),
    })
}

fn sample_auth_state(rps_limit: f64, capacity: f64) -> Arc<AuthState> {
    let toml_str = format!(
        r#"
        [tiers.test]
        rps_limit = {rps_limit}
        capacity = {capacity}

        [keys."valid-key"]
        tier = "test"
        "#
    );
    Arc::new(AuthState {
        api_keys: ApiKeyStore::from_toml_str(&toml_str).unwrap(),
        rate_limiter: RateLimiter::new(),
    })
}

async fn get_score(app: axum::Router, address: &str) -> (StatusCode, serde_json::Value) {
    get_score_with_auth(app, address, None).await
}

async fn get_score_with_auth(
    app: axum::Router,
    address: &str,
    api_key: Option<&str>,
) -> (StatusCode, serde_json::Value) {
    let mut builder = Request::builder().uri(format!("/score/{address}"));
    if let Some(key) = api_key {
        builder = builder.header("Authorization", format!("Bearer {key}"));
    }
    let response = app
        .oneshot(builder.body(Body::empty()).unwrap())
        .await
        .unwrap();
    let status = response.status();
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap_or(serde_json::Value::Null);
    (status, json)
}

async fn get_metrics_text(app: axum::Router) -> String {
    let response = app
        .oneshot(
            Request::builder()
                .uri("/metrics")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    String::from_utf8(body.to_vec()).unwrap()
}

#[tokio::test]
async fn a_known_address_is_answered_inside_the_budget() {
    let app = router(sample_state());
    let start = std::time::Instant::now();
    let (status, json) = get_score(app, "0:known").await;
    let elapsed = start.elapsed();

    assert_eq!(status, StatusCode::OK);
    assert!(
        elapsed < std::time::Duration::from_millis(200),
        "elapsed={elapsed:?}, against a budget of 200 ms"
    );
    assert_eq!(json["partial"], false);
}

/// The hot path must not acquire a Python dependency.
///
/// Crude on purpose: a binding pulled in transitively would make every request depend on an
/// interpreter's startup and its global lock, and the manifest is where that would first appear.
#[test]
fn nothing_on_the_request_path_depends_on_an_interpreter() {
    let cargo_toml = include_str!("../Cargo.toml");
    let lower = cargo_toml.to_lowercase();
    assert!(!lower.contains("pyo3") && !lower.contains("python"));
}

#[tokio::test]
async fn an_unknown_address_is_answered_rather_than_resolved() {
    let app = router(sample_state());
    let start = std::time::Instant::now();
    let (status, json) = get_score(app, "0:unknown").await;
    let elapsed = start.elapsed();

    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        json["partial"], true,
        "a miss says so rather than being served as if it were a hit"
    );
    assert!(
        elapsed < std::time::Duration::from_millis(200),
        "a miss costs the same as a hit: nothing goes looking"
    );
}

#[tokio::test]
async fn every_answer_says_which_model_and_why() {
    let app = router(sample_state());
    let (_, json) = get_score(app, "0:known").await;

    assert!(json["model_version"].as_str().is_some());
    let reasons = json["reasons"].as_array().expect("reasons is a list");
    assert!(
        !reasons.is_empty(),
        "a score with no reason attached is a number to be taken on trust"
    );
}

#[tokio::test]
async fn the_score_in_the_answer_is_the_score_the_model_computes() {
    let state = sample_state();
    let features = state.feature_cache["0:known"].clone();
    let expected_risk = state.scorer.score(&features);

    let app = router(state);
    let (_, json) = get_score(app, "0:known").await;
    let actual_risk = json["risk"].as_f64().unwrap();
    assert!((actual_risk - expected_risk).abs() < 1e-9);
}

#[tokio::test]
async fn a_request_with_no_key_is_refused() {
    let app = router_with_auth(sample_state(), sample_auth_state(50.0, 50.0));
    let (status, _) = get_score_with_auth(app, "0:known", None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn a_request_with_a_key_nobody_issued_is_refused() {
    let app = router_with_auth(sample_state(), sample_auth_state(50.0, 50.0));
    let (status, _) = get_score_with_auth(app, "0:known", Some("not-a-real-key")).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn a_request_with_a_key_and_allowance_left_is_served() {
    let app = router_with_auth(sample_state(), sample_auth_state(50.0, 50.0));
    let (status, json) = get_score_with_auth(app, "0:known", Some("valid-key")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(json["partial"], false);
}

#[tokio::test]
async fn a_burst_past_the_allowance_is_refused() {
    // Two tokens, three requests, no delay between them: the refill over microseconds is far
    // below one token, so this does not depend on how fast the machine is.
    let auth_state = sample_auth_state(1.0, 2.0);
    let state = sample_state();

    let app1 = router_with_auth(state.clone(), auth_state.clone());
    let (s1, _) = get_score_with_auth(app1, "0:known", Some("valid-key")).await;
    let app2 = router_with_auth(state.clone(), auth_state.clone());
    let (s2, _) = get_score_with_auth(app2, "0:known", Some("valid-key")).await;
    let app3 = router_with_auth(state, auth_state);
    let (s3, _) = get_score_with_auth(app3, "0:known", Some("valid-key")).await;

    assert_eq!(s1, StatusCode::OK);
    assert_eq!(s2, StatusCode::OK);
    assert_eq!(s3, StatusCode::TOO_MANY_REQUESTS);
}

#[tokio::test]
async fn metrics_are_readable_without_a_key() {
    // A scraper lives on the same private network, and a key in a scrape config is a key in a
    // config file.
    let app = router_with_auth(sample_state(), sample_auth_state(1.0, 1.0));
    let text = get_metrics_text(app).await;
    assert!(text.contains("sybil_http_requests_total"));
}

#[tokio::test]
async fn the_counters_and_percentiles_follow_the_requests_served() {
    let state = sample_state();
    let app1 = router(state.clone());
    get_score(app1, "0:known").await;
    let app2 = router(state.clone());
    get_score(app2, "0:unknown").await;

    let app3 = router(state);
    let text = get_metrics_text(app3).await;

    assert!(text.starts_with("# HELP"), "the exposition format needs it");
    assert!(text.contains("sybil_http_requests_total 2"));
    assert!(text.contains("sybil_cache_hits_total 1"));
    assert!(text.contains("sybil_cache_misses_total 1"));
    assert!(text.contains("sybil_http_request_duration_ms_p50"));
    assert!(text.contains("sybil_http_request_duration_ms_p99"));
}

/// Tracing is applied by what serves rather than by the router itself, so what is checked is
/// that wrapping the router in it still answers. What the spans contain would need a capturing
/// subscriber, which is a dependency for a weaker claim than this one.
#[tokio::test]
async fn the_tracing_layer_does_not_change_the_answer() {
    let app = router(sample_state()).layer(tower_http::trace::TraceLayer::new_for_http());
    let (status, json) = get_score(app, "0:known").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(json["partial"], false);
}

/// The whole answer, against a recorded snapshot.
///
/// Every other test here asserts one field. This one records all of them, which is what catches a
/// change nobody thought to assert: a band boundary that moved, a reason that lost its
/// contribution, a field that quietly changed its name for whoever is parsing the response.
///
/// The latency is dropped before recording. It is a measurement of the host and would make the
/// snapshot fail on a slower machine for a reason that has nothing to do with the answer.
#[tokio::test]
async fn the_whole_answer_matches_its_recorded_snapshot() {
    let app = router(sample_state());
    let (_, mut json) = get_score(app, "0:known").await;
    json.as_object_mut()
        .expect("the response is an object")
        .remove("latency_ms");
    insta::assert_json_snapshot!(json);
}
