//! `docs/TESTING.md`'s property-based round-trip
//! requirement: `deserialize(serialize(x)) == x` for arbitrary valid
//! `ContractSchema` values. Hand-composed strategies rather than
//! `proptest-derive`'s `#[derive(Arbitrary)]` — this is the only property
//! test in the crate so far, and every field here is either a flat
//! primitive or an index (`TypeRef`), never a recursive `TypeShape`, so
//! there is no recursion to fight; a handful of `prop_map`/`prop_oneof!`
//! calls composes the whole tree without a proc-macro dependency.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "docs/ARCHITECTURE.md §3.2 scopes the no-unwrap/expect/panic policy to library code outside tests"
)]

use proptest::prelude::*;
use sting_abi_core::{
    ContractMetadata, ContractSchema, ErrorCode, ExtractionConfidence, ExtractionTier,
    FieldConfidence, FieldRef, GetMethod, IR_SCHEMA_VERSION, MessageRef, NamedType, Param,
    SourceLanguage, TlbPrefix, TypeRef, TypeShape, UnionVariant,
};

fn arb_type_ref() -> impl Strategy<Value = TypeRef> {
    any::<u32>().prop_map(TypeRef)
}

fn arb_field_ref() -> impl Strategy<Value = FieldRef> {
    ("[a-z][a-z0-9_]{0,10}", arb_type_ref()).prop_map(|(name, ty)| FieldRef { name, ty })
}

fn arb_tlb_prefix() -> impl Strategy<Value = TlbPrefix> {
    (any::<u64>(), 0u32..=32).prop_map(|(prefix_num, prefix_len)| TlbPrefix {
        prefix_num,
        prefix_len,
    })
}

fn arb_union_variant() -> impl Strategy<Value = UnionVariant> {
    (arb_type_ref(), arb_tlb_prefix(), any::<bool>()).prop_map(
        |(ty, prefix, is_prefix_implicit)| UnionVariant {
            ty,
            prefix,
            is_prefix_implicit,
        },
    )
}

fn arb_type_shape() -> impl Strategy<Value = TypeShape> {
    prop_oneof![
        Just(TypeShape::Void),
        (proptest::option::of(1u16..=257u16), any::<bool>())
            .prop_map(|(bits, signed)| TypeShape::Int { bits, signed }),
        Just(TypeShape::Bool),
        Just(TypeShape::Coins),
        Just(TypeShape::Address),
        Just(TypeShape::Cell),
        Just(TypeShape::Slice),
        Just(TypeShape::Builder),
        (1u16..=1023u16).prop_map(|width| TypeShape::Bits { width }),
        Just(TypeShape::Remaining),
        arb_type_ref().prop_map(|inner| TypeShape::Nullable { inner }),
        arb_type_ref().prop_map(|inner| TypeShape::Array { inner }),
        arb_type_ref().prop_map(|inner| TypeShape::CellPacked { inner }),
        arb_type_ref().prop_map(|inner| TypeShape::LispList { inner }),
        arb_type_ref().prop_map(|target| TypeShape::Alias { target }),
        (arb_type_ref(), arb_type_ref()).prop_map(|(key, value)| TypeShape::Map { key, value }),
        proptest::collection::vec(arb_field_ref(), 0..4)
            .prop_map(|fields| TypeShape::Struct { fields }),
        proptest::collection::vec(arb_union_variant(), 0..4)
            .prop_map(|variants| TypeShape::Union { variants }),
        proptest::collection::vec(arb_type_ref(), 0..4)
            .prop_map(|items| TypeShape::Tensor { items }),
        (
            arb_type_ref(),
            proptest::collection::vec("[a-zA-Z][a-zA-Z0-9_]{0,15}", 0..4)
        )
            .prop_map(|(encoded_as, member_names)| TypeShape::Enum {
                encoded_as,
                member_names
            }),
        Just(TypeShape::Unknown),
    ]
}

fn arb_named_type() -> impl Strategy<Value = NamedType> {
    (
        proptest::option::of("[a-zA-Z][a-zA-Z0-9_]{0,15}"),
        proptest::option::of("[ -~]{0,30}"),
        proptest::option::of(arb_tlb_prefix()),
        arb_type_shape(),
    )
        .prop_map(|(name, description, prefix, shape)| NamedType {
            name,
            description,
            prefix,
            shape,
        })
}

fn arb_message_ref() -> impl Strategy<Value = MessageRef> {
    arb_type_ref().prop_map(|ty| MessageRef { ty })
}

fn arb_param() -> impl Strategy<Value = Param> {
    ("[a-z][a-z0-9_]{0,10}", arb_type_ref()).prop_map(|(name, ty)| Param { name, ty })
}

fn arb_get_method() -> impl Strategy<Value = GetMethod> {
    (
        proptest::option::of(any::<i64>()),
        "[a-z][a-z0-9_]{0,15}",
        proptest::collection::vec(arb_param(), 0..3),
        proptest::option::of(arb_type_ref()),
    )
        .prop_map(|(tvm_method_id, name, parameters, return_ty)| GetMethod {
            tvm_method_id,
            name,
            parameters,
            return_ty,
        })
}

fn arb_error_code() -> impl Strategy<Value = ErrorCode> {
    prop_oneof![
        any::<u32>().prop_map(|code| ErrorCode::Plain { code }),
        ("[A-Z_]{3,20}", any::<u32>()).prop_map(|(name, code)| ErrorCode::Named { name, code }),
    ]
}

fn arb_confidence() -> impl Strategy<Value = ExtractionConfidence> {
    (0.0f32..=1.0f32)
        .prop_map(|v| ExtractionConfidence::new(v).expect("in [0.0, 1.0] by construction"))
}

fn arb_field_confidence() -> impl Strategy<Value = FieldConfidence> {
    (
        proptest::option::of(arb_confidence()),
        proptest::option::of(arb_confidence()),
    )
        .prop_map(|(get_methods, storage)| FieldConfidence {
            get_methods,
            storage,
        })
}

fn arb_source_language() -> impl Strategy<Value = SourceLanguage> {
    prop_oneof![
        Just(SourceLanguage::Tolk),
        Just(SourceLanguage::Tact),
        Just(SourceLanguage::Func),
    ]
}

fn arb_extraction_tier() -> impl Strategy<Value = ExtractionTier> {
    prop_oneof![
        Just(ExtractionTier::Compiled),
        Just(ExtractionTier::Heuristic),
        Just(ExtractionTier::Statistical),
    ]
}

fn arb_metadata() -> impl Strategy<Value = ContractMetadata> {
    (
        proptest::option::of("[ -~]{0,20}"),
        proptest::option::of("[0-9]\\.[0-9]\\.[0-9]"),
        proptest::option::of("[ -~]{0,40}"),
    )
        .prop_map(|(author, version, description)| ContractMetadata {
            author,
            version,
            description,
        })
}

fn arb_contract_schema() -> impl Strategy<Value = ContractSchema> {
    let head = (
        arb_source_language(),
        arb_extraction_tier(),
        proptest::option::of("[A-Z][a-zA-Z0-9]{0,15}"),
        arb_metadata(),
        proptest::option::of(arb_type_ref()),
        proptest::collection::vec(arb_named_type(), 0..6),
    );
    let tail = (
        proptest::collection::vec(arb_message_ref(), 0..4),
        proptest::collection::vec(arb_message_ref(), 0..4),
        proptest::collection::vec(arb_message_ref(), 0..4),
        proptest::collection::vec(arb_get_method(), 0..4),
        proptest::collection::vec(arb_error_code(), 0..4),
        proptest::option::of(arb_confidence()),
        proptest::option::of(arb_field_confidence()),
    );
    (head, tail).prop_map(
        |(
            (source_language, extraction_tier, contract_name, metadata, storage, types),
            (
                incoming_internal,
                incoming_external,
                outgoing,
                get_methods,
                errors,
                confidence,
                field_confidence,
            ),
        )| ContractSchema {
            ir_schema_version: IR_SCHEMA_VERSION.to_string(),
            source_language,
            extraction_tier,
            contract_name,
            metadata,
            storage,
            types,
            incoming_internal,
            incoming_external,
            outgoing,
            get_methods,
            errors,
            confidence,
            field_confidence,
        },
    )
}

proptest! {
    #[test]
    fn contract_schema_round_trips_through_json(schema in arb_contract_schema()) {
        let json = serde_json::to_string(&schema).expect("a generated ContractSchema always serializes");
        let decoded: ContractSchema = serde_json::from_str(&json).expect("what we just serialized must deserialize");
        prop_assert_eq!(schema, decoded);
    }
}
