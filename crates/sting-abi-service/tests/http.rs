//! Integration tests of the HTTP layer via `tower::ServiceExt::oneshot`
//! (`docs/TESTING.md`) — routing, error format,
//! timeouts, and rate-limit middleware, without a real TCP socket. Every
//! test here uses a mock [`Extractor`] (`sting-abi-pipeline`'s own
//! established test pattern), not a real compiler — real-compiler
//! end-to-end coverage is `tests/real_end_to_end.rs`, kept separate so
//! these routing/error-format tests stay fast and never skip.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "docs/ARCHITECTURE.md §3.2 scopes the no-unwrap/expect/panic policy to library code outside tests"
)]

use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::{Path, PathBuf};
use std::time::Duration;

use axum::body::Body;
use axum::extract::connect_info::ConnectInfo;
use axum::http::{Request, StatusCode};
use axum::response::Response;
use http_body_util::BodyExt;
use tower::ServiceExt;

use sting_abi_core::{
    ContractMetadata, ContractSchema, ExtractError, ExtractionTier, Extractor, SourceLanguage,
};
use sting_abi_pipeline::ExtractController;
use sting_abi_service::{AppState, ServiceConfig, build_router};

fn test_addr() -> SocketAddr {
    SocketAddr::new(IpAddr::V4(Ipv4Addr::new(203, 0, 113, 1)), 12345)
}

fn minimal_schema(language: SourceLanguage) -> ContractSchema {
    ContractSchema {
        ir_schema_version: sting_abi_core::IR_SCHEMA_VERSION.to_string(),
        source_language: language,
        extraction_tier: ExtractionTier::Compiled,
        contract_name: Some("Mock".to_string()),
        metadata: ContractMetadata::default(),
        storage: None,
        types: Vec::new(),
        incoming_internal: Vec::new(),
        incoming_external: Vec::new(),
        outgoing: Vec::new(),
        get_methods: Vec::new(),
        errors: Vec::new(),
        confidence: None,
        field_confidence: None,
    }
}

enum MockBehavior {
    Succeed,
    Fail,
    Sleep(Duration),
}

struct MockExtractor {
    language: SourceLanguage,
    behavior: MockBehavior,
}

impl Extractor for MockExtractor {
    fn language(&self) -> SourceLanguage {
        self.language
    }

    fn extract(
        &self,
        _project_dir: &Path,
        _entrypoint: &Path,
    ) -> Result<ContractSchema, ExtractError> {
        match &self.behavior {
            MockBehavior::Succeed => Ok(minimal_schema(self.language)),
            MockBehavior::Fail => Err(ExtractError::Failed {
                entrypoint: PathBuf::from("contract"),
                message: "synthetic mock compile failure".to_string(),
                source: None,
            }),
            MockBehavior::Sleep(duration) => {
                std::thread::sleep(*duration);
                Ok(minimal_schema(self.language))
            }
        }
    }
}

fn state_with(extractors: Vec<(SourceLanguage, MockBehavior)>, config: ServiceConfig) -> AppState {
    let mut map: HashMap<SourceLanguage, Box<dyn Extractor>> = HashMap::new();
    for (language, behavior) in extractors {
        map.insert(language, Box::new(MockExtractor { language, behavior }));
    }
    build_state(map, config)
}

/// Shared by every test in this file — no `/v1/verify`/`/v1/schema/{address}`
/// test here exercises the registry path (that's `sting-abi-registry`'s
/// own `tests/registry.rs`, real Tact compilation, no mocks), so `corpus`/
/// `bytecode_sources` are always empty/absent here; the store still needs
/// a real scratch directory that outlives this function (`.keep()`, not
/// dropped at the end of scope) so `AppState` doesn't hold a path to an
/// already-deleted directory.
fn build_state(
    extractors: HashMap<SourceLanguage, Box<dyn Extractor>>,
    config: ServiceConfig,
) -> AppState {
    let store_dir = tempfile::tempdir().expect("scratch store dir");
    AppState::new(
        ExtractController::new(extractors),
        config,
        None,
        HashMap::new(),
        sting_abi_registry::VerifiedSchemaStore::new(store_dir.keep()),
        sting_abi_service::Tier3Config::default(),
    )
}

fn with_connect_info(mut req: Request<Body>) -> Request<Body> {
    req.extensions_mut().insert(ConnectInfo(test_addr()));
    req
}

fn post_json(path: &str, body: &serde_json::Value) -> Request<Body> {
    with_connect_info(
        Request::post(path)
            .header("content-type", "application/json")
            .body(Body::from(serde_json::to_vec(body).expect("valid JSON")))
            .expect("valid request"),
    )
}

async fn body_json(response: Response) -> serde_json::Value {
    let bytes = response
        .into_body()
        .collect()
        .await
        .expect("readable body")
        .to_bytes();
    serde_json::from_slice(&bytes).expect("valid JSON body")
}

#[tokio::test]
async fn health_returns_ok() {
    let router = build_router(state_with(vec![], ServiceConfig::default()));
    let response = router
        .oneshot(with_connect_info(
            Request::get("/v1/health")
                .body(Body::empty())
                .expect("valid request"),
        ))
        .await
        .expect("router never errors");
    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn version_reports_ir_schema_version() {
    let router = build_router(state_with(vec![], ServiceConfig::default()));
    let response = router
        .oneshot(with_connect_info(
            Request::get("/v1/version")
                .body(Body::empty())
                .expect("valid request"),
        ))
        .await
        .expect("router never errors");
    assert_eq!(response.status(), StatusCode::OK);
    let json = body_json(response).await;
    assert_eq!(json["ir_schema_version"], sting_abi_core::IR_SCHEMA_VERSION);
}

#[tokio::test]
async fn schema_by_address_with_nothing_on_file_is_an_honest_404_not_an_empty_success() {
    // With no `POST /v1/verify` ever having recorded this address,
    // the honest answer is `404 no_verified_source`, not an empty `200`
    // or the old `501`. The real, positive lookup path (a genuinely
    // recorded verification) is `sting-abi-registry`'s own
    // `tests/registry.rs` plus this crate's `real_end_to_end.rs`, not
    // reproduced here with mocks.
    let router = build_router(state_with(vec![], ServiceConfig::default()));
    let response = router
        .oneshot(with_connect_info(
            Request::get("/v1/schema/EQsomeaddress")
                .body(Body::empty())
                .expect("valid request"),
        ))
        .await
        .expect("router never errors");
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    let json = body_json(response).await;
    assert_eq!(json["error"]["code"], "no_verified_source");
}

#[tokio::test]
async fn schema_by_address_returns_a_previously_recorded_verification() {
    let state = state_with(vec![], ServiceConfig::default());
    state
        .verified_store
        .put(
            "EQrecorded",
            &minimal_schema(SourceLanguage::Tact),
            sting_abi_registry::MatchStatus::Full,
        )
        .expect("put must succeed");
    let router = build_router(state);

    let response = router
        .oneshot(with_connect_info(
            Request::get("/v1/schema/EQrecorded")
                .body(Body::empty())
                .expect("valid request"),
        ))
        .await
        .expect("router never errors");

    assert_eq!(response.status(), StatusCode::OK);
    let json = body_json(response).await;
    assert_eq!(json["source_language"], "tact");
}

#[tokio::test]
async fn extract_single_succeeds_on_a_registered_language() {
    let router = build_router(state_with(
        vec![(SourceLanguage::Tolk, MockBehavior::Succeed)],
        ServiceConfig::default(),
    ));
    let response = router
        .oneshot(post_json(
            "/v1/extract",
            &serde_json::json!({"source": "contract Foo {}", "lang": "tolk"}),
        ))
        .await
        .expect("router never errors");
    assert_eq!(response.status(), StatusCode::OK);
    let json = body_json(response).await;
    assert_eq!(json["source_language"], "tolk");
}

#[tokio::test]
async fn extract_single_rejects_auto_with_a_clear_message_not_a_guess() {
    let router = build_router(state_with(
        vec![(SourceLanguage::Tolk, MockBehavior::Succeed)],
        ServiceConfig::default(),
    ));
    let response = router
        .oneshot(post_json(
            "/v1/extract",
            &serde_json::json!({"source": "contract Foo {}", "lang": "auto"}),
        ))
        .await
        .expect("router never errors");
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let json = body_json(response).await;
    assert_eq!(json["error"]["code"], "invalid_request");
}

#[tokio::test]
async fn extract_single_unregistered_language_is_unsupported_language_not_a_panic() {
    let router = build_router(state_with(vec![], ServiceConfig::default()));
    let response = router
        .oneshot(post_json(
            "/v1/extract",
            &serde_json::json!({"source": "contract Foo {}", "lang": "tact"}),
        ))
        .await
        .expect("router never errors");
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let json = body_json(response).await;
    assert_eq!(json["error"]["code"], "unsupported_language");
}

#[tokio::test]
async fn extract_single_compile_failure_is_compilation_failed() {
    let router = build_router(state_with(
        vec![(SourceLanguage::Tolk, MockBehavior::Fail)],
        ServiceConfig::default(),
    ));
    let response = router
        .oneshot(post_json(
            "/v1/extract",
            &serde_json::json!({"source": "not real tolk", "lang": "tolk"}),
        ))
        .await
        .expect("router never errors");
    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    let json = body_json(response).await;
    assert_eq!(json["error"]["code"], "compilation_failed");
}

#[tokio::test]
async fn extract_single_malformed_json_body_is_invalid_request_not_axums_default_shape() {
    let router = build_router(state_with(vec![], ServiceConfig::default()));
    let response = router
        .oneshot(with_connect_info(
            Request::post("/v1/extract")
                .header("content-type", "application/json")
                .body(Body::from("{not valid json"))
                .expect("valid request"),
        ))
        .await
        .expect("router never errors");
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let json = body_json(response).await;
    assert_eq!(json["error"]["code"], "invalid_request");
}

#[tokio::test]
async fn extract_single_oversized_body_is_payload_too_large() {
    let config = ServiceConfig {
        max_body_bytes: 32,
        ..ServiceConfig::default()
    };
    let router = build_router(state_with(
        vec![(SourceLanguage::Tolk, MockBehavior::Succeed)],
        config,
    ));
    let big_source = "x".repeat(1000);
    let response = router
        .oneshot(post_json(
            "/v1/extract",
            &serde_json::json!({"source": big_source, "lang": "tolk"}),
        ))
        .await
        .expect("router never errors");
    assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);
    let json = body_json(response).await;
    assert_eq!(json["error"]["code"], "payload_too_large");
}

#[tokio::test]
async fn extract_single_times_out_when_extraction_exceeds_the_request_timeout() {
    let config = ServiceConfig {
        request_timeout: Duration::from_millis(50),
        ..ServiceConfig::default()
    };
    let router = build_router(state_with(
        vec![(
            SourceLanguage::Tolk,
            MockBehavior::Sleep(Duration::from_secs(2)),
        )],
        config,
    ));
    let response = router
        .oneshot(post_json(
            "/v1/extract",
            &serde_json::json!({"source": "contract Foo {}", "lang": "tolk"}),
        ))
        .await
        .expect("router never errors");
    assert_eq!(response.status(), StatusCode::GATEWAY_TIMEOUT);
    let json = body_json(response).await;
    assert_eq!(json["error"]["code"], "timeout");
}

#[tokio::test]
async fn extract_batch_partial_failure_does_not_fail_the_whole_batch() {
    let router = build_router(state_with(
        vec![(SourceLanguage::Tolk, MockBehavior::Succeed)],
        ServiceConfig::default(),
    ));
    let response = router
        .oneshot(post_json(
            "/v1/extract/batch",
            &serde_json::json!({"items": [
                {"id": "a", "source": "contract Foo {}", "lang": "tolk"},
                {"id": "b", "source": "contract Bar {}", "lang": "func"},
                {"id": "c", "source": "contract Baz {}", "lang": "not-a-real-language"},
            ]}),
        ))
        .await
        .expect("router never errors");
    assert_eq!(response.status(), StatusCode::OK);
    let json = body_json(response).await;
    let results = json["results"].as_array().expect("results array");
    assert_eq!(results.len(), 3);
    assert_eq!(results[0]["id"], "a");
    assert_eq!(results[0]["status"], "ok");
    assert!(results[0]["schema"].is_object());
    assert_eq!(results[1]["id"], "b");
    assert_eq!(results[1]["status"], "error");
    assert_eq!(results[2]["id"], "c");
    assert_eq!(results[2]["status"], "error");
}

/// `MockExtractor` here panics only when the source contains a marker
/// string, so one request can mix a panicking item with succeeding ones
/// (the language-keyed `state_with` helper builds one mock per
/// *language*, not per item, so it can't express "only this specific item
/// panics").
struct PanicOnMarkerExtractor;

impl Extractor for PanicOnMarkerExtractor {
    fn language(&self) -> SourceLanguage {
        SourceLanguage::Tolk
    }

    fn extract(
        &self,
        project_dir: &Path,
        entrypoint: &Path,
    ) -> Result<ContractSchema, ExtractError> {
        let source = std::fs::read_to_string(project_dir.join(entrypoint)).unwrap_or_default();
        assert!(!source.contains("PANIC_MARKER"), "synthetic mock panic");
        Ok(minimal_schema(SourceLanguage::Tolk))
    }
}

/// Real bug found by review: `rayon`'s `into_par_iter().collect()`
/// propagates a single item's panic and discards the whole batch's
/// already-computed results, violating the rule that one invalid item answers
/// `status: error` without taking the rest down. Fixed with `catch_unwind`
/// around each item in
/// `routes/extract.rs::extract_batch_item`.
#[tokio::test]
async fn extract_batch_one_item_panicking_does_not_lose_the_others_results() {
    let mut map: HashMap<SourceLanguage, Box<dyn Extractor>> = HashMap::new();
    map.insert(SourceLanguage::Tolk, Box::new(PanicOnMarkerExtractor));
    let state = build_state(map, ServiceConfig::default());
    let router = build_router(state);

    let response = router
        .oneshot(post_json(
            "/v1/extract/batch",
            &serde_json::json!({"items": [
                {"id": "good-before", "source": "contract Foo {}", "lang": "tolk"},
                {"id": "panics", "source": "PANIC_MARKER", "lang": "tolk"},
                {"id": "good-after", "source": "contract Bar {}", "lang": "tolk"},
            ]}),
        ))
        .await
        .expect("router never errors");

    assert_eq!(
        response.status(),
        StatusCode::OK,
        "one panicking item must not fail the whole batch request"
    );
    let json = body_json(response).await;
    let results = json["results"].as_array().expect("results array");
    assert_eq!(results.len(), 3);
    let by_id = |id: &str| results.iter().find(|r| r["id"] == id).expect("present");
    assert_eq!(by_id("good-before")["status"], "ok");
    assert_eq!(by_id("panics")["status"], "error");
    assert_eq!(by_id("good-after")["status"], "ok");
}

#[tokio::test]
async fn extract_batch_over_the_configured_limit_is_rejected_whole() {
    let config = ServiceConfig {
        batch_max_items: 2,
        ..ServiceConfig::default()
    };
    let router = build_router(state_with(
        vec![(SourceLanguage::Tolk, MockBehavior::Succeed)],
        config,
    ));
    let items: Vec<_> = (0..3)
        .map(|i| serde_json::json!({"id": i.to_string(), "source": "contract Foo {}", "lang": "tolk"}))
        .collect();
    let response = router
        .oneshot(post_json(
            "/v1/extract/batch",
            &serde_json::json!({"items": items}),
        ))
        .await
        .expect("router never errors");
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let json = body_json(response).await;
    assert_eq!(json["error"]["code"], "invalid_request");
}

#[tokio::test]
async fn rate_limit_blocks_the_same_ip_after_its_budget_is_exhausted() {
    let config = ServiceConfig {
        rate_limit_requests: 1,
        rate_limit_window: Duration::from_secs(60),
        ..ServiceConfig::default()
    };
    let router = build_router(state_with(
        vec![(SourceLanguage::Tolk, MockBehavior::Succeed)],
        config,
    ));

    let first = router
        .clone()
        .oneshot(post_json(
            "/v1/extract",
            &serde_json::json!({"source": "contract Foo {}", "lang": "tolk"}),
        ))
        .await
        .expect("router never errors");
    assert_eq!(first.status(), StatusCode::OK);

    let second = router
        .oneshot(post_json(
            "/v1/extract",
            &serde_json::json!({"source": "contract Foo {}", "lang": "tolk"}),
        ))
        .await
        .expect("router never errors");
    assert_eq!(second.status(), StatusCode::TOO_MANY_REQUESTS);
    let json = body_json(second).await;
    assert_eq!(json["error"]["code"], "rate_limited");
}

/// Regression test for the batch-amplification gap found by review
/// : under the old flat-1-token-per-request
/// charge, a batch of any size cost exactly as much as a single-item
/// request. Cost-aware limiting must charge (approximately) one token per
/// item — proven here by spending the whole budget on one 5-item batch and
/// confirming a subsequent single-item request from the same IP is then
/// rejected, which could only happen if the batch really did consume all 5
/// tokens.
#[tokio::test]
async fn extract_batch_charges_proportionally_to_its_item_count() {
    let config = ServiceConfig {
        rate_limit_requests: 5,
        rate_limit_window: Duration::from_secs(60),
        batch_max_items: 10,
        ..ServiceConfig::default()
    };
    let router = build_router(state_with(
        vec![(SourceLanguage::Tolk, MockBehavior::Succeed)],
        config,
    ));

    let items: Vec<_> = (0..5)
        .map(
            |i| serde_json::json!({"id": i.to_string(), "source": "contract Foo {}", "lang": "tolk"}),
        )
        .collect();
    let first = router
        .clone()
        .oneshot(post_json(
            "/v1/extract/batch",
            &serde_json::json!({"items": items}),
        ))
        .await
        .expect("router never errors");
    assert_eq!(
        first.status(),
        StatusCode::OK,
        "a 5-item batch against a 5-token budget must exactly succeed"
    );

    let second = router
        .oneshot(post_json(
            "/v1/extract",
            &serde_json::json!({"source": "contract Foo {}", "lang": "tolk"}),
        ))
        .await
        .expect("router never errors");
    assert_eq!(
        second.status(),
        StatusCode::TOO_MANY_REQUESTS,
        "the budget must already be fully spent by the batch above"
    );
}

/// A batch whose item count exceeds the rate limiter's own capacity can
/// never be funded no matter how idle the bucket is — documented as
/// intentional in `RateLimiter::check_n`'s doc comment. Confirms the
/// rejection happens as `429 rate_limited`, not e.g. a partial/degraded
/// `200`, and that it does not depend on `batch_max_items` (5 items here is
/// comfortably under `batch_max_items: 10`, so this is purely the rate
/// limiter rejecting it, not the separate batch-size cap).
#[tokio::test]
async fn extract_batch_over_the_rate_limit_capacity_is_rejected_as_rate_limited() {
    let config = ServiceConfig {
        rate_limit_requests: 3,
        rate_limit_window: Duration::from_secs(60),
        batch_max_items: 10,
        ..ServiceConfig::default()
    };
    let router = build_router(state_with(
        vec![(SourceLanguage::Tolk, MockBehavior::Succeed)],
        config,
    ));

    let items: Vec<_> = (0..5)
        .map(
            |i| serde_json::json!({"id": i.to_string(), "source": "contract Foo {}", "lang": "tolk"}),
        )
        .collect();
    let response = router
        .oneshot(post_json(
            "/v1/extract/batch",
            &serde_json::json!({"items": items}),
        ))
        .await
        .expect("router never errors");
    assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
    let json = body_json(response).await;
    assert_eq!(json["error"]["code"], "rate_limited");
}

#[tokio::test]
async fn health_is_never_rate_limited() {
    let config = ServiceConfig {
        rate_limit_requests: 1,
        rate_limit_window: Duration::from_secs(60),
        ..ServiceConfig::default()
    };
    let router = build_router(state_with(vec![], config));

    // Exhaust the (shared-IP) budget via /v1/extract first — a real
    // extractor isn't registered, so this fails for an unrelated reason,
    // but it still consumes a rate-limit token before the language check
    // runs, since rate-limiting is a router-level layer applied before the
    // handler.
    let _ = router
        .clone()
        .oneshot(post_json(
            "/v1/extract",
            &serde_json::json!({"source": "x", "lang": "tolk"}),
        ))
        .await
        .expect("router never errors");

    let health_response = router
        .oneshot(with_connect_info(
            Request::get("/v1/health")
                .body(Body::empty())
                .expect("valid request"),
        ))
        .await
        .expect("router never errors");
    assert_eq!(health_response.status(), StatusCode::OK);
}
