//! The run: read every readable target in the scope, and refuse to call any of it clean unless it
//! was actually read.
//!
//! Two states are deliberately kept apart the whole way through. A file the grammar read completely
//! and found nothing in is clean. A file the grammar read only part of has an unread remainder, and
//! the findings from the part it did read say nothing about the rest. The second never reports as
//! the first: it is counted, named with a position, and ends the run non-zero. That is the whole
//! reason this type carries `unparsed` and `unreadable` separately from `findings`.

use std::path::PathBuf;

use serde::Serialize;
use sting_detectors::DetectorRegistry;
use sting_pipeline::LintController;

use crate::error::HuntError;
use crate::fetch::{self, Listing};
use crate::reach::{Reach, ReachSummary};
use crate::scope::{Scope, Target};

/// The shape of the JSON a run writes. Raised when a field changes meaning or leaves, never for an
/// addition a reader can ignore.
pub const SCHEMA_VERSION: &str = "1.0";

/// One finding, flattened to what a report and a triage entry both need.
#[derive(Debug, Clone, Serialize)]
pub struct Hit {
    /// The detector that fired.
    pub detector: String,
    /// Severity as the detector assigned it, before any dynamic verification.
    pub severity: String,
    /// The file, relative to the repository root, so the line survives being read on another
    /// machine with a different clone path.
    pub file: String,
    /// First line of the finding, 1-indexed.
    pub line: u32,
    /// What the detector said.
    pub message: String,
}

/// One file the grammar read only part of.
#[derive(Debug, Clone, Serialize)]
pub struct Unparsed {
    /// The file, relative to the repository root.
    pub file: String,
    /// `ERROR` or `MISSING`, as the grammar names it.
    pub kind: String,
    /// Where the first unread region starts.
    pub line: u32,
    /// How many such regions the file carries.
    pub regions: usize,
}

/// One file that could not be read or parsed at all.
#[derive(Debug, Clone, Serialize)]
pub struct Unreadable {
    /// The file, relative to the repository root.
    pub file: String,
    /// Why it did not yield an analysis.
    pub reason: String,
}

/// What one target yielded.
#[derive(Debug, Clone, Serialize)]
pub struct TargetRun {
    /// What the published scope calls the asset.
    pub target: &'static str,
    /// Who published the scope, so a finding knows where it is addressed.
    pub programme: &'static str,
    /// The commit the finding is against.
    pub commit: String,
    /// Files the reach pass said were in scope and readable.
    pub files_expected: usize,
    /// Files the static contour actually analysed.
    pub files_analysed: usize,
    /// Files it read only part of. Any entry here fails the run.
    pub unparsed: Vec<Unparsed>,
    /// Files it could not read at all. Any entry here fails the run.
    pub unreadable: Vec<Unreadable>,
    /// What fired.
    pub hits: Vec<Hit>,
}

/// The whole hunt: what could be reached, and what was found where it could.
#[derive(Debug, Clone, Serialize)]
pub struct Report {
    /// The shape of this document, so a reader that stored one run can tell whether a later run is
    /// the same shape before comparing the two.
    pub schema_version: &'static str,
    /// How much of the published scope this analyser can read at all.
    pub reach: ReachSummary,
    /// One entry per readable target.
    pub runs: Vec<TargetRun>,
}

impl Report {
    /// Whether every readable file in the scope was read whole.
    ///
    /// The caller turns this into an exit status. A run with an unread file is not a run with no
    /// findings, and the two must not share a status code.
    #[must_use]
    pub fn every_file_was_read_whole(&self) -> bool {
        self.runs.iter().all(|run| {
            run.unparsed.is_empty()
                && run.unreadable.is_empty()
                && run.files_analysed == run.files_expected
        })
    }

    /// Files the reach pass counted as readable that the run never analysed.
    ///
    /// Always zero unless the two disagree, which is the failure this crate is least able to see any
    /// other way: a file counted and not read leaves a target printed as examined when it was not.
    #[must_use]
    pub fn files_never_reached(&self) -> usize {
        self.runs
            .iter()
            .map(|run| run.files_expected.saturating_sub(run.files_analysed))
            .sum()
    }

    /// Files the grammar read only part of, across the whole hunt.
    #[must_use]
    pub fn unparsed_files(&self) -> usize {
        self.runs.iter().map(|run| run.unparsed.len()).sum()
    }

    /// Files that yielded no analysis at all, across the whole hunt.
    #[must_use]
    pub fn unreadable_files(&self) -> usize {
        self.runs.iter().map(|run| run.unreadable.len()).sum()
    }

    /// Findings, across the whole hunt.
    #[must_use]
    pub fn hits(&self) -> usize {
        self.runs.iter().map(|run| run.hits.len()).sum()
    }
}

/// Measure the reach of the whole published scope, then analyse the part of it that can be read.
///
/// `cache` holds the clones. It is a working directory rather than a product: what the run publishes
/// is the report, and a target is never copied into this repository.
///
/// # Errors
/// Returns [`HuntError`] when a target cannot be fetched, when a scope names a path the pinned
/// commit does not contain, when a published exclusion has gone stale, or when a file inside a
/// scope cannot be read.
pub fn run(cache: &std::path::Path, refresh: bool) -> Result<Report, HuntError> {
    let mut measured = Vec::with_capacity(crate::scope::SCOPE.len());
    let mut readable = Vec::new();
    for target in crate::scope::SCOPE {
        let Scope::Repository {
            url,
            reference,
            paths,
            ..
        } = target.scope
        else {
            measured.push(Reach::unpinned(target));
            continue;
        };
        let listing = fetch::list(url, reference, cache, refresh)?;
        let reach = Reach::measure(target, &listing.commit, &listing.files);
        if reach.files_in_scope == 0 {
            return Err(HuntError::ScopePathMissing {
                target: target.name.to_owned(),
                path: describe(paths),
                commit: listing.commit.clone(),
            });
        }
        if !reach.unmatched_paths.is_empty() {
            return Err(HuntError::ScopePathMissing {
                target: target.name.to_owned(),
                path: reach.unmatched_paths.join(", "),
                commit: listing.commit.clone(),
            });
        }
        let stale = reach.stale_exclusions();
        if !stale.is_empty() {
            return Err(HuntError::StaleExclusion {
                target: target.name.to_owned(),
                rules: stale.join(", "),
                commit: listing.commit.clone(),
            });
        }
        if reach.is_readable() {
            readable.push((target, listing, reach.readable.clone()));
        }
        measured.push(reach);
    }

    let registry = DetectorRegistry::builtin();
    let mut runs = Vec::with_capacity(readable.len());
    for (target, listing, files) in readable {
        runs.push(analyse(
            target, &listing, &files, cache, refresh, &registry,
        )?);
    }

    Ok(Report {
        schema_version: SCHEMA_VERSION,
        reach: ReachSummary::of(measured),
        runs,
    })
}

/// How a scope's paths are written in an error, so a failure names what was looked for.
fn describe(paths: &[&'static str]) -> String {
    if paths.is_empty() {
        "the whole repository".to_owned()
    } else {
        paths.join(", ")
    }
}

/// Clone one target's contents and lint exactly the files the reach pass accepted.
///
/// The file list is not recomputed here, and that is the whole point. Walking the checkout again
/// would be a second derivation of "what is in scope", and a second derivation is free to disagree
/// with the first — to miss a path the programme excluded, to read a name the listing spelled
/// differently, to walk into a symlink. One list, decided once, read once.
fn analyse(
    target: &'static Target,
    listing: &Listing,
    files_in_scope: &[String],
    cache: &std::path::Path,
    refresh: bool,
    registry: &DetectorRegistry,
) -> Result<TargetRun, HuntError> {
    let root = fetch::materialise(target, cache, refresh, listing)?;
    let mut files = Vec::with_capacity(files_in_scope.len());
    for name in files_in_scope {
        let path = root.join(name);
        if !path.is_file() {
            return Err(HuntError::ScopeFileMissing {
                target: target.name.to_owned(),
                file: name.clone(),
                commit: listing.commit.clone(),
            });
        }
        files.push(path);
    }
    let outcome = LintController::new(registry).lint_paths(&files);
    let relative = |file: &PathBuf| {
        file.strip_prefix(&root)
            .unwrap_or(file)
            .to_string_lossy()
            .replace('\\', "/")
    };
    Ok(TargetRun {
        target: target.name,
        programme: target.programme.name,
        commit: listing.commit.clone(),
        files_expected: files_in_scope.len(),
        files_analysed: files.len(),
        unparsed: outcome
            .unparsed
            .iter()
            .map(|file| Unparsed {
                file: relative(&file.file),
                kind: file.region.kind.to_owned(),
                line: file.region.range.start_line,
                regions: file.region.total,
            })
            .collect(),
        unreadable: outcome
            .failures
            .iter()
            .map(|failure| Unreadable {
                file: relative(&failure.file),
                reason: failure.error.to_string(),
            })
            .collect(),
        hits: outcome
            .findings
            .iter()
            .map(|finding| Hit {
                detector: finding.detector_id.to_string(),
                severity: format!("{:?}", finding.severity),
                file: relative(&finding.location.file),
                line: finding.location.start_line,
                message: finding.message.clone(),
            })
            .collect(),
    })
}

#[cfg(test)]
mod tests {
    use super::{Report, SCHEMA_VERSION, TargetRun, Unparsed};
    use crate::reach::ReachSummary;

    fn run_with(unparsed: Vec<Unparsed>, expected: usize, analysed: usize) -> Report {
        Report {
            schema_version: SCHEMA_VERSION,
            reach: ReachSummary::of(Vec::new()),
            runs: vec![TargetRun {
                target: "target",
                programme: "programme",
                commit: "c0ffee".to_owned(),
                files_expected: expected,
                files_analysed: analysed,
                unparsed,
                unreadable: Vec::new(),
                hits: Vec::new(),
            }],
        }
    }

    /// The gate first row: a file the grammar read only part of must not leave the run looking like
    /// a run with nothing to report.
    #[test]
    fn a_partially_read_file_fails_the_run() {
        assert!(run_with(Vec::new(), 1, 1).every_file_was_read_whole());
        let partial = run_with(
            vec![Unparsed {
                file: "contracts/main.tolk".to_owned(),
                kind: "ERROR".to_owned(),
                line: 12,
                regions: 3,
            }],
            1,
            1,
        );
        assert!(!partial.every_file_was_read_whole());
        assert_eq!(partial.unparsed_files(), 1);
    }

    /// A file the reach pass counted and the run never opened is the failure this crate is least
    /// able to see any other way: nothing fired, and nothing was looked at.
    #[test]
    fn a_file_counted_but_never_analysed_fails_the_run() {
        let short = run_with(Vec::new(), 49, 48);
        assert!(!short.every_file_was_read_whole());
        assert_eq!(short.files_never_reached(), 1);
    }
}
