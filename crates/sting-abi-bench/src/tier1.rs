//! Tier 1 (Tolk/Tact) structural completeness —
//! `docs/domains/precision-sources.md`: not precision/recall (the
//! compiler's own artifact *is* ground truth here, there's no independent
//! "correct answer" to compare against), but "did the normalizer carry
//! every field of the real compiler artifact into `ContractSchema` without
//! losing or corrupting it". Scored per category (storage, `get_methods[]`,
//! `errors[]`, `incoming_internal`/`incoming_external`/`outgoing`), since an
//! aggregated single number would hide exactly where a gap is, the same
//! principle `docs/domains/precision-sources.md` already applies to
//! detector precision/recall.
//!
//! Reuses each extractor crate's own already-tested `abi_to_schema`
//! (`sting_abi_tact::abi_to_schema`/`sting_abi_tolk::abi_to_schema`) rather
//! than re-deriving a `ContractSchema` a second way — this harness's job is
//! to verify that function's *output* still carries everything the raw
//! artifact had, not to duplicate its logic as a second, competing oracle.

use std::collections::HashSet;

use sting_abi_core::{ContractSchema, TypeRef, TypeShape};

/// One category's match count — `ratio()` is `None` (not `Some(0.0)`) when
/// `total` is 0, the same "denominator-is-zero is a distinct case from
/// zero-scored" principle `sting-bench`'s own `DetectorMetrics` already
/// applies: a contract with no getters at all shouldn't render as "0%
/// `get_methods` completeness," it should render as "not applicable."
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize)]
pub struct CategoryScore {
    /// How many raw items were verified present, correctly named, and
    /// correctly typed in the normalized schema.
    pub matched: u32,
    /// How many raw items exist in this category in total.
    pub total: u32,
}

impl CategoryScore {
    fn record(&mut self, ok: bool) {
        self.total += 1;
        if ok {
            self.matched += 1;
        }
    }

    /// `matched / total`, or `None` if `total == 0`.
    #[must_use]
    pub fn ratio(&self) -> Option<f64> {
        if self.total == 0 {
            None
        } else {
            Some(f64::from(self.matched) / f64::from(self.total))
        }
    }
}

/// Per-category completeness for one contract, plus a human-readable
/// account of every specific gap found — the block's own risk table
/// requires distinguishing "normalizer bug" from "IR needs extending"
/// (`docs/domains/precision-sources.md`'s risk table),
/// which requires knowing exactly *what* didn't match, not just a
/// percentage.
#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct Tier1Result {
    /// The contract's file name / project name, for reporting.
    pub contract: String,
    /// Storage type completeness — `total` is 0 for a contract with no
    /// declared persistent storage at all (a real, valid case, not a gap).
    pub storage: CategoryScore,
    /// `get_methods[]` completeness.
    pub get_methods: CategoryScore,
    /// `errors[]` completeness.
    pub errors: CategoryScore,
    /// `incoming_internal` completeness.
    pub incoming_internal: CategoryScore,
    /// `incoming_external` completeness.
    pub incoming_external: CategoryScore,
    /// `outgoing` completeness. **Corrected 2026-07-22** (found stale by
    /// review): this previously claimed
    /// `total: 0` for both Tolk and Tact — true for Tact (`score_tact`
    /// never touches this field at all, since `ContractAbi` carries no
    /// outgoing-message concept — see `sting_abi_tact::mapping::
    /// abi_to_schema`'s own doc comment for why), but **not** for Tolk:
    /// `score_tolk_messages` below genuinely scores `abi.outgoing_messages`
    /// against `schema.outgoing`, and does find real data (100% in
    /// `docs/domains/extractors.md`'s own numbers) — Tolk's raw
    /// artifact does carry this concept.
    pub outgoing: CategoryScore,
    /// One line per specific mismatch found, e.g. `"get_methods: getWalletData return type resolved to Unknown"`.
    pub gaps: Vec<String>,
}

impl Tier1Result {
    fn new(contract: String) -> Self {
        Self {
            contract,
            ..Default::default()
        }
    }
}

/// Walks `ty` (and, up to `depth` levels, everything it structurally
/// contains — struct fields, nullable/array/map inner types, tensor
/// elements, union arms) looking for a [`TypeShape::Unknown`] that counts as
/// a real completeness gap. **Not every `Unknown` is a gap** — real finding
/// from this harness's own first run against `corpus/clean/acton-contracts/
/// config/Config.tolk`: Tolk's stdlib `tuple` type is a real, compiler-
/// declared `alias tuple = arrayOf<unknown>` (`get_proposal`'s return type),
/// where that innermost `"kind":"unknown"` comes **from the real compiler
/// itself** (its own doc comment: "an array of unknown elements... reading
/// an individual element results in `unknown`"), not from
/// `sting-abi-tolk`'s normalizer failing to recognize a shape it should
/// have modeled. Treating that as a gap the first time this ran flagged two
/// real get-methods as "did not round-trip cleanly" when they, in fact,
/// round-tripped exactly what the compiler said. `is_expected_unknown`
/// lets a caller distinguish the two: return `true` for a `TypeRef` whose
/// raw source-side kind was *itself* already "unknown"/opaque (not a gap),
/// `false` (the default, via [`no_expected_unknowns`]) when there is no such
/// raw signal to check against (Tact has no artifact-level "unknown" kind
/// today — see [`score_tact`]'s own call site).
///
/// Depth-bounded (not full cycle detection) because a real `ContractSchema`
/// type table can't recurse arbitrarily in practice (TON's own storage/
/// message shapes bottom out in primitives within a handful of levels), and
/// an accidental cycle should fail closed (report `false`/"no gap found"
/// past the depth cap) rather than hang the benchmark run.
fn contains_unknown(
    schema: &ContractSchema,
    ty: TypeRef,
    depth: u32,
    is_expected_unknown: &dyn Fn(TypeRef) -> bool,
) -> bool {
    let Some(named) = schema.types.get(ty.0 as usize) else {
        // A dangling index would itself be a real, worse bug than Unknown —
        // treated as a match failure, not silently ignored.
        return true;
    };
    if matches!(named.shape, TypeShape::Unknown) {
        return !is_expected_unknown(ty);
    }
    if depth == 0 {
        return false;
    }
    match &named.shape {
        TypeShape::Nullable { inner }
        | TypeShape::Array { inner }
        | TypeShape::CellPacked { inner }
        | TypeShape::LispList { inner }
        | TypeShape::Alias { target: inner } => {
            contains_unknown(schema, *inner, depth - 1, is_expected_unknown)
        }
        TypeShape::Map { key, value } => {
            contains_unknown(schema, *key, depth - 1, is_expected_unknown)
                || contains_unknown(schema, *value, depth - 1, is_expected_unknown)
        }
        TypeShape::Struct { fields } => fields
            .iter()
            .any(|f| contains_unknown(schema, f.ty, depth - 1, is_expected_unknown)),
        TypeShape::Union { variants } => variants
            .iter()
            .any(|v| contains_unknown(schema, v.ty, depth - 1, is_expected_unknown)),
        TypeShape::Tensor { items } => items
            .iter()
            .any(|item| contains_unknown(schema, *item, depth - 1, is_expected_unknown)),
        TypeShape::Enum { encoded_as, .. } => {
            contains_unknown(schema, *encoded_as, depth - 1, is_expected_unknown)
        }
        _ => false,
    }
}

/// How deep [`contains_unknown`] walks before giving up — covers every real
/// shape observed in this workspace's corpora (a struct field pointing at
/// another struct pointing at a nullable primitive is 3 levels; this leaves
/// comfortable headroom without risking a pathological blow-up on a
/// hypothetical deeply-nested contract).
const UNKNOWN_CHECK_DEPTH: u32 = 8;

/// No raw-side signal to check against — every `Unknown` counts as a gap.
/// Correct for Tact today: `AbiTypeRef` has no `"kind":"unknown"`-equivalent
/// the real compiler emits for an intentionally opaque type (its only
/// unmodeled-shape fallback, `AbiTypeRef::Other`, has never been observed on
/// real output — `abi_json.rs`'s own doc comment); if that ever changes,
/// [`score_tact`] would need the same raw-aware treatment [`score_tolk`]
/// now has.
fn no_expected_unknowns(_ty: TypeRef) -> bool {
    false
}

fn type_ok(
    schema: &ContractSchema,
    ty: Option<TypeRef>,
    is_expected_unknown: &dyn Fn(TypeRef) -> bool,
) -> bool {
    ty.is_none_or(|ty| !contains_unknown(schema, ty, UNKNOWN_CHECK_DEPTH, is_expected_unknown))
}

/// Scores one Tact contract's structural completeness — compiled `abi`
/// (already-parsed raw artifact) plus its `schema` (already-produced by
/// [`sting_abi_tact::abi_to_schema`]), both from the *same* compile call, so
/// this never triggers a second, redundant compiler subprocess.
#[must_use]
pub fn score_tact(
    contract: &str,
    abi: &sting_abi_tact::ContractAbi,
    schema: &ContractSchema,
) -> Tier1Result {
    let mut result = Tier1Result::new(contract.to_string());

    // storage — Tact's own `$Data`-suffix naming convention
    // (`mapping::abi_to_schema`'s own doc comment).
    let data_type_name = format!("{}$Data", abi.name);
    if abi.types.iter().any(|t| t.name == data_type_name) {
        let ok = schema.storage.is_some_and(|storage_ty| {
            !contains_unknown(
                schema,
                storage_ty,
                UNKNOWN_CHECK_DEPTH,
                &no_expected_unknowns,
            ) && schema.types[storage_ty.0 as usize].name.as_deref()
                == Some(data_type_name.as_str())
        });
        result.storage.record(ok);
        if !ok {
            result.gaps.push(format!(
                "storage: {data_type_name} not resolved cleanly in ContractSchema"
            ));
        }
    }

    let get_method_by_name: std::collections::HashMap<&str, &sting_abi_core::GetMethod> = schema
        .get_methods
        .iter()
        .map(|m| (m.name.as_str(), m))
        .collect();
    for getter in &abi.getters {
        let ok = get_method_by_name
            .get(getter.name.as_str())
            .is_some_and(|m| {
                m.tvm_method_id == Some(getter.method_id)
                    && m.parameters.len() == getter.arguments.len()
                    && m.parameters.iter().zip(&getter.arguments).all(|(p, a)| {
                        p.name == a.name && type_ok(schema, Some(p.ty), &no_expected_unknowns)
                    })
                    && (getter.return_type.is_none() == m.return_ty.is_none())
                    && type_ok(schema, m.return_ty, &no_expected_unknowns)
            });
        result.get_methods.record(ok);
        if !ok {
            result.gaps.push(format!(
                "get_methods: {} did not round-trip cleanly",
                getter.name
            ));
        }
    }

    for (code_str, error) in &abi.errors {
        let Ok(code) = code_str.parse::<u32>() else {
            continue;
        };
        let ok = schema.errors.iter().any(|e| match e {
            sting_abi_core::ErrorCode::Named { name, code: c } => {
                *c == code && *name == error.message
            }
            sting_abi_core::ErrorCode::Plain { .. } => false,
        });
        result.errors.record(ok);
        if !ok {
            result.gaps.push(format!(
                "errors: code {code} ({}) not found in ContractSchema",
                error.message
            ));
        }
    }

    let internal_names: HashSet<&str> = schema
        .incoming_internal
        .iter()
        .filter_map(|m| schema.types.get(m.ty.0 as usize))
        .filter_map(|t| t.name.as_deref())
        .collect();
    let external_names: HashSet<&str> = schema
        .incoming_external
        .iter()
        .filter_map(|m| schema.types.get(m.ty.0 as usize))
        .filter_map(|t| t.name.as_deref())
        .collect();
    for receiver in &abi.receivers {
        let sting_abi_tact::AbiReceiverMessage::Typed { type_name } = &receiver.message else {
            // Text/other receivers are a documented, intentional v1 gap
            // (`mapping::abi_to_schema`'s own doc comment) — not counted
            // toward completeness at all, matching this block's own risk
            // table distinction between "normalizer bug" and "IR/scope
            // boundary".
            continue;
        };
        match receiver.receiver.as_str() {
            "internal" => result
                .incoming_internal
                .record(internal_names.contains(type_name.as_str())),
            "external" => result
                .incoming_external
                .record(external_names.contains(type_name.as_str())),
            _ => {}
        }
    }

    result
}

/// Scores one Tolk contract's structural completeness — same shape as
/// [`score_tact`], against `sting_tolkc::AbiJson` instead.
#[must_use]
pub fn score_tolk(
    contract: &str,
    abi: &sting_tolkc::AbiJson,
    schema: &ContractSchema,
) -> Tier1Result {
    let mut result = Tier1Result::new(contract.to_string());
    // `unique_types[i]`/`schema.types[i]` share the same index space by
    // construction (`mapping::abi_to_schema` builds `types` via
    // `abi.unique_types.iter().enumerate()`), so a raw-side lookback by
    // `TypeRef` index is always valid here — unlike Tact, whose
    // `TypeInterner` builds an independently-indexed table. See
    // `contains_unknown`'s own doc comment for why this distinction matters:
    // a real, compiler-declared `"kind":"unknown"` (e.g. inside the stdlib
    // `tuple` alias) is not a normalizer gap the way an unresolved
    // `StructRef`/`Callable`/`VarUintN`/`GenericT` degrading to `Unknown`
    // would be.
    let is_expected_unknown = |ty: TypeRef| {
        matches!(
            abi.unique_types.get(ty.0 as usize),
            Some(sting_tolkc::TypeDescriptor::Unknown)
        )
    };

    if let Some(storage_idx) = abi.storage.storage_ty_idx {
        let ok = schema.storage == Some(TypeRef(storage_idx))
            && !contains_unknown(
                schema,
                TypeRef(storage_idx),
                UNKNOWN_CHECK_DEPTH,
                &is_expected_unknown,
            );
        result.storage.record(ok);
        if !ok {
            result
                .gaps
                .push("storage: storage_ty_idx did not resolve cleanly".to_string());
        }
    }

    let get_method_by_name: std::collections::HashMap<&str, &sting_abi_core::GetMethod> = schema
        .get_methods
        .iter()
        .map(|m| (m.name.as_str(), m))
        .collect();
    for getter in &abi.get_methods {
        let ok = get_method_by_name
            .get(getter.name.as_str())
            .is_some_and(|m| {
                m.tvm_method_id == Some(i64::from(getter.tvm_method_id))
                    && m.parameters.len() == getter.parameters.len()
                    && m.parameters.iter().zip(&getter.parameters).all(|(mp, gp)| {
                        mp.name == gp.name
                            && mp.ty == TypeRef(gp.ty_idx)
                            && type_ok(schema, Some(mp.ty), &is_expected_unknown)
                    })
                    && m.return_ty == Some(TypeRef(getter.return_ty_idx))
                    && type_ok(schema, m.return_ty, &is_expected_unknown)
            });
        result.get_methods.record(ok);
        if !ok {
            result.gaps.push(format!(
                "get_methods: {} did not round-trip cleanly",
                getter.name
            ));
        }
    }

    for error in &abi.thrown_errors {
        let expected = match error {
            sting_tolkc::ThrownError::PlainInt { err_code } => {
                sting_abi_core::ErrorCode::Plain { code: *err_code }
            }
            sting_tolkc::ThrownError::Constant { name, err_code }
            | sting_tolkc::ThrownError::EnumMember { name, err_code } => {
                sting_abi_core::ErrorCode::Named {
                    name: name.clone(),
                    code: *err_code,
                }
            }
        };
        let ok = schema.errors.contains(&expected);
        result.errors.record(ok);
        if !ok {
            result
                .gaps
                .push(format!("errors: {expected:?} not found in ContractSchema"));
        }
    }

    score_tolk_messages(
        &mut result.incoming_internal,
        &abi.incoming_messages,
        &schema.incoming_internal,
    );
    score_tolk_messages(
        &mut result.incoming_external,
        &abi.incoming_external,
        &schema.incoming_external,
    );
    score_tolk_messages(
        &mut result.outgoing,
        &abi.outgoing_messages,
        &schema.outgoing,
    );

    result
}

/// Checked against the specific corresponding schema list (not the union of
/// all three) — Tolk's own `abi_to_schema` maps each of
/// `incoming_messages`/`incoming_external`/`outgoing_messages` straight
/// across with no cross-category branching, so a raw entry landing in the
/// wrong normalized category would itself be a real bug this check should
/// catch, not paper over by treating any of the three as an acceptable
/// match.
fn score_tolk_messages(
    score: &mut CategoryScore,
    raw: &[sting_tolkc::MessageRef],
    normalized: &[sting_abi_core::MessageRef],
) {
    let present: HashSet<u32> = normalized.iter().map(|m| m.ty.0).collect();
    for msg in raw {
        score.record(present.contains(&msg.body_ty_idx));
    }
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "docs/ARCHITECTURE.md §3.2 scopes the no-unwrap/expect policy to library code outside tests"
)]
mod tests {
    use super::{score_tact, score_tolk};

    fn tact_abi(json: &str) -> sting_abi_tact::ContractAbi {
        serde_json::from_str(json).expect("synthetic ContractAbi must be well-formed")
    }

    fn tolk_abi(extra: &str) -> sting_tolkc::AbiJson {
        let json = format!(
            r#"{{
                "abi_schema_version": "1.0",
                "contract_name": "Synthetic",
                "unique_types": [{{"kind": "void"}}, {{"kind": "int"}}],
                "declarations": [],
                "storage": {{}},
                "compiler_name": "tolk",
                "compiler_version": "1.4.1"
                {extra}
            }}"#
        );
        serde_json::from_str(&json).expect("synthetic AbiJson must be well-formed")
    }

    /// The honest baseline: `sting_abi_tact::abi_to_schema` on a
    /// well-formed real-shaped ABI must score 100% on every populated
    /// category — proves the checker doesn't cry wolf on a clean mapping.
    #[test]
    fn a_clean_tact_mapping_scores_full_completeness_on_every_populated_category() {
        let abi = tact_abi(
            r#"{
                "name": "Synthetic",
                "types": [
                    {"name": "Synthetic$Data", "header": null, "fields": [
                        {"name": "owner", "type": {"kind": "simple", "type": "address", "optional": false}}
                    ]}
                ],
                "receivers": [],
                "getters": [{
                    "name": "getOwner",
                    "methodId": 12345,
                    "arguments": [],
                    "returnType": {"kind": "simple", "type": "address", "optional": false}
                }],
                "errors": {"50": {"message": "Access denied"}}
            }"#,
        );
        let schema = sting_abi_tact::abi_to_schema(&abi);
        let result = score_tact("synthetic", &abi, &schema);

        assert_eq!(result.storage.ratio(), Some(1.0), "gaps: {:?}", result.gaps);
        assert_eq!(
            result.get_methods.ratio(),
            Some(1.0),
            "gaps: {:?}",
            result.gaps
        );
        assert_eq!(result.errors.ratio(), Some(1.0), "gaps: {:?}", result.gaps);
        assert!(result.gaps.is_empty());
    }

    /// Proves the checker actually detects a real, injected loss — a
    /// getter's return type manually corrupted to `Unknown` after mapping
    /// (simulating a hypothetical future `abi_to_schema` regression) must
    /// drag `get_methods` completeness below 100%, not silently pass.
    #[test]
    fn a_corrupted_return_type_is_caught_not_silently_passed() {
        let abi = tact_abi(
            r#"{
                "name": "Synthetic",
                "types": [],
                "receivers": [],
                "getters": [{
                    "name": "getOwner",
                    "methodId": 12345,
                    "arguments": [],
                    "returnType": {"kind": "simple", "type": "address", "optional": false}
                }],
                "errors": {}
            }"#,
        );
        let mut schema = sting_abi_tact::abi_to_schema(&abi);
        // Corrupt the resolved return type in place to simulate a
        // regression a future `abi_to_schema` change might introduce.
        let return_ty = schema.get_methods[0]
            .return_ty
            .expect("return type must exist");
        schema.types[return_ty.0 as usize].shape = sting_abi_core::TypeShape::Unknown;

        let result = score_tact("synthetic", &abi, &schema);
        assert_eq!(
            result.get_methods,
            super::CategoryScore {
                matched: 0,
                total: 1
            }
        );
        assert!(
            result.gaps.iter().any(|g| g.contains("getOwner")),
            "gaps: {:?}",
            result.gaps
        );
    }

    /// A getter with a dropped parameter (mapping produced fewer params
    /// than the raw artifact declared) must also be caught.
    #[test]
    fn a_dropped_parameter_is_caught() {
        let abi = tact_abi(
            r#"{
                "name": "Synthetic",
                "types": [],
                "receivers": [],
                "getters": [{
                    "name": "getBalanceOf",
                    "methodId": 999,
                    "arguments": [{"name": "owner", "type": {"kind": "simple", "type": "address", "optional": false}}],
                    "returnType": {"kind": "simple", "type": "int", "optional": false, "format": 257}
                }],
                "errors": {}
            }"#,
        );
        let mut schema = sting_abi_tact::abi_to_schema(&abi);
        schema.get_methods[0].parameters.clear();

        let result = score_tact("synthetic", &abi, &schema);
        assert_eq!(result.get_methods.matched, 0);
        assert_eq!(result.get_methods.total, 1);
    }

    /// A contract with no getters/errors/storage at all — every category
    /// must report `None` (not applicable), never `Some(0.0)` (a wrong,
    /// misleadingly-alarming "0% complete").
    #[test]
    fn an_empty_contract_reports_not_applicable_not_zero() {
        let abi = tact_abi(
            r#"{"name": "Empty", "types": [], "receivers": [], "getters": [], "errors": {}}"#,
        );
        let schema = sting_abi_tact::abi_to_schema(&abi);
        let result = score_tact("empty", &abi, &schema);

        assert_eq!(result.storage.ratio(), None);
        assert_eq!(result.get_methods.ratio(), None);
        assert_eq!(result.errors.ratio(), None);
        assert_eq!(result.incoming_internal.ratio(), None);
        assert_eq!(result.outgoing.ratio(), None);
    }

    /// A text receiver (`receive("Hello") { ... }`) is a documented,
    /// intentional v1 gap — must not count against completeness at all.
    #[test]
    fn text_receivers_do_not_count_against_completeness() {
        let abi = tact_abi(
            r#"{
                "name": "Synthetic",
                "types": [{"name": "Ping", "header": 1, "fields": []}],
                "receivers": [
                    {"receiver": "internal", "message": {"kind": "text", "text": "hello"}},
                    {"receiver": "internal", "message": {"kind": "typed", "type": "Ping"}}
                ],
                "getters": [],
                "errors": {}
            }"#,
        );
        let schema = sting_abi_tact::abi_to_schema(&abi);
        let result = score_tact("synthetic", &abi, &schema);

        assert_eq!(
            result.incoming_internal,
            super::CategoryScore {
                matched: 1,
                total: 1
            },
            "the text receiver must not inflate the denominator"
        );
    }

    #[test]
    fn a_clean_tolk_mapping_scores_full_completeness() {
        let abi = tolk_abi(
            r#", "get_methods": [{
                "tvm_method_id": 12345,
                "name": "getOwner",
                "parameters": [],
                "return_ty_idx": 1
            }],
            "thrown_errors": [{"kind": "plain_int", "err_code": 50}]"#,
        );
        let schema = sting_abi_tolk::abi_to_schema(&abi);
        let result = score_tolk("synthetic", &abi, &schema);

        assert_eq!(
            result.get_methods.ratio(),
            Some(1.0),
            "gaps: {:?}",
            result.gaps
        );
        assert_eq!(result.errors.ratio(), Some(1.0), "gaps: {:?}", result.gaps);
        assert!(result.gaps.is_empty());
    }

    #[test]
    fn a_tolk_getter_pointing_at_an_unresolved_type_is_caught() {
        // ty_idx 2 does not exist in unique_types (only 0/1 are declared) —
        // `abi_to_schema` cannot fabricate a type table entry for it, so
        // the getter's own presence check must fail, not silently pass.
        let abi: sting_tolkc::AbiJson = serde_json::from_str(
            r#"{
                "abi_schema_version": "1.0",
                "contract_name": "Synthetic",
                "unique_types": [{"kind": "void"}, {"kind": "int"}],
                "declarations": [],
                "storage": {},
                "compiler_name": "tolk",
                "compiler_version": "1.4.1",
                "get_methods": [{
                    "tvm_method_id": 1,
                    "name": "broken",
                    "parameters": [],
                    "return_ty_idx": 99
                }]
            }"#,
        )
        .expect("well-formed");
        let schema = sting_abi_tolk::abi_to_schema(&abi);
        let result = score_tolk("synthetic", &abi, &schema);
        assert_eq!(result.get_methods.matched, 0);
        assert_eq!(result.get_methods.total, 1);
    }

    /// Regression test for the real finding this harness's first run
    /// against `Config.tolk` surfaced (see [`super::contains_unknown`]'s own
    /// doc comment): a getter whose return type is the real compiler's own
    /// `"kind":"unknown"` (e.g. inside Tolk's stdlib `tuple` alias) must
    /// round-trip cleanly — that `Unknown` came from the source, not from
    /// `sting-abi-tolk` failing to model something it should have.
    #[test]
    fn a_raw_source_declared_unknown_type_is_not_a_gap() {
        let abi: sting_tolkc::AbiJson = serde_json::from_str(
            r#"{
                "abi_schema_version": "1.0",
                "contract_name": "Synthetic",
                "unique_types": [{"kind": "void"}, {"kind": "unknown"}],
                "declarations": [],
                "storage": {},
                "compiler_name": "tolk",
                "compiler_version": "1.4.1",
                "get_methods": [{
                    "tvm_method_id": 1,
                    "name": "getTupleElement",
                    "parameters": [],
                    "return_ty_idx": 1
                }]
            }"#,
        )
        .expect("well-formed");
        let schema = sting_abi_tolk::abi_to_schema(&abi);
        let result = score_tolk("synthetic", &abi, &schema);
        assert_eq!(
            result.get_methods,
            super::CategoryScore {
                matched: 1,
                total: 1
            },
            "gaps: {:?}",
            result.gaps
        );
    }

    /// Contrast case for the previous test: a return type that degrades to
    /// `Unknown` because `sting-abi-tolk` couldn't resolve a
    /// **`StructRef`** (not because the compiler itself said "unknown") is
    /// still a real gap — the fix must not have over-corrected into
    /// treating every `Unknown` as fine.
    #[test]
    fn an_unresolved_struct_ref_is_still_a_real_gap() {
        let abi: sting_tolkc::AbiJson = serde_json::from_str(
            r#"{
                "abi_schema_version": "1.0",
                "contract_name": "Synthetic",
                "unique_types": [
                    {"kind": "void"},
                    {"kind": "StructRef", "struct_name": "NeverDeclared"}
                ],
                "declarations": [],
                "storage": {},
                "compiler_name": "tolk",
                "compiler_version": "1.4.1",
                "get_methods": [{
                    "tvm_method_id": 1,
                    "name": "broken",
                    "parameters": [],
                    "return_ty_idx": 1
                }]
            }"#,
        )
        .expect("well-formed");
        let schema = sting_abi_tolk::abi_to_schema(&abi);
        let result = score_tolk("synthetic", &abi, &schema);
        assert_eq!(result.get_methods.matched, 0);
        assert_eq!(result.get_methods.total, 1);
    }
}
