//! `FixController`: applies machine-applicable [`Finding::suggestion`]s back
//! to the files they point at — the write side of `Suggestion` and `--fix`;
//! `sting-detectors` (`DEPR`/`SUA` only, see [`Finding::suggestion`]'s own
//! doc comment) computes *what* to change, this controller performs the
//! actual on-disk edit.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use sting_core::Finding;

/// Applies every `suggestion`-carrying [`Finding`] in a batch back to disk.
pub struct FixController;

/// Result of [`FixController::apply`].
#[derive(Debug, Default)]
pub struct FixOutcome {
    /// Every finding whose suggestion was actually written to disk — kept
    /// as full [`Finding`]s, not just a per-file count, so a caller can
    /// report exactly which detector/line each fix touched (a
    /// `rustc`/`clippy`-style "Fixed N warnings" line needs more than a
    /// count to be useful).
    pub applied: Vec<Finding>,
    /// A file this run tried to fix but could not read from disk.
    pub read_failures: Vec<(PathBuf, std::io::Error)>,
    /// A file this run fixed some suggestions in but could not write back.
    pub write_failures: Vec<(PathBuf, std::io::Error)>,
    /// Findings whose suggestion's byte range overlapped another
    /// suggestion's own range in the same file — skipped, not guessed at
    /// which one should win: on a collision both are skipped with a warning,
    /// because guessing which matters more is guessing.
    pub skipped_overlaps: Vec<Finding>,
}

impl FixController {
    /// Groups `findings` by [`sting_core::Location::file`], and for each
    /// file with at least one `suggestion`, reads it, applies every
    /// non-overlapping suggestion (sorted by **descending** `start_byte` so
    /// an earlier edit's own replacement text never invalidates a later
    /// edit's byte offsets — both were computed against the same original,
    /// unmodified source), and writes the result back.
    ///
    /// Findings with no `suggestion` at all are silently ignored — this is
    /// the expected common case (most detectors never populate one, see
    /// [`sting_core::Finding::suggestion`]'s own doc comment), not an
    /// error.
    #[must_use]
    pub fn apply(&self, findings: &[Finding]) -> FixOutcome {
        let mut by_file: HashMap<&Path, Vec<&Finding>> = HashMap::new();
        for finding in findings {
            if finding.suggestion.is_some() {
                by_file
                    .entry(finding.location.file.as_path())
                    .or_default()
                    .push(finding);
            }
        }

        let mut outcome = FixOutcome::default();
        // Sorted so `applied`'s own order is stable/deterministic across
        // runs (`HashMap` iteration order is not) — no functional
        // requirement, just reproducible output for a human reading it.
        let mut files: Vec<&Path> = by_file.keys().copied().collect();
        files.sort();

        for file in files {
            let mut candidates = by_file.remove(file).unwrap_or_default();
            candidates.sort_by_key(|f| {
                std::cmp::Reverse(f.suggestion.as_ref().map_or(0, |s| s.start_byte))
            });

            let (non_overlapping, overlapping) = partition_non_overlapping(candidates);
            outcome
                .skipped_overlaps
                .extend(overlapping.into_iter().cloned());
            if non_overlapping.is_empty() {
                continue;
            }

            let source = match std::fs::read_to_string(file) {
                Ok(text) => text,
                Err(error) => {
                    outcome.read_failures.push((file.to_path_buf(), error));
                    continue;
                }
            };

            let mut fixed = source;
            for finding in &non_overlapping {
                let Some(s) = &finding.suggestion else {
                    continue;
                };
                fixed.replace_range(s.start_byte..s.end_byte, &s.replacement);
            }

            match std::fs::write(file, &fixed) {
                Ok(()) => outcome.applied.extend(non_overlapping.into_iter().cloned()),
                Err(error) => outcome.write_failures.push((file.to_path_buf(), error)),
            }
        }

        outcome
    }
}

/// Splits `candidates` into (non-overlapping, overlapping) — pairwise: a
/// suggestion is rejected if its own byte range overlaps *any other*
/// candidate's range, full stop. Deliberately not "accept the first one
/// seen and reject only what conflicts with what's already accepted" — that
/// greedy shape would keep exactly one side of a genuine collision instead
/// of neither, which is precisely the "guess which one wins" outcome
/// plan rules out. `O(n²)` — `n` is
/// the count of suggestion-carrying findings in one file, realistically
/// single digits, so the simple pairwise check is not a real cost.
/// Preserves `candidates`' own relative order in both output `Vec`s (needed
/// by [`FixController::apply`]'s caller, which relies on the accepted list
/// still being sorted by descending `start_byte`).
fn partition_non_overlapping(candidates: Vec<&Finding>) -> (Vec<&Finding>, Vec<&Finding>) {
    let ranges: Vec<Option<(usize, usize)>> = candidates
        .iter()
        .map(|finding| {
            finding
                .suggestion
                .as_ref()
                .map(|s| (s.start_byte, s.end_byte))
        })
        .collect();

    let mut accepted: Vec<&Finding> = Vec::new();
    let mut rejected: Vec<&Finding> = Vec::new();

    for (i, finding) in candidates.into_iter().enumerate() {
        let Some((start_i, end_i)) = ranges[i] else {
            continue;
        };
        let overlaps_another = ranges.iter().enumerate().any(|(j, range)| {
            let Some((start_j, end_j)) = range else {
                return false;
            };
            i != j && start_i < *end_j && *start_j < end_i
        });
        if overlaps_another {
            rejected.push(finding);
        } else {
            accepted.push(finding);
        }
    }

    (accepted, rejected)
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "docs/ARCHITECTURE.md §3.2 scopes the no-unwrap/expect policy to library code outside tests"
)]
mod tests {
    use sting_core::{Location, Severity, Suggestion};

    use super::*;

    fn finding_with_suggestion(
        file: &Path,
        start: usize,
        end: usize,
        replacement: &str,
    ) -> Finding {
        Finding {
            detector_id: "DEPR",
            severity: Severity::Info,
            message: "test".to_string(),
            remediation: "test".to_string(),
            location: Location {
                file: file.to_path_buf(),
                start_line: 1,
                start_column: 1,
                end_line: 1,
                end_column: 1,
            },
            confidence: None,
            dynamic_verification: None,
            suggestion: Some(Suggestion {
                start_byte: start,
                end_byte: end,
                replacement: replacement.to_string(),
            }),
        }
    }

    #[test]
    fn applies_a_single_suggestion() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("a.tolk");
        std::fs::write(&path, "val x = createEmptyTuple();\n").expect("write");
        let start = "val x = ".len();
        let end = start + "createEmptyTuple()".len();
        let findings = vec![finding_with_suggestion(&path, start, end, "[]")];

        let outcome = FixController.apply(&findings);
        assert_eq!(outcome.applied.len(), 1);
        assert_eq!(outcome.applied[0].location.file, path);
        assert_eq!(
            std::fs::read_to_string(&path).expect("read back"),
            "val x = [];\n"
        );
    }

    #[test]
    fn applies_two_non_overlapping_suggestions_in_the_same_file_in_one_pass() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("a.tolk");
        std::fs::write(
            &path,
            "val a = createEmptyTuple(); val b = createEmptyMap();\n",
        )
        .expect("write");
        let first_start = "val a = ".len();
        let first_end = first_start + "createEmptyTuple()".len();
        let second_start = "val a = createEmptyTuple(); val b = ".len();
        let second_end = second_start + "createEmptyMap()".len();
        let findings = vec![
            finding_with_suggestion(&path, first_start, first_end, "[]"),
            finding_with_suggestion(&path, second_start, second_end, "[]"),
        ];

        let outcome = FixController.apply(&findings);
        assert_eq!(outcome.applied.len(), 2);
        assert_eq!(
            std::fs::read_to_string(&path).expect("read back"),
            "val a = []; val b = [];\n"
        );
    }

    #[test]
    fn skips_two_overlapping_suggestions_and_leaves_the_file_untouched() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("a.tolk");
        let original = "val x = createEmptyTuple();\n";
        std::fs::write(&path, original).expect("write");
        // Two suggestions whose byte ranges genuinely overlap (contrived —
        // real DEPR/SUA suggestions never do this to each other, see this
        // module's own doc comment).
        let findings = vec![
            finding_with_suggestion(&path, 8, 26, "[]"),
            finding_with_suggestion(&path, 10, 20, "??"),
        ];

        let outcome = FixController.apply(&findings);
        assert!(outcome.applied.is_empty());
        assert_eq!(outcome.skipped_overlaps.len(), 2);
        assert_eq!(std::fs::read_to_string(&path).expect("read back"), original);
    }

    #[test]
    fn ignores_findings_with_no_suggestion() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("a.tolk");
        let original = "val x = 1;\n";
        std::fs::write(&path, original).expect("write");
        let mut finding = finding_with_suggestion(&path, 0, 0, "");
        finding.suggestion = None;

        let outcome = FixController.apply(std::slice::from_ref(&finding));
        assert!(outcome.applied.is_empty());
        assert_eq!(std::fs::read_to_string(&path).expect("read back"), original);
    }

    #[test]
    fn reports_a_read_failure_without_panicking() {
        let findings = vec![finding_with_suggestion(
            Path::new("this-file-does-not-exist-anywhere.tolk"),
            0,
            1,
            "x",
        )];
        let outcome = FixController.apply(&findings);
        assert_eq!(outcome.read_failures.len(), 1);
        assert!(outcome.applied.is_empty());
    }
}
