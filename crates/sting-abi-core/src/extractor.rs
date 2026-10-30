use std::error::Error as StdError;
use std::path::{Path, PathBuf};

use crate::{ContractSchema, SourceLanguage};

/// Implemented by every Tier 1/Tier 2 extractor (`TolkExtractor`,
/// `TactExtractor`, `FuncExtractor`) — the single interface
/// `sting-abi-pipeline`'s `ExtractController` dispatches through, not one
/// bespoke API per language.
///
/// **The signature takes a project directory, not a source string.** The
/// first sketch had `extract(&self, source: &str)`, and the real
/// `TolkExtractor` hit the limit immediately:
/// `sting_tolkc::TolkCompiler::compile` takes a `project_dir` *and* an
/// `entrypoint`, because Tolk's `import "..."` resolves other files on
/// disk; a single already-read `&str` has no way to satisfy that. Tact has
/// the same shape of problem (`import "./other.tact";` is real Tact
/// syntax, even where the early fixtures happened not to use it).
/// `FuncExtractor` does not strictly need a whole project either, but the
/// two-argument shape costs it
/// nothing (it can just read `project_dir.join(entrypoint)` itself).
pub trait Extractor: Send + Sync {
    /// Which source language this extractor handles.
    fn language(&self) -> SourceLanguage;

    /// Extracts a [`ContractSchema`] from `entrypoint` (a path relative to
    /// `project_dir`, matching `sting_tolkc::TolkCompiler::compile`'s own
    /// convention so callers don't need to special-case Tolk).
    ///
    /// # Errors
    /// Returns [`ExtractError`] if the underlying compiler/parser fails, or
    /// if its output cannot be normalized into a [`ContractSchema`].
    fn extract(
        &self,
        project_dir: &Path,
        entrypoint: &Path,
    ) -> Result<ContractSchema, ExtractError>;
}

/// Error from any [`Extractor`] implementation. Deliberately generic over
/// the real underlying cause rather than naming it directly: `sting-abi-core`
/// has zero internal `sting-*` dependencies by design (this crate's own
/// top-level doc comment) specifically so the IR stays independent of
/// whichever extractor filled it in — it cannot name
/// `sting_tolkc::TolkcError` or any Tact/`FunC`-specific error type without
/// breaking that.
#[derive(Debug, thiserror::Error)]
pub enum ExtractError {
    /// The underlying compiler/parser subprocess or library call failed.
    #[error("extracting {entrypoint}: {message}")]
    Failed {
        /// The entrypoint path that failed to extract.
        entrypoint: PathBuf,
        /// Human-readable summary of what went wrong.
        message: String,
        /// The underlying error, if the failure has one worth chaining
        /// (a compile error already has its own message *as* `message`
        /// above, so this is more often populated for I/O/parsing
        /// failures than for a compiler's own diagnostic text).
        #[source]
        source: Option<Box<dyn StdError + Send + Sync + 'static>>,
    },
}
