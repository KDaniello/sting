use crate::{ExtractionConfidence, FieldConfidence, GetMethod, MessageRef, NamedType, TypeRef};

/// This IR's own format version — evolves independently of any source
/// compiler's own artifact version (Tolk's `abi_schema_version`, Tact's
/// `ContractABI` version). Bump on any breaking change to this crate's
/// public types once `sting-abi-core` is published
/// (`docs/domains/abi-ir.md`).
pub const IR_SCHEMA_VERSION: &str = "0.2";

/// Which of the three source languages a [`ContractSchema`] was extracted
/// from. `Hash` because `sting-abi-pipeline::ExtractController` uses it as a
/// dispatch-table key.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceLanguage {
    /// Extracted from Tolk source, Tier 1 (compiled).
    Tolk,
    /// Extracted from Tact source, Tier 1 (compiled).
    Tact,
    /// Extracted from `FunC` source, Tier 2 (heuristic).
    Func,
}

/// How a [`ContractSchema`] was produced — see
/// `docs/domains/abi-ir.md` for the full tier model.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExtractionTier {
    /// Tier 1 — a real compiler emitted structured metadata; accuracy is
    /// bounded only by the correctness of this crate's own normalizer, not
    /// by heuristics.
    Compiled,
    /// Tier 2 — AST-heuristic extraction (`FunC`); accuracy is calibrated,
    /// never guaranteed. Every [`ContractSchema`] at this tier carries a
    /// `confidence`.
    Heuristic,
    /// Tier 3 — statistical inference from on-chain history with no source
    /// available at all. Research track, outside MVP — see
    /// `docs/domains/method-dictionary.md`.
    Statistical,
}

/// The contract header's free-text metadata, if the source language and
/// this particular contract's header carry any of it — Tolk's
/// `contract Name { author: "...", version: "...", description: "..." }`
/// is the confirmed source for this shape (`crates/sting-tolkc/src/abi.rs`).
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ContractMetadata {
    /// The contract header's optional `author` field.
    pub author: Option<String>,
    /// The contract header's optional `version` field.
    pub version: Option<String>,
    /// The contract header's optional `description` field.
    pub description: Option<String>,
}

/// The single, language-independent schema every abi-layer extractor
/// produces and every consumer (CLI, HTTP API, library) reads — the
/// central artifact of the whole `abi-layer` branch. Full field-by-field
/// lineage (which real compiler artifact each field is modeled on, and
/// why) is documented in `docs/domains/abi-ir.md` — this type
/// is a direct, intentionally-unembellished implementation of that sketch,
/// not a redesign.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ContractSchema {
    /// This IR's own format version — always [`IR_SCHEMA_VERSION`] for a
    /// schema produced by the current crate version.
    pub ir_schema_version: String,
    /// Which source language produced this schema.
    pub source_language: SourceLanguage,
    /// Which tier produced this schema — determines whether `confidence`
    /// is meaningful.
    pub extraction_tier: ExtractionTier,
    /// The contract's declared name, if the source carries one.
    pub contract_name: Option<String>,
    /// Free-text header metadata.
    pub metadata: ContractMetadata,
    /// The contract's persistent storage type. `None` is a valid, real
    /// value — a project with no `contract{}`-style header still compiles
    /// under Tolk and genuinely has no declared storage type (see
    /// `crates/sting-tolkc/src/abi.rs`'s `StorageRef` doc comment) — this
    /// is not a placeholder for "not yet extracted".
    pub storage: Option<TypeRef>,
    /// The deduplicated type table — every other field indexes into this
    /// one, never by name. See [`crate::TypeShape`].
    pub types: Vec<NamedType>,
    /// Message bodies accepted by the contract's internal-message handler.
    pub incoming_internal: Vec<MessageRef>,
    /// Message bodies accepted by the contract's external-message handler.
    pub incoming_external: Vec<MessageRef>,
    /// Message bodies this contract is declared to send.
    pub outgoing: Vec<MessageRef>,
    /// The contract's `get`-methods.
    pub get_methods: Vec<GetMethod>,
    /// Every named/plain-integer error code reachable in the contract.
    pub errors: Vec<crate::ErrorCode>,
    /// Extraction confidence — `Some` iff [`ExtractionTier::Heuristic`],
    /// `None` for [`ExtractionTier::Compiled`] (a compiler's output needs
    /// no confidence score) and for [`ExtractionTier::Statistical`] (out of
    /// MVP scope; that tier's own eventual confidence model is unspecified
    /// as of this writing). Fields are `pub` (no invariant is enforced by
    /// construction alone, e.g. via a private field + constructor) — call
    /// [`ContractSchema::validate`] to check this before trusting it.
    pub confidence: Option<ExtractionConfidence>,
    /// Per-category confidence breakdown — see [`FieldConfidence`]'s own
    /// doc comment for why this exists alongside the single overall
    /// `confidence` above rather than replacing it. `None` for every Tier 1
    /// schema; also legitimately `None` for a Tier 2 schema whose
    /// extractor didn't populate this optional breakdown.
    pub field_confidence: Option<FieldConfidence>,
}

impl ContractSchema {
    /// Checks the one cross-field invariant this type's fields don't
    /// enforce on their own: `confidence` is populated iff
    /// `extraction_tier` is [`ExtractionTier::Heuristic`]. Every extractor
    /// should call this before returning a schema; nothing in this crate
    /// calls it automatically, since a plain-data IR with `pub` fields
    /// can't refuse to be constructed in an invalid state the way a
    /// private-field-plus-constructor type could.
    ///
    /// # Errors
    /// Returns [`SchemaInvariantError`] describing exactly which half of
    /// the invariant was violated.
    pub fn validate(&self) -> Result<(), SchemaInvariantError> {
        match (self.extraction_tier, self.confidence.is_some()) {
            (ExtractionTier::Heuristic, false) => Err(SchemaInvariantError::MissingConfidence),
            (ExtractionTier::Compiled | ExtractionTier::Statistical, true) => Err(
                SchemaInvariantError::UnexpectedConfidence(self.extraction_tier),
            ),
            _ => Ok(()),
        }
    }
}

/// Error from [`ContractSchema::validate`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum SchemaInvariantError {
    /// `extraction_tier` was [`ExtractionTier::Heuristic`] but `confidence`
    /// was `None`.
    #[error("extraction_tier is Heuristic but confidence is None")]
    MissingConfidence,
    /// `confidence` was `Some` despite `extraction_tier` not being
    /// [`ExtractionTier::Heuristic`].
    #[error("confidence is populated but extraction_tier is {0:?}, not Heuristic")]
    UnexpectedConfidence(ExtractionTier),
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "docs/ARCHITECTURE.md §3.2 scopes the no-unwrap/expect policy to library code outside tests"
)]
mod tests {
    use super::{
        ContractMetadata, ContractSchema, ExtractionTier, IR_SCHEMA_VERSION, SchemaInvariantError,
        SourceLanguage,
    };
    use crate::ExtractionConfidence;

    fn schema_with(
        extraction_tier: ExtractionTier,
        confidence: Option<ExtractionConfidence>,
    ) -> ContractSchema {
        ContractSchema {
            ir_schema_version: IR_SCHEMA_VERSION.to_string(),
            source_language: SourceLanguage::Tolk,
            extraction_tier,
            contract_name: None,
            metadata: ContractMetadata::default(),
            storage: None,
            types: Vec::new(),
            incoming_internal: Vec::new(),
            incoming_external: Vec::new(),
            outgoing: Vec::new(),
            get_methods: Vec::new(),
            errors: Vec::new(),
            confidence,
            field_confidence: None,
        }
    }

    #[test]
    fn compiled_without_confidence_is_valid() {
        assert!(
            schema_with(ExtractionTier::Compiled, None)
                .validate()
                .is_ok()
        );
    }

    #[test]
    fn statistical_without_confidence_is_valid() {
        assert!(
            schema_with(ExtractionTier::Statistical, None)
                .validate()
                .is_ok()
        );
    }

    #[test]
    fn heuristic_with_confidence_is_valid() {
        let confidence = ExtractionConfidence::new(0.5).expect("0.5 is in range");
        assert!(
            schema_with(ExtractionTier::Heuristic, Some(confidence))
                .validate()
                .is_ok()
        );
    }

    #[test]
    fn heuristic_without_confidence_is_rejected() {
        assert_eq!(
            schema_with(ExtractionTier::Heuristic, None).validate(),
            Err(SchemaInvariantError::MissingConfidence)
        );
    }

    #[test]
    fn compiled_with_confidence_is_rejected() {
        let confidence = ExtractionConfidence::new(0.5).expect("0.5 is in range");
        assert_eq!(
            schema_with(ExtractionTier::Compiled, Some(confidence)).validate(),
            Err(SchemaInvariantError::UnexpectedConfidence(
                ExtractionTier::Compiled
            ))
        );
    }

    #[test]
    fn statistical_with_confidence_is_rejected() {
        let confidence = ExtractionConfidence::new(0.5).expect("0.5 is in range");
        assert_eq!(
            schema_with(ExtractionTier::Statistical, Some(confidence)).validate(),
            Err(SchemaInvariantError::UnexpectedConfidence(
                ExtractionTier::Statistical
            ))
        );
    }
}
