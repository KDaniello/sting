use crate::TypeRef;

/// One named, typed parameter of a [`GetMethod`]. Kept as a distinct type
/// from [`crate::FieldRef`] — see that type's own doc comment for why.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Param {
    /// Parameter name as declared.
    pub name: String,
    /// Index into [`ContractSchema::types`](crate::ContractSchema::types).
    pub ty: TypeRef,
}

/// One `get`-method, as populated into
/// [`ContractSchema::get_methods`](crate::ContractSchema::get_methods).
/// Structurally compatible with all three source tiers by construction —
/// see `docs/domains/abi-ir.md`'s `get_methods` paragraph:
/// - **Tolk** and **Tact** (Tier 1): both always carry `tvm_method_id` and a
///   typed `return_ty` on every real compiled `GetMethod`/`ABIGetter` — Tact
///   confirmed empirically (`crates/sting-abi-tact/tests/fixtures/SOURCE.md`):
///   the compiler fills in a real `methodId` even
///   for getters that never declared one explicitly in source.
/// - **`FunC`** (Tier 2, heuristic): reliably recovers `name` and raw
///   `parameters`, but not always a resolved `tvm_method_id` or a typed
///   `return_ty` purely from source text without compiling — see
///   `docs/domains/abi-ir.md`'s reading of `verifier.ton.org`'s
///   `getterParser.ts`. `Option` here exists for `FunC`'s sake, not Tact's.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct GetMethod {
    /// The TVM method id (CRC16-derived from the name by default, per
    /// TON's own get-method convention) that off-chain callers invoke by,
    /// if known. **Signed, not `u32`** — Tolk's own default-derived ids are
    /// always non-negative, but Tact allows an explicit `get(EXPR) fun`
    /// annotation whose value can be genuinely negative (confirmed on real
    /// compiled output: `getters.tact`'s `get(-pow2(18))` produces
    /// `methodId: -262144` — TON's own get-method ids are documented as
    /// 19-bit *signed* integers, not the unsigned-only range an earlier
    /// version of this field assumed from Tolk's output alone).
    pub tvm_method_id: Option<i64>,
    /// Method name as declared.
    pub name: String,
    /// Named, typed parameter list.
    pub parameters: Vec<Param>,
    /// Index into [`ContractSchema::types`](crate::ContractSchema::types)
    /// for the return type, if the extractor could resolve one.
    pub return_ty: Option<TypeRef>,
}
