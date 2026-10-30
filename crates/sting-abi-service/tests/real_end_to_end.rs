//! Real-compiler end-to-end tests: real Tolk and real Tact source through the
//! real router via `tower::ServiceExt::oneshot` (still no TCP socket,
//! `docs/TESTING.md`), not just unit-tested handler
//! logic. `tests/http.rs` covers routing/error-format/timeout/rate-limit
//! with mocks; this file is the complement.
//!
//! Needs the real vendored compilers (`vendor/tolk/`/`vendor/tact/`, both
//! `.gitignore`d) — skips gracefully if absent, same pattern as every
//! other real-compiler test in this workspace
//! (`crates/sting-abi-cli/tests/extract.rs`).
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "docs/ARCHITECTURE.md §3.2 scopes the no-unwrap/expect/panic policy to library code outside tests"
)]

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use axum::body::Body;
use axum::extract::connect_info::ConnectInfo;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use tower::ServiceExt;

use sting_abi_core::{Extractor, SourceLanguage};
use sting_abi_dictionary::MethodDictionary;
use sting_abi_func::FuncExtractor;
use sting_abi_pipeline::ExtractController;
use sting_abi_service::{AppState, ServiceConfig, build_router};
use sting_abi_tact::{TactCompiler, TactExtractor};
use sting_abi_tolk::TolkExtractor;
use sting_corpus::{ContractSource, CorpusController, DiskCache, TonapiClient, ToncenterClient};
use sting_emulator_sys::LibEmulator;
use sting_tolkc::TolkCompiler;

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// A standalone, single-file Tolk contract with no `import`s — every real
/// Tier 1 golden fixture in `crates/sting-abi-tolk/tests/fixtures/` is a
/// multi-file project (they import shared `types`/`errors`/`storage`
/// modules), which cannot round-trip through this service's inline
/// `{source}` body (no sibling files exist to resolve against). Reused
/// verbatim from `corpus/fixtures/bridge/` — already proven to compile
/// standalone by the security branch's own dynamic-verification tests
/// (`sting-pipeline/tests/gvr_cross_file_real_tolk.rs` compiles and runs
/// this exact file through the real emulator).
fn standalone_tolk_source() -> String {
    std::fs::read_to_string(repo_root().join("corpus/fixtures/bridge/GvrShadowPattern.tolk"))
        .expect("fixture file must exist")
}

fn standalone_tact_source() -> String {
    std::fs::read_to_string(repo_root().join("crates/sting-abi-tact/tests/fixtures/opcodes.tact"))
        .expect("fixture file must exist")
}

fn tolk_extractor_or_skip() -> Option<(SourceLanguage, Box<dyn Extractor>)> {
    let path = repo_root()
        .join("vendor/tolk")
        .join(sting_tolkc::platform_executable_filename());
    if !path.is_file() {
        println!(
            "SKIP: {} not present locally (see vendor/tolk/ in .gitignore)",
            path.display()
        );
        return None;
    }
    Some((
        SourceLanguage::Tolk,
        Box::new(TolkExtractor::new(TolkCompiler::new(path))),
    ))
}

fn tact_extractor_or_skip() -> Option<(SourceLanguage, Box<dyn Extractor>)> {
    let script = repo_root().join("vendor/tact/node_modules/@tact-lang/compiler/bin/tact.js");
    if !script.is_file() {
        println!(
            "SKIP: {} not present locally (see vendor/tact/README.md)",
            script.display()
        );
        return None;
    }
    Some((
        SourceLanguage::Tact,
        Box::new(TactExtractor::new(TactCompiler::new(script))),
    ))
}

fn build_state(extractors: Vec<(SourceLanguage, Box<dyn Extractor>)>) -> AppState {
    let map: HashMap<SourceLanguage, Box<dyn Extractor>> = extractors.into_iter().collect();
    let store_dir = tempfile::tempdir().expect("scratch store dir");
    AppState::new(
        ExtractController::new(map),
        ServiceConfig::default(),
        None,
        HashMap::new(),
        sting_abi_registry::VerifiedSchemaStore::new(store_dir.keep()),
        sting_abi_service::Tier3Config::default(),
    )
}

fn dictionary_or_skip() -> Option<MethodDictionary> {
    let path = repo_root().join("corpus-abi/method-dictionary.json");
    match MethodDictionary::load(&path) {
        Ok(dictionary) => Some(dictionary),
        Err(err) => {
            println!("SKIP: {} not loadable locally: {err}", path.display());
            None
        }
    }
}

fn emulator_lib_or_skip() -> Option<LibEmulator> {
    match sting_emulator::load_default_library() {
        Ok(lib) => {
            let _ = sting_emulator::set_verbosity_level(&lib, 0);
            Some(lib)
        }
        Err(err) => {
            println!(
                "SKIP: libemulator not present locally (see vendor/libemulator/ in .gitignore): {err}"
            );
            None
        }
    }
}

/// A real, network-backed `AppState` for `/v1/probe` — the same
/// `toncenter`/`tonapi` `CorpusController` every other real-address test in
/// this workspace uses (`sting-abi-dictionary`'s own CLI, `sting-abi-registry`'s
/// `tests/registry.rs`), plus the real Tier 3 dictionary/`libemulator`.
fn build_state_with_tier3(dictionary: MethodDictionary, emulator_lib: LibEmulator) -> AppState {
    let cache_root = sting_corpus::DiskCache::default_root().expect("platform cache dir");
    let sources: Vec<Box<dyn ContractSource>> = vec![
        Box::new(ToncenterClient::new()),
        Box::new(TonapiClient::new()),
    ];
    let corpus = CorpusController::new(
        sources,
        DiskCache::new(cache_root, sting_corpus::DEFAULT_TTL),
    );
    let store_dir = tempfile::tempdir().expect("scratch store dir");
    AppState::new(
        ExtractController::new(HashMap::new()),
        ServiceConfig::default(),
        Some(corpus),
        HashMap::new(),
        sting_abi_registry::VerifiedSchemaStore::new(store_dir.keep()),
        sting_abi_service::Tier3Config {
            dictionary: Some(dictionary),
            emulator_lib: Some(emulator_lib),
            operations: None,
        },
    )
}

fn post_json(path: &str, body: &serde_json::Value) -> Request<Body> {
    let mut req = Request::post(path)
        .header("content-type", "application/json")
        .body(Body::from(serde_json::to_vec(body).expect("valid JSON")))
        .expect("valid request");
    req.extensions_mut()
        .insert(ConnectInfo(std::net::SocketAddr::from((
            [203, 0, 113, 1],
            12345,
        ))));
    req
}

#[tokio::test]
async fn extract_a_real_tolk_contract_end_to_end() {
    let Some(tolk) = tolk_extractor_or_skip() else {
        return;
    };
    let router = build_router(build_state(vec![tolk]));

    let response = router
        .oneshot(post_json(
            "/v1/extract",
            &serde_json::json!({"source": standalone_tolk_source(), "lang": "tolk"}),
        ))
        .await
        .expect("router never errors");

    assert_eq!(response.status(), StatusCode::OK);
    let bytes = response
        .into_body()
        .collect()
        .await
        .expect("readable body")
        .to_bytes();
    let schema: serde_json::Value =
        serde_json::from_slice(&bytes).expect("valid ContractSchema JSON");
    assert_eq!(schema["source_language"], "tolk");
    assert_eq!(schema["contract_name"], "GvrShadowPattern");
}

#[tokio::test]
async fn extract_a_real_tact_contract_end_to_end() {
    let Some(tact) = tact_extractor_or_skip() else {
        return;
    };
    let router = build_router(build_state(vec![tact]));

    let response = router
        .oneshot(post_json(
            "/v1/extract",
            &serde_json::json!({"source": standalone_tact_source(), "lang": "tact"}),
        ))
        .await
        .expect("router never errors");

    assert_eq!(response.status(), StatusCode::OK);
    let bytes = response
        .into_body()
        .collect()
        .await
        .expect("readable body")
        .to_bytes();
    let schema: serde_json::Value =
        serde_json::from_slice(&bytes).expect("valid ContractSchema JSON");
    assert_eq!(schema["source_language"], "tact");
    assert_eq!(schema["contract_name"], "Tester");
}

/// A mixed batch — Tolk, Tact and one deliberately invalid item — where the
/// one failure does not break the others.
#[tokio::test]
async fn extract_batch_mixes_real_tolk_real_tact_and_one_deliberately_invalid_item() {
    let mut extractors = Vec::new();
    if let Some(tolk) = tolk_extractor_or_skip() {
        extractors.push(tolk);
    }
    if let Some(tact) = tact_extractor_or_skip() {
        extractors.push(tact);
    }
    if extractors.is_empty() {
        println!("SKIP: neither vendored compiler is present locally");
        return;
    }
    let have_tolk = extractors
        .iter()
        .any(|(lang, _)| *lang == SourceLanguage::Tolk);
    let have_tact = extractors
        .iter()
        .any(|(lang, _)| *lang == SourceLanguage::Tact);
    let router = build_router(build_state(extractors));

    let mut items = vec![serde_json::json!({
        "id": "invalid",
        "source": "this is not valid source in any supported language {{{",
        "lang": "tolk",
    })];
    if have_tolk {
        items.push(
            serde_json::json!({"id": "tolk", "source": standalone_tolk_source(), "lang": "tolk"}),
        );
    }
    if have_tact {
        items.push(
            serde_json::json!({"id": "tact", "source": standalone_tact_source(), "lang": "tact"}),
        );
    }

    let response = router
        .oneshot(post_json(
            "/v1/extract/batch",
            &serde_json::json!({"items": items}),
        ))
        .await
        .expect("router never errors");

    assert_eq!(response.status(), StatusCode::OK);
    let bytes = response
        .into_body()
        .collect()
        .await
        .expect("readable body")
        .to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&bytes).expect("valid JSON");
    let results = json["results"].as_array().expect("results array");

    let invalid = results
        .iter()
        .find(|r| r["id"] == "invalid")
        .expect("present");
    assert_eq!(
        invalid["status"], "error",
        "malformed source must not crash the batch"
    );

    if have_tolk {
        let tolk_result = results.iter().find(|r| r["id"] == "tolk").expect("present");
        assert_eq!(tolk_result["status"], "ok");
    }
    if have_tact {
        let tact_result = results.iter().find(|r| r["id"] == "tact").expect("present");
        assert_eq!(tact_result["status"], "ok");
    }
}

/// `FuncExtractor` needs no external compiler — unlike the Tolk/Tact tests
/// above, this never skips.
#[tokio::test]
async fn extract_a_real_func_contract_end_to_end() {
    let extractors: Vec<(SourceLanguage, Box<dyn Extractor>)> =
        vec![(SourceLanguage::Func, Box::new(FuncExtractor))];
    let router = build_router(build_state(extractors));

    let source = std::fs::read_to_string(
        repo_root().join("crates/sting-abi-func/tests/fixtures/jetton-wallet.fc"),
    )
    .expect("fixture file must exist");

    let response = router
        .oneshot(post_json(
            "/v1/extract",
            &serde_json::json!({"source": source, "lang": "func"}),
        ))
        .await
        .expect("router never errors");

    assert_eq!(response.status(), StatusCode::OK);
    let bytes = response
        .into_body()
        .collect()
        .await
        .expect("readable body")
        .to_bytes();
    let schema: serde_json::Value =
        serde_json::from_slice(&bytes).expect("valid ContractSchema JSON");
    assert_eq!(schema["source_language"], "func");
    assert_eq!(schema["extraction_tier"], "heuristic");
}

/// `POST /v1/verify` end-to-end with no `corpus`/network backend
/// configured (this file's `build_state` never wires one) — a clean
/// `503 registry_unavailable`, not a panic. The full `RegistryController`
/// logic (real Tact compilation, real byte comparison, full/no-match
/// classification) is already covered end-to-end with real compilation in
/// `sting-abi-registry`'s own `tests/registry.rs` — that crate builds its
/// own `CorpusController` over a local `ContractTarget::LocalBoc` fixture
/// specifically so it needs no live network call
/// (`docs/domains/service.md` principle 1).
/// This file's own `/v1/verify` route always constructs
/// `ContractTarget::OnChainAddress` (the real production shape), so a full
/// HTTP-layer `Full`-match test would need either a live network call or a
/// real deployed contract that deliberately was not created — see
/// `docs/domains/service.md` for why that step is flagged rather than silently
/// skipped.
#[tokio::test]
async fn verify_with_no_registry_configured_is_a_clean_error_not_a_panic() {
    let extractors: Vec<(SourceLanguage, Box<dyn Extractor>)> = Vec::new();
    let router = build_router(build_state(extractors));

    let response = router
        .oneshot(post_json(
            "/v1/verify",
            &serde_json::json!({"address": "EQsomeaddress", "source": "message(1) Foo {}", "lang": "tact"}),
        ))
        .await
        .expect("router never errors");

    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    let bytes = response
        .into_body()
        .collect()
        .await
        .expect("readable body")
        .to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&bytes).expect("valid JSON");
    assert_eq!(json["error"]["code"], "registry_unavailable");
}

/// `POST /v1/probe` end-to-end with no Tier 3 dictionary/`libemulator`
/// configured (this file's `build_state` never wires either) — a clean
/// `503 tier3_unavailable`, not a panic. Mirrors
/// `verify_with_no_registry_configured_is_a_clean_error_not_a_panic`'s
/// exact reasoning for the analogous gap.
#[tokio::test]
async fn probe_with_no_tier3_configured_is_a_clean_error_not_a_panic() {
    let router = build_router(build_state(Vec::new()));

    let response = router
        .oneshot(post_json(
            "/v1/probe",
            &serde_json::json!({"address": "EQsomeaddress"}),
        ))
        .await
        .expect("router never errors");

    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    let bytes = response
        .into_body()
        .collect()
        .await
        .expect("readable body")
        .to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&bytes).expect("valid JSON");
    assert_eq!(json["error"]["code"], "tier3_unavailable");
}

/// `POST /v1/probe` end-to-end against the real, live USDT jetton-master
/// address — the same address+dictionary already validated repeatedly this
/// address and dictionary the grail test and the profiler already use.
/// Real network fetch, real `libemulator` TVM calls, driven entirely
/// through the HTTP route (`tower::ServiceExt::oneshot`, no real socket).
#[tokio::test]
async fn probe_a_real_usdt_address_end_to_end() {
    let Some(dictionary) = dictionary_or_skip() else {
        return;
    };
    let Some(emulator_lib) = emulator_lib_or_skip() else {
        return;
    };
    let router = build_router(build_state_with_tier3(dictionary, emulator_lib));

    let response = router
        .oneshot(post_json(
            "/v1/probe",
            &serde_json::json!({"address": "EQCxE6mUtQJKFnGfaROTKOt1lZbDiiX1kCixRv7Nw2Id_sDs"}),
        ))
        .await
        .expect("router never errors");

    assert_eq!(response.status(), StatusCode::OK);
    let bytes = response
        .into_body()
        .collect()
        .await
        .expect("readable body")
        .to_bytes();
    let report: serde_json::Value = serde_json::from_slice(&bytes).expect("valid ProbeReport JSON");
    let recognized: Vec<&str> = report["recognized"]
        .as_array()
        .expect("recognized array")
        .iter()
        .map(|r| r["name"].as_str().expect("name field"))
        .collect();
    assert!(
        recognized.contains(&"get_jetton_data"),
        "real USDT jetton master must recognize get_jetton_data: {recognized:?}"
    );
    assert_eq!(
        report["kind_conflicts"]
            .as_array()
            .expect("kind_conflicts array")
            .len(),
        0,
        "USDT's own recognized methods must not conflict in ContractKind"
    );
}
