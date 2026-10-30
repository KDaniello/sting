//! `HypothesisSpec` — parses a user-supplied TOML file into [`Hypothesis`]
//! values, keyed by detector id ('s direction 3:
//! wire `BridgeController`'s already-proven strategies into a reusable
//! `sting verify` command instead of a one-off Rust test).
//!
//! Deliberately **not** an attempt at deriving a [`Hypothesis`] from source —
//! [`super::bridge::Hypothesis`]'s own doc comment explains why that is a
//! confirmed-out-of-scope, research-grade problem (general message-dispatch
//! convention inference), not a gap this module tries to close. This module
//! only parses what a human who already knows the answer writes down —
//! the same information `crates/sting-pipeline/tests/bridge_ubm_end_to_end.rs`
//! and `bridge_id_end_to_end.rs` hardcode as Rust literals, moved into a
//! plain-text file so it does not require a recompile per contract.

use super::bridge::{Hypothesis, ScenarioField};

/// One `[GVR]` section.
#[derive(Debug, Clone, serde::Deserialize)]
struct GvrSpec {
    trigger_op: OpCode,
    get_method: String,
}

/// One `[LEP]` section.
#[derive(Debug, Clone, serde::Deserialize)]
struct LepSpec {
    trigger_op: OpCode,
    #[serde(default)]
    fields: Vec<ScenarioFieldSpec>,
    #[serde(default)]
    trailing_bytes: Vec<u8>,
}

/// One `[ID]` section.
#[derive(Debug, Clone, serde::Deserialize)]
struct IdSpec {
    save_op: OpCode,
    field_a: u32,
    field_b: u32,
    get_method: String,
}

/// One `[UBM]` section.
#[derive(Debug, Clone, serde::Deserialize)]
struct UbmSpec {
    mint_op: OpCode,
    mint_amount: u64,
}

/// One [`ScenarioField`], in the shape a human writes in TOML — an
/// `[[LEP.fields]]` array-of-tables, `type` tag plus `value`/`bytes`
/// depending on which. Mirrors [`ScenarioField`] exactly; kept as a
/// separate type rather than deriving `Deserialize` on `ScenarioField`
/// itself so `sting-pipeline`'s public API (used by
/// `bridge_ubm_end_to_end.rs`-style callers building a `Hypothesis`
/// directly in Rust) does not gain a serde dependency it does not need for
/// that path.
#[derive(Debug, Clone, serde::Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
enum ScenarioFieldSpec {
    /// A 32-bit unsigned field.
    U32 {
        /// The value.
        value: u32,
    },
    /// A 64-bit unsigned field.
    U64 {
        /// The value.
        value: u64,
    },
    /// A `coins` field.
    Coins {
        /// The value.
        value: u64,
    },
    /// Raw trailing bytes.
    Raw {
        /// The bytes.
        bytes: Vec<u8>,
    },
}

impl From<ScenarioFieldSpec> for ScenarioField {
    fn from(spec: ScenarioFieldSpec) -> Self {
        match spec {
            ScenarioFieldSpec::U32 { value } => Self::U32(value),
            ScenarioFieldSpec::U64 { value } => Self::U64(value),
            ScenarioFieldSpec::Coins { value } => Self::Coins(value),
            ScenarioFieldSpec::Raw { bytes } => Self::Raw(bytes),
        }
    }
}

/// An op code, written in TOML as either a plain decimal integer
/// (`1396790853`) or a `"0x..."`/`"0X..."` hex string (`"0x53415645"`) —
/// hex is how every op code in this codebase's own fixtures and docs is
/// actually written (`corpus/fixtures/bridge/*.tolk`), so requiring decimal
/// only (TOML has no native hex literal) would make every real hypothesis
/// file a error-prone manual conversion exercise.
#[derive(Debug, Clone, Copy)]
struct OpCode(u32);

impl<'de> serde::Deserialize<'de> for OpCode {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(serde::Deserialize)]
        #[serde(untagged)]
        enum Repr {
            Int(u32),
            Text(String),
        }
        match Repr::deserialize(deserializer)? {
            Repr::Int(v) => Ok(Self(v)),
            Repr::Text(s) => {
                let trimmed = s.trim();
                let digits = trimmed
                    .strip_prefix("0x")
                    .or_else(|| trimmed.strip_prefix("0X"))
                    .unwrap_or(trimmed);
                let radix = if digits.len() == trimmed.len() {
                    10
                } else {
                    16
                };
                u32::from_str_radix(digits, radix)
                    .map(Self)
                    .map_err(|e| serde::de::Error::custom(format!("invalid op code {s:?}: {e}")))
            }
        }
    }
}

/// A parsed hypothesis-spec TOML document: at most one entry per detector
/// id, since [`BridgeController::verify`](super::bridge::BridgeController::verify)
/// needs exactly one [`Hypothesis`] to try per finding, not several
/// candidates to search over.
#[derive(Debug, Clone, Default, serde::Deserialize)]
pub struct HypothesisSpec {
    #[serde(rename = "GVR")]
    gvr: Option<GvrSpec>,
    #[serde(rename = "LEP")]
    lep: Option<LepSpec>,
    #[serde(rename = "ID")]
    id: Option<IdSpec>,
    #[serde(rename = "UBM")]
    ubm: Option<UbmSpec>,
}

impl HypothesisSpec {
    /// Parses a hypothesis-spec document from its TOML source text.
    ///
    /// # Errors
    /// Returns [`HypothesisSpecError::Toml`] on malformed TOML or a value
    /// that does not fit its declared shape (e.g. a non-hex, non-decimal
    /// op code string).
    pub fn parse(source: &str) -> Result<Self, HypothesisSpecError> {
        Ok(toml::from_str(source)?)
    }

    /// The [`Hypothesis`] this spec supplies for `detector_id`, if any —
    /// `None` for a detector id with no matching section (not an error: a
    /// hypothesis file only needs to cover the findings its author actually
    /// wants dynamically verified, `sting verify`'s own doc comment) and
    /// for any detector id outside the 4 classes `BridgeController`
    /// implements a strategy for at all (`docs/domains/emulator.md`
    /// §7.1 — `BR`/`PL`/`UR` have no strategy yet, same as
    /// [`super::bridge::Hypothesis`] itself does not have a variant for them).
    #[must_use]
    pub fn hypothesis_for(&self, detector_id: &str) -> Option<Hypothesis> {
        match detector_id {
            "GVR" => self.gvr.as_ref().map(|s| Hypothesis::Gvr {
                trigger_op: s.trigger_op.0,
                get_method: s.get_method.clone(),
            }),
            "LEP" => self.lep.as_ref().map(|s| Hypothesis::Lep {
                trigger_op: s.trigger_op.0,
                fields: s.fields.iter().cloned().map(ScenarioField::from).collect(),
                trailing_bytes: s.trailing_bytes.clone(),
            }),
            "ID" => self.id.as_ref().map(|s| Hypothesis::Id {
                save_op: s.save_op.0,
                field_a: s.field_a,
                field_b: s.field_b,
                get_method: s.get_method.clone(),
            }),
            "UBM" => self.ubm.as_ref().map(|s| Hypothesis::Ubm {
                mint_op: s.mint_op.0,
                mint_amount: s.mint_amount,
            }),
            _ => None,
        }
    }

    /// Every detector id this spec supplies a hypothesis for — used by
    /// `sting verify` to warn about a section that named a detector no
    /// finding actually matched (a likely typo, not silently ignored).
    #[must_use]
    pub fn detector_ids(&self) -> Vec<&'static str> {
        let mut ids = Vec::new();
        if self.gvr.is_some() {
            ids.push("GVR");
        }
        if self.lep.is_some() {
            ids.push("LEP");
        }
        if self.id.is_some() {
            ids.push("ID");
        }
        if self.ubm.is_some() {
            ids.push("UBM");
        }
        ids
    }
}

/// Errors from [`HypothesisSpec::parse`].
#[derive(Debug, thiserror::Error)]
pub enum HypothesisSpecError {
    /// The document is not valid TOML, or not shaped like a hypothesis spec.
    #[error("failed to parse hypothesis spec: {0}")]
    Toml(#[from] toml::de::Error),
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "docs/ARCHITECTURE.md §3.2 scopes the no-unwrap/expect/panic policy to library code outside tests"
)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_ubm_section_with_a_hex_op_code() {
        let spec = HypothesisSpec::parse(
            r#"
[UBM]
mint_op = "0x4d494e54"
mint_amount = 1000000000
"#,
        )
        .expect("parses");
        let hypothesis = spec.hypothesis_for("UBM").expect("UBM section present");
        assert!(matches!(
            hypothesis,
            Hypothesis::Ubm {
                mint_op: 0x4d49_4e54,
                mint_amount: 1_000_000_000
            }
        ));
    }

    #[test]
    fn parses_a_ubm_section_with_a_decimal_op_code() {
        let spec = HypothesisSpec::parse(
            r"
[UBM]
mint_op = 1296911692
mint_amount = 500
",
        )
        .expect("parses");
        let hypothesis = spec.hypothesis_for("UBM").expect("UBM section present");
        assert!(matches!(
            hypothesis,
            Hypothesis::Ubm {
                mint_op: 1_296_911_692,
                mint_amount: 500
            }
        ));
    }

    #[test]
    fn parses_a_gvr_section() {
        let spec = HypothesisSpec::parse(
            r#"
[GVR]
trigger_op = "0x12345678"
get_method = "getCounter"
"#,
        )
        .expect("parses");
        let hypothesis = spec.hypothesis_for("GVR").expect("GVR section present");
        match hypothesis {
            Hypothesis::Gvr {
                trigger_op,
                get_method,
            } => {
                assert_eq!(trigger_op, 0x1234_5678);
                assert_eq!(get_method, "getCounter");
            }
            other => panic!("expected Hypothesis::Gvr, got {other:?}"),
        }
    }

    #[test]
    fn parses_an_id_section() {
        let spec = HypothesisSpec::parse(
            r#"
[ID]
save_op = "0x53415645"
field_a = 11
field_b = 22
get_method = "getFields"
"#,
        )
        .expect("parses");
        let hypothesis = spec.hypothesis_for("ID").expect("ID section present");
        match hypothesis {
            Hypothesis::Id {
                save_op,
                field_a,
                field_b,
                get_method,
            } => {
                assert_eq!(save_op, 0x5341_5645);
                assert_eq!(field_a, 11);
                assert_eq!(field_b, 22);
                assert_eq!(get_method, "getFields");
            }
            other => panic!("expected Hypothesis::Id, got {other:?}"),
        }
    }

    #[test]
    fn parses_a_lep_section_with_fields_and_trailing_bytes() {
        let spec = HypothesisSpec::parse(
            r#"
[LEP]
trigger_op = "0x12345678"
trailing_bytes = [222, 173]

[[LEP.fields]]
type = "u32"
value = 42

[[LEP.fields]]
type = "coins"
value = 1000000000
"#,
        )
        .expect("parses");
        let hypothesis = spec.hypothesis_for("LEP").expect("LEP section present");
        match hypothesis {
            Hypothesis::Lep {
                trigger_op,
                fields,
                trailing_bytes,
            } => {
                assert_eq!(trigger_op, 0x1234_5678);
                assert_eq!(trailing_bytes, vec![0xde, 0xad]);
                assert_eq!(fields.len(), 2);
                assert!(matches!(fields[0], ScenarioField::U32(42)));
                assert!(matches!(fields[1], ScenarioField::Coins(1_000_000_000)));
            }
            other => panic!("expected Hypothesis::Lep, got {other:?}"),
        }
    }

    #[test]
    fn a_section_with_no_entry_returns_none_not_an_error() {
        let spec = HypothesisSpec::parse("[UBM]\nmint_op = 1\nmint_amount = 1\n").expect("parses");
        assert!(spec.hypothesis_for("GVR").is_none());
        assert!(spec.hypothesis_for("LEP").is_none());
        assert!(spec.hypothesis_for("ID").is_none());
    }

    #[test]
    fn an_unknown_detector_id_returns_none() {
        let spec = HypothesisSpec::default();
        assert!(spec.hypothesis_for("BR").is_none());
        assert!(spec.hypothesis_for("DEMO-LONG-FN").is_none());
    }

    #[test]
    fn an_empty_document_parses_to_a_spec_with_no_entries() {
        let spec = HypothesisSpec::parse("").expect("empty TOML is valid TOML");
        assert!(spec.detector_ids().is_empty());
    }

    #[test]
    fn rejects_malformed_toml() {
        assert!(HypothesisSpec::parse("this is not [ valid toml").is_err());
    }

    #[test]
    fn rejects_an_op_code_that_is_neither_decimal_nor_hex() {
        let result = HypothesisSpec::parse(
            r#"
[UBM]
mint_op = "not-a-number"
mint_amount = 1
"#,
        );
        assert!(result.is_err());
    }

    #[test]
    fn detector_ids_lists_exactly_the_sections_present() {
        let spec = HypothesisSpec::parse(
            r#"
[GVR]
trigger_op = 1
get_method = "m"

[UBM]
mint_op = 2
mint_amount = 3
"#,
        )
        .expect("parses");
        assert_eq!(spec.detector_ids(), vec!["GVR", "UBM"]);
    }
}
