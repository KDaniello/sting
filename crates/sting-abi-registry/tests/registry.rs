//! `RegistryController` tests — real Tact compilation, real byte
//! comparison logic, but no live network call: `ContractTarget::LocalBoc`
//! (the same interface `CorpusController::resolve` already exposes for
//! exactly this reason) reads "on-chain" code from a local, vendored
//! fixture file instead of a real address — matching
//! `docs/domains/service.md` principle 1
//! ("testability without real external systems").
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "docs/ARCHITECTURE.md §3.2 scopes the no-unwrap/expect/panic policy to library code outside tests"
)]

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64;

use sting_abi_core::{Extractor, SourceLanguage};
use sting_abi_pipeline::ExtractController;
use sting_abi_registry::{
    BytecodeSource, MatchStatus, RegistryController, TactBytecodeSource, TolkBytecodeSource,
};
use sting_abi_tact::{TactCompiler, TactExtractor};
use sting_abi_tolk::TolkExtractor;
use sting_corpus::{ContractTarget, CorpusController, DiskCache};
use sting_fift::FiftAssembler;
use sting_tolkc::TolkCompiler;

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn tact_compiler_or_skip() -> Option<TactCompiler> {
    let script = repo_root().join("vendor/tact/node_modules/@tact-lang/compiler/bin/tact.js");
    if !script.is_file() {
        println!(
            "SKIP: {} not present locally (see vendor/tact/README.md)",
            script.display()
        );
        return None;
    }
    Some(TactCompiler::new(script))
}

/// Builds real `tolk`/`fift` handles for [`TolkBytecodeSource`], or prints
/// a message and returns `None` if either vendored binary is not present
/// locally — mirrors [`tact_compiler_or_skip`]'s identical reasoning.
fn tolk_and_fift_or_skip() -> Option<(TolkCompiler, FiftAssembler)> {
    let tolk_path = repo_root()
        .join("vendor/tolk")
        .join(sting_tolkc::platform_executable_filename());
    let fift_path = repo_root()
        .join("vendor/fift")
        .join(sting_fift::platform_executable_filename());
    if !tolk_path.is_file() {
        println!(
            "SKIP: {} not present locally (see vendor/tolk/ in .gitignore)",
            tolk_path.display()
        );
        return None;
    }
    if !fift_path.is_file() {
        println!(
            "SKIP: {} not present locally (see vendor/fift/ in .gitignore)",
            fift_path.display()
        );
        return None;
    }
    Some((TolkCompiler::new(tolk_path), FiftAssembler::new(fift_path)))
}

fn tolk_config_project_dir() -> PathBuf {
    repo_root().join("corpus/clean/acton-contracts/config")
}

/// A `CorpusController` whose `sources` are never actually invoked —
/// every test here uses `ContractTarget::LocalBoc`, which
/// `CorpusController::resolve` reads directly from disk without touching
/// `sources`/`cache` at all (confirmed by reading `sting-pipeline`'s own
/// `resolve`/`read_local_boc`).
fn local_only_corpus_controller() -> (CorpusController, tempfile::TempDir) {
    let cache_dir = tempfile::tempdir().expect("scratch cache dir");
    let controller = CorpusController::new(
        Vec::new(),
        DiskCache::new(cache_dir.path().to_path_buf(), sting_corpus::DEFAULT_TTL),
    );
    (controller, cache_dir)
}

/// Writes `code` as a `LocalBoc` fixture pair (data cell content is
/// irrelevant to this crate's comparison logic, which only ever reads
/// `code_boc_base64` — an empty data file is a real, valid `LocalBoc`,
/// not a shortcut around validation).
fn write_local_boc(dir: &Path, code: &[u8]) -> ContractTarget {
    let code_path = dir.join("on_chain.code.boc.base64");
    let data_path = dir.join("on_chain.data.boc.base64");
    std::fs::write(&code_path, BASE64.encode(code)).expect("write code fixture");
    std::fs::write(&data_path, BASE64.encode([])).expect("write data fixture");
    ContractTarget::LocalBoc {
        code: code_path,
        data: data_path,
    }
}

#[test]
fn full_match_is_recognized_and_returns_a_schema() {
    let Some(compiler) = tact_compiler_or_skip() else {
        return;
    };
    let fixtures_dir = repo_root().join("crates/sting-abi-tact/tests/fixtures");
    let entrypoint = Path::new("opcodes.tact");

    // "On-chain" code — really just this exact source compiled once ahead
    // of time and saved as a local fixture, so the test needs no network.
    let onchain_output = compiler
        .compile(&fixtures_dir, entrypoint)
        .expect("real fixture must compile");
    let (corpus, _cache_dir) = local_only_corpus_controller();
    let scratch = tempfile::tempdir().expect("scratch dir");
    let target = write_local_boc(scratch.path(), &onchain_output.code_boc);

    let mut bytecode_sources: HashMap<SourceLanguage, Box<dyn BytecodeSource>> = HashMap::new();
    bytecode_sources.insert(
        SourceLanguage::Tact,
        Box::new(TactBytecodeSource::new(compiler)),
    );

    let mut extractors: HashMap<SourceLanguage, Box<dyn Extractor>> = HashMap::new();
    extractors.insert(
        SourceLanguage::Tact,
        Box::new(TactExtractor::new(TactCompiler::new(repo_root().join(
            "vendor/tact/node_modules/@tact-lang/compiler/bin/tact.js",
        )))),
    );
    let extract = ExtractController::new(extractors);

    let registry = RegistryController::new(&corpus, &bytecode_sources, &extract);
    let outcome = registry
        .verify(&target, &fixtures_dir, entrypoint, SourceLanguage::Tact)
        .expect("verification must succeed");

    assert_eq!(outcome.status, MatchStatus::Full);
    let schema = outcome.schema.expect("Full match must carry a schema");
    assert_eq!(schema.source_language, SourceLanguage::Tact);
    assert_eq!(schema.contract_name.as_deref(), Some("Tester"));
}

#[test]
fn no_match_is_recognized_and_carries_no_schema() {
    let Some(compiler) = tact_compiler_or_skip() else {
        return;
    };
    let fixtures_dir = repo_root().join("crates/sting-abi-tact/tests/fixtures");
    let entrypoint = Path::new("opcodes.tact");

    // Deliberately mismatched "on-chain" code — a handful of bytes that is
    // never going to equal a real compiled BOC.
    let (corpus, _cache_dir) = local_only_corpus_controller();
    let scratch = tempfile::tempdir().expect("scratch dir");
    let target = write_local_boc(scratch.path(), b"not a real BOC at all");

    let mut bytecode_sources: HashMap<SourceLanguage, Box<dyn BytecodeSource>> = HashMap::new();
    bytecode_sources.insert(
        SourceLanguage::Tact,
        Box::new(TactBytecodeSource::new(compiler)),
    );

    let extractors: HashMap<SourceLanguage, Box<dyn Extractor>> = HashMap::new();
    let extract = ExtractController::new(extractors);

    let registry = RegistryController::new(&corpus, &bytecode_sources, &extract);
    let outcome = registry
        .verify(&target, &fixtures_dir, entrypoint, SourceLanguage::Tact)
        .expect("verification must succeed even on a mismatch — NoMatch is a value, not an error");

    assert_eq!(outcome.status, MatchStatus::NoMatch);
    assert!(
        outcome.schema.is_none(),
        "an unverified source's schema must never be returned"
    );
}

#[test]
fn requesting_verification_for_func_is_a_named_error_not_a_silent_no_match() {
    // `FunC` (Tier 2) is the one language category that is *categorically*
    // never verifiable — no real compiler to recompile against at all
    // (`sting_abi_registry::bytecode`'s own module doc comment). Tolk used
    // to be this test's example until that gap closed; `FunC` is the honest,
    // still-true example now.
    let (corpus, _cache_dir) = local_only_corpus_controller();
    let scratch = tempfile::tempdir().expect("scratch dir");
    let target = write_local_boc(scratch.path(), b"irrelevant, never reached");

    let bytecode_sources: HashMap<SourceLanguage, Box<dyn BytecodeSource>> = HashMap::new();
    let extractors: HashMap<SourceLanguage, Box<dyn Extractor>> = HashMap::new();
    let extract = ExtractController::new(extractors);

    let registry = RegistryController::new(&corpus, &bytecode_sources, &extract);
    let result = registry.verify(
        &target,
        Path::new("."),
        Path::new("whatever.fc"),
        SourceLanguage::Func,
    );

    assert!(matches!(
        result,
        Err(sting_abi_registry::RegistryError::NoBytecodeSourceForLanguage(SourceLanguage::Func))
    ));
}

#[test]
fn tolk_full_match_is_recognized_and_returns_a_schema() {
    let Some((tolk, fift)) = tolk_and_fift_or_skip() else {
        return;
    };
    let project_dir = tolk_config_project_dir();
    let entrypoint = Path::new("Config.tolk");

    // "On-chain" code — no live Tolk contract is confirmed deployed on
    // mainnet, so this
    // fixture is this exact source compiled+assembled once ahead of time
    // and saved as a local fixture, honestly documented as such, not
    // presented as a real mainnet address.
    let onchain_boc = {
        let compiled = tolk
            .compile_keep_scratch(&project_dir, entrypoint)
            .expect("real fixture must compile");
        fift.assemble(&compiled.fift_path)
            .expect("real fixture must assemble")
    };
    let (corpus, _cache_dir) = local_only_corpus_controller();
    let scratch = tempfile::tempdir().expect("scratch dir");
    let target = write_local_boc(scratch.path(), &onchain_boc);

    let mut bytecode_sources: HashMap<SourceLanguage, Box<dyn BytecodeSource>> = HashMap::new();
    bytecode_sources.insert(
        SourceLanguage::Tolk,
        Box::new(TolkBytecodeSource::new(tolk, fift)),
    );

    let mut extractors: HashMap<SourceLanguage, Box<dyn Extractor>> = HashMap::new();
    let extractor_compiler = TolkCompiler::new(
        repo_root()
            .join("vendor/tolk")
            .join(sting_tolkc::platform_executable_filename()),
    );
    extractors.insert(
        SourceLanguage::Tolk,
        Box::new(TolkExtractor::new(extractor_compiler)),
    );
    let extract = ExtractController::new(extractors);

    let registry = RegistryController::new(&corpus, &bytecode_sources, &extract);
    let outcome = registry
        .verify(&target, &project_dir, entrypoint, SourceLanguage::Tolk)
        .expect("verification must succeed");

    assert_eq!(outcome.status, MatchStatus::Full);
    let schema = outcome.schema.expect("Full match must carry a schema");
    assert_eq!(schema.source_language, SourceLanguage::Tolk);
}

#[test]
fn tolk_no_match_is_recognized_and_carries_no_schema() {
    let Some((tolk, fift)) = tolk_and_fift_or_skip() else {
        return;
    };
    let project_dir = tolk_config_project_dir();
    let entrypoint = Path::new("Config.tolk");

    // Deliberately mismatched "on-chain" code — never going to equal a
    // real compiled+assembled BoC.
    let (corpus, _cache_dir) = local_only_corpus_controller();
    let scratch = tempfile::tempdir().expect("scratch dir");
    let target = write_local_boc(scratch.path(), b"not a real BOC at all");

    let mut bytecode_sources: HashMap<SourceLanguage, Box<dyn BytecodeSource>> = HashMap::new();
    bytecode_sources.insert(
        SourceLanguage::Tolk,
        Box::new(TolkBytecodeSource::new(tolk, fift)),
    );

    let extractors: HashMap<SourceLanguage, Box<dyn Extractor>> = HashMap::new();
    let extract = ExtractController::new(extractors);

    let registry = RegistryController::new(&corpus, &bytecode_sources, &extract);
    let outcome = registry
        .verify(&target, &project_dir, entrypoint, SourceLanguage::Tolk)
        .expect("verification must succeed even on a mismatch — NoMatch is a value, not an error");

    assert_eq!(outcome.status, MatchStatus::NoMatch);
    assert!(
        outcome.schema.is_none(),
        "an unverified source's schema must never be returned"
    );
}
