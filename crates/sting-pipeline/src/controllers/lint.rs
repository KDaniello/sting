//! `LintController`: file/directory → parsing → findings
//! (`docs/ARCHITECTURE.md`).

use std::path::{Path, PathBuf};
use std::sync::Arc;

use rayon::prelude::*;
use sting_ast::{AstStore, Language};
use sting_core::{Finding, Severity, SuppressionRule};
use sting_detectors::{DetectorContext, DetectorRegistry};
use sting_tolkc::{ProjectStore, TolkCompiler};

/// Runs a [`DetectorRegistry`] against a batch of files, in parallel
/// (`docs/ARCHITECTURE.md` principle 4: `LintController` is one
/// of only two controllers allowed to create `rayon` parallelism directly).
///
/// Suppressions and the severity floor default to none and no floor —
/// [`Self::new`]'s existing callers (`sting-bench`'s `mutate.rs`/`metrics.rs`,
/// which call [`Self::lint_source`] directly to score fixtures/mutations
/// against ground truth) must never have findings silently filtered out
/// from under them, so both are opt-in via [`Self::with_suppressions`]/
/// [`Self::with_min_severity`], not constructor parameters every caller is
/// forced to thread through.
pub struct LintController<'a> {
    registry: &'a DetectorRegistry,
    suppressions: &'a [SuppressionRule],
    min_severity: Option<Severity>,
    tolk: Option<TolkCompiler>,
}

impl<'a> LintController<'a> {
    /// Builds a controller that runs `registry`'s detectors, with no
    /// suppressions and no severity floor applied.
    ///
    /// Also resolves a best-effort [`TolkCompiler`] handle —
    /// [`TolkCompiler::from_default_path`] only fails if the running
    /// executable's own path cannot be
    /// determined (`sting_tolkc::TolkcError::CurrentExe`, essentially never
    /// in practice), never on the vendored binary itself being absent; that
    /// case is handled per-batch, silently, only by [`Self::lint_paths`] (see
    /// `Self::compile_reachable_projects`) — constructing a
    /// controller must stay infallible, every existing caller (this crate's
    /// own tests, `sting-bench`, `sting-cli`) already relies on that.
    #[must_use]
    pub fn new(registry: &'a DetectorRegistry) -> Self {
        Self {
            registry,
            suppressions: &[],
            min_severity: None,
            tolk: TolkCompiler::from_default_path().ok(),
        }
    }

    /// Applies `[[suppressions.rule]]` entries from `Sting.toml` — only
    /// [`Self::lint_paths`] consults these ([`Self::lint_source`] stays
    /// suppression-free regardless, see this struct's own doc comment).
    #[must_use]
    pub fn with_suppressions(mut self, suppressions: &'a [SuppressionRule]) -> Self {
        self.suppressions = suppressions;
        self
    }

    /// Drops findings below `min_severity` — only [`Self::lint_paths`]
    /// applies this, same reasoning as [`Self::with_suppressions`].
    #[must_use]
    pub fn with_min_severity(mut self, min_severity: Option<Severity>) -> Self {
        self.min_severity = min_severity;
        self
    }

    /// Overrides the [`TolkCompiler`] handle [`Self::new`] already resolved
    /// by default (`STING_TOLKC_PATH`, or next to the running executable —
    /// see [`sting_tolkc::default_executable_path`]). That default
    /// resolution is relative to `std::env::current_exe()`, which for a test
    /// binary is some path under `target/.../deps/`, never actually next to
    /// a vendored `vendor/tolk/tolk.exe` — this exists so a test (or any
    /// caller that already knows exactly where its `tolk` binary lives) can
    /// supply an explicit [`TolkCompiler`] instead of relying on that
    /// convention. Only [`Self::lint_paths`] ever attempts a compile at all
    /// ([`Self::lint_source`] never does, see this struct's own doc
    /// comment), so this has no effect on `lint_source` either way.
    #[must_use]
    pub fn with_tolk_compiler(mut self, tolk: TolkCompiler) -> Self {
        self.tolk = Some(tolk);
        self
    }

    /// Lints every file in `paths`. A file that fails to read, parse, *or*
    /// panics anywhere during analysis is recorded in
    /// [`LintOutcome::failures`], not silently dropped and not allowed to
    /// abort the whole batch — one bad file in a batch must not look
    /// identical to "this file is clean" (`docs/ARCHITECTURE.md`
    /// principle 2, "bulkhead": one compartment flooding must not sink the
    /// ship). A panic specifically (as opposed to a returned `Err`) is
    /// caught per file via [`std::panic::catch_unwind`] — found missing
    /// during a post-Block-09 corpus review: `sting` runs detectors over
    /// arbitrary third-party `.tolk` source, and this workspace's own
    /// no-panic-in-library-code policy (`docs/ARCHITECTURE.md` §3.2) is a
    /// convention enforced by review and lints, not a hard runtime
    /// guarantee against every possible bug in every detector — without
    /// this, a single panic anywhere inside `rayon`'s parallel iterator
    /// below would unwind straight out of `lint_paths`, discarding every
    /// other file's already-computed findings and failures along with it.
    #[must_use]
    pub fn lint_paths(&self, paths: &[PathBuf]) -> LintOutcome {
        tracing::debug!(file_count = paths.len(), "linting a batch in parallel");
        let compiled_projects = self.compile_reachable_projects(paths);
        // Each `lint_one` call below is `#[tracing::instrument]`ed, so the
        // per-file span (and its `file` field) travels with the closure onto
        // whichever `rayon` worker thread actually runs it
        // (`docs/API.md`: readable, attributable
        // log output across parallel emulations/lints, not raw unattributed
        // interleaved lines).
        let results: Vec<Result<AnalyzedFile, LintFailure>> = paths
            .par_iter()
            .map(|path| self.lint_one_catching_panics(path, &compiled_projects))
            .collect();

        let mut findings = Vec::new();
        let mut failures = Vec::new();
        let mut unparsed = Vec::new();
        let mut without_a_rule = Vec::new();
        for result in results {
            match result {
                Ok(mut analyzed) => {
                    findings.append(&mut analyzed.findings);
                    if !analyzed.without_a_rule.is_empty() {
                        without_a_rule.push(DetectorsWithoutARule {
                            file: analyzed.file.clone(),
                            language: analyzed.language,
                            detectors: std::mem::take(&mut analyzed.without_a_rule),
                        });
                    }
                    if let Some(region) = analyzed.unparsed {
                        unparsed.push(UnparsedFile {
                            file: analyzed.file,
                            region,
                        });
                    }
                }
                Err(failure) => failures.push(failure),
            }
        }

        let mut rule_matched = vec![false; self.suppressions.len()];
        findings.retain(|finding| {
            let Some(idx) = self.suppressions.iter().position(|rule| {
                rule.detector == finding.detector_id
                    && rule.file == finding.location.file
                    && rule.line == finding.location.start_line
            }) else {
                return true;
            };
            rule_matched[idx] = true;
            false
        });
        let unused_suppressions = self
            .suppressions
            .iter()
            .zip(rule_matched)
            .filter(|(_, matched)| !matched)
            .map(|(rule, _)| rule.clone())
            .collect();

        if let Some(min_severity) = self.min_severity {
            findings.retain(|finding| finding.severity >= min_severity);
        }

        LintOutcome {
            findings,
            failures,
            unparsed,
            without_a_rule,
            unused_suppressions,
        }
    }

    /// Runs [`Self::lint_one`] inside [`std::panic::catch_unwind`], turning
    /// a panic into a [`LintFailure`] with [`LintFileError::Panic`] instead
    /// of letting it unwind past this call (see [`Self::lint_paths`]'s doc
    /// comment for why). `AssertUnwindSafe` is sound here: `lint_one` is a
    /// pure computation over its own arguments returning owned data — no
    /// shared mutable state could be left in a torn, half-updated state by
    /// an unwind partway through it.
    fn lint_one_catching_panics(
        &self,
        path: &Path,
        compiled_projects: &[Arc<ProjectStore>],
    ) -> Result<AnalyzedFile, LintFailure> {
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            self.lint_one(path, compiled_projects)
        }))
        .unwrap_or_else(|payload| {
            Err(LintFailure {
                file: path.to_path_buf(),
                error: LintFileError::Panic(panic_message(&payload)),
            })
        })
    }

    #[tracing::instrument(level = "debug", skip(self, compiled_projects), fields(file = %path.display()))]
    fn lint_one(
        &self,
        path: &Path,
        compiled_projects: &[Arc<ProjectStore>],
    ) -> Result<AnalyzedFile, LintFailure> {
        let source = std::fs::read_to_string(path).map_err(|source| LintFailure {
            file: path.to_path_buf(),
            error: LintFileError::Io(source),
        })?;
        // A file this analyser does not read is refused by name rather than parsed as Tolk and
        // reported on whatever error recovery produced.
        let language = language_of(path).ok_or_else(|| LintFailure {
            file: path.to_path_buf(),
            error: LintFileError::UnknownLanguage,
        })?;
        let tree = sting_ast::parse_as(language, &source).map_err(|source| LintFailure {
            file: path.to_path_buf(),
            error: LintFileError::Parse(source),
        })?;
        // Captured before the tree moves into the context: a file the grammar
        // read half of has to be named, not silently analysed as if whole.
        let unparsed = sting_ast::first_unparsed(&tree);
        let store = AstStore::build_as(language, &tree, &source);
        // `source` is cloned, not moved, specifically so it is still
        // available afterward for inline-suppression scanning — cheap
        // enough for real Tolk source sizes.
        let mut ctx =
            DetectorContext::new(path.to_path_buf(), tree, source.clone()).in_language(language);
        // The first already-compiled project (see `compile_reachable_projects`)
        // that reports a `file_id` for `path` at all — `file_id_for_path`'s
        // own lookup already only ever succeeds for a file that specific
        // compile actually reached (`symbol_types.files` only ever lists
        // what `tolk` itself needed to resolve the entrypoint it was given),
        // so finding *any* match here already proves reachability; which one
        // wins when more than one entrypoint reaches the same file is
        // arbitrary, not something either compile could disagree about for
        // this one file's own signature.
        if let Some((project, file_id)) = compiled_projects
            .iter()
            .find_map(|project| Some((project, project.file_id_for_path(path)?)))
        {
            ctx = ctx.with_project(Arc::clone(project), file_id);
        }
        let (findings, without_a_rule) = self.registry.run_all(&store, &ctx);
        Ok(AnalyzedFile {
            file: path.to_path_buf(),
            findings: filter_inline_suppressed(findings, &source),
            language,
            without_a_rule: without_a_rule.into_iter().map(str::to_owned).collect(),
            unparsed,
        })
    }

    /// Best-effort: compiles one project per file in `paths` that defines
    /// `onInternalMessage` (bounding real `tolk` subprocess spawns to actual
    /// contract entrypoints, not every helper file linted alongside them —
    /// same reasoning as before), via a real, vendored `tolk` binary.
    ///
    /// Returns every project that compiled successfully — [`Self::lint_one`]
    /// then attaches whichever one (if any) reports `file_id_for_path` for
    /// its own file, not necessarily the entrypoint itself. This is the fix
    /// for a real gap found while extending `UTA` across files: the
    /// function whose own body needs cross-file resolution
    /// (`transfer`, or a `storage.tolk`-style file's `load`) usually does
    /// **not** itself define `onInternalMessage` — only the file that
    /// reaches it, transitively, via `import`, does. The original, narrower
    /// design attached a compiled project only to the
    /// same file that triggered its own compile — silently leaving every
    /// other reachable file's own detector pass without one, even though
    /// that same compile's own `symbol_types.files` already fully describes
    /// them too.
    ///
    /// Never a hard error: no vendored binary, a compile failure for any
    /// reason, or zero `onInternalMessage`-bearing files in the batch all
    /// simply produce an empty result — `sting lint` must keep working on a
    /// machine that never fetched `vendor/tolk/`, and on a batch of
    /// intentionally incomplete `.tolk` snippets that were never meant to
    /// compile as a whole project at all (most of this workspace's own
    /// `corpus/fixtures/detectors/*/positive/*.tolk`, for instance).
    fn compile_reachable_projects(&self, paths: &[PathBuf]) -> Vec<Arc<ProjectStore>> {
        if self.tolk.is_none() {
            return Vec::new();
        }
        paths
            .par_iter()
            .filter_map(|path| self.compile_entrypoint(path))
            .collect()
    }

    /// Compiles `path` as its own entrypoint if (and only if) it defines
    /// `onInternalMessage` — the one piece of [`Self::compile_reachable_
    /// projects`] that is genuinely per-file, factored out only so that
    /// function's own `par_iter` stays a plain `filter_map`.
    fn compile_entrypoint(&self, path: &Path) -> Option<Arc<ProjectStore>> {
        // Only Tolk has a project index; a `FunC` file has no entrypoint this can look for, and
        // reading it to parse it with the wrong grammar costs a file read per file for nothing.
        if language_of(path) != Some(Language::Tolk) {
            return None;
        }
        let source = std::fs::read_to_string(path).ok()?;
        let tree = sting_ast::parse(&source).ok()?;
        let store = AstStore::build(&tree, &source);
        store.function_by_name("onInternalMessage")?;
        let tolk = self.tolk.as_ref()?;
        let project_dir = path.parent()?;
        let entrypoint = Path::new(path.file_name()?);
        let output = tolk.compile(project_dir, entrypoint).ok()?;
        Some(Arc::new(ProjectStore::new(output.symbol_types)))
    }

    /// Lints already-loaded source text directly, no file I/O — the
    /// building block `Self::lint_one` wraps for real files, and what
    /// `sting-bench` uses for merged multi-file corpus projects and
    /// in-memory mutation-engine instances that are never written to disk
    /// (`docs/domains/precision-sources.md`) — a file-path-only interface
    /// cannot serve either of those, so this is the actual shared core
    /// `docs/ARCHITECTURE.md`'s "sting bench uses `LintController`"
    /// refers to.
    ///
    /// # Errors
    /// Returns [`sting_ast::ParseError`] if `tree-sitter-tolk` fails to
    /// parse `source` (a grammar-load failure, not malformed Tolk source —
    /// see [`sting_ast::parse`]'s doc comment).
    pub fn lint_source(
        &self,
        name: PathBuf,
        source: String,
    ) -> Result<Vec<Finding>, sting_ast::ParseError> {
        self.lint_source_as(Language::Tolk, name, source)
    }

    /// The same, in a language the caller names.
    ///
    /// The language is a parameter rather than a guess from `name`, because the names this is given
    /// are synthetic — a merged corpus project, an in-memory mutation — and inferring a language
    /// from a name nobody had to make truthful would put the wrong grammar on a real measurement.
    ///
    /// # Errors
    /// Returns [`sting_ast::ParseError`] if the grammar fails to load, per [`Self::lint_source`].
    pub fn lint_source_as(
        &self,
        language: Language,
        name: PathBuf,
        source: String,
    ) -> Result<Vec<Finding>, sting_ast::ParseError> {
        let tree = sting_ast::parse_as(language, &source)?;
        let store = AstStore::build_as(language, &tree, &source);
        let ctx = DetectorContext::new(name, tree, source).in_language(language);
        Ok(self.registry.run_all(&store, &ctx).0)
    }
}

/// Result of [`LintController::lint_paths`].
#[derive(Debug)]
pub struct LintOutcome {
    /// Findings from every successfully analyzed file, after suppression
    /// filtering.
    pub findings: Vec<Finding>,
    /// Files that could not be read or parsed, and why.
    pub failures: Vec<LintFailure>,
    /// Files the grammar read only part of. Their findings are in `findings`
    /// — what parsed was analysed — and the caller is expected to refuse to
    /// call such a run clean: a file with an `ERROR` node has an unread
    /// remainder, and silence about it is indistinguishable from a file with
    /// nothing in it.
    pub unparsed: Vec<UnparsedFile>,
    /// Detectors held back on a file because they have no rule for its language, per file.
    ///
    /// Empty for a Tolk-only run, which is what every run was. Reported rather than dropped: a
    /// detector that never looked is not a detector that found nothing, and a report that shows
    /// only the second credits this analyser for code no rule of its own examined.
    pub without_a_rule: Vec<DetectorsWithoutARule>,
    /// `[[suppressions.rule]]` entries that matched zero findings this
    /// run — a stale exception, most often left behind after the code it
    /// once excused was fixed — the same hygiene check Misti's own
    /// config-suppression mechanism performs. The caller decides how loudly to report these;
    /// this controller only detects them.
    pub unused_suppressions: Vec<SuppressionRule>,
}

/// The detectors one file's language had no rule for.
#[derive(Debug, Clone)]
pub struct DetectorsWithoutARule {
    /// The file.
    pub file: PathBuf,
    /// The language it was read in.
    pub language: Language,
    /// The ids that were not run against it.
    pub detectors: Vec<String>,
}

/// One file the grammar read only part of.
#[derive(Debug, Clone)]
pub struct UnparsedFile {
    /// The file.
    pub file: PathBuf,
    /// The first region the grammar could not read, and how many there are.
    pub region: sting_ast::UnparsedRegion,
}

/// One file's analysis: what was found, and what the grammar could not read.
struct AnalyzedFile {
    file: PathBuf,
    findings: Vec<Finding>,
    language: Language,
    without_a_rule: Vec<String>,
    unparsed: Option<sting_ast::UnparsedRegion>,
}

/// Line-above-the-finding inline suppression comment:
/// `// sting:allow(ID1,ID2) reason text`. Deliberately not a straight copy
/// of Misti's `// @misti:suppress ID1,ID2` — a `#[allow(...)]`-shaped spelling matches
/// this codebase's own style better, and, the actual functional
/// difference: **a reason is mandatory**, matching
/// [`SuppressionRule::reason`]'s equivalent config-side requirement (Misti's
/// inline form has no such requirement at all). A comment missing the
/// reason, or otherwise not matching this exact shape, is simply not
/// recognized as a suppression — the finding stays visible. Failing open
/// (more findings shown than intended) is the safe direction for a
/// malformed comment to fail in, not failing closed.
fn inline_suppression_line_covers(line: &str, detector_id: &str) -> bool {
    let Some(after_comment) = line.trim_start().strip_prefix("//") else {
        return false;
    };
    let Some(after_prefix) = after_comment.trim_start().strip_prefix("sting:allow(") else {
        return false;
    };
    let Some((ids_part, reason)) = after_prefix.split_once(')') else {
        return false;
    };
    if reason.trim().is_empty() {
        return false;
    }
    ids_part
        .split(',')
        .map(str::trim)
        .any(|id| id == detector_id)
}

/// Drops any finding whose line immediately above (in `source`) carries a
/// matching [`inline_suppression_line_covers`] comment. `source` must be
/// the exact text `findings` were computed from — a mismatched `source`
/// would check the wrong lines entirely, silently.
fn filter_inline_suppressed(findings: Vec<Finding>, source: &str) -> Vec<Finding> {
    let lines: Vec<&str> = source.lines().collect();
    findings
        .into_iter()
        .filter(|finding| {
            let Some(line_above_idx) = finding.location.start_line.checked_sub(2) else {
                return true;
            };
            let Some(text) = lines.get(line_above_idx as usize) else {
                return true;
            };
            !inline_suppression_line_covers(text, finding.detector_id)
        })
        .collect()
}

/// One file [`LintController::lint_paths`] could not analyze.
#[derive(Debug)]
pub struct LintFailure {
    /// The file that failed.
    pub file: PathBuf,
    /// Why.
    pub error: LintFileError,
}

/// Why a single file could not be analyzed.
#[derive(Debug, thiserror::Error)]
pub enum LintFileError {
    /// The file could not be read.
    #[error("failed to read file: {0}")]
    Io(#[source] std::io::Error),
    /// `tree-sitter-tolk` could not parse the file (a grammar-load failure,
    /// not malformed Tolk source — see [`sting_ast::parse`]'s doc comment:
    /// malformed source becomes `ERROR` nodes in an otherwise-successful
    /// parse, not this variant).
    #[error("failed to parse: {0}")]
    Parse(#[source] sting_ast::ParseError),
    /// Analyzing this file panicked somewhere inside the detector registry —
    /// a bug in a detector (or a framework crate it calls into), not a
    /// property of the file itself, but still this file's own failure to
    /// report, not a reason to lose every other file's results (see
    /// [`LintController::lint_paths`]'s doc comment). The message is
    /// best-effort: whatever text the panic payload actually carried, or a
    /// generic placeholder for a payload that carried none (`panic_message`).
    #[error("analysis panicked: {0}")]
    Panic(String),
    /// The file is not written in a language this analyser reads. Named rather than skipped: a file
    /// nobody looked at must not leave a run looking like a run with nothing to report.
    #[error("no grammar reads this file's extension")]
    UnknownLanguage,
}

/// Extracts a human-readable message from a caught panic payload
/// (`Box<dyn Any + Send>`). Covers the two shapes `std::panic::catch_unwind`
/// actually produces in practice — `&'static str` for `panic!("literal")`
/// and `String` for `panic!("{}", ...)`/`.expect("...")` — falling back to a
/// generic message for anything else (a custom payload type via
/// `std::panic::panic_any`, not used anywhere in this workspace today but
/// not assumed impossible either).
fn panic_message(payload: &(dyn std::any::Any + Send)) -> String {
    if let Some(s) = payload.downcast_ref::<&str>() {
        (*s).to_string()
    } else if let Some(s) = payload.downcast_ref::<String>() {
        s.clone()
    } else if let Some(inner) = payload.downcast_ref::<Box<dyn std::any::Any + Send>>() {
        // Empirically observed, not merely theoretical: a panic that
        // propagates through this workspace's actual `lint_one` ->
        // `lint_source` -> `DetectorRegistry::run_all` -> `Detector::check`
        // call chain (confirmed via a scratch diagnostic test during this
        // fix, not assumed) arrives at `catch_unwind` already wrapped one
        // extra level deep as `Box<dyn Any + Send>` holding the *real*
        // `String`/`&str` payload, rather than that payload directly — a
        // bare `registry.run_all` call with no intervening call chain does
        // not show this rewrapping, so something in that stack (not
        // pinpointed to one exact frame) re-boxes it. Recursing one level
        // unwraps it either way, without needing to know exactly why.
        panic_message(inner.as_ref())
    } else {
        "non-string panic payload".to_string()
    }
}

/// Which grammar a file is read with, decided by its extension.
///
/// A thin name over [`sting_ast::Language::of_path`], which is where the mapping lives: it is a
/// fact about grammars, and this crate is not the one that owns them. `None` for a file this
/// analyser does not read at all — the caller is expected to say so rather than to hand it to a
/// parser and report whatever error recovery produced.
#[must_use]
pub fn language_of(path: &Path) -> Option<Language> {
    Language::of_path(path)
}

/// Expands `path` into the `.tolk` files it names: `path` itself if it is a
/// file, or every `.tolk` file found by walking `path` recursively if it is
/// a directory: `sting lint <path>` takes either
/// (`docs/domains/detectors.md`).
///
/// # Errors
/// Returns [`std::io::Error`] if `path` does not exist as either a file or
/// a directory, or a directory along the walk cannot be read.
pub fn resolve_tolk_files(path: &Path) -> std::io::Result<Vec<PathBuf>> {
    if path.is_file() {
        return Ok(vec![path.to_path_buf()]);
    }
    if !path.is_dir() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            format!("{} is neither a file nor a directory", path.display()),
        ));
    }
    let mut files = Vec::new();
    resolve_recursive(path, &mut files)?;
    files.sort();
    Ok(files)
}

fn resolve_recursive(dir: &Path, out: &mut Vec<PathBuf>) -> std::io::Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let path = entry?.path();
        // A symlinked *directory* is not followed. `is_dir` resolves the link, so a tree containing
        // `x -> .` — which a repository may carry, deliberately or by accident — would recurse until
        // the stack gives out, and an abort produces no report and no error at all. A symlinked file
        // is read like any other: dropping it would leave a file the reach pass counted and nothing
        // ever opened, which is the silence this walk exists to make impossible.
        let is_link =
            std::fs::symlink_metadata(&path).is_ok_and(|meta| meta.file_type().is_symlink());
        if is_link && path.is_dir() {
            continue;
        }
        if path.is_dir() {
            resolve_recursive(&path, out)?;
        } else if language_of(&path).is_some() {
            out.push(path);
        }
    }
    Ok(())
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "docs/ARCHITECTURE.md §3.2 scopes the no-unwrap/expect policy to library code outside tests"
)]
mod tests {
    use std::path::PathBuf;

    use sting_ast::AstStore;
    use sting_core::{DetectorId, Finding, Severity};
    use sting_detectors::{Detector, DetectorContext, DetectorKind, DetectorRegistry};

    use super::{LintController, LintFileError, panic_message, resolve_tolk_files};

    #[test]
    fn panic_message_extracts_a_plain_string_literal_payload() {
        let payload: Box<dyn std::any::Any + Send> = Box::new("a literal panic message");
        assert_eq!(panic_message(payload.as_ref()), "a literal panic message");
    }

    #[test]
    fn panic_message_extracts_a_formatted_string_payload() {
        let payload: Box<dyn std::any::Any + Send> = Box::new(format!("value was {}", 42));
        assert_eq!(panic_message(payload.as_ref()), "value was 42");
    }

    #[test]
    fn panic_message_recurses_through_a_boxed_any_wrapper() {
        // Locks in the empirically observed real shape (see this function's
        // own doc comment): a panic propagating through this workspace's
        // actual lint_one -> lint_source -> run_all -> check call chain
        // arrives one level deeper-boxed than a bare panic! payload would
        // be, so panic_message must unwrap through at least one such layer.
        let inner: Box<dyn std::any::Any + Send> = Box::new("nested message".to_string());
        let outer: Box<dyn std::any::Any + Send> = Box::new(inner);
        assert_eq!(panic_message(outer.as_ref()), "nested message");
    }

    #[test]
    fn panic_message_falls_back_for_a_non_string_payload() {
        let payload: Box<dyn std::any::Any + Send> = Box::new(404_i32);
        assert_eq!(panic_message(payload.as_ref()), "non-string panic payload");
    }

    /// A detector that panics only when analyzing a file named
    /// `poison_name` — used to prove [`LintController::lint_paths`]
    /// isolates a panic to the one file that triggered it instead of
    /// losing the whole batch (a real detector bug would look like this:
    /// broken on one specific real-world input shape, fine on everything
    /// else, not "always panics" — that shape is what this test mirrors).
    struct PanicsOnFileNameDetector {
        poison_name: &'static str,
    }
    impl Detector for PanicsOnFileNameDetector {
        fn id(&self) -> DetectorId {
            "PANICS"
        }
        fn kind(&self) -> DetectorKind {
            DetectorKind::Ast
        }
        fn default_severity(&self) -> Severity {
            Severity::Low
        }
        fn check(&self, _store: &AstStore, ctx: &DetectorContext) -> Vec<Finding> {
            assert!(
                ctx.file().file_name().and_then(|n| n.to_str()) != Some(self.poison_name),
                "deliberate test panic for {}",
                self.poison_name
            );
            Vec::new()
        }
    }

    fn corpus_dir() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../corpus/fixtures/tolk-bench")
    }

    #[test]
    fn lint_source_finds_a_finding_without_touching_disk() {
        let registry = DetectorRegistry::builtin();
        let controller = LintController::new(&registry);
        let source =
            "fun onExternalMessage(inMsgBody: slice) {\n    val x = inMsgBody.loadUint(32);\n}\n";
        let findings = controller
            .lint_source("in-memory.tolk".into(), source.to_string())
            .expect("parses");
        assert!(findings.iter().any(|f| f.detector_id == "LEP"));
    }

    #[test]
    fn lint_source_propagates_a_parse_error_instead_of_panicking() {
        // A grammar-load failure is the only real error case here; feeding
        // it genuinely malformed Tolk is not one (tree-sitter error-recovers
        // instead, see sting_ast::parse's own doc comment) — this test just
        // confirms the Result plumbing compiles and an Ok source never
        // errors, since a real LanguageError isn't reproducible from a test.
        let registry = DetectorRegistry::builtin();
        let controller = LintController::new(&registry);
        let result = controller.lint_source("x.tolk".into(), "fun f( { garbage".to_string());
        assert!(
            result.is_ok(),
            "malformed source error-recovers, not a hard parse error"
        );
    }

    #[test]
    fn resolve_tolk_files_returns_a_single_file_as_is() {
        let file = corpus_dir().join("storage.tolk");
        let resolved = resolve_tolk_files(&file).expect("file exists");
        assert_eq!(resolved, vec![file]);
    }

    #[test]
    fn resolve_tolk_files_walks_a_directory_for_tolk_files_only() {
        let resolved = resolve_tolk_files(&corpus_dir()).expect("directory exists");
        // 5 real .tolk fixtures plus SOURCE.md, which must be excluded.
        assert_eq!(resolved.len(), 5);
        assert!(
            resolved
                .iter()
                .all(|p| p.extension().is_some_and(|e| e == "tolk"))
        );
    }

    #[test]
    fn resolve_tolk_files_errors_on_a_nonexistent_path() {
        let missing = corpus_dir().join("does-not-exist.tolk");
        assert!(resolve_tolk_files(&missing).is_err());
    }

    #[test]
    fn lint_paths_reports_findings_across_multiple_real_files() {
        let registry = DetectorRegistry::builtin();
        let controller = LintController::new(&registry);
        let paths = resolve_tolk_files(&corpus_dir()).expect("directory exists");
        let outcome = controller.lint_paths(&paths);
        assert!(
            outcome.failures.is_empty(),
            "all real fixtures must parse cleanly"
        );
        // JettonMinter.tolk's UBM finding (`docs/domains/detectors.md`) is
        // the one known real finding in this corpus.
        assert!(
            outcome.findings.iter().any(|f| f.detector_id == "UBM"),
            "expected at least the known JettonMinter.tolk UBM finding"
        );
    }

    #[test]
    fn lint_paths_isolates_a_panic_to_the_file_that_triggered_it() {
        // Real gap this fix closes: without catch_unwind, a panic anywhere
        // inside `rayon`'s parallel iterator used to unwind straight out of
        // `lint_paths`, discarding every other file's already-computed
        // results along with it — a single detector bug on one real-world
        // file could silently blank out an entire batch scan instead of
        // surfacing as one LintFailure.
        let registry = DetectorRegistry::empty().with(Box::new(PanicsOnFileNameDetector {
            poison_name: "storage.tolk",
        }));
        let controller = LintController::new(&registry);
        let poison = corpus_dir().join("storage.tolk");
        let other = corpus_dir().join("JettonMinter.tolk");
        let outcome = controller.lint_paths(&[poison.clone(), other.clone()]);

        assert_eq!(
            outcome.failures.len(),
            1,
            "only the poisoned file should be reported as a failure"
        );
        assert_eq!(outcome.failures[0].file, poison);
        assert!(
            matches!(outcome.failures[0].error, LintFileError::Panic(ref msg) if msg.contains("deliberate test panic")),
            "expected a Panic failure carrying the panic's own message, got {:?}",
            outcome.failures[0].error
        );
        assert!(
            outcome.failures.iter().all(|f| f.file != other),
            "the other, non-panicking file's own result must not be lost"
        );
    }

    #[test]
    fn lint_paths_reports_a_failure_for_an_unreadable_file_without_dropping_the_batch() {
        let registry = DetectorRegistry::builtin();
        let controller = LintController::new(&registry);
        let missing = corpus_dir().join("does-not-exist.tolk");
        let real_file = corpus_dir().join("storage.tolk");
        let outcome = controller.lint_paths(&[missing.clone(), real_file]);
        assert_eq!(outcome.failures.len(), 1);
        assert_eq!(outcome.failures[0].file, missing);
    }

    use super::{filter_inline_suppressed, inline_suppression_line_covers};
    use sting_core::{Location, SuppressionRule};

    #[test]
    fn inline_suppression_recognizes_the_documented_shape() {
        assert!(inline_suppression_line_covers(
            "    // sting:allow(AC) reason text here",
            "AC"
        ));
    }

    #[test]
    fn inline_suppression_requires_a_non_empty_reason() {
        assert!(!inline_suppression_line_covers(
            "    // sting:allow(AC)",
            "AC"
        ));
        assert!(!inline_suppression_line_covers(
            "    // sting:allow(AC)   ",
            "AC"
        ));
    }

    #[test]
    fn inline_suppression_matches_any_of_several_comma_separated_ids() {
        assert!(inline_suppression_line_covers(
            "// sting:allow(AC,UBM) both fire on this save",
            "UBM"
        ));
        assert!(!inline_suppression_line_covers(
            "// sting:allow(AC,UBM) both fire on this save",
            "LEP"
        ));
    }

    #[test]
    fn inline_suppression_does_not_match_an_ordinary_comment() {
        assert!(!inline_suppression_line_covers(
            "// just a normal explanatory comment",
            "AC"
        ));
    }

    fn finding_at(detector_id: sting_core::DetectorId, line: u32) -> Finding {
        Finding {
            detector_id,
            severity: Severity::Medium,
            message: "test finding".to_string(),
            remediation: "test remediation".to_string(),
            location: Location {
                file: PathBuf::from("x.tolk"),
                start_line: line,
                start_column: 1,
                end_line: line,
                end_column: 1,
            },
            confidence: None,
            dynamic_verification: None,
            suggestion: None,
        }
    }

    #[test]
    fn filter_inline_suppressed_drops_a_finding_covered_by_the_line_above() {
        let source = "fun f() {\n    // sting:allow(AC) intentionally public\n    doThing();\n}\n";
        let findings = vec![finding_at("AC", 3)];
        assert!(filter_inline_suppressed(findings, source).is_empty());
    }

    #[test]
    fn filter_inline_suppressed_keeps_a_finding_with_no_comment_above() {
        let source = "fun f() {\n    doThing();\n    doThing();\n}\n";
        let findings = vec![finding_at("AC", 3)];
        assert_eq!(filter_inline_suppressed(findings, source).len(), 1);
    }

    #[test]
    fn filter_inline_suppressed_keeps_a_finding_on_the_first_line() {
        // No line above line 1 to even check — must not underflow/panic.
        let source = "fun f() { doThing(); }\n";
        let findings = vec![finding_at("AC", 1)];
        assert_eq!(filter_inline_suppressed(findings, source).len(), 1);
    }

    #[test]
    fn lint_paths_applies_a_config_suppression_to_the_real_known_ubm_finding() {
        // JettonMinter.tolk:86:23's UBM finding is the same stable, known
        // real finding lint_paths_reports_findings_across_multiple_real_files
        // already relies on.
        let registry = DetectorRegistry::builtin();
        let rules = vec![SuppressionRule {
            detector: "UBM".to_string(),
            file: corpus_dir().join("JettonMinter.tolk"),
            line: 86,
            reason: "test: verifying config-rule suppression against a real finding".to_string(),
        }];
        let controller = LintController::new(&registry).with_suppressions(&rules);
        let paths = vec![corpus_dir().join("JettonMinter.tolk")];
        let outcome = controller.lint_paths(&paths);

        assert!(
            !outcome.findings.iter().any(|f| f.detector_id == "UBM"),
            "the suppressed UBM finding must not appear in the output"
        );
        assert!(
            outcome.unused_suppressions.is_empty(),
            "the rule matched a real finding, it must not be reported as unused"
        );
    }

    #[test]
    fn lint_paths_reports_a_suppression_that_matched_nothing_as_unused() {
        let registry = DetectorRegistry::builtin();
        let rules = vec![SuppressionRule {
            detector: "UBM".to_string(),
            file: corpus_dir().join("JettonMinter.tolk"),
            line: 9999,
            reason: "test: a line that will never actually have a finding".to_string(),
        }];
        let controller = LintController::new(&registry).with_suppressions(&rules);
        let paths = vec![corpus_dir().join("JettonMinter.tolk")];
        let outcome = controller.lint_paths(&paths);

        assert_eq!(outcome.unused_suppressions.len(), 1);
        assert_eq!(outcome.unused_suppressions[0].line, 9999);
        assert!(
            outcome.findings.iter().any(|f| f.detector_id == "UBM"),
            "an unmatched rule must not suppress the real finding at its real line"
        );
    }

    #[test]
    fn with_min_severity_drops_every_finding_below_the_floor() {
        let registry = DetectorRegistry::builtin();
        let paths = resolve_tolk_files(&corpus_dir()).expect("directory exists");
        let unfiltered = LintController::new(&registry).lint_paths(&paths);
        assert!(
            unfiltered
                .findings
                .iter()
                .any(|f| f.severity < sting_core::Severity::Critical),
            "the real corpus must contain at least one below-Critical finding for this test to mean anything"
        );

        let filtered = LintController::new(&registry)
            .with_min_severity(Some(sting_core::Severity::Critical))
            .lint_paths(&paths);
        assert!(
            filtered
                .findings
                .iter()
                .all(|f| f.severity >= sting_core::Severity::Critical),
            "every surviving finding must be at or above the floor"
        );
        assert!(
            filtered.findings.len() < unfiltered.findings.len(),
            "the floor must have actually dropped something"
        );
    }

    #[test]
    fn with_min_severity_none_keeps_every_finding() {
        let registry = DetectorRegistry::builtin();
        let paths = resolve_tolk_files(&corpus_dir()).expect("directory exists");
        let without_builder_call = LintController::new(&registry).lint_paths(&paths);
        let with_explicit_none = LintController::new(&registry)
            .with_min_severity(None)
            .lint_paths(&paths);
        assert_eq!(
            without_builder_call.findings.len(),
            with_explicit_none.findings.len()
        );
    }

    #[test]
    fn lint_source_is_never_severity_filtered_even_with_a_floor_configured() {
        // Same load-bearing guarantee as the suppression case just above.
        let registry = DetectorRegistry::builtin();
        let controller =
            LintController::new(&registry).with_min_severity(Some(sting_core::Severity::Critical));
        let source =
            "fun onExternalMessage(inMsgBody: slice) {\n    val x = inMsgBody.loadUint(32);\n}\n";
        let findings = controller
            .lint_source("in-memory.tolk".into(), source.to_string())
            .expect("parses");
        assert!(
            findings.iter().any(|f| f.detector_id == "LEP"),
            "LEP is below Critical, lint_source must ignore the configured floor entirely"
        );
    }

    #[test]
    fn lint_source_is_never_suppression_filtered_even_with_rules_configured() {
        // The load-bearing guarantee for sting-bench (see LintController's
        // own doc comment): with_suppressions must not affect lint_source at
        // all, only lint_paths.
        let registry = DetectorRegistry::builtin();
        let rules = vec![SuppressionRule {
            detector: "LEP".to_string(),
            file: PathBuf::from("in-memory.tolk"),
            line: 2,
            reason: "test: must be ignored by lint_source".to_string(),
        }];
        let controller = LintController::new(&registry).with_suppressions(&rules);
        let source =
            "fun onExternalMessage(inMsgBody: slice) {\n    val x = inMsgBody.loadUint(32);\n}\n";
        let findings = controller
            .lint_source("in-memory.tolk".into(), source.to_string())
            .expect("parses");
        assert!(
            findings.iter().any(|f| f.detector_id == "LEP"),
            "lint_source must ignore configured suppressions entirely"
        );
    }

    #[test]
    fn compile_entrypoint_is_none_without_a_working_tolk_compiler() {
        // On a machine (or, as here, a test binary path) with no vendored
        // `tolk` resolvable, `LintController::new`'s own default resolution
        // still produces a `TolkCompiler` handle (construction never
        // verifies the binary exists — see its own doc comment) that will
        // simply fail to spawn; this proves that failure degrades to `None`
        // silently rather than propagating as a hard error anywhere.
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("main.tolk");
        std::fs::write(
            &path,
            "fun onInternalMessage(in: InMessage) {\n    doThing();\n}\n",
        )
        .expect("write main.tolk");
        let registry = DetectorRegistry::builtin();
        let controller = LintController::new(&registry);
        assert!(controller.compile_entrypoint(&path).is_none());
    }

    #[test]
    fn compile_entrypoint_is_none_without_on_internal_message() {
        // The entry gate itself, independent of whether `tolk` is even
        // available — bounds real compile attempts to actual contract
        // entrypoints (see this method's own doc comment).
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("helper.tolk");
        std::fs::write(&path, "fun helper() {\n    doThing();\n}\n").expect("write helper.tolk");
        let registry = DetectorRegistry::builtin();
        let controller = LintController::new(&registry);
        assert!(controller.compile_entrypoint(&path).is_none());
    }

    #[test]
    fn compile_reachable_projects_is_empty_without_a_working_tolk_compiler() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("main.tolk");
        std::fs::write(
            &path,
            "fun onInternalMessage(in: InMessage) {\n    doThing();\n}\n",
        )
        .expect("write main.tolk");
        let registry = DetectorRegistry::builtin();
        let controller = LintController::new(&registry);
        assert!(controller.compile_reachable_projects(&[path]).is_empty());
    }
}
