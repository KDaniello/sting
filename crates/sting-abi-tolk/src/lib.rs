//! Tier 1 extractor: adapts `sting_tolkc`'s real, already-compiled `AbiJson`
//! into a normalized `sting_abi_core::ContractSchema`. See
//! `crates/sting-abi-tolk/src/mapping.rs` for the field-by-field mapping
//! and the empirically-confirmed index-table relationship it depends on.
//!
//! # What this crate does not do in v1
//! - No `symbolTypes.json` fields are consumed — `abi.json` alone is
//!   sufficient for every `ContractSchema` field; `SymbolTypes` exists for
//!   the security branch's cross-file call-graph use case, not this one.
//! - `Declaration::Enum::members`' per-member *values* are not recovered
//!   (only names) — see `mapping.rs`'s `enum_member_names` doc comment for
//!   why: the shape is unconfirmed even in `sting_tolkc` itself.
//! - `TypeDescriptor::Callable`/`VarUintN`/`GenericT` all degrade to
//!   `TypeShape::Unknown` — none were observed in the 7 real contracts
//!   (`corpus/clean/acton-contracts/{config,counter,nft-v1.1,w5.2,dns,
//!   elector}`, `corpus/clean/tolk-bench/01_jetton`) compiled while
//!   building this extractor; extend when a real one is found, not before.

mod mapping;

use std::path::{Path, PathBuf};

pub use mapping::abi_to_schema;
use sting_abi_core::{ContractSchema, ExtractError, Extractor, SourceLanguage};
use sting_tolkc::TolkCompiler;

/// Tier 1 Tolk extractor — wraps a [`TolkCompiler`] and normalizes its
/// output.
#[derive(Debug, Clone)]
pub struct TolkExtractor {
    compiler: TolkCompiler,
}

impl TolkExtractor {
    /// Builds an extractor driving the given `tolk` compiler handle.
    #[must_use]
    pub fn new(compiler: TolkCompiler) -> Self {
        Self { compiler }
    }
}

impl Extractor for TolkExtractor {
    fn language(&self) -> SourceLanguage {
        SourceLanguage::Tolk
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
                entrypoint: entrypoint_display(project_dir, entrypoint),
                message: source.to_string(),
                source: Some(Box::new(source)),
            })?;

        let schema = mapping::abi_to_schema(&output.abi);
        // `ContractSchema::validate`'s own doc comment names this extractor
        // as one that "should call this before returning a schema" — real
        // gap found by review: it never did. `abi_to_schema` is infallible
        // by construction (always `Compiled`/`None`, an always-valid pair),
        // so this should never actually fail — surfaced as an error rather
        // than a `debug_assert!` anyway, so a future bug here is caught in
        // release builds too, not just debug ones.
        schema
            .validate()
            .map_err(|invariant_error| ExtractError::Failed {
                entrypoint: entrypoint_display(project_dir, entrypoint),
                message: format!(
                    "internal bug: extracted schema violates its own invariant: {invariant_error}"
                ),
                source: None,
            })?;
        Ok(schema)
    }
}

fn entrypoint_display(project_dir: &Path, entrypoint: &Path) -> PathBuf {
    project_dir.join(entrypoint)
}
