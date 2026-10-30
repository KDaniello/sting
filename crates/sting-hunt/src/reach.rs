//! How much of a published scope this analyser can read at all.
//!
//! Reach is not precision, and confusing the two is the failure this module exists to prevent. A
//! target written in a language the static contour does not parse produces no findings, and so does
//! a target that was read and found clean; printed side by side without a name for the difference,
//! the first would be read as the second. Every entry below therefore carries the measured mix of
//! what the commit contains, so that "nothing was found here" is always qualified by "and this much
//! of it could be looked at".
//!
//! The mix is counted from the pinned commit rather than declared in the scope table. A language
//! typed by hand into a table stops being true the first time a project migrates, and a migration is
//! precisely the event this measurement is watching for.
//!
//! What counts as readable is asked of the pipeline rather than answered here, so that the number of
//! files this says are reachable and the number the run actually opens cannot drift apart.

use std::collections::BTreeMap;

use serde::Serialize;
use sting_ast::Language;

use crate::scope::{Scope, Target, Tier};

/// What one scope entry turned out to contain.
#[derive(Debug, Clone, Serialize)]
pub struct Reach {
    /// What the published scope calls the asset.
    pub target: &'static str,
    /// Where it sits in the ecosystem.
    pub tier: Tier,
    /// Who published the scope.
    pub programme: &'static str,
    /// The commit the reference resolved to, absent when the scope names no repository.
    pub commit: Option<String>,
    /// Why there is no commit, in the programme's own terms. Carried rather than inferred: a
    /// programme that names products and one whose asset list cannot be read from here are both
    /// unresolvable and not for the same reason, and a report that prints one sentence for both
    /// states something about the second that nobody checked.
    pub unresolved: Option<&'static str>,
    /// Files the scope covers at that commit, after its own exclusions.
    pub files_in_scope: usize,
    /// Of those, the ones the static contour reads.
    pub files_readable: usize,
    /// Those same files by name, repository-relative. This is not a convenience: it is the list the
    /// analysis reads, so that what gets linted and what the reach table counted cannot be two
    /// different derivations that quietly disagree.
    pub readable: Vec<String>,
    /// Paths the published scope names that the pinned commit does not carry. A scope of four files
    /// that silently becomes three is a scope nobody agreed to.
    pub unmatched_paths: Vec<&'static str>,
    /// Each exclusion the programme published, and how many files it removed. Kept per exclusion
    /// rather than as a total: a zero here is a stale rule, and a stale rule hides the day a project
    /// moves code back into scope.
    pub excluded: BTreeMap<&'static str, usize>,
    /// Extension to file count, across everything in scope. The whole mix is kept, not just the
    /// readable share, because it is the evidence that a target was skipped for its language rather
    /// than skipped for convenience.
    pub extensions: BTreeMap<String, usize>,
}

impl Reach {
    /// A scope entry that names no repository, and therefore no commit to measure.
    ///
    /// Counted rather than dropped: a programme that pays for findings but never says which code it
    /// covers is a fact about the ecosystem, and dropping the row would hide it.
    #[must_use]
    pub fn unpinned(target: &'static Target) -> Self {
        Self {
            target: target.name,
            tier: target.tier,
            programme: target.programme.name,
            commit: None,
            unresolved: Some(match target.scope {
                Scope::Unpinned { names } => names,
                Scope::Repository { .. } => "the scope names a repository that was not read",
            }),
            files_in_scope: 0,
            files_readable: 0,
            readable: Vec::new(),
            unmatched_paths: Vec::new(),
            excluded: BTreeMap::new(),
            extensions: BTreeMap::new(),
        }
    }

    /// Measure one pinned target from the file list of its commit.
    #[must_use]
    pub fn measure(target: &'static Target, commit: &str, files: &[String]) -> Self {
        let Scope::Repository {
            paths,
            excluded: exclusions,
            ..
        } = target.scope
        else {
            return Self::unpinned(target);
        };
        let mut extensions = BTreeMap::new();
        let mut excluded: BTreeMap<&'static str, usize> =
            exclusions.iter().map(|rule| (*rule, 0)).collect();
        let mut matched: Vec<&'static str> = Vec::new();
        let mut readable = Vec::new();
        let mut files_in_scope = 0;
        for file in files {
            match named_by(file, paths) {
                Named::No => continue,
                Named::Wholesale => {}
                Named::By(path) => {
                    if !matched.contains(&path) {
                        matched.push(path);
                    }
                }
            }
            if let Some(rule) = exclusions.iter().find(|rule| covers(file, rule)) {
                *excluded.entry(*rule).or_insert(0) += 1;
                continue;
            }
            files_in_scope += 1;
            let extension = extension_of(file);
            if Language::of_extension(&extension).is_some() {
                readable.push(file.clone());
            }
            *extensions.entry(extension).or_insert(0) += 1;
        }
        Self {
            target: target.name,
            tier: target.tier,
            programme: target.programme.name,
            commit: Some(commit.to_owned()),
            unresolved: None,
            files_in_scope,
            files_readable: readable.len(),
            readable,
            unmatched_paths: paths
                .iter()
                .copied()
                .filter(|path| !matched.contains(path))
                .collect(),
            excluded,
            extensions,
        }
    }

    /// Whether the hunt has anything to analyse here.
    #[must_use]
    pub const fn is_readable(&self) -> bool {
        self.files_readable > 0
    }

    /// Exclusions the programme published that removed nothing at this commit.
    #[must_use]
    pub fn stale_exclusions(&self) -> Vec<&'static str> {
        self.excluded
            .iter()
            .filter(|(_, removed)| **removed == 0)
            .map(|(rule, _)| *rule)
            .collect()
    }

    /// Files the scope's own exclusions removed.
    #[must_use]
    pub fn files_excluded(&self) -> usize {
        self.excluded.values().sum()
    }

    /// The extensions in scope, most frequent first, for a report that has room for a few.
    #[must_use]
    pub fn dominant_extensions(&self, how_many: usize) -> Vec<(String, usize)> {
        let mut ranked: Vec<(String, usize)> = self
            .extensions
            .iter()
            .map(|(name, count)| (name.clone(), *count))
            .collect();
        ranked.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        ranked.truncate(how_many);
        ranked
    }
}

/// Whether a repository-relative path is one of the paths a scope named, or sits under one.
///
/// An empty list is the whole repository, which is how a programme that names a project rather than
/// a folder is written down.
fn named_by(file: &str, paths: &[&'static str]) -> Named {
    if paths.is_empty() {
        return Named::Wholesale;
    }
    paths
        .iter()
        .copied()
        .find(|path| covers(file, path))
        .map_or(Named::No, Named::By)
}

/// The three answers to whether a published scope covers a file, kept apart because the middle one
/// is what proves a named path still exists at the pinned commit.
enum Named {
    /// The scope names the whole repository, so no single path claims this file.
    Wholesale,
    /// This path of the scope covers the file.
    By(&'static str),
    /// The scope does not cover the file.
    No,
}

/// Whether a scope path covers a file: the file itself, or anything beneath it.
///
/// The comparison is on whole path components. A scope covering `contracts` does not cover
/// `contracts-mock`, and a plain prefix test would quietly hand the second to the first.
fn covers(file: &str, path: &str) -> bool {
    file == path
        || file
            .strip_prefix(path)
            .is_some_and(|rest| rest.starts_with('/'))
}

/// The extension of a path, or a name for having none, so that every file lands in exactly one
/// bucket and the mix adds up to the files in scope.
fn extension_of(file: &str) -> String {
    let name = file.rsplit('/').next().unwrap_or(file);
    match name.rsplit_once('.') {
        Some((stem, extension)) if !stem.is_empty() => extension.to_ascii_lowercase(),
        _ => "no extension".to_owned(),
    }
}

/// The whole scope, in one line each, plus the two totals the milestone turns on.
#[derive(Debug, Clone, Serialize)]
pub struct ReachSummary {
    /// Every scope entry, in scope-table order.
    pub targets: Vec<Reach>,
    /// Scope entries carrying code the static contour reads.
    pub readable_targets: usize,
    /// Scope entries in total.
    pub total_targets: usize,
    /// Files the static contour reads, across the whole scope.
    pub readable_files: usize,
    /// Files the scope covers, across the whole scope.
    pub total_files: usize,
    /// Files the programmes' own exclusions removed, across the whole scope.
    pub excluded_files: usize,
}

impl ReachSummary {
    /// Fold the per-target measurements into the totals a report leads with.
    #[must_use]
    pub fn of(targets: Vec<Reach>) -> Self {
        Self {
            readable_targets: targets.iter().filter(|reach| reach.is_readable()).count(),
            total_targets: targets.len(),
            readable_files: targets.iter().map(|reach| reach.files_readable).sum(),
            total_files: targets.iter().map(|reach| reach.files_in_scope).sum(),
            excluded_files: targets.iter().map(Reach::files_excluded).sum(),
            targets,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Reach, ReachSummary, covers, extension_of};
    use crate::scope::{Programme, Scope, Target, Tier};

    const PROGRAMME: Programme = Programme {
        name: "programme",
        url: "https://example.invalid",
    };

    const fn target(paths: &'static [&'static str], excluded: &'static [&'static str]) -> Target {
        Target {
            tier: Tier::Ecosystem,
            name: "target",
            programme: PROGRAMME,
            scope: Scope::Repository {
                url: "https://example.invalid/repo",
                reference: "main",
                paths,
                excluded,
            },
        }
    }

    const CONTRACTS: Target = target(&["contracts"], &[]);
    const WITH_MOCK_OUT: Target = target(&["contracts"], &["contracts/mock"]);
    const WITH_STALE_RULE: Target = target(&["contracts"], &["contracts/gone"]);
    const TWO_FILES: Target = target(&["contracts/a.tolk", "contracts/b.tolk"], &[]);
    const UNPINNED: Target = Target {
        tier: Tier::Ecosystem,
        name: "target",
        programme: PROGRAMME,
        scope: Scope::Unpinned { names: "products" },
    };

    /// A scope naming a folder must not swallow its neighbours: `contracts-mock` sits outside a
    /// scope that says `contracts`, and counting it would report against code nobody permitted.
    #[test]
    fn a_scoped_path_does_not_swallow_a_sibling_with_the_same_prefix() {
        assert!(covers("contracts/main.tolk", "contracts"));
        assert!(!covers("contracts-mock/main.tolk", "contracts"));
        assert!(covers("contracts/pool.func", "contracts/pool.func"));
        assert!(!covers("contracts/pool.func.bak", "contracts/pool.func"));
    }

    /// A dotfile has no extension, and calling its name one would invent a language.
    #[test]
    fn every_file_lands_in_exactly_one_language_bucket() {
        assert_eq!(extension_of("contracts/main.tolk"), "tolk");
        assert_eq!(extension_of("contracts/Main.TOLK"), "tolk");
        assert_eq!(extension_of("Makefile"), "no extension");
        assert_eq!(extension_of("dir/.gitignore"), "no extension");
    }

    /// The mix has to add up to the files in scope, or the table understates what was skipped, and
    /// the readable list has to name exactly the files the count claims — in every language the
    /// contour reads, which is what `contracts/c.fc` is doing in this fixture.
    #[test]
    fn the_language_mix_accounts_for_every_file_in_scope() {
        let files = vec![
            "contracts/a.tolk".to_owned(),
            "contracts/b.tolk".to_owned(),
            "contracts/c.fc".to_owned(),
            "contracts/Makefile".to_owned(),
            "tests/d.ts".to_owned(),
        ];
        let reach = Reach::measure(&CONTRACTS, "c0ffee", &files);
        assert_eq!(reach.files_in_scope, 4);
        assert_eq!(reach.files_readable, 3);
        assert_eq!(reach.extensions.values().sum::<usize>(), 4);
        assert_eq!(reach.dominant_extensions(1), vec![("tolk".to_owned(), 2)]);
        assert_eq!(
            reach.readable,
            vec![
                "contracts/a.tolk".to_owned(),
                "contracts/b.tolk".to_owned(),
                "contracts/c.fc".to_owned(),
            ]
        );
    }

    /// A language the contour does not read stays out of the readable list, or the reach number
    /// stops meaning what the report says it means.
    #[test]
    fn a_language_the_contour_does_not_read_is_not_counted_readable() {
        let files = vec![
            "contracts/node.cpp".to_owned(),
            "contracts/tool.py".to_owned(),
            "contracts/main.tact".to_owned(),
        ];
        let reach = Reach::measure(&CONTRACTS, "c0ffee", &files);
        assert_eq!(reach.files_in_scope, 3);
        assert_eq!(reach.files_readable, 0);
        assert!(reach.readable.is_empty());
    }

    /// What a programme excludes is removed from the scope and counted, never silently dropped, and
    /// it must not reach the list the analysis reads.
    #[test]
    fn an_exclusion_removes_files_and_keeps_them_out_of_what_is_read() {
        let files = vec![
            "contracts/a.tolk".to_owned(),
            "contracts/mock/b.tolk".to_owned(),
            "contracts/mock/c.tolk".to_owned(),
        ];
        let reach = Reach::measure(&WITH_MOCK_OUT, "c0ffee", &files);
        assert_eq!(reach.files_in_scope, 1);
        assert_eq!(reach.files_readable, 1);
        assert_eq!(reach.files_excluded(), 2);
        assert_eq!(reach.readable, vec!["contracts/a.tolk".to_owned()]);
        assert!(reach.stale_exclusions().is_empty());
    }

    /// An exclusion that removes nothing is stale, and a stale rule hides the day code moves back
    /// into scope — the same hygiene the linter applies to its own unused suppressions.
    #[test]
    fn an_exclusion_that_removes_nothing_is_named() {
        let files = vec!["contracts/a.tolk".to_owned()];
        let reach = Reach::measure(&WITH_STALE_RULE, "c0ffee", &files);
        assert_eq!(reach.stale_exclusions(), vec!["contracts/gone"]);
    }

    /// A scope of two files that quietly becomes a scope of one is a scope nobody agreed to: the
    /// path the commit no longer carries has to be named, not subtracted.
    #[test]
    fn a_named_path_the_commit_does_not_carry_is_reported() {
        let reach = Reach::measure(&TWO_FILES, "c0ffee", &["contracts/a.tolk".to_owned()]);
        assert_eq!(reach.files_in_scope, 1);
        assert_eq!(reach.unmatched_paths, vec!["contracts/b.tolk"]);
        let whole = Reach::measure(
            &TWO_FILES,
            "c0ffee",
            &["contracts/a.tolk".to_owned(), "contracts/b.tolk".to_owned()],
        );
        assert!(whole.unmatched_paths.is_empty());
    }

    /// An unpinned entry stays in the table with zero files, because a programme that names no
    /// code is a measurement, not an omission.
    #[test]
    fn an_unpinned_target_is_counted_and_not_dropped() {
        let summary = ReachSummary::of(vec![
            Reach::measure(&CONTRACTS, "c0ffee", &["contracts/a.tolk".to_owned()]),
            Reach::unpinned(&UNPINNED),
        ]);
        assert_eq!(summary.total_targets, 2);
        assert_eq!(summary.readable_targets, 1);
        assert_eq!(summary.readable_files, 1);
    }

    /// Measuring an unpinned entry through the pinned path must not invent a commit for it.
    #[test]
    fn measuring_an_unpinned_target_yields_no_commit() {
        let reach = Reach::measure(&UNPINNED, "c0ffee", &["a.tolk".to_owned()]);
        assert!(reach.commit.is_none());
        assert_eq!(reach.files_readable, 0);
    }
}
