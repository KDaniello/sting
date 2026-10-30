//! Tier 2 (`FunC`) precision/recall —
//! `docs/domains/precision-sources.md`/§2.2: ground truth comes
//! from a `func2tolk`-converted, Tier-1-extracted `ContractSchema` of the
//! *same* contract, not an independent hand-written spec. This module
//! reuses the security branch's already-converted, already
//! compile-verified corpus (`corpus/clean/converted/`) directly rather than
//! re-running the converter live — those `.tolk` files are already proven
//! to compile and pass their own tests (`corpus/clean/converted/SOURCE.md`),
//! which is the required mitigation: a converted contract has to compile
//! before it counts, never be taken on trust.
//!
//! Only two categories are measurable in v1: `get_methods` and storage
//! fields. `errors`/message-body fields are **not** scored at all (`None`
//! throughout, not `Some(0.0)`) — `sting-abi-func`'s own v1 scope
//! (`crates/sting-abi-func/src/lib.rs`'s module doc comment) never attempts
//! either, so scoring them would measure "did v1 do something it explicitly
//! doesn't do," not extraction accuracy.
//!
//! **Name normalization, an explicit judgment call**: `func2tolk`'s
//! converted Tolk source renames `FunC`'s raw identifiers into camelCase
//! (confirmed in `corpus/clean/converted/SOURCE.md`'s own fix log, e.g.
//! `ownerAddress`), while `FuncExtractor` recovers names verbatim from the
//! original `FunC` source (typically `snake_case` or arbitrary). A strict
//! string-equality match would undercount true positives purely on casing,
//! not on any real extraction defect — so names are compared
//! case/underscore/camelCase-insensitively here (`normalize_name`),
//! documented rather than silently baked in.

use sting_abi_core::{ContractSchema, TypeShape};

/// Case/separator-insensitive form of an identifier, for comparing a
/// `FunC`-sourced name against its `func2tolk`-renamed camelCase
/// counterpart — see this module's own doc comment for why this
/// normalization exists at all.
fn normalize_name(name: &str) -> String {
    name.chars()
        .filter(|c| *c != '_')
        .flat_map(char::to_lowercase)
        .collect()
}

/// TP/FP/FN counts for one category on one contract.
#[derive(Debug, Clone, Copy, Default, serde::Serialize)]
pub struct PrecisionRecall {
    /// Extracted, and a matching ground-truth item exists.
    pub true_positives: u32,
    /// Extracted, but no matching ground-truth item exists.
    pub false_positives: u32,
    /// A ground-truth item that nothing extracted matched.
    pub false_negatives: u32,
}

impl PrecisionRecall {
    /// `TP / (TP + FP)`, or `None` if nothing was extracted at all (not
    /// `Some(0.0)` — a category the extractor never populates for this
    /// contract, e.g. storage in the ~45% of real `FunC` that doesn't use
    /// the `load_data`/`save_data` pair, is "not applicable," not "wrong").
    #[must_use]
    pub fn precision(&self) -> Option<f64> {
        let denom = self.true_positives + self.false_positives;
        if denom == 0 {
            None
        } else {
            Some(f64::from(self.true_positives) / f64::from(denom))
        }
    }

    /// `TP / (TP + FN)`, or `None` if there was nothing to find at all
    /// (e.g. a contract with no get-methods).
    #[must_use]
    pub fn recall(&self) -> Option<f64> {
        let denom = self.true_positives + self.false_negatives;
        if denom == 0 {
            None
        } else {
            Some(f64::from(self.true_positives) / f64::from(denom))
        }
    }
}

/// Per-category Tier 2 result for one contract.
#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct Tier2Result {
    /// The contract's name, for reporting.
    pub contract: String,
    /// `get_methods` precision/recall.
    pub get_methods: PrecisionRecall,
    /// Storage-field precision/recall.
    pub storage_fields: PrecisionRecall,
}

fn struct_field_names(schema: &ContractSchema) -> Vec<String> {
    let Some(storage) = schema.storage else {
        return Vec::new();
    };
    let Some(named) = schema.types.get(storage.0 as usize) else {
        return Vec::new();
    };
    let TypeShape::Struct { fields } = &named.shape else {
        return Vec::new();
    };
    fields.iter().map(|f| normalize_name(&f.name)).collect()
}

/// Scores `candidate` (a real `FuncExtractor` output) against
/// `ground_truth` (a real `TolkExtractor` output on the `func2tolk`-
/// converted equivalent of the same contract), per this module's own doc
/// comment.
#[must_use]
pub fn score_func(
    contract: &str,
    ground_truth: &ContractSchema,
    candidate: &ContractSchema,
) -> Tier2Result {
    let mut result = Tier2Result {
        contract: contract.to_string(),
        ..Default::default()
    };

    let truth_methods: std::collections::HashSet<String> = ground_truth
        .get_methods
        .iter()
        .map(|m| normalize_name(&m.name))
        .collect();
    let mut matched_truth_methods = std::collections::HashSet::new();
    for method in &candidate.get_methods {
        let key = normalize_name(&method.name);
        // Parameter *count* is comparable across both sides even though
        // resolved *types* aren't (see this module's own doc comment on
        // why a raw `FunC` type-text match against Tolk's resolved type
        // system would be an unfair apples-to-oranges comparison) — a
        // count mismatch on an otherwise name-matching getter still counts
        // as a false positive, since it means the extracted signature is
        // wrong, not just under-detailed.
        let truth_method = ground_truth
            .get_methods
            .iter()
            .find(|m| normalize_name(&m.name) == key);
        let arity_ok = truth_method.is_some_and(|m| m.parameters.len() == method.parameters.len());
        // A ground-truth item can only be claimed once — real bug found by
        // independent review: without this guard, two candidate items that
        // both normalize to the same key (e.g. `get_data`/`getdata`, or a
        // genuine extractor duplicate) each counted as an independent true
        // positive against the *same* single ground-truth method, silently
        // inflating precision. A second candidate claiming an
        // already-matched key is a real false positive — there is nothing
        // left in the ground truth for it to correctly match.
        let ok = arity_ok && !matched_truth_methods.contains(&key);
        if ok {
            result.get_methods.true_positives += 1;
            matched_truth_methods.insert(key);
        } else {
            result.get_methods.false_positives += 1;
        }
    }
    result.get_methods.false_negatives =
        u32::try_from(truth_methods.difference(&matched_truth_methods).count()).unwrap_or(u32::MAX);

    let truth_fields: std::collections::HashSet<String> =
        struct_field_names(ground_truth).into_iter().collect();
    let candidate_fields = struct_field_names(candidate);
    let mut matched_truth_fields = std::collections::HashSet::new();
    for field in &candidate_fields {
        // Same already-claimed-key guard as `get_methods` above.
        if truth_fields.contains(field) && !matched_truth_fields.contains(field) {
            result.storage_fields.true_positives += 1;
            matched_truth_fields.insert(field.clone());
        } else {
            result.storage_fields.false_positives += 1;
        }
    }
    result.storage_fields.false_negatives =
        u32::try_from(truth_fields.difference(&matched_truth_fields).count()).unwrap_or(u32::MAX);

    result
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "docs/ARCHITECTURE.md §3.2 scopes the no-unwrap/expect policy to library code outside tests"
)]
mod tests {
    use sting_abi_core::{
        ContractMetadata, ContractSchema, ExtractionTier, FieldRef, GetMethod, NamedType, Param,
        SourceLanguage, TypeRef, TypeShape,
    };

    use super::score_func;

    fn base_schema(lang: SourceLanguage) -> ContractSchema {
        ContractSchema {
            ir_schema_version: sting_abi_core::IR_SCHEMA_VERSION.to_string(),
            source_language: lang,
            extraction_tier: ExtractionTier::Compiled,
            contract_name: None,
            metadata: ContractMetadata::default(),
            storage: None,
            types: Vec::new(),
            incoming_internal: Vec::new(),
            incoming_external: Vec::new(),
            outgoing: Vec::new(),
            get_methods: Vec::new(),
            errors: Vec::new(),
            confidence: None,
            field_confidence: None,
        }
    }

    fn storage_of(fields: &[&str]) -> (Vec<NamedType>, Option<TypeRef>) {
        let types = vec![NamedType {
            name: Some("Storage".to_string()),
            description: None,
            prefix: None,
            shape: TypeShape::Struct {
                fields: fields
                    .iter()
                    .map(|name| FieldRef {
                        name: (*name).to_string(),
                        ty: TypeRef(0),
                    })
                    .collect(),
            },
        }];
        (types, Some(TypeRef(0)))
    }

    #[test]
    fn matching_names_are_true_positives_regardless_of_casing() {
        let mut truth = base_schema(SourceLanguage::Tolk);
        truth.get_methods.push(GetMethod {
            tvm_method_id: Some(1),
            name: "getWalletData".to_string(),
            parameters: Vec::new(),
            return_ty: None,
        });
        let (types, storage) = storage_of(&["ownerAddress"]);
        truth.types = types;
        truth.storage = storage;

        let mut candidate = base_schema(SourceLanguage::Func);
        candidate.get_methods.push(GetMethod {
            tvm_method_id: None,
            name: "get_wallet_data".to_string(),
            parameters: Vec::new(),
            return_ty: None,
        });
        let (types, storage) = storage_of(&["owner_address"]);
        candidate.types = types;
        candidate.storage = storage;

        let result = score_func("synthetic", &truth, &candidate);
        assert_eq!(result.get_methods.true_positives, 1);
        assert_eq!(result.get_methods.false_positives, 0);
        assert_eq!(result.get_methods.false_negatives, 0);
        assert_eq!(result.storage_fields.true_positives, 1);
        assert_eq!(result.storage_fields.false_positives, 0);
        assert_eq!(result.storage_fields.false_negatives, 0);
    }

    #[test]
    fn a_method_with_no_ground_truth_match_is_a_false_positive() {
        let truth = base_schema(SourceLanguage::Tolk);
        let mut candidate = base_schema(SourceLanguage::Func);
        candidate.get_methods.push(GetMethod {
            tvm_method_id: None,
            name: "phantomMethod".to_string(),
            parameters: Vec::new(),
            return_ty: None,
        });

        let result = score_func("synthetic", &truth, &candidate);
        assert_eq!(result.get_methods.true_positives, 0);
        assert_eq!(result.get_methods.false_positives, 1);
        assert_eq!(result.get_methods.precision(), Some(0.0));
        assert_eq!(
            result.get_methods.recall(),
            None,
            "nothing to recall — truth has no methods"
        );
    }

    #[test]
    fn a_ground_truth_method_nothing_extracted_is_a_false_negative() {
        let mut truth = base_schema(SourceLanguage::Tolk);
        truth.get_methods.push(GetMethod {
            tvm_method_id: Some(1),
            name: "getOwner".to_string(),
            parameters: Vec::new(),
            return_ty: None,
        });
        let candidate = base_schema(SourceLanguage::Func);

        let result = score_func("synthetic", &truth, &candidate);
        assert_eq!(result.get_methods.true_positives, 0);
        assert_eq!(result.get_methods.false_negatives, 1);
        assert_eq!(result.get_methods.recall(), Some(0.0));
        assert_eq!(
            result.get_methods.precision(),
            None,
            "nothing was extracted at all"
        );
    }

    #[test]
    fn a_parameter_count_mismatch_on_a_name_matching_getter_is_a_false_positive() {
        let mut truth = base_schema(SourceLanguage::Tolk);
        truth.get_methods.push(GetMethod {
            tvm_method_id: Some(1),
            name: "balanceOf".to_string(),
            parameters: vec![Param {
                name: "owner".to_string(),
                ty: TypeRef(0),
            }],
            return_ty: None,
        });
        let mut candidate = base_schema(SourceLanguage::Func);
        candidate.get_methods.push(GetMethod {
            tvm_method_id: None,
            name: "balance_of".to_string(),
            parameters: Vec::new(),
            return_ty: None,
        });

        let result = score_func("synthetic", &truth, &candidate);
        assert_eq!(
            result.get_methods.true_positives, 0,
            "the name matches but the arity doesn't — this must not count as a real match"
        );
        assert_eq!(result.get_methods.false_positives, 1);
        assert_eq!(result.get_methods.false_negatives, 1);
    }

    /// Regression test for a real bug found by independent review: two
    /// candidate methods whose names both normalize to the same key
    /// (`get_data`/`getdata`) against a single ground-truth method must not
    /// both count as true positives — there is only one real ground-truth
    /// item, so the second claim is a false positive, not a second match.
    #[test]
    fn two_candidates_normalizing_to_the_same_key_are_not_both_true_positives() {
        let mut truth = base_schema(SourceLanguage::Tolk);
        truth.get_methods.push(GetMethod {
            tvm_method_id: Some(1),
            name: "getData".to_string(),
            parameters: Vec::new(),
            return_ty: None,
        });
        let mut candidate = base_schema(SourceLanguage::Func);
        candidate.get_methods.push(GetMethod {
            tvm_method_id: None,
            name: "get_data".to_string(),
            parameters: Vec::new(),
            return_ty: None,
        });
        candidate.get_methods.push(GetMethod {
            tvm_method_id: None,
            name: "getdata".to_string(),
            parameters: Vec::new(),
            return_ty: None,
        });

        let result = score_func("synthetic", &truth, &candidate);
        assert_eq!(
            result.get_methods.true_positives, 1,
            "only one of the two normalized-identical candidates can be the real match"
        );
        assert_eq!(result.get_methods.false_positives, 1);
        assert_eq!(result.get_methods.false_negatives, 0);
    }

    /// Same guard, for `storage_fields`.
    #[test]
    fn two_candidate_fields_normalizing_to_the_same_key_are_not_both_true_positives() {
        let truth = {
            let mut s = base_schema(SourceLanguage::Tolk);
            let (types, storage) = storage_of(&["ownerAddress"]);
            s.types = types;
            s.storage = storage;
            s
        };
        let candidate = {
            let mut s = base_schema(SourceLanguage::Func);
            let (types, storage) = storage_of(&["owner_address", "OwnerAddress"]);
            s.types = types;
            s.storage = storage;
            s
        };

        let result = score_func("synthetic", &truth, &candidate);
        assert_eq!(result.storage_fields.true_positives, 1);
        assert_eq!(result.storage_fields.false_positives, 1);
        assert_eq!(result.storage_fields.false_negatives, 0);
    }
}
