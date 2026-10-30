//! `sting_tolkc::AbiJson` → `sting_abi_core::ContractSchema`, field by
//! field, per `docs/domains/abi-ir.md`'s lineage and
//! `docs/domains/extractors.md`
//!
//! **The index-table relationship this module depends on, confirmed
//! empirically, not assumed** (compiled `corpus/clean/acton-contracts/
//! config/Config.tolk` and inspected the real `AbiJson` directly): a named
//! struct/alias/enum shows up *twice* — once in `unique_types[i]` as a bare
//! `StructRef`/`AliasRef`/`EnumRef{name}` placeholder, and once in
//! `declarations[]` as the real entry (fields, prefix, description) whose
//! own `ty_idx` equals that same `i`. Anything that references index `i`
//! (a field's `ty_idx`, a message's `body_ty_idx`) is pointing at the
//! *placeholder* slot, so resolving a name to its real shape means joining
//! `unique_types[i]` against the `declarations` entry with matching
//! `ty_idx` — never the other way around, and never by name lookup at
//! extraction-consumer time (that join happens once, here).

use std::collections::HashMap;

use sting_abi_core::{
    ContractMetadata, ContractSchema, ErrorCode, ExtractionTier, FieldRef, GetMethod, MessageRef,
    NamedType, Param, SourceLanguage, TlbPrefix, TypeRef, TypeShape, UnionVariant,
};
use sting_tolkc::{AbiJson, Declaration, ThrownError, TypeDescriptor};

/// Converts a full `tolk` `AbiJson` artifact into a normalized
/// `ContractSchema`. Infallible: every `AbiJson` this crate has ever seen
/// (real `tolk` output) maps cleanly — a shape this module doesn't
/// recognize degrades to [`TypeShape::Unknown`] rather than erroring, the
/// same "carried through, not dropped or panicked on" principle
/// `sting_tolkc::TypeDescriptor::Other` already applies one level down.
#[must_use]
pub fn abi_to_schema(abi: &AbiJson) -> ContractSchema {
    let declarations_by_ty_idx = index_declarations(abi);

    // Positional pass — indices `0..unique_types.len()` are stable and must
    // never move: every `TypeRef` elsewhere in the schema (a field's
    // `ty_idx`, a message's `body_ty_idx`, ...) already points at a
    // position in *this* vec, not a name. Only ever appended to after this
    // point, never inserted into or reordered — see `expand_address_opt`.
    let mut types: Vec<NamedType> = abi
        .unique_types
        .iter()
        .enumerate()
        .map(|(idx, descriptor)| {
            convert_named_type(
                u32::try_from(idx).unwrap_or(u32::MAX),
                descriptor,
                &declarations_by_ty_idx,
            )
        })
        .collect();

    expand_address_opt(abi, &mut types);

    ContractSchema {
        ir_schema_version: sting_abi_core::IR_SCHEMA_VERSION.to_string(),
        source_language: SourceLanguage::Tolk,
        extraction_tier: ExtractionTier::Compiled,
        contract_name: Some(abi.contract_name.clone()),
        metadata: ContractMetadata {
            author: abi.author.clone(),
            version: abi.version.clone(),
            description: abi.description.clone(),
        },
        storage: abi.storage.storage_ty_idx.map(TypeRef),
        types,
        incoming_internal: abi
            .incoming_messages
            .iter()
            .copied()
            .map(convert_message_ref)
            .collect(),
        incoming_external: abi
            .incoming_external
            .iter()
            .copied()
            .map(convert_message_ref)
            .collect(),
        outgoing: abi
            .outgoing_messages
            .iter()
            .copied()
            .map(convert_message_ref)
            .collect(),
        get_methods: abi.get_methods.iter().map(convert_get_method).collect(),
        errors: abi.thrown_errors.iter().map(convert_thrown_error).collect(),
        confidence: None,
        field_confidence: None,
    }
}

/// `TypeDescriptor::AddressOpt` is a short-form the compiler emits in
/// place of the general `{"kind":"nullable","inner_ty_idx":...}` wrapper
/// every other nullable type uses — there is no `unique_types[]` slot
/// holding a bare `Address` for it to point at as `Nullable::inner`, unlike
/// every other nullable shape (see `sting_tolkc::TypeDescriptor::AddressOpt`'s
/// own doc comment for how this was found — a systematic audit of this
/// module's coverage against the real corpus, not a one-off report).
/// Reuses an existing bare (undeclared, anonymous) `Address` slot if one
/// is already present in `unique_types[]` — the common real case, since a
/// contract with a nullable address field usually has a plain one
/// elsewhere too — only appending a new synthesized slot if genuinely none
/// exists. Mutates each `AddressOpt` entry's already-positional `types[i]`
/// in place (from `convert_shape`'s safe `Unknown` fallback) to the real
/// `Nullable{inner}` shape — never moves or removes an existing index.
fn expand_address_opt(abi: &AbiJson, types: &mut Vec<NamedType>) {
    let address_opt_indices: Vec<usize> = abi
        .unique_types
        .iter()
        .enumerate()
        .filter(|(_, d)| matches!(d, TypeDescriptor::AddressOpt))
        .map(|(idx, _)| idx)
        .collect();
    if address_opt_indices.is_empty() {
        return;
    }

    let address_slot = find_bare_address_slot(&abi.unique_types).unwrap_or_else(|| {
        types.push(NamedType {
            name: None,
            description: None,
            prefix: None,
            shape: TypeShape::Address,
        });
        u32::try_from(types.len() - 1).unwrap_or(u32::MAX)
    });

    for idx in address_opt_indices {
        types[idx].shape = TypeShape::Nullable {
            inner: TypeRef(address_slot),
        };
    }
}

/// Finds an existing anonymous `{"kind":"address"}` entry in `unique_types`
/// — always undeclared (`Declaration` only ever has `Struct`/`Alias`/`Enum`
/// variants, never a bare primitive), so no `declarations_by_ty_idx` lookup
/// is needed to tell a "real" `Address` slot apart from a declared one.
fn find_bare_address_slot(unique_types: &[TypeDescriptor]) -> Option<u32> {
    unique_types
        .iter()
        .position(|d| matches!(d, TypeDescriptor::Address))
        .and_then(|idx| u32::try_from(idx).ok())
}

fn index_declarations(abi: &AbiJson) -> HashMap<u32, &Declaration> {
    abi.declarations
        .iter()
        .map(|decl| (declaration_ty_idx(decl), decl))
        .collect()
}

fn declaration_ty_idx(decl: &Declaration) -> u32 {
    match decl {
        Declaration::Alias { ty_idx, .. }
        | Declaration::Struct { ty_idx, .. }
        | Declaration::Enum { ty_idx, .. } => *ty_idx,
    }
}

fn convert_message_ref(msg: sting_tolkc::MessageRef) -> MessageRef {
    MessageRef {
        ty: TypeRef(msg.body_ty_idx),
    }
}

fn convert_get_method(method: &sting_tolkc::GetMethod) -> GetMethod {
    GetMethod {
        tvm_method_id: Some(i64::from(method.tvm_method_id)),
        name: method.name.clone(),
        parameters: method
            .parameters
            .iter()
            .map(|p| Param {
                name: p.name.clone(),
                ty: TypeRef(p.ty_idx),
            })
            .collect(),
        return_ty: Some(TypeRef(method.return_ty_idx)),
    }
}

fn convert_thrown_error(error: &ThrownError) -> ErrorCode {
    match error {
        ThrownError::PlainInt { err_code } => ErrorCode::Plain { code: *err_code },
        // Constant (`const ERROR_X = 33`) and EnumMember (`enum Errors { ... }`)
        // are two different Tolk declaration styles for the same normalized
        // shape here — a consumer decoding an error code doesn't need to
        // know which style the source used, only that it has a name.
        ThrownError::Constant { name, err_code } | ThrownError::EnumMember { name, err_code } => {
            ErrorCode::Named {
                name: name.clone(),
                code: *err_code,
            }
        }
    }
}

fn convert_tlb_prefix(prefix: sting_tolkc::TlbPrefix) -> TlbPrefix {
    TlbPrefix {
        prefix_num: prefix.prefix_num,
        prefix_len: prefix.prefix_len,
    }
}

fn bit_width(n: u32) -> u16 {
    u16::try_from(n).unwrap_or(u16::MAX)
}

fn convert_named_type(
    idx: u32,
    descriptor: &TypeDescriptor,
    declarations_by_ty_idx: &HashMap<u32, &Declaration>,
) -> NamedType {
    if let Some(decl) = declarations_by_ty_idx.get(&idx) {
        return convert_declaration(decl);
    }

    NamedType {
        name: None,
        description: None,
        prefix: None,
        shape: convert_shape(descriptor),
    }
}

fn convert_declaration(decl: &Declaration) -> NamedType {
    match decl {
        Declaration::Struct {
            name,
            prefix,
            fields,
            description,
            ..
        } => NamedType {
            name: Some(name.clone()),
            description: description.clone(),
            prefix: prefix.map(convert_tlb_prefix),
            shape: TypeShape::Struct {
                fields: fields
                    .iter()
                    .map(|f| FieldRef {
                        name: f.name.clone(),
                        ty: TypeRef(f.ty_idx),
                    })
                    .collect(),
            },
        },
        Declaration::Alias {
            name,
            target_ty_idx,
            description,
            ..
        } => NamedType {
            name: Some(name.clone()),
            description: description.clone(),
            prefix: None,
            shape: TypeShape::Alias {
                target: TypeRef(*target_ty_idx),
            },
        },
        Declaration::Enum {
            name,
            encoded_as_ty_idx,
            members,
            ..
        } => NamedType {
            name: Some(name.clone()),
            // `tolk`'s own `Declaration::Enum` carries no `description`
            // field at all (confirmed against `sting_tolkc::Declaration`'s
            // real shape — only `Struct`/`Alias` have one), not a gap in
            // this mapping.
            description: None,
            prefix: None,
            shape: TypeShape::Enum {
                encoded_as: TypeRef(*encoded_as_ty_idx),
                member_names: enum_member_names(members),
            },
        },
    }
}

/// `Declaration::Enum::members` is opaque, unconfirmed JSON even inside
/// `sting_tolkc` itself (that crate's own doc comment: "real per-member
/// field shape not yet confirmed against compiler output, kept opaque
/// rather than guessed"). This extracts just a `"name"` string field if one
/// is present, on the reasonable-but-unconfirmed assumption every member
/// object has one — anything that doesn't match that shape is silently
/// skipped rather than guessed at further, so a wrong assumption here loses
/// a name, not the whole extraction.
fn enum_member_names(members: &[serde_json::Value]) -> Vec<String> {
    members
        .iter()
        .filter_map(|member| member.get("name").and_then(serde_json::Value::as_str))
        .map(str::to_string)
        .collect()
}

fn convert_shape(descriptor: &TypeDescriptor) -> TypeShape {
    match descriptor {
        TypeDescriptor::Void => TypeShape::Void,
        TypeDescriptor::Int => TypeShape::Int {
            bits: None,
            signed: true,
        },
        TypeDescriptor::IntN { n } => TypeShape::Int {
            bits: Some(bit_width(*n)),
            signed: true,
        },
        TypeDescriptor::UintN { n } => TypeShape::Int {
            bits: Some(bit_width(*n)),
            signed: false,
        },
        TypeDescriptor::Bool => TypeShape::Bool,
        TypeDescriptor::Coins => TypeShape::Coins,
        // `AddressAny` (the full TL-B `MsgAddress` union) vs. `Address`'s
        // narrower, internal-only common case — this IR does not yet
        // distinguish them as separate shapes
        // (`sting_tolkc::TypeDescriptor::AddressAny`'s own doc comment), a
        // documented simplification, not a silent gap.
        TypeDescriptor::Address | TypeDescriptor::AddressAny => TypeShape::Address,
        TypeDescriptor::Cell => TypeShape::Cell,
        TypeDescriptor::Slice => TypeShape::Slice,
        TypeDescriptor::Builder => TypeShape::Builder,
        TypeDescriptor::String => TypeShape::Str,
        TypeDescriptor::BitsN { n } => TypeShape::Bits {
            width: bit_width(*n),
        },
        TypeDescriptor::Remaining => TypeShape::Remaining,
        TypeDescriptor::Nullable { inner_ty_idx, .. } => TypeShape::Nullable {
            inner: TypeRef(*inner_ty_idx),
        },
        TypeDescriptor::ArrayOf { inner_ty_idx } => TypeShape::Array {
            inner: TypeRef(*inner_ty_idx),
        },
        TypeDescriptor::CellOf { inner_ty_idx } => TypeShape::CellPacked {
            inner: TypeRef(*inner_ty_idx),
        },
        TypeDescriptor::LispListOf { inner_ty_idx } => TypeShape::LispList {
            inner: TypeRef(*inner_ty_idx),
        },
        TypeDescriptor::MapKv {
            key_ty_idx,
            value_ty_idx,
        } => TypeShape::Map {
            key: TypeRef(*key_ty_idx),
            value: TypeRef(*value_ty_idx),
        },
        TypeDescriptor::Tensor { items_ty_idx } => TypeShape::Tensor {
            items: items_ty_idx.iter().copied().map(TypeRef).collect(),
        },
        TypeDescriptor::Union { variants, .. } => TypeShape::Union {
            variants: variants
                .iter()
                .map(|v| UnionVariant {
                    ty: TypeRef(v.variant_ty_idx),
                    prefix: TlbPrefix {
                        prefix_num: v.prefix_num,
                        prefix_len: v.prefix_len,
                    },
                    is_prefix_implicit: v.is_prefix_implicit,
                })
                .collect(),
        },
        // StructRef/AliasRef/EnumRef reaching this branch means no
        // declaration matched its ty_idx — should not happen against
        // real, well-formed `tolk` output (every real case observed joins
        // cleanly, see this module's own top doc comment), but a malformed
        // or future-version artifact degrading to Unknown here is
        // consistent with `sting_tolkc::TypeDescriptor::Other`'s own
        // "carried through, not panicked on" principle rather than a new
        // failure mode.
        //
        // Callable (bare marker, unclear real semantics — never observed
        // paired with actual field data across 7 real contracts sampled
        // building this extractor), VarUintN (variable-width length-
        // prefixed encoding — modeling it as a fixed-width `Int` would
        // misrepresent the wire format; not observed in practice either),
        // GenericT (unresolved generic type parameter — should not survive
        // into monomorphized ABI output at all), and NullLiteral (the type
        // of a bare `null` expression — confirmed real by the same corpus
        // audit that found `AddressAny`/`AddressOpt`, but never observed as
        // any real struct/message field's type, unlike those two — see
        // `sting_tolkc::TypeDescriptor::NullLiteral`'s own doc comment) are
        // the same "not guessed ahead of real evidence" call as the
        // module-level gaps already documented on `sting_abi_core::TypeShape`.
        //
        // `AddressOpt` lands here too, but only as a placeholder —
        // `expand_address_opt` overwrites every `AddressOpt` entry's
        // `types[i]` with the real `Nullable{inner}` shape in a second
        // pass, since building that shape needs a synthesized `Address`
        // slot this single-descriptor match can't allocate on its own (see
        // `expand_address_opt`'s own doc comment).
        TypeDescriptor::StructRef { .. }
        | TypeDescriptor::AliasRef { .. }
        | TypeDescriptor::EnumRef { .. }
        | TypeDescriptor::Callable
        | TypeDescriptor::VarUintN { .. }
        | TypeDescriptor::GenericT { .. }
        | TypeDescriptor::NullLiteral
        | TypeDescriptor::AddressOpt
        | TypeDescriptor::Unknown
        | TypeDescriptor::Other => TypeShape::Unknown,
    }
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "docs/ARCHITECTURE.md §3.2 scopes the no-unwrap/expect/panic policy to library code outside tests"
)]
mod tests {
    use sting_abi_core::{ErrorCode, TypeRef, TypeShape};

    use super::abi_to_schema;

    /// Minimal, hand-written (not real-compiler-produced) `AbiJson`, valid
    /// per every field `sting_tolkc::AbiJson` actually requires — fast,
    /// synthetic coverage per `docs/TESTING.md`,
    /// complementary to `tests/golden.rs`'s real-compiler fixtures, not a
    /// replacement for them.
    fn synthetic_abi(extra_json: &str) -> sting_tolkc::AbiJson {
        let json = format!(
            r#"{{
                "abi_schema_version": "1.0",
                "contract_name": "Synthetic",
                "unique_types": [
                    {{"kind": "void"}},
                    {{"kind": "int"}},
                    {{"kind": "StructRef", "struct_name": "Foo"}}
                ],
                "declarations": [
                    {{"kind": "struct", "name": "Foo", "ty_idx": 2, "fields": [{{"name": "x", "ty_idx": 1}}]}}
                ],
                "storage": {{}},
                "compiler_name": "tolk",
                "compiler_version": "1.4.1"
                {extra_json}
            }}"#
        );
        serde_json::from_str(&json)
            .expect("synthetic AbiJson must be well-formed per the schema above")
    }

    #[test]
    fn storage_none_maps_to_schema_storage_none() {
        let abi = synthetic_abi("");
        let schema = abi_to_schema(&abi);
        assert_eq!(schema.storage, None);
    }

    #[test]
    fn a_struct_declaration_resolves_via_its_matching_ty_idx() {
        let abi = synthetic_abi("");
        let schema = abi_to_schema(&abi);

        // index 2 in unique_types is a bare StructRef placeholder; the real
        // shape must come from the declarations entry whose own ty_idx is 2.
        let foo = &schema.types[2];
        assert_eq!(foo.name.as_deref(), Some("Foo"));
        match &foo.shape {
            TypeShape::Struct { fields } => {
                assert_eq!(fields.len(), 1);
                assert_eq!(fields[0].name, "x");
                assert_eq!(fields[0].ty, TypeRef(1));
            }
            other => panic!("expected Struct, got {other:?}"),
        }
    }

    #[test]
    fn an_unresolved_type_descriptor_degrades_to_unknown_not_a_panic() {
        let abi: sting_tolkc::AbiJson = serde_json::from_str(
            r#"{
                "abi_schema_version": "1.0",
                "contract_name": "Synthetic",
                "unique_types": [{"kind": "void"}, {"kind": "genericT", "name_t": "T"}],
                "declarations": [],
                "storage": {},
                "compiler_name": "tolk",
                "compiler_version": "1.4.1"
            }"#,
        )
        .expect("well-formed");
        let schema = abi_to_schema(&abi);
        assert_eq!(schema.types[1].shape, TypeShape::Unknown);
    }

    /// Regression test for a real gap found by a corpus run
    /// (`corpus/clean/acton-contracts/nft-v1.1`'s `content`-shaped storage
    /// fields): Tolk's `string` type used to have no matching
    /// `TypeDescriptor`/`TypeShape` arm at all and silently degraded to
    /// `Unknown` — confirmed real (the compiler genuinely emits
    /// `{"kind":"string"}`), not a hypothetical gap.
    #[test]
    fn a_string_type_descriptor_resolves_to_str() {
        let abi: sting_tolkc::AbiJson = serde_json::from_str(
            r#"{
                "abi_schema_version": "1.0",
                "contract_name": "Synthetic",
                "unique_types": [{"kind": "void"}, {"kind": "string"}],
                "declarations": [],
                "storage": {},
                "compiler_name": "tolk",
                "compiler_version": "1.4.1"
            }"#,
        )
        .expect("well-formed");
        let schema = abi_to_schema(&abi);
        assert_eq!(schema.types[1].shape, TypeShape::Str);
    }

    /// Regression test for the real gap found by a systematic corpus audit
    /// (`docs/domains/service.md`, real
    /// examples: `MinterStorage.adminAddress`, `NftDataReply.ownerAddress`):
    /// `addressOpt` used to have no matching `TypeShape` arm and silently
    /// degraded to `Unknown`. Reuses the existing bare `address` slot (index
    /// 1 here) rather than allocating a new one.
    #[test]
    fn an_address_opt_type_descriptor_resolves_to_nullable_address_reusing_the_existing_slot() {
        let abi: sting_tolkc::AbiJson = serde_json::from_str(
            r#"{
                "abi_schema_version": "1.0",
                "contract_name": "Synthetic",
                "unique_types": [{"kind": "void"}, {"kind": "address"}, {"kind": "addressOpt"}],
                "declarations": [],
                "storage": {},
                "compiler_name": "tolk",
                "compiler_version": "1.4.1"
            }"#,
        )
        .expect("well-formed");
        let schema = abi_to_schema(&abi);
        assert_eq!(
            schema.types.len(),
            3,
            "no new slot allocated — one already existed"
        );
        assert_eq!(
            schema.types[2].shape,
            TypeShape::Nullable { inner: TypeRef(1) }
        );
    }

    /// Same gap, but with no existing bare `address` slot to reuse — a new
    /// one must be synthesized and appended, never inserted into the
    /// existing positional range (every other index must stay stable).
    #[test]
    fn an_address_opt_type_descriptor_synthesizes_a_new_address_slot_when_none_exists() {
        let abi: sting_tolkc::AbiJson = serde_json::from_str(
            r#"{
                "abi_schema_version": "1.0",
                "contract_name": "Synthetic",
                "unique_types": [{"kind": "void"}, {"kind": "addressOpt"}],
                "declarations": [],
                "storage": {},
                "compiler_name": "tolk",
                "compiler_version": "1.4.1"
            }"#,
        )
        .expect("well-formed");
        let schema = abi_to_schema(&abi);
        assert_eq!(schema.types.len(), 3, "one new slot appended at the end");
        assert_eq!(schema.types[2].shape, TypeShape::Address);
        assert_eq!(
            schema.types[1].shape,
            TypeShape::Nullable { inner: TypeRef(2) }
        );
    }

    /// `addressAny` (the full TL-B `MsgAddress` union) maps directly to the
    /// same `TypeShape::Address` as the narrower, common `address` kind —
    /// a documented simplification (`sting_tolkc::TypeDescriptor::AddressAny`),
    /// not silently lost to `Unknown` the way it was before this audit.
    #[test]
    fn an_address_any_type_descriptor_resolves_to_address() {
        let abi: sting_tolkc::AbiJson = serde_json::from_str(
            r#"{
                "abi_schema_version": "1.0",
                "contract_name": "Synthetic",
                "unique_types": [{"kind": "void"}, {"kind": "addressAny"}],
                "declarations": [],
                "storage": {},
                "compiler_name": "tolk",
                "compiler_version": "1.4.1"
            }"#,
        )
        .expect("well-formed");
        let schema = abi_to_schema(&abi);
        assert_eq!(schema.types[1].shape, TypeShape::Address);
    }

    /// `nullLiteral` — confirmed real by the same audit, but never observed
    /// as a real schema field's type, unlike `addressOpt`/`addressAny` —
    /// modeled explicitly, degrades to `Unknown` deliberately, not by
    /// falling through `TypeDescriptor::Other`'s generic catch-all.
    #[test]
    fn a_null_literal_type_descriptor_degrades_to_unknown_explicitly() {
        let abi: sting_tolkc::AbiJson = serde_json::from_str(
            r#"{
                "abi_schema_version": "1.0",
                "contract_name": "Synthetic",
                "unique_types": [{"kind": "void"}, {"kind": "nullLiteral"}],
                "declarations": [],
                "storage": {},
                "compiler_name": "tolk",
                "compiler_version": "1.4.1"
            }"#,
        )
        .expect("well-formed");
        let schema = abi_to_schema(&abi);
        assert_eq!(schema.types[1].shape, TypeShape::Unknown);
    }

    #[test]
    fn all_three_thrown_error_kinds_map_correctly() {
        let abi = synthetic_abi(
            r#", "thrown_errors": [
                {"kind": "plain_int", "err_code": 0},
                {"kind": "constant", "name": "ERR_A", "err_code": 1},
                {"kind": "enum_member", "name": "E.B", "err_code": 2}
            ]"#,
        );
        let schema = abi_to_schema(&abi);
        assert_eq!(
            schema.errors,
            vec![
                ErrorCode::Plain { code: 0 },
                ErrorCode::Named {
                    name: "ERR_A".to_string(),
                    code: 1
                },
                ErrorCode::Named {
                    name: "E.B".to_string(),
                    code: 2
                },
            ]
        );
    }

    #[test]
    fn extracted_schema_always_satisfies_its_own_invariant() {
        // Tier 1 output must always validate — confidence is always None,
        // extraction_tier is always Compiled, by construction in this
        // module, never left to chance per-call.
        let abi = synthetic_abi("");
        abi_to_schema(&abi)
            .validate()
            .expect("TolkExtractor output must always satisfy ContractSchema::validate");
    }
}
