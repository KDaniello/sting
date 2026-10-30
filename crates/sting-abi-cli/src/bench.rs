//! `abi-layer bench` — thin CLI wiring around `sting-abi-bench`. Argument
//! parsing plus a call into the library crate, no orchestration logic here
//! (same convention `sting-cli`'s own `bench()` follows for the security
//! branch).

use std::path::{Path, PathBuf};

use anyhow::Context as _;
use sting_abi_bench::{corpus, report, throughput, tier1, tier2};
use sting_abi_core::Extractor;
use sting_abi_func::FuncExtractor;
use sting_abi_tact::{TactCompiler, TactExtractor};
use sting_abi_tolk::TolkExtractor;
use sting_tolkc::TolkCompiler;

use crate::TierArg;

pub fn run_bench(
    corpus_root: &Path,
    report_path: &Path,
    tier: TierArg,
    baseline_path: &Path,
    update_baseline: bool,
) -> anyhow::Result<()> {
    let mut accuracy_report = report::AccuracyReport::default();

    if tier == TierArg::Tier1 || tier == TierArg::All {
        run_tier1(corpus_root, &mut accuracy_report)?;
    }
    if tier == TierArg::Tier2 || tier == TierArg::All {
        run_tier2(&mut accuracy_report)?;
    }

    let markdown = report::to_markdown(&accuracy_report);
    std::fs::write(report_path, &markdown)
        .with_context(|| format!("writing {}", report_path.display()))?;
    let json_path = report_path.with_extension("json");
    let json = report::to_json_string(&accuracy_report).context("serializing report as JSON")?;
    std::fs::write(&json_path, json).with_context(|| format!("writing {}", json_path.display()))?;
    println!("{markdown}");
    println!(
        "Wrote {} and {}",
        report_path.display(),
        json_path.display()
    );

    let current_baseline = report::Baseline::from(&accuracy_report);
    if update_baseline {
        let existing_baseline = if baseline_path.is_file() {
            let existing_text = std::fs::read_to_string(baseline_path)
                .with_context(|| format!("reading {}", baseline_path.display()))?;
            serde_json::from_str(&existing_text)
                .context("parsing existing baseline JSON to merge into")?
        } else {
            report::Baseline::default()
        };
        let merged_baseline = merge_baseline_update(tier, current_baseline, existing_baseline);
        let text =
            report::baseline_to_json_string(&merged_baseline).context("serializing baseline")?;
        std::fs::write(baseline_path, text)
            .with_context(|| format!("writing {}", baseline_path.display()))?;
        println!("Updated baseline: {}", baseline_path.display());
        return Ok(());
    }

    if !baseline_path.is_file() {
        println!(
            "No baseline at {} — skipping regression check (run with --update-baseline to create one)",
            baseline_path.display()
        );
        return Ok(());
    }
    let baseline_text = std::fs::read_to_string(baseline_path)
        .with_context(|| format!("reading {}", baseline_path.display()))?;
    let baseline: report::Baseline =
        serde_json::from_str(&baseline_text).context("parsing baseline JSON")?;
    // Small-corpus statistical wobble tolerance, same ±1 the security
    // branch's own `sting-cli::bench()` uses for the same reason.
    let regressions = report::compare_to_baseline(&current_baseline, &baseline, 1);
    if regressions.is_empty() {
        println!("No regressions vs {}", baseline_path.display());
        return Ok(());
    }
    for regression in &regressions {
        eprintln!(
            "REGRESSION [{}]: {}",
            regression.category, regression.message
        );
    }
    anyhow::bail!(
        "{} regression(s) vs {}",
        regressions.len(),
        baseline_path.display()
    );
}

fn run_tier1(
    corpus_root: &Path,
    accuracy_report: &mut report::AccuracyReport,
) -> anyhow::Result<()> {
    let tolk_compiler = TolkCompiler::vendored_or_default().context(
        "no Tolk compiler is reachable: see docs/domains/extractors.md for vendoring it",
    )?;
    let tact_compiler = TactCompiler::vendored_or_default().context(
        "no Tact compiler is reachable: see docs/domains/extractors.md for vendoring it",
    )?;

    let tolk_roots = [
        Path::new("corpus/clean/acton-contracts"),
        Path::new("corpus/clean/tolk-bench"),
    ];
    let tolk_projects = corpus::discover_tolk_projects(&tolk_roots);
    let tact_projects = corpus::discover_tact_projects(corpus_root);

    println!(
        "Tier 1: {} Tolk project(s), {} Tact project(s)",
        tolk_projects.len(),
        tact_projects.len()
    );

    for project in &tolk_projects {
        match tolk_compiler.compile(&project.project_dir, &project.entrypoint) {
            Ok(output) => {
                let schema = sting_abi_tolk::abi_to_schema(&output.abi);
                accuracy_report
                    .tier1
                    .push(tier1::score_tolk(&project.label, &output.abi, &schema));
            }
            Err(err) => eprintln!("SKIP {}: compile failed: {err}", project.label),
        }
    }
    for project in &tact_projects {
        match tact_compiler.compile(&project.project_dir, &project.entrypoint) {
            Ok(output) => {
                let schema = sting_abi_tact::abi_to_schema(&output.abi);
                accuracy_report
                    .tier1
                    .push(tier1::score_tact(&project.label, &output.abi, &schema));
            }
            Err(err) => eprintln!("SKIP {}: compile failed: {err}", project.label),
        }
    }

    let tolk_extractor = TolkExtractor::new(tolk_compiler);
    let tolk_batch = throughput::measure_batch(&tolk_projects, |p| {
        tolk_extractor
            .extract(&p.project_dir, &p.entrypoint)
            .is_ok()
    });
    accuracy_report
        .throughput
        .insert("tolk_batch".to_string(), tolk_batch);

    let tact_extractor = TactExtractor::new(tact_compiler);
    let tact_batch = throughput::measure_batch(&tact_projects, |p| {
        tact_extractor
            .extract(&p.project_dir, &p.entrypoint)
            .is_ok()
    });
    accuracy_report
        .throughput
        .insert("tact_batch".to_string(), tact_batch);

    Ok(())
}

fn run_tier2(accuracy_report: &mut report::AccuracyReport) -> anyhow::Result<()> {
    let tolk_compiler = TolkCompiler::vendored_or_default().context(
        "no Tolk compiler is reachable: see docs/domains/extractors.md for vendoring it",
    )?;
    let tolk_extractor = TolkExtractor::new(tolk_compiler);
    let func_extractor = FuncExtractor;
    let func_fixtures = Path::new("crates/sting-abi-func/tests/fixtures");

    let mut func_files: Vec<PathBuf> = Vec::new();
    for pair in corpus::TIER2_PAIRS {
        let ground_truth = tolk_extractor.extract(
            Path::new(pair.tolk_project_dir),
            Path::new(pair.tolk_entrypoint),
        );
        let candidate = func_extractor.extract(func_fixtures, Path::new(pair.func_file));
        match (ground_truth, candidate) {
            (Ok(ground_truth), Ok(candidate)) => {
                accuracy_report.tier2.push(tier2::score_func(
                    pair.func_file,
                    &ground_truth,
                    &candidate,
                ));
                func_files.push(PathBuf::from(pair.func_file));
            }
            (Err(err), _) => eprintln!(
                "SKIP {}: ground truth compile failed: {err}",
                pair.tolk_entrypoint
            ),
            (_, Err(err)) => eprintln!("SKIP {}: extraction failed: {err}", pair.func_file),
        }
    }

    let func_batch = throughput::measure_batch(&func_files, |file| {
        func_extractor.extract(func_fixtures, file).is_ok()
    });
    accuracy_report
        .throughput
        .insert("func_batch".to_string(), func_batch);

    Ok(())
}

/// Merges a fresh `--update-baseline` run's `current` [`report::Baseline`]
/// into `existing` (whatever is already committed), replacing only the
/// tier(s) `tier` actually ran this invocation. `tier == TierArg::All`
/// replaces both wholesale (`current` already has everything).
///
/// **A real bug, found while validating the walk**: a single-tier run
/// (`--tier tier2
/// --update-baseline`) used to overwrite the *whole* committed baseline
/// file with `current`, whose other tier is empty
/// (`AccuracyReport::default()` — that tier was never run this
/// invocation), silently discarding real, already-measured data (hit
/// live: all 45 real Tolk/Tact Tier 1 contracts' worth, while updating only
/// Tier 2's `storage_fields` count). Pulled out as a pure function so this
/// exact scenario is unit-testable without a real corpus/compiler.
fn merge_baseline_update(
    tier: TierArg,
    current: report::Baseline,
    existing: report::Baseline,
) -> report::Baseline {
    match tier {
        TierArg::All => current,
        TierArg::Tier1 => report::Baseline {
            tier1: current.tier1,
            tier2: existing.tier2,
        },
        TierArg::Tier2 => report::Baseline {
            tier1: existing.tier1,
            tier2: current.tier2,
        },
    }
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "docs/ARCHITECTURE.md §3.2 scopes the no-unwrap/expect policy to library code outside tests"
)]
mod tests {
    use std::collections::BTreeMap;

    use sting_abi_bench::report::{Baseline, Tier1Snapshot, Tier2Snapshot};

    use super::{TierArg, merge_baseline_update};

    fn baseline_with_tier1(matched: u32) -> Baseline {
        let mut tier1 = BTreeMap::new();
        tier1.insert(
            "get_methods".to_string(),
            Tier1Snapshot {
                matched,
                total: matched,
            },
        );
        Baseline {
            tier1,
            tier2: BTreeMap::new(),
        }
    }

    fn baseline_with_tier2(false_positives: u32) -> Baseline {
        let mut tier2 = BTreeMap::new();
        tier2.insert(
            "storage_fields".to_string(),
            Tier2Snapshot {
                true_positives: 0,
                false_positives,
                false_negatives: 0,
            },
        );
        Baseline {
            tier1: BTreeMap::new(),
            tier2,
        }
    }

    #[test]
    fn a_tier2_only_update_preserves_the_existing_tier1_data() {
        let current = baseline_with_tier2(11); // this run's fresh tier2, no tier1 at all
        let existing = baseline_with_tier1(154); // what was already committed
        let merged = merge_baseline_update(TierArg::Tier2, current, existing);

        assert_eq!(
            merged.tier1.get("get_methods").map(|s| s.matched),
            Some(154),
            "tier1 must survive a tier2-only update, not be wiped to empty"
        );
        assert_eq!(
            merged
                .tier2
                .get("storage_fields")
                .map(|s| s.false_positives),
            Some(11),
            "tier2 must reflect this run's fresh numbers"
        );
    }

    #[test]
    fn a_tier1_only_update_preserves_the_existing_tier2_data() {
        let current = baseline_with_tier1(200);
        let existing = baseline_with_tier2(8);
        let merged = merge_baseline_update(TierArg::Tier1, current, existing);

        assert_eq!(
            merged.tier1.get("get_methods").map(|s| s.matched),
            Some(200)
        );
        assert_eq!(
            merged
                .tier2
                .get("storage_fields")
                .map(|s| s.false_positives),
            Some(8),
            "tier2 must survive a tier1-only update, not be wiped to empty"
        );
    }

    #[test]
    fn an_all_tier_update_replaces_both_wholesale() {
        let current = baseline_with_tier1(999); // has no tier2 data of its own
        let existing = baseline_with_tier2(8);
        let merged = merge_baseline_update(TierArg::All, current, existing);

        assert_eq!(
            merged.tier1.get("get_methods").map(|s| s.matched),
            Some(999)
        );
        assert!(
            merged.tier2.is_empty(),
            "an --tier all run's own (empty, in this synthetic case) tier2 replaces the \
             existing one wholesale — All means 'this is now the complete picture'"
        );
    }
}
