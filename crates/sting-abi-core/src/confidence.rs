/// A Tier 2 (`FunC`) extraction result's confidence score, constrained to
/// `[0.0, 1.0]` by construction — same invariant, same reasoning, as
/// `sting-core::Confidence` in the security branch (see
/// `docs/TESTING.md`). `None` for Tier 1 (Tolk/Tact),
/// where the compiler is the ground truth and a confidence score would be
/// meaningless — see [`crate::ExtractionTier`].
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(try_from = "f32", into = "f32")]
pub struct ExtractionConfidence(f32);

impl ExtractionConfidence {
    /// # Errors
    /// Returns [`ExtractionConfidenceRangeError`] if `value` is not a finite
    /// number in `[0.0, 1.0]`.
    pub fn new(value: f32) -> Result<Self, ExtractionConfidenceRangeError> {
        if value.is_finite() && (0.0..=1.0).contains(&value) {
            Ok(Self(value))
        } else {
            Err(ExtractionConfidenceRangeError(value))
        }
    }

    /// The underlying score.
    #[must_use]
    pub fn value(self) -> f32 {
        self.0
    }
}

impl TryFrom<f32> for ExtractionConfidence {
    type Error = ExtractionConfidenceRangeError;

    fn try_from(value: f32) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl From<ExtractionConfidence> for f32 {
    fn from(confidence: ExtractionConfidence) -> f32 {
        confidence.0
    }
}

/// Error from [`ExtractionConfidence::new`], and from deserializing a
/// [`ExtractionConfidence`] whose JSON value is out of range.
#[derive(Debug, Clone, Copy, thiserror::Error)]
#[error("extraction confidence {0} is outside the valid range [0.0, 1.0]")]
pub struct ExtractionConfidenceRangeError(f32);

/// Per-category confidence breakdown for a [`crate::ExtractionTier::Heuristic`]
/// schema. Added for `FunC`, whose confidence differs by category rather than
/// holding one figure for the whole result.
/// [`crate::ContractSchema::confidence`] stays a single overall score (the
/// existing [`crate::ContractSchema::validate`] invariant — populated iff
/// [`crate::ExtractionTier::Heuristic`] — is unchanged); this is additive,
/// `None` for every Tier 1 schema and for a Tier 2 schema that only
/// populated one category (nothing yet to differentiate).
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct FieldConfidence {
    /// Confidence in the extracted `get_methods` — `FunC`'s Level 1
    /// (mirrors `ton-blockchain/verifier`'s `getterParser.ts` precedent),
    /// `None` if no `method_id`-annotated function was found at all.
    pub get_methods: Option<ExtractionConfidence>,
    /// Confidence in the extracted `storage` type — `FunC`'s Level 2,
    /// structurally lower-confidence than get-methods (no compiler-level
    /// guarantee at all, purely heuristic pattern recognition), `None` if
    /// no storage form was recognized.
    pub storage: Option<ExtractionConfidence>,
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "docs/ARCHITECTURE.md §3.2 scopes the no-unwrap/expect policy to library code outside tests"
)]
mod tests {
    use super::ExtractionConfidence;

    #[test]
    fn rejects_out_of_range_and_non_finite() {
        assert!(ExtractionConfidence::new(-0.1).is_err());
        assert!(ExtractionConfidence::new(1.1).is_err());
        assert!(ExtractionConfidence::new(f32::NAN).is_err());
        assert!(ExtractionConfidence::new(f32::INFINITY).is_err());
    }

    #[test]
    fn accepts_the_boundary_and_interior_values() {
        assert!(ExtractionConfidence::new(0.0).is_ok());
        assert!(ExtractionConfidence::new(1.0).is_ok());
        assert!(ExtractionConfidence::new(0.42).is_ok());
    }

    #[test]
    fn deserializing_an_out_of_range_json_number_fails() {
        let result: Result<ExtractionConfidence, _> = serde_json::from_str("1.5");
        assert!(result.is_err());
    }

    #[test]
    fn serializes_as_a_bare_json_number() {
        let confidence = ExtractionConfidence::new(0.75).expect("0.75 is in range");
        assert_eq!(
            serde_json::to_string(&confidence).expect("serializable"),
            "0.75"
        );
    }
}
