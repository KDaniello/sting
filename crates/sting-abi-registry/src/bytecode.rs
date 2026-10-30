//! Compiling submitted source down to real, deployable bytecode — a
//! different question from [`sting_abi_core::Extractor`]'s "extract an
//! ABI" (Tier 1 extractors never needed raw bytecode; `RegistryController`
//! does, to byte-compare against on-chain code).
//!
//! **Tact and Tolk are both implemented; `FunC` categorically is not.**
//! `tact.js` already produces a `.code.boc` artifact on every compile call
//! (`sting_abi_tact::TactCompiler` was extended this same block to read it
//! back — `TactCompileOutput::code_boc`). Tolk had **no equivalent path in
//! this workspace**, because `sting_tolkc::TolkCompiler::compile` only ever
//! produces Fift *assembly text*. Closed by vendoring a real `fift` subprocess
//! (`sting_fift`, `vendor/fift/`) and composing it with `sting_tolkc` in
//! [`TolkBytecodeSource`] below — see `sting_fift`'s own module doc comment
//! for the empirically-confirmed pipeline this composition depends on.
//! `FunC` (Tier 2, heuristic, no real compiler at all) remains categorically
//! outside this trait's scope by construction, not an oversight.

use std::path::Path;

use sting_abi_core::SourceLanguage;
use sting_abi_tact::{TactCompiler, TactcError};
use sting_fift::FiftAssembler;
use sting_tolkc::TolkCompiler;

/// A source of real, deployable bytecode for one [`SourceLanguage`] —
/// deliberately not [`sting_abi_core::Extractor`]: that trait answers "what
/// is this contract's ABI", this one answers "what bytes would deploying
/// this source actually produce".
pub trait BytecodeSource: Send + Sync {
    /// Which language this compiles.
    fn language(&self) -> SourceLanguage;

    /// Compiles `entrypoint` (relative to `project_dir`) to raw bytecode.
    ///
    /// # Errors
    /// Returns [`BytecodeError`] if the underlying compiler fails.
    fn compile_to_boc(
        &self,
        project_dir: &Path,
        entrypoint: &Path,
    ) -> Result<Vec<u8>, BytecodeError>;
}

/// Error from a [`BytecodeSource`].
#[derive(Debug, thiserror::Error)]
pub enum BytecodeError {
    /// The underlying compiler failed.
    #[error("{language:?} compilation failed: {source}")]
    CompileFailed {
        /// Which language's compiler failed.
        language: SourceLanguage,
        /// The underlying error.
        #[source]
        source: Box<dyn std::error::Error + Send + Sync + 'static>,
    },
}

/// [`BytecodeSource`] for Tact — thin wrapper around
/// [`sting_abi_tact::TactCompiler`], the only piece already producing real
/// bytecode, reusable directly.
pub struct TactBytecodeSource {
    compiler: TactCompiler,
}

impl TactBytecodeSource {
    /// Wraps an existing [`TactCompiler`] — the same instance a caller may
    /// already be using for ABI extraction, not a second, independently
    /// configured one.
    #[must_use]
    pub fn new(compiler: TactCompiler) -> Self {
        Self { compiler }
    }
}

impl BytecodeSource for TactBytecodeSource {
    fn language(&self) -> SourceLanguage {
        SourceLanguage::Tact
    }

    fn compile_to_boc(
        &self,
        project_dir: &Path,
        entrypoint: &Path,
    ) -> Result<Vec<u8>, BytecodeError> {
        self.compiler
            .compile(project_dir, entrypoint)
            .map(|output| output.code_boc)
            .map_err(|source: TactcError| BytecodeError::CompileFailed {
                language: SourceLanguage::Tact,
                source: Box::new(source),
            })
    }
}

/// [`BytecodeSource`] for Tolk — composes [`TolkCompiler`] (semantic
/// compile to Fift assembly text) with [`FiftAssembler`] (assembly to real
/// bytecode), the two-step pipeline this module's own doc comment
/// describes. Uses [`TolkCompiler::compile_keep_scratch`], not
/// [`TolkCompiler::compile`] — the scratch directory `tolk` wrote its
/// `--output` artifacts into must still exist on disk when [`FiftAssembler::
/// assemble`] runs (see `sting_fift`'s own module doc comment for why).
pub struct TolkBytecodeSource {
    tolk: TolkCompiler,
    fift: FiftAssembler,
}

impl TolkBytecodeSource {
    /// Wraps an existing [`TolkCompiler`] and [`FiftAssembler`] — the same
    /// instances a caller may already be using elsewhere (e.g. `tolk` for
    /// ABI extraction), not independently reconstructed ones.
    #[must_use]
    pub fn new(tolk: TolkCompiler, fift: FiftAssembler) -> Self {
        Self { tolk, fift }
    }
}

impl BytecodeSource for TolkBytecodeSource {
    fn language(&self) -> SourceLanguage {
        SourceLanguage::Tolk
    }

    fn compile_to_boc(
        &self,
        project_dir: &Path,
        entrypoint: &Path,
    ) -> Result<Vec<u8>, BytecodeError> {
        let compiled = self
            .tolk
            .compile_keep_scratch(project_dir, entrypoint)
            .map_err(|source| BytecodeError::CompileFailed {
                language: SourceLanguage::Tolk,
                source: Box::new(source),
            })?;
        self.fift
            .assemble(&compiled.fift_path)
            .map_err(|source| BytecodeError::CompileFailed {
                language: SourceLanguage::Tolk,
                source: Box::new(source),
            })
    }
}
