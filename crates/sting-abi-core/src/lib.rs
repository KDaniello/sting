//! Normalized, language-independent contract ABI schema — the single
//! artifact every `abi-layer` extractor (Tolk, Tact, `FunC`) produces and
//! every consumer (CLI, HTTP API, direct library use) reads. Zero internal
//! `sting-*` dependencies by design, same reasoning as `sting-core` in the
//! security branch: the IR must stay independent of whichever extractor
//! filled it in, or normalization loses its point.
//!
//! See `docs/domains/abi-ir.md` for the full design rationale
//! and the real-artifact lineage behind every field on [`ContractSchema`].

mod confidence;
mod error;
mod extractor;
mod message;
mod method;
mod schema;
mod types;

pub use confidence::{ExtractionConfidence, ExtractionConfidenceRangeError, FieldConfidence};
pub use error::ErrorCode;
pub use extractor::{ExtractError, Extractor};
pub use message::MessageRef;
pub use method::{GetMethod, Param};
pub use schema::{
    ContractMetadata, ContractSchema, ExtractionTier, IR_SCHEMA_VERSION, SchemaInvariantError,
    SourceLanguage,
};
pub use types::{FieldRef, NamedType, TlbPrefix, TypeRef, TypeShape, UnionVariant};
