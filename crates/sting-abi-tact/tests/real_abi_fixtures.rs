//! Confirms the real, current `@tact-lang/compiler` CLI path produces real
//! `ContractABI` JSON, on real contracts, and records its exact shape.
//!
//! Deliberately does **not** shell out to `tact`/Node.js here — that
//! subprocess wrapper is `TactExtractor`'s job
//! (`docs/domains/extractors.md`), out of scope for
//! here, and would make `cargo test` depend on a Node.js toolchain being
//! installed, which nothing else in this workspace requires. What this test
//! does check: that the real compiler output already captured in
//! `tests/fixtures/*.abi` (byte-for-byte, produced exactly as
//! `tests/fixtures/SOURCE.md` documents, not hand-written) is well-formed
//! JSON of the shape that was found — so the shape stays a CI-checked fact
//! rather than a one-time claim in a doc comment.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "docs/ARCHITECTURE.md §3.2 scopes the no-unwrap/expect/panic policy to library code outside tests"
)]

use std::fs;
use std::path::Path;

use serde_json::Value;

fn fixtures_dir() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

fn load_abi(file_name: &str) -> Value {
    let path = fixtures_dir().join(file_name);
    let text =
        fs::read_to_string(&path).unwrap_or_else(|e| panic!("reading {}: {e}", path.display()));
    serde_json::from_str(&text)
        .unwrap_or_else(|e| panic!("{} is not valid JSON: {e}", path.display()))
}

#[test]
fn at_least_two_real_tact_abi_fixtures_are_committed() {
    let count = fs::read_dir(fixtures_dir())
        .expect("fixtures directory must exist")
        .filter_map(Result::ok)
        .filter(|entry| entry.path().extension().and_then(|e| e.to_str()) == Some("abi"))
        .count();
    assert!(
        count >= 2,
        "at least 2 real ContractABI fixtures are required (docs/domains/abi-ir.md); found {count}"
    );
}

#[test]
fn every_fixture_abi_has_the_documented_top_level_shape() {
    for file_name in [
        "getters.tact_Test.abi",
        "message.tact_MessageTester.abi",
        "opcodes.tact_Tester.abi",
    ] {
        let abi = load_abi(file_name);
        let obj = abi
            .as_object()
            .unwrap_or_else(|| panic!("{file_name}: top level must be a JSON object"));

        for key in [
            "name",
            "types",
            "receivers",
            "getters",
            "errors",
            "interfaces",
        ] {
            assert!(
                obj.contains_key(key),
                "{file_name}: missing top-level key {key:?}"
            );
        }
        assert!(
            obj["name"].is_string(),
            "{file_name}: \"name\" must be a string"
        );
        assert!(
            obj["types"].is_array(),
            "{file_name}: \"types\" must be an array"
        );
        assert!(
            obj["getters"].is_array(),
            "{file_name}: \"getters\" must be an array"
        );
        assert!(
            obj["errors"].is_object(),
            "{file_name}: \"errors\" must be an object keyed by exit code"
        );
    }
}

#[test]
fn every_getter_carries_a_populated_numeric_method_id() {
    // The specific, previously uncertain fact: docs/domains/abi-ir.md
    // originally assumed (reasoning
    // from the OTP-002 spec text, not observed output) that Tact's
    // `methodId` is sometimes absent. Real compiled output disagrees — the
    // compiler always fills it in, even for getters that never declared one
    // explicitly in source (`getters.tact`'s `testGetter` has no `get(N)`
    // annotation and still gets a real numeric id). Pin that fact here so a
    // future `@tact-lang/compiler` version silently changing it is caught.
    let abi = load_abi("getters.tact_Test.abi");
    let getters = abi["getters"]
        .as_array()
        .expect("\"getters\" must be an array");
    assert!(
        !getters.is_empty(),
        "getters.tact_Test.abi must have at least one getter"
    );

    for getter in getters {
        let name = getter["name"].as_str().expect("each getter has a \"name\"");
        assert!(
            getter["methodId"].is_number(),
            "getter {name:?} must carry a populated numeric \"methodId\""
        );
        assert!(
            getter.get("returnType").is_some(),
            "getter {name:?} must carry a \"returnType\""
        );
    }
}

#[test]
fn message_opcodes_are_populated_where_the_source_declares_them() {
    // opcodes.tact declares four messages with explicit binary/octal/
    // decimal/hex-literal opcodes — confirms the compiler surfaces the
    // resolved numeric opcode (not the literal's original radix/text) in
    // each type's "header" field.
    let abi = load_abi("opcodes.tact_Tester.abi");
    let types = abi["types"].as_array().expect("\"types\" must be an array");

    for (expected_name, expected_header) in [("Bin", 2), ("Oct", 8), ("Dec", 10), ("Hex", 16)] {
        let matching = types
            .iter()
            .find(|t| t["name"].as_str() == Some(expected_name))
            .unwrap_or_else(|| {
                panic!("opcodes.tact_Tester.abi must declare a type named {expected_name:?}")
            });
        assert_eq!(
            matching["header"].as_i64(),
            Some(expected_header),
            "type {expected_name:?} must carry the resolved numeric opcode {expected_header}"
        );
    }
}
