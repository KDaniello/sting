//! `FunC` pipeline — the one Tier 1/2 extractor with no external compiler
//! subprocess at all (`tree-sitter`, in-process). Two stages: `tree-sitter`
//! parse alone (replicated externally, byte-for-byte the same calls
//! `FuncExtractor::extract` makes — `tree_sitter::Parser`/
//! `tree_sitter_func::LANGUAGE` are both public, no need to touch
//! `sting-abi-func`'s internals to isolate this), and the full
//! `FuncExtractor::extract` call. The heuristic-only cost (get-methods +
//! storage extraction, `sting-abi-func`'s own actual "algorithm") is
//! reported as a **derived** row, `full − parse`, not measured directly —
//! labeled as such in its note, not presented as a real third measurement.

use std::path::Path;

use sting_abi_core::Extractor;
use sting_abi_func::FuncExtractor;

use crate::report::StageResult;

const COMPONENT: &str = "FunC";

const FIXTURES: &[&str] = &[
    "jetton-minter.fc",
    "jetton-wallet.fc",
    "nft-item.fc",
    "wallet3-code.fc",
    "stdlib.fc",
    "synthetic-storage-pair.fc",
];

/// Fixed (not wall-clock-adaptive) repeat count per fixture — deliberately
/// *not* [`crate::report::time_stage`]: that helper stops each stage
/// independently once its own wall-clock budget elapses, which would let
/// "parse only" and "`extract()` full" sample the 6 fixtures (whose sizes
/// span a real 36x range, `stdlib.fc` vs. the smallest) a *different*
/// number of times each — silently invalidating the `full − parse`
/// subtraction below. A fixed, shared call count per fixture guarantees
/// both stages see the exact same per-file weighting, so the derived row
/// means what it says.
const REPEATS_PER_FIXTURE: usize = 300;

pub fn profile(repo_root: &Path) -> Vec<StageResult> {
    let fixtures_dir = repo_root.join("crates/sting-abi-func/tests/fixtures");
    let sources: Vec<String> = FIXTURES
        .iter()
        .filter_map(|f| std::fs::read_to_string(fixtures_dir.join(f)).ok())
        .collect();

    let start = std::time::Instant::now();
    for _ in 0..REPEATS_PER_FIXTURE {
        for source in &sources {
            let mut parser = tree_sitter::Parser::new();
            let _ = parser.set_language(&tree_sitter_func::LANGUAGE.into());
            let _ = parser.parse(source, None);
        }
    }
    let parse_total = start.elapsed();
    let parse_calls = u64::try_from(REPEATS_PER_FIXTURE * sources.len()).unwrap_or(u64::MAX);

    let extractor = FuncExtractor;
    let start = std::time::Instant::now();
    for _ in 0..REPEATS_PER_FIXTURE {
        for f in FIXTURES {
            let _ = extractor.extract(&fixtures_dir, Path::new(f));
        }
    }
    let full_total = start.elapsed();
    let full_calls = u64::try_from(REPEATS_PER_FIXTURE * FIXTURES.len()).unwrap_or(u64::MAX);

    let parse_stage = StageResult {
        component: COMPONENT,
        stage: "tree-sitter parse only",
        calls: parse_calls,
        total: parse_total,
        note: format!(
            "{} real fixtures x {REPEATS_PER_FIXTURE} repeats, incl. stdlib.fc (36KB, the outlier)",
            sources.len()
        ),
    };
    let full_stage = StageResult {
        component: COMPONENT,
        stage: "extract() (parse + heuristics)",
        calls: full_calls,
        total: full_total,
        note: format!(
            "{} real fixtures x {REPEATS_PER_FIXTURE} repeats",
            FIXTURES.len()
        ),
    };
    let derived = StageResult {
        component: COMPONENT,
        stage: "heuristics only (get_methods + storage)",
        calls: full_calls,
        total: full_total.saturating_sub(parse_total),
        note: "DERIVED: extract() total − parse-only total, same fixed per-file weighting"
            .to_string(),
    };

    vec![parse_stage, full_stage, derived]
}
