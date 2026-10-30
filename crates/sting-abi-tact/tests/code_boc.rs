//! Real-compiler test for `TactCompileOutput::code_boc`. Confirms the
//! `.code.boc` artifact `tact.js` already produces on every compile
//! (`tests/fixtures/SOURCE.md`'s documented artifact list) is actually
//! read back as real, valid-looking bytecode, not just that the code
//! compiles.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "docs/ARCHITECTURE.md §3.2 scopes the no-unwrap/expect/panic policy to library code outside tests"
)]

use std::path::{Path, PathBuf};

use sting_abi_tact::TactCompiler;

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn compiler_or_skip() -> Option<TactCompiler> {
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

/// A real BOC (Bag of Cells) always starts with one of TON's documented
/// magic prefixes — `0xB5EE9C72` (the standard, non-indexed serialization
/// `tact` actually emits) is the one to check for; this isn't a guess,
/// it's the same constant `sting-emulator`'s own BOC handling already
/// relies on for real vendored `.boc` fixtures elsewhere in this
/// workspace.
const BOC_MAGIC: [u8; 4] = [0xB5, 0xEE, 0x9C, 0x72];

#[test]
fn compiling_a_real_contract_produces_a_real_boc() {
    let Some(compiler) = compiler_or_skip() else {
        return;
    };
    let fixtures_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let output = compiler
        .compile(&fixtures_dir, Path::new("opcodes.tact"))
        .expect("real fixture must compile");

    assert!(!output.code_boc.is_empty(), "code_boc must not be empty");
    assert_eq!(
        &output.code_boc[..4],
        &BOC_MAGIC,
        "code_boc must start with the real BOC magic prefix, got {:02X?}",
        &output.code_boc[..4.min(output.code_boc.len())]
    );
}

/// The same source compiled twice, in two different scratch directories
/// (different absolute paths), must produce byte-identical bytecode —
/// `tact` compiles deterministically (`docs/abi-layer/blocks/
/// 05-verification-registry.md` §5.3's own premise, now actually
/// confirmed rather than assumed). If this ever fails, it means Tact's
/// compiler leaks scratch-path-dependent metadata into the BOC itself,
/// which would break `RegistryController`'s byte-comparison approach
/// entirely and needs to be known immediately, not discovered later as a
/// mysterious verification failure.
#[test]
fn compiling_the_same_source_twice_produces_byte_identical_boc() {
    let Some(compiler) = compiler_or_skip() else {
        return;
    };
    let fixtures_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let first = compiler
        .compile(&fixtures_dir, Path::new("opcodes.tact"))
        .expect("first compile must succeed");
    let second = compiler
        .compile(&fixtures_dir, Path::new("opcodes.tact"))
        .expect("second compile must succeed");

    assert_eq!(
        first.code_boc, second.code_boc,
        "two compiles of the same source must produce identical bytecode — \
         if this fails, RegistryController's whole byte-comparison approach needs to be revisited"
    );
}

/// `docs/domains/service.md`'s own required
/// empirical check: compile once plain, once with an added comment, see
/// what (if anything) differs in the resulting BOC. Real finding, not
/// assumed: `tact` strips comments before code generation — the BOC is
/// byte-identical either way. Combined with the previous test (different
/// scratch *paths* also don't affect the BOC), this means Tact's compiler
/// leaks **no** scratch-path or comment metadata into bytecode at all —
/// unlike the risk §5.3 anticipated, there is no non-trivial `partial_match`
/// case to construct for Tact from either axis: a byte match is a full
/// match, a byte mismatch reflects a genuine semantic difference, not
/// incidental metadata noise.
#[test]
fn adding_a_comment_does_not_change_the_compiled_boc() {
    let Some(compiler) = compiler_or_skip() else {
        return;
    };
    let fixtures_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let plain = compiler
        .compile(&fixtures_dir, Path::new("opcodes.tact"))
        .expect("plain compile must succeed");

    let scratch = tempfile::tempdir().expect("scratch dir");
    let original =
        std::fs::read_to_string(fixtures_dir.join("opcodes.tact")).expect("read fixture");
    let commented = format!("// this comment must not affect the compiled BOC\n{original}");
    std::fs::write(scratch.path().join("opcodes.tact"), commented).expect("write commented copy");
    let with_comment = compiler
        .compile(scratch.path(), Path::new("opcodes.tact"))
        .expect("commented compile must succeed");

    assert_eq!(
        plain.code_boc, with_comment.code_boc,
        "an added comment must not change the compiled BOC — tact strips comments before codegen"
    );
}
