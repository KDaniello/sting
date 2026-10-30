//! `POST /v1/decode`, its one error shape, and what keys and quotas do to it.
//!
//! Driven through `tower::ServiceExt::oneshot` like the rest of this crate's HTTP tests, so no
//! socket is bound and no chain is reached: every message here is passed as a body, which is the
//! half of the route that needs no network.
//!
//! What these pin down is the shape of the answers, including the one that says nothing was found.
//! An unknown opcode is a `200` with `unknown: true`, and a test is the only thing that keeps it
//! from quietly becoming a `404` — which would tell a caller their question was wrong when it was
//! this platform's knowledge that ran out.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "docs/ARCHITECTURE.md §3.2 scopes the no-unwrap/expect/panic policy to library code outside tests"
)]

use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::time::Duration;

use axum::body::Body;
use axum::extract::connect_info::ConnectInfo;
use axum::http::{Request, StatusCode};
use axum::response::Response;
use base64::Engine as _;
use http_body_util::BodyExt as _;
use tower::ServiceExt as _;

use sting_abi_dictionary::OperationDictionary;
use sting_abi_pipeline::ExtractController;
use sting_abi_service::{API_KEY_HEADER, AppState, ServiceConfig, Tier3Config, build_router};
use tonlib_core::cell::{BagOfCells, CellBuilder};

/// The committed dataset, not a fixture written to make this pass.
fn committed_operations() -> OperationDictionary {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../corpus-abi/operation-dictionary.json");
    OperationDictionary::load(&path).expect("the committed operation dictionary")
}

fn state_with(config: ServiceConfig, operations: Option<OperationDictionary>) -> AppState {
    let store_dir = tempfile::tempdir().expect("scratch store dir");
    AppState::new(
        ExtractController::new(HashMap::new()),
        config,
        None,
        HashMap::new(),
        sting_abi_registry::VerifiedSchemaStore::new(store_dir.keep()),
        Tier3Config {
            operations,
            ..Tier3Config::default()
        },
    )
}

fn decode_request(body_b64: &str, key: Option<&str>) -> Request<Body> {
    let mut request = Request::post("/v1/decode").header("content-type", "application/json");
    if let Some(key) = key {
        request = request.header(API_KEY_HEADER, key);
    }
    let mut request = request
        .body(Body::from(
            serde_json::to_vec(&serde_json::json!({ "body": body_b64 })).expect("valid JSON"),
        ))
        .expect("valid request");
    request.extensions_mut().insert(ConnectInfo(SocketAddr::new(
        IpAddr::V4(Ipv4Addr::new(203, 0, 113, 7)),
        12345,
    )));
    request
}

async fn body_json(response: Response) -> serde_json::Value {
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    serde_json::from_slice(&bytes).expect("a JSON body")
}

/// A body opening with `opcode` and nothing else.
fn body_of(opcode: u32) -> String {
    let mut builder = CellBuilder::new();
    builder.store_u32(32, opcode).unwrap();
    let bytes = BagOfCells::from_root(builder.build().unwrap())
        .serialize(false)
        .unwrap();
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

#[tokio::test]
async fn a_known_opcode_comes_back_named() {
    let app = build_router(state_with(
        ServiceConfig::default(),
        Some(committed_operations()),
    ));

    let response = app
        .oneshot(decode_request(&body_of(0x0f8a_7ea5), None))
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let answer = body_json(response).await;
    assert_eq!(answer["schema_version"], "1.0");
    assert_eq!(answer["unknown"], false);
    assert_eq!(answer["opcode"], 0x0f8a_7ea5_u32);
    assert!(
        !answer["operations"].as_array().unwrap().is_empty(),
        "{answer}"
    );
}

/// The answer that measures the product: not knowing is `200`, not `404`.
#[tokio::test]
async fn an_unknown_opcode_is_an_answer_rather_than_an_error() {
    let app = build_router(state_with(
        ServiceConfig::default(),
        Some(committed_operations()),
    ));

    let response = app
        .oneshot(decode_request(&body_of(0x0bad_0bad), None))
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let answer = body_json(response).await;
    assert_eq!(answer["unknown"], true);
    assert!(answer["operations"].as_array().unwrap().is_empty());
    assert!(answer["error"].is_null(), "{answer}");
}

#[tokio::test]
async fn a_deployment_with_no_dataset_says_so_in_the_one_error_shape() {
    let app = build_router(state_with(ServiceConfig::default(), None));

    let response = app
        .oneshot(decode_request(&body_of(0x0f8a_7ea5), None))
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    let answer = body_json(response).await;
    assert_eq!(answer["error"]["code"], "decode_unavailable");
    assert!(answer["error"]["message"].is_string());
}

#[tokio::test]
async fn a_body_that_is_not_a_message_fails_in_the_same_shape() {
    let app = build_router(state_with(
        ServiceConfig::default(),
        Some(committed_operations()),
    ));

    let response = app
        .oneshot(decode_request("not base64 at all", None))
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    assert_eq!(
        body_json(response).await["error"]["code"],
        "invalid_request"
    );
}

#[tokio::test]
async fn an_answer_over_the_result_limit_is_refused_rather_than_truncated() {
    let config = ServiceConfig {
        max_result_bytes: 10,
        ..ServiceConfig::default()
    };
    let app = build_router(state_with(config, Some(committed_operations())));

    let response = app
        .oneshot(decode_request(&body_of(0x0f8a_7ea5), None))
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::INSUFFICIENT_STORAGE);
    assert_eq!(
        body_json(response).await["error"]["code"],
        "result_too_large"
    );
}

#[tokio::test]
async fn a_deployment_with_keys_refuses_a_request_that_presents_none() {
    let config = ServiceConfig {
        api_keys: vec!["a-real-key".to_string()],
        ..ServiceConfig::default()
    };
    let app = build_router(state_with(config, Some(committed_operations())));

    let response = app
        .oneshot(decode_request(&body_of(0x0f8a_7ea5), None))
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(body_json(response).await["error"]["code"], "unauthorized");
}

#[tokio::test]
async fn a_key_this_deployment_does_not_know_is_refused_as_such() {
    let config = ServiceConfig {
        api_keys: vec!["a-real-key".to_string()],
        ..ServiceConfig::default()
    };
    let app = build_router(state_with(config, Some(committed_operations())));

    let response = app
        .oneshot(decode_request(&body_of(0x0f8a_7ea5), Some("not-the-key")))
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(body_json(response).await["error"]["code"], "unauthorized");
}

/// The path a paying caller actually hits, and the one nobody tests until it fires in production.
#[tokio::test]
async fn a_key_that_has_spent_its_quota_is_told_which_limit_it_hit() {
    let config = ServiceConfig {
        api_keys: vec!["a-real-key".to_string()],
        quota_requests: 1,
        quota_window: Duration::from_secs(3600),
        ..ServiceConfig::default()
    };
    let state = state_with(config, Some(committed_operations()));

    let first = build_router(state.clone())
        .oneshot(decode_request(&body_of(0x0f8a_7ea5), Some("a-real-key")))
        .await
        .unwrap();
    assert_eq!(first.status(), StatusCode::OK);

    let second = build_router(state)
        .oneshot(decode_request(&body_of(0x0f8a_7ea5), Some("a-real-key")))
        .await
        .unwrap();

    assert_eq!(second.status(), StatusCode::TOO_MANY_REQUESTS);
    let answer = body_json(second).await;
    assert_eq!(
        answer["error"]["code"], "quota_exhausted",
        "a spent quota is not the same refusal as a burst limit"
    );
}

/// A deployment that names no key stays open, which is what this service has been since it was
/// written — keys are a decision a deployment makes, not a default it inherits.
#[tokio::test]
async fn a_deployment_with_no_keys_serves_an_anonymous_request() {
    let app = build_router(state_with(
        ServiceConfig::default(),
        Some(committed_operations()),
    ));

    let response = app
        .oneshot(decode_request(&body_of(0x0f8a_7ea5), None))
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
}
