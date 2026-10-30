//! Benchmark harness: precision/recall/FP-rate measurement over the labeled
//! corpus (`docs/domains/precision-sources.md`, `docs/domains/precision-sources.md`).

mod corpus;
mod ground_truth;
mod metrics;
mod mutate;
mod report;

use std::path::Path;

pub use corpus::{
    CorpusError, GroundTruthEntry, Project, discover_merged_projects, discover_single_file_projects,
};
pub use ground_truth::{Manifest, ManifestError, ManifestLocation, load as load_manifest};
pub use metrics::{
    BenchReport, DatasetUnit, DetectorMetrics, MatchGranularity, MissedDefect, SpuriousFinding,
    UnitOrigin, build_dataset, score, score_dataset, score_dataset_with,
};
pub use mutate::{MutatedInstance, mutate_all};
pub use report::{
    BASELINE_SCHEMA_VERSION, Baseline, BaselineError, DetectorSnapshot, Regression,
    baseline_to_json_string, compare_to_baseline, load_baseline, to_json, to_json_string,
    to_markdown,
};

/// Every registered detector code, in the order every report/table presents
/// them (`docs/domains/detectors.md`'s own ordering for the original 4:
/// 2 `Ast`-kind, then 2 `Dataflow`-kind) plus `TLB`/`PL`/`BR`/`UR`/`AC`,
/// appended in the order they were added, plus `UTA`, which came out of a
/// batch of external validation (`hacktheton.com`, see `uta.rs`), plus
/// `SUA`, which came out of the Misti prior-art teardown
/// (`sua.rs`), plus `EPS` (`eps.rs`), plus `UDA`, sting's first taint-tracking
/// detector (`uda.rs`).
/// `DEPR` is deliberately absent — see `deprecated.rs`'s own doc comment for
/// why no ground-truth concept applies to it.
pub const DETECTOR_CODES: [&str; 13] = [
    "GVR", "LEP", "ID", "UBM", "TLB", "PL", "BR", "UR", "AC", "UTA", "SUA", "EPS", "UDA",
];

/// Subdirectories of `corpus/clean/` that hold merged, multi-file projects
/// (`docs/domains/precision-sources.md`) — `converted/`/`synthetic/`
/// are included whenever present, but their absence is not an error (a
/// fresh checkout before running the conversion step, or before the
/// mutation-engine-seeding files existed, is still a valid, just smaller,
/// clean corpus). `synthetic/` is hand-authored, not vendored — see
/// `corpus/clean/synthetic/SOURCE.md` for why it exists.
const CLEAN_CORPUS_DIRS: [&str; 5] = [
    "tolk-bench",
    "acton-contracts",
    "converted",
    "synthetic",
    "production",
];

/// Runs the full benchmark: discovers the clean corpus and the golden
/// fixtures under `corpus_root` (normally the workspace's `corpus/`
/// directory), generates mutation instances from the clean corpus, and
/// scores everything against `sting-detectors`' built-in registry.
///
/// # Errors
/// Returns [`CorpusError`] if a corpus directory or a `.manifest.yaml`
/// inside it cannot be read/parsed.
pub fn run_benchmark(corpus_root: &Path) -> Result<BenchReport, CorpusError> {
    let (clean_projects, mutations, golden_fixtures) = discover_corpus(corpus_root)?;
    Ok(score(&clean_projects, &mutations, &golden_fixtures))
}

/// Drops an `import` of a **sibling project file** while keeping `@stdlib/*`.
///
/// A dataset unit is several project files merged into one text; the original `import "types.tolk"`
/// lines survive in it while the files beside it do not, their content having already been merged
/// in. This tree's own detectors never resolve an import at all, so the lines are harmless to us —
/// and a participant with a real resolver fails on every unit because of them, which would measure
/// our splicing rather than its rules.
///
/// `@stdlib/*` stays: those dependencies are external, nothing merged them in, and removing them
/// would break parsing a different way.
#[must_use]
pub fn strip_sibling_imports(source: &str) -> String {
    source
        .lines()
        .filter(|line| {
            let trimmed = line.trim_start();
            !trimmed.starts_with("import ") || trimmed.contains("@stdlib/")
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Assembles the dataset every participant is scored against — the same one
/// [`run_benchmark`] uses for our own detectors.
///
/// Public because the measurer is split from the product:
/// `sting-benchmark` needs the identical dataset to run an external analyzer
/// over, and rebuilding it there would let the two drift apart, which is the
/// one thing a published comparison cannot survive.
///
/// # Errors
/// Returns [`CorpusError`] if a corpus directory or a `.manifest.yaml`
/// inside it cannot be read/parsed.
pub fn load_dataset(corpus_root: &Path) -> Result<Vec<DatasetUnit>, CorpusError> {
    let (clean_projects, mutations, golden_fixtures) = discover_corpus(corpus_root)?;
    Ok(build_dataset(&clean_projects, &mutations, &golden_fixtures))
}

/// The three corpus halves: vendored clean projects, mutation instances
/// derived from them, and hand-written golden fixtures.
type CorpusHalves = (Vec<Project>, Vec<MutatedInstance>, Vec<Project>);

/// Discovers the three corpus halves under `corpus_root`.
fn discover_corpus(corpus_root: &Path) -> Result<CorpusHalves, CorpusError> {
    let mut clean_projects = Vec::new();
    for subdir in CLEAN_CORPUS_DIRS {
        let dir = corpus_root.join("clean").join(subdir);
        if dir.is_dir() {
            clean_projects.extend(discover_merged_projects(&dir, &format!("clean/{subdir}"))?);
        }
    }

    let mutations: Vec<MutatedInstance> = clean_projects.iter().flat_map(mutate_all).collect();

    let mut golden_fixtures = Vec::new();
    for code in DETECTOR_CODES {
        for category in ["positive", "negative"] {
            let dir = corpus_root
                .join("fixtures/detectors")
                .join(code)
                .join(category);
            if dir.is_dir() {
                golden_fixtures.extend(discover_single_file_projects(
                    &dir,
                    &format!("fixtures/{code}/{category}"),
                )?);
            }
        }
    }

    Ok((clean_projects, mutations, golden_fixtures))
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "docs/ARCHITECTURE.md §3.2 scopes the no-unwrap/expect policy to library code outside tests"
)]
mod tests {
    use std::path::PathBuf;

    use super::{DETECTOR_CODES, run_benchmark};

    #[test]
    fn run_benchmark_succeeds_end_to_end_on_the_real_corpus() {
        let corpus_root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../corpus");
        let report = run_benchmark(&corpus_root).expect("real corpus discoverable");
        assert!(
            report.clean_project_count >= 17,
            "at least the 17 native projects"
        );
        // GVR is known to have real applicable projects across the corpus
        // (docs/domains/precision-sources.md).
        assert!(
            report
                .mutation_instance_count
                .get("GVR")
                .copied()
                .unwrap_or(0)
                > 0
        );
    }

    /// A coverage meta-test, one of the three highest-value adoptions from the
    /// Misti comparison. It guards against the exact real bug that was hit once
    /// by hand: a detector registered in
    /// `DetectorRegistry::builtin()` but missing from [`DETECTOR_CODES`]
    /// (`UTA` was briefly invisible to `sting bench` this way) or missing a
    /// positive fixture directory entirely silently scores as if it had
    /// zero true positives everywhere, not as a loud failure. `DEPR` is the
    /// one deliberate exception — not a [`sting_core::DefectClass`] member,
    /// no ground-truth concept applies to it (`deprecated.rs`'s own doc
    /// comment), so it is excluded from [`DETECTOR_CODES`] on purpose, not
    /// by oversight.
    #[test]
    fn every_registered_detector_except_depr_is_covered_by_bench() {
        let corpus_root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../corpus");
        let registry = sting_detectors::DetectorRegistry::builtin();
        let mut missing_from_detector_codes = Vec::new();
        let mut missing_positive_fixtures = Vec::new();

        for id in registry.detectors().map(sting_detectors::Detector::id) {
            if id == "DEPR" {
                continue;
            }
            if !DETECTOR_CODES.contains(&id) {
                missing_from_detector_codes.push(id);
            }
            let positive_dir = corpus_root
                .join("fixtures/detectors")
                .join(id)
                .join("positive");
            let has_a_fixture = positive_dir
                .read_dir()
                .is_ok_and(|mut entries| entries.next().is_some());
            if !has_a_fixture {
                missing_positive_fixtures.push(id);
            }
        }

        assert!(
            missing_from_detector_codes.is_empty(),
            "registered but missing from DETECTOR_CODES: {missing_from_detector_codes:?}"
        );
        assert!(
            missing_positive_fixtures.is_empty(),
            "registered but no positive fixture under corpus/fixtures/detectors/<ID>/positive/: {missing_positive_fixtures:?}"
        );
    }
}
