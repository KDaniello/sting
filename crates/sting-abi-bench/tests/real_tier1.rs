//! Real-compiler Tier 1 completeness check — runs [`sting_abi_bench::tier1`]
//! against real, already-vendored corpus fixtures (not just the synthetic
//! unit tests in `src/tier1.rs`), confirming the "expected 100%" bar
//! (`docs/domains/precision-sources.md`) actually holds on real
//! compiler output, not just hand-crafted JSON. Skips gracefully (same
//! pattern as every other real-compiler test in this workspace) when the
//! relevant vendored toolchain isn't present locally.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "docs/ARCHITECTURE.md §3.2 scopes the no-unwrap/expect/panic policy to library code outside tests"
)]

use std::path::{Path, PathBuf};

use sting_abi_bench::tier1::{score_tact, score_tolk};
use sting_abi_tact::TactCompiler;
use sting_tolkc::TolkCompiler;

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn tact_compiler_or_skip() -> Option<TactCompiler> {
    let script = repo_root().join("vendor/tact/node_modules/@tact-lang/compiler/bin/tact.js");
    if !script.is_file() {
        println!("SKIP: {} not present locally", script.display());
        return None;
    }
    Some(TactCompiler::new(script))
}

fn tolk_compiler_or_skip() -> Option<TolkCompiler> {
    let path = repo_root()
        .join("vendor/tolk")
        .join(sting_tolkc::platform_executable_filename());
    if !path.is_file() {
        println!("SKIP: {} not present locally", path.display());
        return None;
    }
    Some(TolkCompiler::new(path))
}

/// Every real Tact fixture vendored under
/// `crates/sting-abi-tact/tests/fixtures/` must score full
/// completeness on every category it actually populates: 100% is the bar and
/// any gap is an open defect, checked against real rather than synthetic
/// compiler output.
#[test]
fn every_vendored_tact_fixture_scores_full_completeness() {
    let Some(compiler) = tact_compiler_or_skip() else {
        return;
    };
    let fixtures_dir = repo_root().join("crates/sting-abi-tact/tests/fixtures");
    for entrypoint in ["getters.tact", "message.tact", "opcodes.tact"] {
        let output = compiler
            .compile(&fixtures_dir, Path::new(entrypoint))
            .unwrap_or_else(|err| panic!("{entrypoint} must compile: {err}"));
        let schema = sting_abi_tact::abi_to_schema(&output.abi);
        let result = score_tact(entrypoint, &output.abi, &schema);

        for (label, score) in [
            ("storage", result.storage),
            ("get_methods", result.get_methods),
            ("errors", result.errors),
            ("incoming_internal", result.incoming_internal),
            ("incoming_external", result.incoming_external),
        ] {
            if let Some(ratio) = score.ratio() {
                assert!(
                    (ratio - 1.0).abs() < f64::EPSILON,
                    "{entrypoint}/{label}: {ratio} (gaps: {:?})",
                    result.gaps
                );
            }
        }
    }
}

/// Same bar, for the real vendored Tolk `Config.tolk` fixture (already used
/// live elsewhere) — Tolk's own mapping is unconditional (no
/// filtering step the way Tact's text-receiver gap is), so 100% here is the
/// expected, not aspirational, result.
#[test]
fn config_tolk_scores_full_completeness() {
    let Some(compiler) = tolk_compiler_or_skip() else {
        return;
    };
    let project_dir = repo_root().join("corpus/clean/acton-contracts/config");
    let output = compiler
        .compile(&project_dir, Path::new("Config.tolk"))
        .expect("Config.tolk must compile");
    let schema = sting_abi_tolk::abi_to_schema(&output.abi);
    let result = score_tolk("Config.tolk", &output.abi, &schema);

    for (label, score) in [
        ("storage", result.storage),
        ("get_methods", result.get_methods),
        ("errors", result.errors),
        ("incoming_internal", result.incoming_internal),
        ("incoming_external", result.incoming_external),
        ("outgoing", result.outgoing),
    ] {
        if let Some(ratio) = score.ratio() {
            assert!(
                (ratio - 1.0).abs() < f64::EPSILON,
                "Config.tolk/{label}: {ratio} (gaps: {:?})",
                result.gaps
            );
        }
    }
}
