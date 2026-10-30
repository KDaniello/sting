//! `RegistryController`: verifies a submitted `(address, source)` pair by
//! recompiling the source and comparing byte-for-byte against the real
//! on-chain code (fetched via `sting_corpus::CorpusController`, the same
//! provider the security branch already uses) — the Sourcify verification
//! model (`docs/domains/service.md`).
//!
//! **Language coverage, honestly bounded**: Tact and Tolk are both
//! verifiable in this crate — see `bytecode`'s own module doc comment for the
//! Tolk pipeline — while `FunC` categorically cannot be (Tier 2 has no real
//! compiler to recompile against at all). Requesting verification for an
//! unsupported language fails with a named
//! [`RegistryError::NoBytecodeSourceForLanguage`], never a silent
//! `NoMatch`.
//!
//! **`MatchStatus::Partial` is a real variant with no producing code path
//! yet** — the empirical check
//! (`docs/domains/service.md`: compile the
//! same Tact source twice, once with a different scratch path and once
//! with an added comment, see what differs) found **nothing** differs:
//! `crates/sting-abi-tact/tests/code_boc.rs`'s own tests confirm `tact`
//! leaks neither scratch-path nor comment metadata into its `.code.boc`
//! output. So for Tact specifically, a byte match already *is* a full
//! match and a byte mismatch already reflects a genuine semantic
//! difference — there is no metadata-only middle ground to construct
//! `Partial` from, on either axis actually tested. The variant stays in
//! the type (the interface it was sketched against always had it, and a future
//! language or a compiler-version axis not yet tested might
//! give it a real meaning) rather than being deleted on the strength of
//! one dimension of one language's empirical result.

mod bytecode;
mod store;

pub use bytecode::{BytecodeError, BytecodeSource, TactBytecodeSource, TolkBytecodeSource};
pub use store::{ORIGIN_ORGANIC_REGISTRY, StoreError, StoredVerification, VerifiedSchemaStore};

use std::collections::HashMap;
use std::path::Path;

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64;

use sting_abi_core::{ContractSchema, SourceLanguage};
use sting_abi_pipeline::{ExtractController, LanguageHint, PipelineError};
use sting_corpus::{ContractTarget, CorpusController, CorpusError, FetchOptions};

/// The Sourcify-model verification result — `Full`/`Partial`/`NoMatch`,
/// the same terminology, not reinvented
/// (`docs/domains/service.md`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MatchStatus {
    /// Recompiled bytecode is byte-identical to the on-chain code.
    Full,
    /// Recompiled bytecode matches modulo non-semantic metadata — see this
    /// crate's own module doc comment for why no real code path produces
    /// this today.
    Partial,
    /// Recompiled bytecode does not match the on-chain code at all.
    NoMatch,
}

/// The result of [`RegistryController::verify`].
#[derive(Debug, Clone)]
pub struct VerificationOutcome {
    /// The match classification.
    pub status: MatchStatus,
    /// The extracted schema — `Some` only for [`MatchStatus::Full`]/
    /// [`MatchStatus::Partial`], never for [`MatchStatus::NoMatch`] (an
    /// unverified source's schema is not trustworthy output).
    pub schema: Option<ContractSchema>,
}

/// Error from [`RegistryController::verify`].
#[derive(Debug, thiserror::Error)]
pub enum RegistryError {
    /// Resolving the on-chain code for `target` failed. `target` is boxed
    /// — `ContractTarget` is large enough on its own (an owned `String`/
    /// two `PathBuf`s in its biggest variant) that `clippy::result_large_err`
    /// flags an unboxed copy in every `RegistryError` variant, even ones
    /// that never carry one.
    #[error("failed to resolve on-chain code for {target:?}: {source}")]
    Corpus {
        /// The target that failed to resolve.
        target: Box<ContractTarget>,
        /// The underlying error.
        #[source]
        source: CorpusError,
    },
    /// The on-chain code fetched for `target` was not valid base64 — would
    /// indicate the corpus provider changed its response encoding.
    #[error("on-chain code for {target:?} is not valid base64: {source}")]
    InvalidBase64 {
        /// The target whose on-chain code failed to decode.
        target: Box<ContractTarget>,
        /// The underlying error.
        #[source]
        source: base64::DecodeError,
    },
    /// No [`BytecodeSource`] is registered for the requested language —
    /// see this crate's own module doc comment for which languages are
    /// actually supported and why.
    #[error(
        "no bytecode compiler registered for {0:?} — verification is not supported for this language yet"
    )]
    NoBytecodeSourceForLanguage(SourceLanguage),
    /// Compiling the submitted source to bytecode failed.
    #[error(transparent)]
    Compile(#[from] BytecodeError),
    /// A `Full`/`Partial` match's ABI extraction failed — should be rare
    /// (the same source just compiled successfully to bytecode above), but
    /// not impossible (e.g. the schema-normalization step itself has a
    /// bug) — surfaced as a named error, not silently downgraded to
    /// `NoMatch`.
    #[error(transparent)]
    Extract(#[from] PipelineError),
}

/// Verifies `(address, source)` pairs against real on-chain bytecode — see
/// this crate's own module doc comment for full scope and the `Partial`
/// variant's current status.
pub struct RegistryController<'a> {
    corpus: &'a CorpusController,
    bytecode_sources: &'a HashMap<SourceLanguage, Box<dyn BytecodeSource>>,
    extract: &'a ExtractController,
}

impl<'a> RegistryController<'a> {
    /// Builds a controller over already-constructed dependencies — nothing
    /// here is forked or reimplemented, matching
    /// `docs/domains/service.md` principle 1.
    #[must_use]
    pub fn new(
        corpus: &'a CorpusController,
        bytecode_sources: &'a HashMap<SourceLanguage, Box<dyn BytecodeSource>>,
        extract: &'a ExtractController,
    ) -> Self {
        Self {
            corpus,
            bytecode_sources,
            extract,
        }
    }

    /// Verifies that `entrypoint` (relative to `project_dir`), compiled as
    /// `lang`, matches the real on-chain code resolved by `target`.
    ///
    /// Takes a [`ContractTarget`] directly, not a bare address string —
    /// the same interface [`CorpusController::resolve`] itself already
    /// uses. Production callers (`sting-abi-service`'s `/v1/verify`) pass
    /// [`ContractTarget::OnChainAddress`]; this is also what makes this
    /// controller testable against local, vendored `LocalBoc` fixtures
    /// without a live network call — the same principle
    /// `docs/domains/service.md` principle 1
    /// ("testability without real external systems") already requires of
    /// every controller in this branch.
    ///
    /// # Errors
    /// See [`RegistryError`]'s variants.
    pub fn verify(
        &self,
        target: &ContractTarget,
        project_dir: &Path,
        entrypoint: &Path,
        lang: SourceLanguage,
    ) -> Result<VerificationOutcome, RegistryError> {
        let on_chain = self
            .corpus
            .resolve(target, FetchOptions::default())
            .map_err(|source| RegistryError::Corpus {
                target: Box::new(target.clone()),
                source,
            })?;
        let on_chain_code = BASE64.decode(&on_chain.code_boc_base64).map_err(|source| {
            RegistryError::InvalidBase64 {
                target: Box::new(target.clone()),
                source,
            }
        })?;

        let bytecode_source = self
            .bytecode_sources
            .get(&lang)
            .ok_or(RegistryError::NoBytecodeSourceForLanguage(lang))?;
        let compiled_code = bytecode_source.compile_to_boc(project_dir, entrypoint)?;

        // See this module's own doc comment: no empirically-confirmed
        // `Partial` case exists yet — a byte match is `Full`, anything
        // else is `NoMatch`, not a guessed middle ground.
        let status = if compiled_code == on_chain_code {
            MatchStatus::Full
        } else {
            MatchStatus::NoMatch
        };

        let schema = match status {
            MatchStatus::Full | MatchStatus::Partial => Some(self.extract.extract(
                project_dir,
                entrypoint,
                LanguageHint::Explicit(lang),
            )?),
            MatchStatus::NoMatch => None,
        };

        Ok(VerificationOutcome { status, schema })
    }
}
