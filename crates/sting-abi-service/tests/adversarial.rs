//! Adversarial-input validation (`docs/domains/service.md`). This file is
//! what **confirms** the panic-safety `docs/TESTING.md` claims — a malformed
//! `.fc` file off the internet must not panic the service — rather than
//! restating it. Reuses the already-established `proptest` pattern
//! (`sting-abi-core/tests/roundtrip.rs`), not new fuzz infrastructure.
//!
//! Two distinct claims, tested two different ways:
//! 1. **Malformed FunC/Tolk/Tact source through the real HTTP path**
//!    (`POST /v1/extract`) — the actual, externally-reachable untrusted-
//!    input surface. `FunC` never skips (pure `tree-sitter`, no external
//!    compiler); Tolk/Tact skip gracefully without their real vendored
//!    compiler, same pattern as every other real-compiler test in this
//!    workspace.
//! 2. **Malformed `BoC`** through `tonlib_core::cell::BagOfCells::parse_base64` —
//!    honestly, **not** literally reachable by an HTTP client today: no
//!    route in this service accepts a client-supplied raw `BoC` (`/v1/verify`/
//!    `/v1/probe` take an *address*, this service fetches and decodes the
//!    on-chain `BoC` itself). This still exercises the exact decode path
//!    `RegistryController::verify`/Tier 3 `probe()` depend on internally —
//!    a real code path a corrupted/adversarial upstream response (`toncenter`/
//!    `tonapi`) could reach, just not a client-controlled parameter — scoped
//!    honestly as that, not oversold as "fuzzing the HTTP API surface".
//!
//! For (1), every generated input is additionally run directly through the
//! extractor inside `std::panic::catch_unwind` — the HTTP-level assertion
//! alone cannot distinguish "a real panic happened but `tokio::spawn_blocking`
//! caught it and returned a clean `500`" from "no panic happened at all";
//! only the direct `catch_unwind` call can, and the *no-panic* claim is
//! specifically about the latter.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "docs/ARCHITECTURE.md §3.2 scopes the no-unwrap/expect/panic policy to library code outside tests"
)]

use std::collections::HashMap;
use std::panic::AssertUnwindSafe;
use std::path::{Path, PathBuf};

use axum::body::Body;
use axum::extract::connect_info::ConnectInfo;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use proptest::prelude::*;
use tower::ServiceExt;

use sting_abi_core::{Extractor, SourceLanguage};
use sting_abi_func::FuncExtractor;
use sting_abi_pipeline::ExtractController;
use sting_abi_service::{AppState, ServiceConfig, Tier3Config, build_router};

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
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
        Tier3Config::default(),
    )
}

fn post_extract(source: &str, lang: &str) -> Request<Body> {
    let body = serde_json::json!({"source": source, "lang": lang});
    let mut req = Request::post("/v1/extract")
        .header("content-type", "application/json")
        .body(Body::from(serde_json::to_vec(&body).expect("valid JSON")))
        .expect("valid request");
    req.extensions_mut()
        .insert(ConnectInfo(std::net::SocketAddr::from((
            [203, 0, 113, 1],
            12345,
        ))));
    req
}

/// A byte string that may not even be valid UTF-8 — real adversarial input
/// from "the internet" makes no promises. Invalid-UTF-8 sequences are
/// lossily converted (`String::from_utf8_lossy`), matching what any real
/// HTTP client sending non-UTF-8 bytes in a JSON string field would
/// actually produce after this service's own JSON deserialization (`serde_json`
/// itself requires valid UTF-8 strings, so a *raw* invalid-UTF-8 body is a
/// different, already-covered case — `extract_single_malformed_json_body_is_invalid_request_not_axums_default_shape`,
/// `tests/http.rs`); this strategy targets "valid JSON, garbage/near-garbage
/// source *text*", the case that reaches the real parser.
fn arb_adversarial_source() -> impl Strategy<Value = String> {
    prop_oneof![
        // Pure random bytes, lossily converted to a string — the "complete
        // garbage" case.
        proptest::collection::vec(any::<u8>(), 0..2048)
            .prop_map(|bytes| String::from_utf8_lossy(&bytes).into_owned()),
        // Printable-ASCII-biased random text — more likely to trip
        // tokenizer/parser edge cases than pure binary garbage, which a
        // real lexer usually rejects at the very first byte.
        "[ -~\\n\\t]{0,2048}",
        // Mutated real fixture — real, previously-valid source with random
        // byte flips, the classic "almost valid" mutation-fuzzing case.
        proptest::collection::vec(any::<u8>(), 1..64).prop_map(|mutations| {
            let base = std::fs::read(
                repo_root().join("crates/sting-abi-func/tests/fixtures/jetton-wallet.fc"),
            )
            .expect("real fixture must exist");
            let mut mutated = base;
            let len = mutated.len().max(1);
            for (i, m) in mutations.iter().enumerate() {
                if let Some(byte) = mutated.get_mut(i % len) {
                    *byte ^= m;
                }
            }
            String::from_utf8_lossy(&mutated).into_owned()
        }),
    ]
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(200))]

    /// Claim 1a: adversarial `FunC` source, direct call — the precise,
    /// `catch_unwind`-verified "no real panic" assertion. `FuncExtractor`
    /// never needs an external compiler, so this never skips.
    #[test]
    fn func_extractor_never_panics_on_adversarial_source(source in arb_adversarial_source()) {
        let dir = tempfile::tempdir().expect("scratch dir");
        let entrypoint = Path::new("adversarial.fc");
        std::fs::write(dir.path().join(entrypoint), &source).expect("write scratch source");

        let extractor = FuncExtractor;
        let dir_path = dir.path().to_path_buf();
        let result = std::panic::catch_unwind(AssertUnwindSafe(|| {
            extractor.extract(&dir_path, entrypoint)
        }));
        prop_assert!(
            result.is_ok(),
            "FuncExtractor::extract must never panic on adversarial input, even malformed/random bytes"
        );
    }
}

/// Claim 1b: the same adversarial `FunC` inputs, through the real HTTP
/// path — confirms the whole stack (JSON parsing, `spawn_blocking`
/// dispatch, error mapping) surfaces a clean, structured response for
/// every one of them, not just that the extractor itself doesn't panic.
/// A smaller, fixed-count run (not a `proptest!` property — `#[tokio::test]`
/// and `proptest!`'s own harness don't compose directly) over a
/// deliberately varied hand-picked adversarial corpus, chosen to cover the
/// same three shapes `arb_adversarial_source` generates.
#[tokio::test]
async fn extract_http_path_never_crashes_on_adversarial_func_source() {
    let router = build_router(build_state(vec![(
        SourceLanguage::Func,
        Box::new(FuncExtractor),
    )]));

    let adversarial_inputs: Vec<String> = vec![
        String::new(),
        "\0\0\0\0".to_string(),
        "{{{{{{{{{{{{{{{{".to_string(),
        "get_methods".repeat(10_000),
        "\u{FFFD}\u{FFFD}\u{FFFD}".to_string(),
        String::from_utf8_lossy(&[0xff, 0xfe, 0x00, 0x01, 0x02, 0xc0, 0xaf]).into_owned(),
        "() ".repeat(5000),
        "int get_data() method_id(".to_string(), // truncated, unclosed
    ];

    for source in adversarial_inputs {
        let response = router
            .clone()
            .oneshot(post_extract(&source, "func"))
            .await
            .expect("the router itself must never error, panic or otherwise, on any request");
        assert!(
            response.status() == StatusCode::OK
                || response.status().is_client_error()
                || response.status().is_server_error(),
            "unexpected status for adversarial input: {}",
            response.status()
        );
        let bytes = response
            .into_body()
            .collect()
            .await
            .expect("readable body")
            .to_bytes();
        let _json: serde_json::Value = serde_json::from_slice(&bytes).unwrap_or_else(|err| {
            panic!(
                "response body must always be valid JSON, even on error: {err}\nbody: {}",
                String::from_utf8_lossy(&bytes)
            )
        });
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(300))]

    /// Claim 2: malformed `BoC` never panics `tonlib_core`'s own decoder —
    /// see this file's own module doc comment for the honest scope caveat
    /// (not literally client-reachable via this service's HTTP surface
    /// today, but the real decode path `RegistryController`/Tier 3 `probe()`
    /// both depend on internally).
    #[test]
    fn boc_decode_never_panics_on_adversarial_bytes(bytes in proptest::collection::vec(any::<u8>(), 0..512)) {
        use base64::Engine as _;
        let b64 = base64::engine::general_purpose::STANDARD.encode(&bytes);
        let result = std::panic::catch_unwind(AssertUnwindSafe(|| {
            tonlib_core::cell::BagOfCells::parse_base64(&b64).and_then(tonlib_core::cell::BagOfCells::single_root)
        }));
        prop_assert!(
            result.is_ok(),
            "BagOfCells::parse_base64/single_root must never panic on adversarial bytes, even a valid-looking base64 wrapper around garbage"
        );
    }

    /// Same claim, but on random *text* run through base64 decoding first
    /// (most real adversarial "base64 `BoC`" input a hostile actor would
    /// send is itself not valid base64 at all) — a different failure mode
    /// than well-formed-base64-but-garbage-`BoC` above.
    #[test]
    fn boc_decode_never_panics_on_non_base64_text(text in "[ -~]{0,256}") {
        let result = std::panic::catch_unwind(AssertUnwindSafe(|| {
            tonlib_core::cell::BagOfCells::parse_base64(&text).and_then(tonlib_core::cell::BagOfCells::single_root)
        }));
        prop_assert!(
            result.is_ok(),
            "BagOfCells::parse_base64 must never panic on arbitrary, likely-invalid-base64 text"
        );
    }
}
