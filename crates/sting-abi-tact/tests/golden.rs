//! Golden/snapshot tests (`docs/TESTING.md`) — real
//! Tact contracts from `tests/fixtures/` (provenance:
//! `tests/fixtures/SOURCE.md`), compared against
//! `tests/fixtures/golden/*.expected.json` snapshots produced by running
//! this exact extractor once and hand-reviewing the output before
//! committing it — the compiler *is* the ground truth for Tier 1
//! (`critical/BENCHMARKS.md` §1.1), so review confirms the normalization
//! is correct, not that some independent oracle agrees.
//!
//! Unlike `sting-abi-tolk`'s golden test (which reads a single vendored
//! binary), this one needs a real `node` + the `vendor/tact/` npm install
//! — skips gracefully if either is missing, same pattern.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "docs/ARCHITECTURE.md §3.2 scopes the no-unwrap/expect/panic policy to library code outside tests"
)]

use std::path::{Path, PathBuf};

use sting_abi_core::{ContractSchema, Extractor};
use sting_abi_tact::{TactCompiler, TactExtractor};

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn extractor_or_skip() -> Option<TactExtractor> {
    let script = repo_root().join("vendor/tact/node_modules/@tact-lang/compiler/bin/tact.js");
    if !script.is_file() {
        println!(
            "SKIP: {} not present locally (see vendor/tact/README.md) — \
             run `npm install` in vendor/tact/ to run this test for real",
            script.display()
        );
        return None;
    }
    Some(TactExtractor::new(TactCompiler::new(script)))
}

fn load_expected(name: &str) -> ContractSchema {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/golden")
        .join(format!("{name}.expected.json"));
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("reading {}: {e}", path.display()));
    serde_json::from_str(&text)
        .unwrap_or_else(|e| panic!("{} is not a valid ContractSchema: {e}", path.display()))
}

fn assert_matches_golden(extractor: &TactExtractor, entrypoint: &str) {
    let fixtures_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let schema = extractor
        .extract(&fixtures_dir, Path::new(entrypoint))
        .unwrap_or_else(|e| panic!("extracting {entrypoint}: {e}"));
    schema.validate().unwrap_or_else(|e| {
        panic!("{entrypoint}'s extracted schema violates its own invariant: {e}")
    });

    let golden_name = entrypoint.trim_end_matches(".tact");
    let expected = load_expected(golden_name);
    assert_eq!(
        schema, expected,
        "{entrypoint}'s extracted schema no longer matches tests/fixtures/golden/{golden_name}.expected.json — \
         if this is an intentional extractor change, regenerate and re-review the golden file, don't just update the assertion"
    );
}

#[test]
fn five_real_contracts_match_their_reviewed_golden_schema() {
    let Some(extractor) = extractor_or_skip() else {
        return;
    };

    assert_matches_golden(&extractor, "getters.tact");
    assert_matches_golden(&extractor, "message.tact");
    assert_matches_golden(&extractor, "opcodes.tact");
    assert_matches_golden(&extractor, "nested_struct.tact");
    assert_matches_golden(&extractor, "map_comparison.tact");
}

#[test]
fn entrypoint_in_a_subdirectory_extracts_correctly() {
    // Regression test for a real bug found by review: TactCompiler's
    // artifact search only looked in the scratch copy's *root*, so any
    // entrypoint with a directory component (`contracts/main.tact` — the
    // standard Blueprint/Tact project layout, not an edge case) failed
    // with a spurious MissingArtifact error even though compilation
    // itself genuinely succeeded. tests/fixtures/nested_project/ exists
    // solely to keep this exercised going forward.
    let Some(extractor) = extractor_or_skip() else {
        return;
    };
    let project_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/nested_project");
    let schema = extractor
        .extract(&project_dir, Path::new("contracts/main.tact"))
        .expect("a subdirectory entrypoint must extract successfully, not just a flat one");
    assert_eq!(schema.contract_name.as_deref(), Some("Tester"));
}
