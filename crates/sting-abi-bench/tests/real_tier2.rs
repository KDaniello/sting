//! Real Tier 2 (`FunC`) precision/recall — against the 4 real FunC/Tolk
//! pairs currently available in this workspace: `FuncExtractor` on the
//! original `FunC` source (`crates/sting-abi-func/tests/fixtures/`) vs.
//! `TolkExtractor` on the same contract's already `func2tolk`-converted,
//! already compile-verified Tolk equivalent (`corpus/clean/converted/`,
//! provenance in that directory's own `SOURCE.md`).
//!
//! **Honestly small ground-truth set, not padded out** — 4 pairs, the ones
//! where a matching original `FunC` source happens to already be vendored
//! in this workspace for an unrelated reason (the `FunC` extractor's own
//! fixtures) *and* a converted Tolk equivalent already exists
//! (`corpus/clean/converted/`). No fixed numeric target is set for this
//! corpus the
//! way it does for the vulnerable-corpus one (security branch's own, a
//! different corpus for a different purpose) — growing this past 4 is real,
//! separate future work (fetch more `FunC` originals for other already-
//! converted `corpus/clean/converted/` projects), not attempted here.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "docs/ARCHITECTURE.md §3.2 scopes the no-unwrap/expect/panic policy to library code outside tests"
)]

use std::path::{Path, PathBuf};

use sting_abi_bench::tier2::score_func;
use sting_abi_core::Extractor;
use sting_abi_func::FuncExtractor;
use sting_abi_tolk::TolkExtractor;
use sting_tolkc::TolkCompiler;

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn tolk_extractor_or_skip() -> Option<TolkExtractor> {
    let path = repo_root()
        .join("vendor/tolk")
        .join(sting_tolkc::platform_executable_filename());
    if !path.is_file() {
        println!("SKIP: {} not present locally", path.display());
        return None;
    }
    Some(TolkExtractor::new(TolkCompiler::new(path)))
}

/// `(func fixture file, converted Tolk project dir, converted Tolk
/// entrypoint)`.
const PAIRS: &[(&str, &str, &str)] = &[
    (
        "jetton-minter.fc",
        "corpus/clean/converted/jetton-minter",
        "jetton-minter.tolk",
    ),
    (
        "jetton-wallet.fc",
        "corpus/clean/converted/jetton-wallet",
        "jetton-wallet.tolk",
    ),
    (
        "nft-item.fc",
        "corpus/clean/converted/nft-item",
        "nft-item.tolk",
    ),
    (
        "wallet3-code.fc",
        "corpus/clean/converted/wallet3",
        "wallet3-code.tolk",
    ),
];

#[test]
fn tier2_precision_recall_against_real_func2tolk_ground_truth() {
    let Some(tolk_extractor) = tolk_extractor_or_skip() else {
        return;
    };
    let func_extractor = FuncExtractor;
    let func_fixtures = repo_root().join("crates/sting-abi-func/tests/fixtures");

    for (func_file, tolk_dir, tolk_entrypoint) in PAIRS {
        let ground_truth = tolk_extractor
            .extract(&repo_root().join(tolk_dir), Path::new(tolk_entrypoint))
            .unwrap_or_else(|e| panic!("compiling ground truth {tolk_dir}/{tolk_entrypoint}: {e}"));
        let candidate = func_extractor
            .extract(&func_fixtures, Path::new(func_file))
            .unwrap_or_else(|e| panic!("extracting {func_file}: {e}"));

        let result = score_func(func_file, &ground_truth, &candidate);
        println!(
            "{func_file}: get_methods P={:?} R={:?} (tp={} fp={} fn={}) | storage P={:?} R={:?} (tp={} fp={} fn={})",
            result.get_methods.precision(),
            result.get_methods.recall(),
            result.get_methods.true_positives,
            result.get_methods.false_positives,
            result.get_methods.false_negatives,
            result.storage_fields.precision(),
            result.storage_fields.recall(),
            result.storage_fields.true_positives,
            result.storage_fields.false_positives,
            result.storage_fields.false_negatives,
        );

        // Not a hard-coded expected number (that would be circular — this
        // test's job is to let the harness run against real data, not to
        // assert a target this same run computes) — the one invariant that
        // must always hold structurally: FuncExtractor never fabricates a
        // get-method — only real `method_id`-annotated functions are
        // recognised — so false positives on
        // get_methods should be rare/zero on this known-clean corpus. A
        // regression here (a phantom get-method appearing) is worth
        // investigating, not silently tolerated.
        assert_eq!(
            result.get_methods.false_positives, 0,
            "{func_file}: FuncExtractor should not fabricate get-methods on this corpus"
        );
    }
}
