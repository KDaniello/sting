//! End-to-end tests of the real `abi-layer` binary as a subprocess: it runs
//! end to end and prints a valid `ContractSchema` JSON, rather than only its
//! internals being unit-tested.
//!
//! Tolk/Tact cases need the real vendored compiler
//! (`vendor/tolk/`/`vendor/tact/`, both `.gitignore`d) — skip gracefully
//! if absent, same pattern as every other real-compiler test in this
//! workspace. The error-path cases (`--format tlb`, unknown extension)
//! need no compiler at all and always run.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "docs/ARCHITECTURE.md §3.2 scopes the no-unwrap/expect/panic policy to library code outside tests"
)]

use std::path::{Path, PathBuf};
use std::process::Command;

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_abi-layer")
}

#[test]
fn extract_a_real_tolk_contract_end_to_end() {
    let tolk_exe = repo_root()
        .join("vendor/tolk")
        .join(sting_tolkc::platform_executable_filename());
    if !tolk_exe.is_file() {
        println!(
            "SKIP: {} not present locally (see vendor/tolk/ in .gitignore)",
            tolk_exe.display()
        );
        return;
    }

    let output = Command::new(bin())
        .arg("extract")
        .arg(repo_root().join("corpus/clean/acton-contracts/counter/Counter.tolk"))
        .env("STING_TOLKC_PATH", &tolk_exe)
        .output()
        .expect("spawning the real abi-layer binary must succeed");

    assert!(
        output.status.success(),
        "abi-layer extract exited non-zero: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let schema: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("stdout must be valid ContractSchema JSON");
    assert_eq!(schema["source_language"], "tolk");
    assert_eq!(schema["contract_name"], "Counter");
}

#[test]
fn extract_a_real_tact_contract_end_to_end() {
    let tact_js = repo_root().join("vendor/tact/node_modules/@tact-lang/compiler/bin/tact.js");
    if !tact_js.is_file() {
        println!(
            "SKIP: {} not present locally (see vendor/tact/README.md)",
            tact_js.display()
        );
        return;
    }

    let output = Command::new(bin())
        .arg("extract")
        .arg(repo_root().join("crates/sting-abi-tact/tests/fixtures/opcodes.tact"))
        .env("STING_TACT_JS_PATH", &tact_js)
        .output()
        .expect("spawning the real abi-layer binary must succeed");

    assert!(
        output.status.success(),
        "abi-layer extract exited non-zero: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let schema: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("stdout must be valid ContractSchema JSON");
    assert_eq!(schema["source_language"], "tact");
    assert_eq!(schema["contract_name"], "Tester");
}

#[test]
fn extract_a_real_func_contract_end_to_end() {
    // No compiler to skip for — `FuncExtractor` is pure `tree-sitter`,
    // always available, unlike the Tolk/Tact cases above.
    let output = Command::new(bin())
        .arg("extract")
        .arg(repo_root().join("crates/sting-abi-func/tests/fixtures/jetton-wallet.fc"))
        .output()
        .expect("spawning the real abi-layer binary must succeed");

    assert!(
        output.status.success(),
        "abi-layer extract exited non-zero: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let schema: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("stdout must be valid ContractSchema JSON");
    assert_eq!(schema["source_language"], "func");
    assert_eq!(schema["extraction_tier"], "heuristic");
    assert!(
        schema["confidence"].is_number(),
        "Tier 2 schema must carry a confidence"
    );
}

#[test]
fn format_tlb_is_a_clean_error_not_a_panic() {
    // No compiler needed — --format tlb is rejected before any extraction
    // is attempted.
    let output = Command::new(bin())
        .arg("extract")
        .arg("whatever.tolk")
        .arg("--format")
        .arg("tlb")
        .output()
        .expect("spawning the real abi-layer binary must succeed");

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("not yet implemented"),
        "expected a clear 'not yet implemented' message, got: {stderr}"
    );
}

#[test]
fn unknown_extension_without_explicit_lang_is_a_clean_error() {
    let output = Command::new(bin())
        .arg("extract")
        .arg("whatever.txt")
        .output()
        .expect("spawning the real abi-layer binary must succeed");

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("could not determine the source language"),
        "expected an UnknownLanguage message, got: {stderr}"
    );
}
