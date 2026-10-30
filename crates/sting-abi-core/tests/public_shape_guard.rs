//! Compile-time guard against an accidental breaking change to
//! [`ContractSchema`]'s public shape — the "CI gate on breaking change"
//! `docs/domains/abi-ir.md` calls for, but never
//! had . `cargo-semver-checks` was considered and deliberately not
//! wired up instead: it needs a baseline to diff against (a published
//! version, or a git revision), and `sting-abi-core` has `publish = false`
//! (no crates.io baseline) — a git-revision baseline was not ruled out for
//! later, but this crate's own toolchain doesn't have `cargo-semver-checks`
//! installed anywhere in this repo yet, so wiring it into CI blind (without
//! being able to verify it catches anything) would be a bigger and
//! less-verified change than this file.
//!
//! **What this actually catches, and how**: every enum below is matched
//! *exhaustively* — no `_ =>` wildcard arm — so adding, removing, or
//! renaming a variant fails to *compile*, not just fails a snapshot diff.
//! Every struct below is built as a full literal with every field named
//! explicitly — no `..Default::default()` — so adding, removing, or
//! renaming a field fails to compile the same way. Together this covers
//! every public type reachable from [`ContractSchema`].
//!
//! **What this does NOT catch** (honest limitation, not solved here): a
//! field's type narrowing/widening in a way that's still assignable from
//! the same literal (e.g. `bits: Option<u16>` becoming `Option<u32>` — a
//! bare integer literal like `Some(64)` still compiles against either
//! width). Detecting that class of change needs real type-level
//! reflection (`cargo-semver-checks`, or a `schemars`-style JSON Schema
//! diff) — neither is wired up here. This file is a cheap, always-on,
//! zero-new-dependency floor under that gap, not a replacement for it.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "docs/ARCHITECTURE.md §3.2 scopes the no-unwrap/expect policy to library code outside tests"
)]

use sting_abi_core::{
    ContractMetadata, ContractSchema, ErrorCode, ExtractionConfidence, ExtractionTier,
    FieldConfidence, FieldRef, GetMethod, IR_SCHEMA_VERSION, MessageRef, NamedType, Param,
    SourceLanguage, TlbPrefix, TypeRef, TypeShape, UnionVariant,
};

/// Exhaustively matches every [`TypeShape`] variant — adding, removing, or
/// renaming a variant is a compile error here, not a silent gap.
#[allow(
    clippy::match_same_arms,
    reason = "every arm is deliberately a no-op — the point of this function is that the \
              match itself is exhaustive over TypeShape's variants, not differentiated \
              per-variant behavior"
)]
fn exhaustively_match_type_shape(shape: &TypeShape) {
    match shape {
        TypeShape::Void
        | TypeShape::Bool
        | TypeShape::Coins
        | TypeShape::Address
        | TypeShape::Cell
        | TypeShape::Slice
        | TypeShape::Builder
        | TypeShape::Str
        | TypeShape::Remaining
        | TypeShape::Unknown => {}
        TypeShape::Int { bits: _, signed: _ } => {}
        TypeShape::Bits { width: _ } => {}
        TypeShape::Nullable { inner: _ }
        | TypeShape::Array { inner: _ }
        | TypeShape::CellPacked { inner: _ }
        | TypeShape::LispList { inner: _ } => {}
        TypeShape::Alias { target: _ } => {}
        TypeShape::Map { key: _, value: _ } => {}
        TypeShape::Struct { fields: _ } => {}
        TypeShape::Union { variants: _ } => {}
        TypeShape::Enum {
            encoded_as: _,
            member_names: _,
        } => {}
        TypeShape::Tensor { items: _ } => {}
    }
}

/// One instance of every [`TypeShape`] variant — kept in sync with
/// [`exhaustively_match_type_shape`] by construction (both must list every
/// variant, or this crate fails to compile).
fn all_type_shape_variants() -> Vec<TypeShape> {
    vec![
        TypeShape::Void,
        TypeShape::Int {
            bits: Some(64),
            signed: true,
        },
        TypeShape::Bool,
        TypeShape::Coins,
        TypeShape::Address,
        TypeShape::Cell,
        TypeShape::Slice,
        TypeShape::Builder,
        TypeShape::Str,
        TypeShape::Bits { width: 8 },
        TypeShape::Remaining,
        TypeShape::Nullable { inner: TypeRef(0) },
        TypeShape::Array { inner: TypeRef(0) },
        TypeShape::CellPacked { inner: TypeRef(0) },
        TypeShape::LispList { inner: TypeRef(0) },
        TypeShape::Alias { target: TypeRef(0) },
        TypeShape::Map {
            key: TypeRef(0),
            value: TypeRef(1),
        },
        TypeShape::Struct {
            fields: vec![FieldRef {
                name: "field_a".to_string(),
                ty: TypeRef(0),
            }],
        },
        TypeShape::Union {
            variants: vec![UnionVariant {
                ty: TypeRef(0),
                prefix: TlbPrefix {
                    prefix_num: 0x1234,
                    prefix_len: 32,
                },
                is_prefix_implicit: false,
            }],
        },
        TypeShape::Enum {
            encoded_as: TypeRef(0),
            member_names: vec!["A".to_string(), "B".to_string()],
        },
        TypeShape::Tensor {
            items: vec![TypeRef(0), TypeRef(1)],
        },
        TypeShape::Unknown,
    ]
}

/// Exhaustively matches every [`ErrorCode`] variant.
#[allow(
    clippy::match_same_arms,
    reason = "same reasoning as exhaustively_match_type_shape above"
)]
fn exhaustively_match_error_code(error: &ErrorCode) {
    match error {
        ErrorCode::Plain { code: _ } => {}
        ErrorCode::Named { name: _, code: _ } => {}
    }
}

/// Exhaustively matches every [`SourceLanguage`] variant.
fn exhaustively_match_source_language(language: SourceLanguage) {
    match language {
        SourceLanguage::Tolk | SourceLanguage::Tact | SourceLanguage::Func => {}
    }
}

/// Exhaustively matches every [`ExtractionTier`] variant.
fn exhaustively_match_extraction_tier(tier: ExtractionTier) {
    match tier {
        ExtractionTier::Compiled | ExtractionTier::Heuristic | ExtractionTier::Statistical => {}
    }
}

/// A fully-populated [`ContractSchema`] — every field of every reachable
/// struct is named explicitly (no `..`), so a field being added, removed,
/// or renamed anywhere in this graph fails to compile this file.
fn fully_populated_contract_schema() -> ContractSchema {
    let named_type = NamedType {
        name: Some("Storage".to_string()),
        description: Some("doc comment".to_string()),
        prefix: Some(TlbPrefix {
            prefix_num: 0xABCD,
            prefix_len: 32,
        }),
        shape: TypeShape::Struct {
            fields: vec![FieldRef {
                name: "owner".to_string(),
                ty: TypeRef(1),
            }],
        },
    };

    ContractSchema {
        ir_schema_version: IR_SCHEMA_VERSION.to_string(),
        source_language: SourceLanguage::Func,
        extraction_tier: ExtractionTier::Heuristic,
        contract_name: Some("Example".to_string()),
        metadata: ContractMetadata {
            author: Some("author".to_string()),
            version: Some("1.0".to_string()),
            description: Some("description".to_string()),
        },
        storage: Some(TypeRef(0)),
        types: vec![named_type],
        incoming_internal: vec![MessageRef { ty: TypeRef(0) }],
        incoming_external: vec![MessageRef { ty: TypeRef(0) }],
        outgoing: vec![MessageRef { ty: TypeRef(0) }],
        get_methods: vec![GetMethod {
            tvm_method_id: Some(-262_144),
            name: "get_owner".to_string(),
            parameters: vec![Param {
                name: "arg".to_string(),
                ty: TypeRef(0),
            }],
            return_ty: Some(TypeRef(0)),
        }],
        errors: vec![
            ErrorCode::Plain { code: 100 },
            ErrorCode::Named {
                name: "ERROR_BAD_SEQNO".to_string(),
                code: 33,
            },
        ],
        confidence: Some(ExtractionConfidence::new(0.8).expect("0.8 is in range")),
        field_confidence: Some(FieldConfidence {
            get_methods: Some(ExtractionConfidence::new(0.9).expect("0.9 is in range")),
            storage: Some(ExtractionConfidence::new(0.6).expect("0.6 is in range")),
        }),
    }
}

#[test]
fn every_type_shape_variant_is_covered_by_the_exhaustive_match() {
    for shape in all_type_shape_variants() {
        exhaustively_match_type_shape(&shape);
    }
}

#[test]
fn every_error_code_variant_is_covered_by_the_exhaustive_match() {
    exhaustively_match_error_code(&ErrorCode::Plain { code: 1 });
    exhaustively_match_error_code(&ErrorCode::Named {
        name: "X".to_string(),
        code: 2,
    });
}

#[test]
fn the_fully_populated_fixture_is_internally_valid_and_round_trips() {
    let schema = fully_populated_contract_schema();
    exhaustively_match_source_language(schema.source_language);
    exhaustively_match_extraction_tier(schema.extraction_tier);
    schema
        .validate()
        .expect("fixture must satisfy its own confidence/tier invariant");

    let json = serde_json::to_string(&schema).expect("serializable");
    let round_tripped: ContractSchema = serde_json::from_str(&json).expect("deserializable");
    assert_eq!(schema, round_tripped);
}
