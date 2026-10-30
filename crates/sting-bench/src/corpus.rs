//! Corpus discovery: turns directories of `.tolk` files into [`Project`]s —
//! `sting bench`'s unit of analysis.
//!
//! A project is one or more sibling `.tolk` files, merged (concatenated)
//! into a single source before parsing. This is a deliberate difference
//! from `sting lint`, which analyzes one file at a time
//! (`docs/domains/detectors.md`): the real clean corpus
//! (`ton-blockchain/tolk-bench`/`acton-contracts`) always splits a
//! contract's header, storage `struct`, and message handlers across sibling
//! files connected by `import` — `GVR`/`ID`'s v1 scope is explicitly
//! single-file (`docs/domains/detectors.md`), so
//! analyzing real projects file-by-file would silently starve those two
//! detectors of every real applicable case. Merging sibling files into one
//! synthetic source for *benchmarking* (never for `sting lint`, which stays
//! per-file) lets the benchmark exercise detectors against the real
//! multi-file shape without reopening that v1 scope decision.
//!
//! Tree-sitter only parses syntax, not semantics — leftover `import`
//! statements in the merged text are harmless (parsed as ordinary
//! `import_directive` nodes, never resolved), so naive concatenation is
//! sound for this purpose even though it would not be for real compilation.

use std::io;
use std::path::{Path, PathBuf};
use sting_ast::Language;

use crate::ground_truth::{self, ManifestError};

/// One or more sibling `.tolk` files, merged into a single source, plus the
/// ground truth (ready-to-match against `Finding`s) that applies to it.
#[derive(Debug, Clone)]
pub struct Project {
    /// A human-readable name for reports (e.g. `"clean/tolk-bench/01_jetton"`).
    pub name: String,
    /// Which grammar reads [`Self::source`].
    ///
    /// Kept per project rather than assumed for the corpus: a precision figure that mixed two
    /// languages into one row would be a number about neither of them.
    pub language: Language,
    /// The merged source text.
    pub source: String,
    /// Known defects in this project, already translated into `source`'s
    /// line-number space (see [`Self::source`]'s merge order).
    pub ground_truth: Vec<GroundTruthEntry>,
    /// Where each file starts in [`Self::source`], as a 1-indexed line.
    ///
    /// The merge is a concatenation, so every file owns a contiguous run of lines — which is what
    /// lets a mutation of the merged text be written back out as the files it came from. A
    /// participant with a resolver reads a directory, and a directory is what it has to be given.
    pub file_starts: Vec<(String, u32)>,
    /// The original per-file layout: `(file name, content)` in merge order.
    ///
    /// It exists for the published dataset. Merging is a trick of the
    /// **internal** harness: `lint_source` has no project context, so `GVR` and
    /// `ID`, single-file in scope, would otherwise never see a real
    /// multi-file case. But an analyser with a real resolver gets from the
    /// splice not a project but text full of redefined symbols: that is exactly
    /// why `acton 1.1.0` failed to compile 30% of the corpus's real code. What
    /// is published has to be what a participant expects to see — a
    /// directory.
    pub files: Vec<(String, String)>,
}

/// One known defect location, ready to match against a [`sting_core::Finding`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GroundTruthEntry {
    /// Expected `Finding::detector_id` (e.g. `"GVR"`).
    pub defect_class: String,
    /// 1-indexed line, in the owning [`Project::source`]'s coordinate space.
    pub line: u32,
    /// 1-indexed column (informational only — matching tolerates line drift
    /// but not necessarily column, per `docs/domains/precision-sources.md`).
    pub column: u32,
}

/// Errors from corpus discovery.
#[derive(Debug, thiserror::Error)]
pub enum CorpusError {
    /// A directory could not be read.
    #[error("failed to read directory {path}: {source}")]
    Io {
        /// The directory that failed.
        path: String,
        /// Underlying I/O error.
        #[source]
        source: io::Error,
    },
    /// A `.manifest.yaml` file could not be loaded.
    #[error(transparent)]
    Manifest(#[from] ManifestError),
}

fn io_err(path: &Path, source: io::Error) -> CorpusError {
    CorpusError::Io {
        path: path.display().to_string(),
        source,
    }
}

/// Discovers one [`Project`] per immediate subdirectory of `root` that
/// contains at least one `.tolk` file — `corpus/clean/{tolk-bench,
/// acton-contracts,converted}/<project-name>/`. `label` prefixes each
/// project's name for report readability (e.g. `"tolk-bench"`).
///
/// # Errors
/// Returns [`CorpusError`] if `root` or a subdirectory cannot be read, or a
/// `.manifest.yaml` inside it fails to parse.
pub fn discover_merged_projects(root: &Path, label: &str) -> Result<Vec<Project>, CorpusError> {
    let mut projects = Vec::new();
    let mut subdirs: Vec<PathBuf> = std::fs::read_dir(root)
        .map_err(|e| io_err(root, e))?
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.is_dir())
        .collect();
    subdirs.sort();

    for dir in subdirs {
        let mut tolk_files: Vec<PathBuf> = std::fs::read_dir(&dir)
            .map_err(|e| io_err(&dir, e))?
            .filter_map(Result::ok)
            .map(|e| e.path())
            // Tolk only, and not because nothing else exists: these projects are merged by
            // concatenation, and two grammars do not concatenate into one parseable text.
            .filter(|p| p.extension().is_some_and(|ext| ext == "tolk"))
            .collect();
        if tolk_files.is_empty() {
            continue;
        }
        tolk_files.sort();

        let mut source = String::new();
        let mut offsets: Vec<(String, u32)> = Vec::new();
        let mut files = Vec::new();
        for file in &tolk_files {
            let text = std::fs::read_to_string(file).map_err(|e| io_err(file, e))?;
            let start_line = u32::try_from(source.matches('\n').count()).unwrap_or(u32::MAX) + 1;
            let filename = file
                .file_name()
                .map_or_else(String::new, |n| n.to_string_lossy().to_string());
            offsets.push((filename.clone(), start_line));
            files.push((filename, text.clone()));
            source.push_str(&text);
            if !source.ends_with('\n') {
                source.push('\n');
            }
        }

        let ground_truth = load_ground_truth(&dir, &offsets)?;
        let project_name = dir
            .file_name()
            .map_or_else(String::new, |n| n.to_string_lossy().to_string());
        projects.push(Project {
            name: format!("{label}/{project_name}"),
            language: Language::Tolk,
            source,
            ground_truth,
            file_starts: offsets,
            files,
        });
    }
    Ok(projects)
}

/// Discovers one [`Project`] per source file directly inside `dir` — the golden fixtures
/// (`corpus/fixtures/detectors/<CODE>/{positive,negative}/`), where every file is its own
/// independent project, not merged with its siblings.
///
/// Every language the analyser reads is picked up, and each project keeps the one it is written in.
/// A detector that has crossed to a second language is measured in both here, and the two figures
/// are reported apart: averaging them would describe neither.
///
/// # Errors
/// Returns [`CorpusError`] if `dir` cannot be read or a sibling
/// `.manifest.yaml` fails to parse.
pub fn discover_single_file_projects(dir: &Path, label: &str) -> Result<Vec<Project>, CorpusError> {
    let mut files: Vec<PathBuf> = std::fs::read_dir(dir)
        .map_err(|e| io_err(dir, e))?
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| {
            p.extension()
                .and_then(|ext| ext.to_str())
                .and_then(Language::of_extension)
                .is_some()
        })
        .collect();
    files.sort();

    let mut projects = Vec::new();
    for file in files {
        let Some(language) = file
            .extension()
            .and_then(|ext| ext.to_str())
            .and_then(Language::of_extension)
        else {
            continue;
        };
        let source = std::fs::read_to_string(&file).map_err(|e| io_err(&file, e))?;
        let manifest_path = file.with_extension("manifest.yaml");
        let ground_truth = if manifest_path.is_file() {
            let manifest = ground_truth::load(&manifest_path)?;
            vec![GroundTruthEntry {
                defect_class: manifest.defect_class,
                line: manifest.location.line,
                column: manifest.location.column,
            }]
        } else {
            Vec::new()
        };
        let file_name = file
            .file_name()
            .map_or_else(String::new, |n| n.to_string_lossy().to_string());
        projects.push(Project {
            name: format!("{label}/{file_name}"),
            // The filter above admitted this file by its extension, so the same question has an
            // answer here. `Tolk` as a fallback would label a file with the wrong grammar the day
            // the filter widens, which is the silent default the rest of this reads against.
            language,
            source: source.clone(),
            ground_truth,
            file_starts: vec![(file_name.clone(), 1)],
            files: vec![(file_name, source)],
        });
    }
    Ok(projects)
}

fn load_ground_truth(
    dir: &Path,
    offsets: &[(String, u32)],
) -> Result<Vec<GroundTruthEntry>, CorpusError> {
    let mut entries = Vec::new();
    for entry in std::fs::read_dir(dir).map_err(|e| io_err(dir, e))? {
        let path = entry.map_err(|e| io_err(dir, e))?.path();
        let is_manifest = path
            .file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|n| n.ends_with(".manifest.yaml"));
        if !is_manifest {
            continue;
        }
        let manifest = ground_truth::load(&path)?;
        let start_line = offsets
            .iter()
            .find(|(f, _)| *f == manifest.location.file)
            .map_or(1, |(_, l)| *l);
        entries.push(GroundTruthEntry {
            defect_class: manifest.defect_class,
            line: start_line + manifest.location.line - 1,
            column: manifest.location.column,
        });
    }
    Ok(entries)
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "docs/ARCHITECTURE.md §3.2 scopes the no-unwrap/expect policy to library code outside tests"
)]
mod tests {

    /// A fixture keeps the language it is written in, or the per-language figures describe the
    /// wrong grammar and the older language's row silently absorbs the newer one's.
    #[test]
    fn a_single_file_fixture_keeps_its_own_language() {
        let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../corpus/fixtures/detectors/LEP/positive");
        let projects = super::discover_single_file_projects(&dir, "fixtures/LEP/positive")
            .expect("the fixtures are discoverable");
        let tolk = projects
            .iter()
            .filter(|p| p.language == sting_ast::Language::Tolk)
            .count();
        let func = projects
            .iter()
            .filter(|p| p.language == sting_ast::Language::Func)
            .count();
        assert!(tolk > 0, "the Tolk fixtures are still there");
        assert!(func > 0, "the FunC fixtures are read as FunC, not as Tolk");
    }

    use std::path::PathBuf;

    use super::{discover_merged_projects, discover_single_file_projects};

    fn repo_root() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
    }

    #[test]
    fn discovers_and_merges_tolk_bench_families() {
        let projects =
            discover_merged_projects(&repo_root().join("corpus/clean/tolk-bench"), "tolk-bench")
                .expect("directory exists");
        assert_eq!(projects.len(), 7, "7 tolk-bench families");
        let jetton = projects
            .iter()
            .find(|p| p.name.ends_with("01_jetton"))
            .expect("01_jetton project found");
        // Merged source must contain content from more than one sibling file.
        assert!(jetton.source.contains("fun onInternalMessage"));
        assert!(jetton.source.contains("struct"));
    }

    #[test]
    fn discovers_and_merges_acton_contracts_projects() {
        let projects = discover_merged_projects(
            &repo_root().join("corpus/clean/acton-contracts"),
            "acton-contracts",
        )
        .expect("directory exists");
        assert_eq!(projects.len(), 10, "10 acton-contracts projects");
    }

    #[test]
    fn discovers_golden_fixtures_as_independent_single_file_projects() {
        let projects = discover_single_file_projects(
            &repo_root().join("corpus/fixtures/detectors/GVR/positive"),
            "fixtures/GVR/positive",
        )
        .expect("directory exists");
        assert_eq!(projects.len(), 3, "3 GVR positive golden fixtures");
        assert!(
            projects.iter().all(|p| p.ground_truth.len() == 1),
            "every positive fixture has exactly one manifest entry"
        );
    }

    #[test]
    fn negative_fixtures_have_no_ground_truth() {
        let projects = discover_single_file_projects(
            &repo_root().join("corpus/fixtures/detectors/GVR/negative"),
            "fixtures/GVR/negative",
        )
        .expect("directory exists");
        assert_eq!(projects.len(), 5);
        assert!(projects.iter().all(|p| p.ground_truth.is_empty()));
    }
}
