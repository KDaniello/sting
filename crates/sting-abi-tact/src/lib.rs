//! Tier 1 extractor: calls the real `tact` compiler toolchain by
//! subprocess to get a `ContractABI` JSON, then normalizes it into a
//! `sting_abi_core::ContractSchema`. The confirmed CLI call form is in
//! `tests/fixtures/SOURCE.md` and `vendor/tact/README.md`. The
//! substantive mapping decisions (Tact has no index table, `interfaces[]`
//! not extracted, text-receivers not extracted, multi-contract files
//! rejected rather than guessed at) are documented on
//! `mapping::abi_to_schema` and `compiler::TactcError::MultipleContracts`.

mod abi_json;
mod compiler;
mod mapping;

use std::path::Path;

pub use abi_json::{
    AbiError, AbiField, AbiGetter, AbiReceiver, AbiReceiverMessage, AbiType, AbiTypeRef,
    ContractAbi,
};
pub use compiler::{DEFAULT_COMPILE_TIMEOUT, TactCompileOutput, TactCompiler, TactcError};
pub use mapping::abi_to_schema;

use sting_abi_core::{ContractSchema, ExtractError, Extractor, SourceLanguage};

/// Tier 1 Tact extractor — wraps a [`TactCompiler`] and normalizes its
/// output.
#[derive(Debug, Clone)]
pub struct TactExtractor {
    compiler: TactCompiler,
}

impl TactExtractor {
    /// Builds an extractor driving the given `tact.js` compiler handle.
    #[must_use]
    pub fn new(compiler: TactCompiler) -> Self {
        Self { compiler }
    }
}

impl Extractor for TactExtractor {
    fn language(&self) -> SourceLanguage {
        SourceLanguage::Tact
    }

    fn extract(
        &self,
        project_dir: &Path,
        entrypoint: &Path,
    ) -> Result<ContractSchema, ExtractError> {
        let output = self
            .compiler
            .compile(project_dir, entrypoint)
            .map_err(|source| ExtractError::Failed {
                entrypoint: project_dir.join(entrypoint),
                message: source.to_string(),
                source: Some(Box::new(source)),
            })?;

        let schema = mapping::abi_to_schema(&output.abi);
        // See sting-abi-tolk's identical call for why: ContractSchema::
        // validate's own doc comment names this extractor, but nothing
        // actually called it — real gap found by review.
        schema
            .validate()
            .map_err(|invariant_error| ExtractError::Failed {
                entrypoint: project_dir.join(entrypoint),
                message: format!(
                    "internal bug: extracted schema violates its own invariant: {invariant_error}"
                ),
                source: None,
            })?;
        Ok(schema)
    }
}
