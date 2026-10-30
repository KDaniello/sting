//! Integration tests against real, vendored `tolk` + `fift` builds — the
//! full compile-to-bytecode pipeline this crate exists for
//! (`docs/domains/service.md`).
//!
//! `vendor/tolk/` and `vendor/fift/` are both `.gitignore`d — a fresh
//! checkout without them present is expected and not a failure, mirroring
//! `sting-tolkc`'s own `tests/real_compile.rs`. Every test here calls
//! [`pipeline_or_skip`] first and returns early when either binary is
//! absent, so `cargo test --workspace` stays green on a machine that never
//! fetched them.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "docs/ARCHITECTURE.md §3.2 scopes the no-unwrap/expect/panic policy to library code outside tests"
)]

use std::path::{Path, PathBuf};

use sting_fift::FiftAssembler;
use sting_tolkc::TolkCompiler;

/// The standard Serialized `BoC` magic prefix (`0xB5EE9C72`) — every real
/// `.boc` file, of any TVM contract, begins with this, confirmed against
/// the real artifact this pipeline produced while writing this crate.
const BOC_MAGIC: [u8; 4] = [0xb5, 0xee, 0x9c, 0x72];

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// Builds both a [`TolkCompiler`] and a [`FiftAssembler`] for the real
/// vendored binaries, or prints a message and returns `None` if either is
/// not present locally (see this module's doc comment).
fn pipeline_or_skip() -> Option<(TolkCompiler, FiftAssembler)> {
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

fn config_project_dir() -> PathBuf {
    repo_root().join("corpus/clean/acton-contracts/config")
}

#[test]
fn assembles_a_real_config_contract_into_a_valid_boc() {
    let Some((tolk, fift)) = pipeline_or_skip() else {
        return;
    };
    let project_dir = config_project_dir();
    let compiled = tolk
        .compile_keep_scratch(&project_dir, Path::new("Config.tolk"))
        .expect("Config.tolk is a real, previously-compile-verified contract");

    let boc = fift
        .assemble(&compiled.fift_path)
        .expect("a successfully compiled contract must assemble into real bytecode");

    assert!(!boc.is_empty(), "assembled bytecode must not be empty");
    assert_eq!(
        boc[0..4],
        BOC_MAGIC,
        "assembled output must start with the standard Serialized BoC magic prefix"
    );
}

#[test]
fn assembling_the_same_source_twice_produces_byte_identical_bytecode() {
    let Some((tolk, fift)) = pipeline_or_skip() else {
        return;
    };
    let project_dir = config_project_dir();

    let first = tolk
        .compile_keep_scratch(&project_dir, Path::new("Config.tolk"))
        .expect("first compile must succeed");
    let first_boc = fift
        .assemble(&first.fift_path)
        .expect("first assemble must succeed");

    let second = tolk
        .compile_keep_scratch(&project_dir, Path::new("Config.tolk"))
        .expect("second compile must succeed");
    let second_boc = fift
        .assemble(&second.fift_path)
        .expect("second assemble must succeed");

    assert_eq!(
        first_boc, second_boc,
        "compiling and assembling the same source twice, from two independent scratch \
         directories, must be deterministic — a real prerequisite for `RegistryController::verify`'s \
         byte-for-byte on-chain comparison to mean anything for Tolk"
    );
}
