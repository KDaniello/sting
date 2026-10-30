//! Golden/snapshot tests (`docs/TESTING.md`) — real
//! contracts from `corpus/clean/` (reused directly, not duplicated, per
//! `docs/domains/extractors.md`), compared
//! against `tests/fixtures/golden/*.expected.json` snapshots.
//!
//! Every `expected.json` here was produced by running this exact extractor
//! once and hand-reviewing the output line by line before committing it —
//! there is no independent ground truth to compare against (the compiler
//! *is* the ground truth for Tier 1, see `critical/BENCHMARKS.md` §1.1),
//! so review confirms the *normalization* is correct, not that some other
//! oracle agrees. Spot-checked against the raw `AbiJson`:
//! `config.expected.json`'s `types[15]` (`ConfigProposal`, an 8-bit TL-B
//! prefix nested inside another message, not a top-level 32-bit opcode) is
//! exactly the real case that caught `NamedType`'s `opcode: Option<u32>`
//! being the wrong shape while this was being designed.
//!
//! `vendor/tolk/` is `.gitignore`d — every test calls [`compiler_or_skip`]
//! first and returns early if it's absent, same pattern as
//! `sting-tolkc`'s own `tests/real_compile.rs`.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "docs/ARCHITECTURE.md §3.2 scopes the no-unwrap/expect/panic policy to library code outside tests"
)]

use std::path::{Path, PathBuf};

use sting_abi_core::{ContractSchema, Extractor};
use sting_abi_tolk::TolkExtractor;
use sting_tolkc::TolkCompiler;

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn compiler_or_skip() -> Option<TolkCompiler> {
    let path = repo_root()
        .join("vendor/tolk")
        .join(sting_tolkc::platform_executable_filename());
    if !path.is_file() {
        println!(
            "SKIP: {} not present locally (see vendor/tolk/ in .gitignore) — \
             fetch it from ton-blockchain/ton's tolk-X.Y.Z release to run this test for real",
            path.display()
        );
        return None;
    }
    Some(TolkCompiler::new(path))
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

fn assert_matches_golden(
    extractor: &TolkExtractor,
    project_dir: &str,
    entrypoint: &str,
    golden_name: &str,
) {
    let project_dir = repo_root().join(project_dir);
    let schema = extractor
        .extract(&project_dir, Path::new(entrypoint))
        .unwrap_or_else(|e| {
            panic!(
                "extracting {entrypoint} from {}: {e}",
                project_dir.display()
            )
        });
    schema.validate().unwrap_or_else(|e| {
        panic!("{entrypoint}'s extracted schema violates its own invariant: {e}")
    });

    let expected = load_expected(golden_name);
    assert_eq!(
        schema, expected,
        "{entrypoint}'s extracted schema no longer matches tests/fixtures/golden/{golden_name}.expected.json — \
         if this is an intentional extractor change, regenerate and re-review the golden file, don't just update the assertion"
    );
}

#[test]
fn five_real_contracts_match_their_reviewed_golden_schema() {
    let Some(compiler) = compiler_or_skip() else {
        return;
    };
    let extractor = TolkExtractor::new(compiler);

    assert_matches_golden(
        &extractor,
        "corpus/clean/acton-contracts/config",
        "Config.tolk",
        "config",
    );
    assert_matches_golden(
        &extractor,
        "corpus/clean/acton-contracts/counter",
        "Counter.tolk",
        "counter",
    );
    assert_matches_golden(
        &extractor,
        "corpus/clean/tolk-bench/01_jetton",
        "JettonMinter.tolk",
        "jetton_minter",
    );
    assert_matches_golden(
        &extractor,
        "corpus/clean/acton-contracts/nft-v1.1",
        "NftItem.tolk",
        "nft_item",
    );
    assert_matches_golden(
        &extractor,
        "corpus/clean/acton-contracts/w5.2",
        "WalletV5.tolk",
        "wallet_v5",
    );
}
