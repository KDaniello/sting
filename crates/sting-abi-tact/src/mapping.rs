//! `ContractAbi` → `sting_abi_core::ContractSchema`. Substantively
//! different from `sting-abi-tolk`'s mapping, not just a renamed copy — see
//! `docs/domains/extractors.md`, which calls
//! this out explicitly: Tact's ABI has no index table at all (`AbiTypeRef`
//! embeds a full type description *inline* everywhere it's used, by name
//! for anything non-primitive), while `ContractSchema::types` is a
//! deduplicated index table by design (`docs/domains/abi-ir.md`
//! §2). Building that table is this module's real job, not a mechanical
//! field rename.

use std::collections::HashMap;

use sting_abi_core::{
    ContractMetadata, ContractSchema, ErrorCode, ExtractionTier, FieldRef, GetMethod, MessageRef,
    NamedType, Param, SourceLanguage, TlbPrefix, TypeRef, TypeShape,
};

use crate::abi_json::{AbiField, AbiReceiverMessage, AbiTypeRef, ContractAbi};

/// Converts a full `tact` `ContractAbi` artifact into a normalized
/// `ContractSchema`.
///
/// # What this does not extract in v1
/// - **`interfaces[]`** (TEP-like introspection tags) — no Tolk/`FunC`
///   equivalent exists to normalize it against; extending the shared
///   `ContractMetadata` for one language's concept alone was judged worse
///   than an honest gap (`docs/domains/extractors.md`
///   §1.2 leaves this decision to implementation time — this is that
///   decision, recorded here, not silently made).
/// - **`"text"`/other non-`"typed"` receivers** (`receive("Hello") { ... }`
///   style) — `sting_abi_core::MessageRef` only carries a `TypeRef`, and a
///   text-comment receiver has no associated struct type to point at;
///   synthesizing a fake one was judged worse than dropping it with this
///   documented gap. Real effect: `MessageTester` (`tests/fixtures/
///   message.tact`) extracts with only its one typed (`Deploy`) receiver
///   in `incoming_internal`, not its four text receivers.
/// - **`outgoing`** (`ContractSchema::outgoing` — messages the contract is
///   declared to send) — always empty, unconditionally (see `abi_to_schema`
///   below). Tact's `ContractAbi` artifact carries no equivalent of this at
///   all (no field in `crate::abi_json::ContractAbi`), unlike Tolk's own
///   compiler output, which does report outgoing message types and is
///   scored by `sting-abi-bench`'s `score_tolk_messages`. This asymmetry
///   was previously undocumented here, despite being flagged for `FunC`'s
///   equivalent gap (`sting-abi-func/src/lib.rs`'s own "What v1 does NOT
///   extract" list) — recorded explicitly now for the same reason.
#[must_use]
pub fn abi_to_schema(abi: &ContractAbi) -> ContractSchema {
    let mut interner = TypeInterner::new();
    interner.seed_named_types(abi);

    let get_methods = abi
        .getters
        .iter()
        .map(|g| convert_getter(g, &mut interner))
        .collect();
    let (incoming_internal, incoming_external) = convert_receivers(abi, &mut interner);
    let errors = convert_errors(&abi.errors);

    ContractSchema {
        ir_schema_version: sting_abi_core::IR_SCHEMA_VERSION.to_string(),
        source_language: SourceLanguage::Tact,
        extraction_tier: ExtractionTier::Compiled,
        contract_name: Some(abi.name.clone()),
        metadata: ContractMetadata::default(),
        // Tact's ABI carries no separate "persistent storage type" concept
        // the way Tolk's `abi.json` does — the closest real analog,
        // `<ContractName>$Data` (confirmed in every fixture, e.g. `Test$Data`
        // in tests/fixtures/getters.tact_Test.abi), is itself just one more
        // entry in `types[]`, resolved here by name like anything else.
        storage: interner.lookup_named(&format!("{}$Data", abi.name)),
        types: interner.into_types(),
        incoming_internal,
        incoming_external,
        outgoing: Vec::new(),
        get_methods,
        errors,
        confidence: None,
        field_confidence: None,
    }
}

fn convert_getter(getter: &crate::abi_json::AbiGetter, interner: &mut TypeInterner) -> GetMethod {
    GetMethod {
        tvm_method_id: Some(getter.method_id),
        name: getter.name.clone(),
        parameters: getter
            .arguments
            .iter()
            .map(|a| convert_field(a, interner))
            .collect(),
        return_ty: getter.return_type.as_ref().map(|rt| interner.intern(rt)),
    }
}

fn convert_field(field: &AbiField, interner: &mut TypeInterner) -> Param {
    Param {
        name: field.name.clone(),
        ty: interner.intern(&field.ty),
    }
}

fn convert_receivers(
    abi: &ContractAbi,
    interner: &mut TypeInterner,
) -> (Vec<MessageRef>, Vec<MessageRef>) {
    let mut incoming_internal = Vec::new();
    let mut incoming_external = Vec::new();

    for receiver in &abi.receivers {
        let AbiReceiverMessage::Typed { type_name } = &receiver.message else {
            continue;
        };
        let Some(ty) = interner.lookup_named(type_name) else {
            continue;
        };
        match receiver.receiver.as_str() {
            "internal" => incoming_internal.push(MessageRef { ty }),
            "external" => incoming_external.push(MessageRef { ty }),
            // Only "internal" was ever observed across the three real
            // fixtures this module was built against — any other value
            // is carried past silently here rather than guessed at
            // further (dropped from both lists, same principle as the
            // text-receiver gap this module's own doc comment records).
            _ => {}
        }
    }

    (incoming_internal, incoming_external)
}

/// `ContractAbi::errors` is a `HashMap` — real Tact output, and a real
/// bug caught by this crate's own golden test before it shipped:
/// `HashMap` iteration order is randomized per-process (Rust's `DoS`-
/// resistant default hasher), so `.iter().collect()` produced a
/// *different* `Vec<ErrorCode>` ordering on every single run — the exact
/// same input contract would extract to a structurally-equal-but-
/// unequal-as-a-`Vec` `ContractSchema` from one process to the next,
/// which a naive `assert_eq!` against a fixed snapshot fails on almost
/// every run (not flaky-occasionally — *every* run except by chance).
/// Sorted by numeric code here so the output is actually deterministic,
/// not just "deterministic modulo something nobody promised."
fn convert_errors(errors: &HashMap<String, crate::abi_json::AbiError>) -> Vec<ErrorCode> {
    let mut codes: Vec<(u32, &crate::abi_json::AbiError)> = errors
        .iter()
        .filter_map(|(code_str, error)| code_str.parse::<u32>().ok().map(|code| (code, error)))
        .collect();
    codes.sort_by_key(|(code, _)| *code);
    codes
        .into_iter()
        .map(|(code, error)| ErrorCode::Named {
            name: error.message.clone(),
            code,
        })
        .collect()
}

/// A cheap, directly-`Hash`able stand-in for `format: Option<&serde_json::Value>`
/// — `serde_json::Value` itself implements neither `Hash` nor a cheap `Eq`
/// (its `Number` variant can hold an `f64`), which is why an earlier
/// version of this module fell back to `format!("{format:?}")` (`Debug`-
/// formatting the whole value into a throwaway `String`) just to get a
/// hashable cache key. `primitive_shape` (the only reader of this field)
/// only ever inspects it via `as_u64`/`as_str`, so those two cases get a
/// direct, allocation-free representation; anything else (never observed
/// in practice — no real Tact ABI output puts a bool/array/object/null
/// here) still round-trips through a `String` via `Value`'s own
/// `Display`, so two different "unexpected" shapes are still never
/// silently conflated, matching the old key's behavior for that case.
#[derive(Clone, PartialEq, Eq, Hash)]
enum FormatKey {
    Absent,
    Number(u64),
    Str(String),
    Other(String),
}

impl FormatKey {
    fn new(format: Option<&serde_json::Value>) -> Self {
        match format {
            None => FormatKey::Absent,
            Some(value) => value
                .as_u64()
                .map(FormatKey::Number)
                .or_else(|| value.as_str().map(|s| FormatKey::Str(s.to_string())))
                .unwrap_or_else(|| FormatKey::Other(value.to_string())),
        }
    }
}

/// Dedup key for [`TypeInterner::intern`]'s own top-level cache — one
/// variant per [`AbiTypeRef`] shape, structurally mirroring it but with
/// [`FormatKey`] standing in for the un-hashable `format` fields. Replaces
/// an earlier `format!("simple|{type_name}|{optional}|{format:?}")`-style
/// `String` key: same dedup granularity (still exactly one table entry per
/// distinct `(type_name, optional, format)`/`(key, key_format, value,
/// value_format)` combination), no `Debug`-formatting or string
/// concatenation to build it.
#[derive(Clone, PartialEq, Eq, Hash)]
enum InternKey {
    Simple {
        type_name: String,
        optional: bool,
        format: FormatKey,
    },
    Dict {
        key: String,
        key_format: FormatKey,
        value: String,
        value_format: FormatKey,
    },
    Other,
}

fn intern_key_for(type_ref: &AbiTypeRef) -> InternKey {
    match type_ref {
        AbiTypeRef::Simple {
            type_name,
            optional,
            format,
        } => InternKey::Simple {
            type_name: type_name.clone(),
            optional: *optional,
            format: FormatKey::new(format.as_ref()),
        },
        AbiTypeRef::Dict {
            key,
            key_format,
            value,
            value_format,
        } => InternKey::Dict {
            key: key.clone(),
            key_format: FormatKey::new(key_format.as_ref()),
            value: value.clone(),
            value_format: FormatKey::new(value_format.as_ref()),
        },
        AbiTypeRef::Other => InternKey::Other,
    }
}

/// Dedup key for [`TypeInterner::resolve_named_or_primitive`]'s own cache —
/// same reasoning as [`InternKey`], replacing a
/// `format!("primitive|{name}|{format:?}")`-style `String` key. A
/// genuinely separate `HashMap` from [`TypeInterner::anonymous_cache`], not
/// a shared one disambiguated by a string prefix (`"simple|"` vs
/// `"primitive|"`) the way the old single `String`-keyed cache worked —
/// the two caches were already non-overlapping namespaces in practice (a
/// `"primitive|..."` key can never collide with a `"simple|..."`/`"dict|..."`/
/// `"other"` one), so splitting them by type is a pure simplification, not
/// a behavior change.
#[derive(Clone, PartialEq, Eq, Hash)]
struct PrimitiveKey {
    name: String,
    format: FormatKey,
}

/// Builds `ContractSchema::types` incrementally as `AbiTypeRef`s are
/// encountered, since Tact's own ABI has no index table to walk
/// positionally the way `sting-abi-tolk`'s mapping does.
struct TypeInterner {
    types: Vec<NamedType>,
    name_to_index: HashMap<String, u32>,
    /// Dedup cache for [`TypeInterner::intern`]'s own top-level result
    /// (primitives, `Dict`s, and the `Nullable` wrapper around any
    /// `optional` reference) — so the same shape encountered twice (e.g.
    /// two `int257` fields) reuses one table entry instead of allocating a
    /// duplicate.
    anonymous_cache: HashMap<InternKey, u32>,
    /// Dedup cache for [`TypeInterner::resolve_named_or_primitive`]'s own
    /// primitive-only resolution — see [`PrimitiveKey`]'s own doc comment
    /// for why this is a second, separate map rather than reusing
    /// `anonymous_cache`.
    primitive_cache: HashMap<PrimitiveKey, u32>,
}

impl TypeInterner {
    fn new() -> Self {
        Self {
            types: Vec::new(),
            name_to_index: HashMap::new(),
            anonymous_cache: HashMap::new(),
            primitive_cache: HashMap::new(),
        }
    }

    /// First pass: every `ContractAbi::types[]` entry gets a table slot at
    /// a stable index, named-lookup-ready, *before* any getter/field is
    /// converted — mirrors `sting-abi-tolk`'s two-pass approach (index
    /// declarations, then resolve references against them) for the same
    /// reason: a field can reference a type declared later in the source.
    fn seed_named_types(&mut self, abi: &ContractAbi) {
        for abi_type in &abi.types {
            let idx = self.push(NamedType {
                name: Some(abi_type.name.clone()),
                // Tact's ABI carries no per-type doc-comment field at all
                // (confirmed absent on every real `AbiType` observed,
                // unlike Tolk's `abi.json` which sometimes has one).
                description: None,
                prefix: abi_type.header.map(|h| TlbPrefix {
                    prefix_num: u64::try_from(h).unwrap_or_default(),
                    prefix_len: 32,
                }),
                shape: TypeShape::Unknown, // filled in below, once every name is known
            });
            self.name_to_index.insert(abi_type.name.clone(), idx.0);
        }
        // Second sub-pass: now that every name resolves, fill in each
        // struct's real fields (a field can reference a type declared
        // later in abi.types, e.g. StateInit before DeployParameters).
        for (i, abi_type) in abi.types.iter().enumerate() {
            let fields = abi_type
                .fields
                .iter()
                .map(|f| FieldRef {
                    name: f.name.clone(),
                    ty: self.intern(&f.ty),
                })
                .collect();
            self.types[i].shape = TypeShape::Struct { fields };
        }
    }

    fn push(&mut self, named_type: NamedType) -> TypeRef {
        let idx = u32::try_from(self.types.len()).unwrap_or(u32::MAX);
        self.types.push(named_type);
        TypeRef(idx)
    }

    fn lookup_named(&self, name: &str) -> Option<TypeRef> {
        self.name_to_index.get(name).copied().map(TypeRef)
    }

    /// Resolves any `AbiTypeRef` — `Simple` (named, already seeded, or
    /// primitive, interned on demand and deduplicated, wrapped in
    /// `Nullable` if `optional`), `Dict` (interned as a `TypeShape::Map`
    /// over its own interned key/value), or `Other` (degrades to
    /// `Unknown`, never fails).
    fn intern(&mut self, type_ref: &AbiTypeRef) -> TypeRef {
        let cache_key = intern_key_for(type_ref);
        if let Some(&idx) = self.anonymous_cache.get(&cache_key) {
            return TypeRef(idx);
        }

        let resolved = match type_ref {
            AbiTypeRef::Simple {
                type_name,
                optional,
                format,
            } => {
                let core = self.resolve_named_or_primitive(type_name, format.as_ref());
                if *optional {
                    self.push(NamedType {
                        name: None,
                        description: None,
                        prefix: None,
                        shape: TypeShape::Nullable { inner: core },
                    })
                } else {
                    core
                }
            }
            AbiTypeRef::Dict {
                key,
                key_format,
                value,
                value_format,
            } => {
                let key_ref = self.resolve_named_or_primitive(key, key_format.as_ref());
                let value_ref = self.resolve_named_or_primitive(value, value_format.as_ref());
                self.push(NamedType {
                    name: None,
                    description: None,
                    prefix: None,
                    shape: TypeShape::Map {
                        key: key_ref,
                        value: value_ref,
                    },
                })
            }
            AbiTypeRef::Other => self.push(NamedType {
                name: None,
                description: None,
                prefix: None,
                shape: TypeShape::Unknown,
            }),
        };

        self.anonymous_cache.insert(cache_key, resolved.0);
        resolved
    }

    /// Shared by `Simple`'s `type_name` and `Dict`'s `key`/`value`: both
    /// are either the bare name of an already-seeded declared type, or a
    /// primitive keyword interned on demand and deduplicated against
    /// `anonymous_cache` — real bug found by review: an earlier version
    /// pushed a fresh table entry on every call, so e.g. `map<Int, X>`
    /// appearing in six different getters (confirmed against this crate's
    /// own `tests/fixtures/golden/map_comparison.expected.json`, before
    /// this fix) allocated six structurally-identical "plain `Int`" nodes
    /// instead of reusing one — silently contradicting `anonymous_cache`'s
    /// own doc comment two lines below.
    fn resolve_named_or_primitive(
        &mut self,
        name: &str,
        format: Option<&serde_json::Value>,
    ) -> TypeRef {
        if let Some(named) = self.lookup_named(name) {
            return named;
        }
        let cache_key = PrimitiveKey {
            name: name.to_string(),
            format: FormatKey::new(format),
        };
        if let Some(&idx) = self.primitive_cache.get(&cache_key) {
            return TypeRef(idx);
        }
        let shape = primitive_shape(name, format);
        let resolved = self.push(NamedType {
            name: None,
            description: None,
            prefix: None,
            shape,
        });
        self.primitive_cache.insert(cache_key, resolved.0);
        resolved
    }

    fn into_types(self) -> Vec<NamedType> {
        self.types
    }
}

fn primitive_shape(type_name: &str, format: Option<&serde_json::Value>) -> TypeShape {
    let format_bits = format.and_then(serde_json::Value::as_u64);
    let format_name = format.and_then(serde_json::Value::as_str);
    let format_is_remainder = format_name == Some("remainder");

    match type_name {
        // An amount is an integer whose format says how it is written, and the format is the whole
        // of the difference: `Int as coins` is four bits of length and then that many bytes, not a
        // fixed-width integer, and a reader given the latter reads somebody else's bits. The
        // compiler's own type table (`resolveABITypeRef.js`) spells `coins` as
        // `{type:"uint",format:"coins"}` and `varuint16` as the same encoding under its TL-B name.
        //
        // The other three variable-width formats — `varint16`, `varint32`, `varuint32` — have no
        // counterpart in this IR, so they stay an integer of no declared width rather than being
        // called something they are not.
        "int" | "uint" if matches!(format_name, Some("coins" | "varuint16")) => TypeShape::Coins,
        "int" => TypeShape::Int {
            bits: format_bits.map(|n| u16::try_from(n).unwrap_or(u16::MAX)),
            signed: true,
        },
        "uint" => TypeShape::Int {
            bits: format_bits.map(|n| u16::try_from(n).unwrap_or(u16::MAX)),
            signed: false,
        },
        "bool" => TypeShape::Bool,
        // Not directly observed in the three real fixtures (none of them
        // use Tact's `Coins` alias type) — included anyway because it is
        // a documented, unambiguous OTP-002 primitive name with an exact
        // existing `TypeShape` counterpart, unlike the genuinely-unclear
        // cases this function falls back to `Unknown` for below.
        "coins" => TypeShape::Coins,
        "address" => TypeShape::Address,
        "cell" => TypeShape::Cell,
        // Confirmed real and common on real output (a corpus run over
        // `corpus-abi/clean/tact/local-type-inference.tact`'s `String`-
        // returning getters) — the "genuinely unclear without a real
        // example" gap this function's own doc comment used to flag for
        // `"string"` specifically is now resolved with real evidence, not a
        // guess.
        "string" => TypeShape::Str,
        "slice" if format_is_remainder => TypeShape::Remaining,
        "slice" => TypeShape::Slice,
        "builder" => TypeShape::Builder,
        // A byte count, not a bit count — converted to bits (×8, lossless
        // unit conversion, not a semantic reinterpretation) to reuse
        // `TypeShape::Bits` rather than adding a byte-width variant for
        // what both languages ultimately store as a bitstring.
        "fixed-bytes" => TypeShape::Bits {
            width: format_bits.map_or(0, |n| {
                u16::try_from(n.saturating_mul(8)).unwrap_or(u16::MAX)
            }),
        },
        // Any other primitive keyword the OTP-002 spec documents —
        // genuinely unclear without a real example, not guessed at, see
        // this module's own top-level doc comment. `"string"` used to be
        // the flagship example here; it now has its own match arm above,
        // confirmed against real output.
        _ => TypeShape::Unknown,
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
    use crate::abi_json::ContractAbi;

    /// Minimal, hand-written (not real-compiler-produced) `ContractAbi`,
    /// valid per every field `ContractAbi` actually requires — fast,
    /// synthetic coverage per `docs/TESTING.md`,
    /// complementary to `tests/golden.rs`'s real-compiler fixtures.
    fn synthetic_abi(json: &str) -> ContractAbi {
        serde_json::from_str(json).expect("synthetic ContractAbi must be well-formed")
    }

    #[test]
    fn a_dict_type_reference_resolves_to_a_map_shape() {
        let abi = synthetic_abi(
            r#"{
                "name": "Synthetic",
                "types": [],
                "receivers": [],
                "getters": [{
                    "name": "getMap",
                    "methodId": 100,
                    "arguments": [{"name": "m", "type": {"kind": "dict", "key": "uint", "keyFormat": 8, "value": "bool"}}],
                    "returnType": {"kind": "simple", "type": "bool", "optional": false}
                }],
                "errors": {}
            }"#,
        );
        let schema = abi_to_schema(&abi);
        let param_ty = schema.get_methods[0].parameters[0].ty;
        match &schema.types[param_ty.0 as usize].shape {
            TypeShape::Map { key, value } => {
                assert_eq!(
                    schema.types[key.0 as usize].shape,
                    TypeShape::Int {
                        bits: Some(8),
                        signed: false
                    }
                );
                assert_eq!(schema.types[value.0 as usize].shape, TypeShape::Bool);
            }
            other => panic!("expected Map, got {other:?}"),
        }
    }

    /// Regression test for a real gap found by a corpus run over
    /// (`corpus-abi/clean/tact/local-type-inference.tact`'s `String`-
    /// returning getters): Tact's `String` primitive used to have no
    /// matching `TypeShape` arm and silently degraded to `Unknown`.
    #[test]
    fn a_string_primitive_resolves_to_str() {
        let abi = synthetic_abi(
            r#"{
                "name": "Synthetic",
                "types": [],
                "receivers": [],
                "getters": [{
                    "name": "getName",
                    "methodId": 1,
                    "arguments": [],
                    "returnType": {"kind": "simple", "type": "string", "optional": false}
                }],
                "errors": {}
            }"#,
        );
        let schema = abi_to_schema(&abi);
        let return_ty = schema.get_methods[0]
            .return_ty
            .expect("return type must exist");
        assert_eq!(schema.types[return_ty.0 as usize].shape, TypeShape::Str);
    }

    /// Regression test for a real gap the decode dataset found: every jetton amount extracted from
    /// a Tact contract was an integer of no declared width, because `Int as coins` reaches this
    /// function as an `int` whose format is the string `"coins"` rather than a bit count, and only
    /// bit counts were read. The compiler's own table
    /// (`vendor/tact/.../types/resolveABITypeRef.js`) spells it that way and spells `varuint16` as
    /// the same encoding.
    #[test]
    fn an_amount_is_coins_rather_than_an_integer_of_no_width() {
        for format in [r#""coins""#, r#""varuint16""#] {
            let abi = synthetic_abi(&format!(
                r#"{{
                "name": "Synthetic",
                "types": [{{"name": "S", "header": null, "fields": [
                    {{"name": "amount", "type": {{"kind": "simple", "type": "uint", "optional": false, "format": {format}}}}}
                ]}}],
                "receivers": [],
                "getters": [],
                "errors": {{}}
            }}"#
            ));
            let schema = abi_to_schema(&abi);
            let TypeShape::Struct { fields } = &schema.types[0].shape else {
                panic!("the declared type is a struct");
            };
            assert_eq!(
                schema.types[fields[0].ty.0 as usize].shape,
                TypeShape::Coins
            );
        }
    }

    /// The three variable-width formats this IR has no counterpart for stay what they are: an
    /// integer whose width nobody stated, rather than a fixed width nobody wrote.
    #[test]
    fn the_other_variable_width_formats_stay_of_no_declared_width() {
        for format in [r#""varint16""#, r#""varint32""#, r#""varuint32""#] {
            let abi = synthetic_abi(&format!(
                r#"{{
                "name": "Synthetic",
                "types": [{{"name": "S", "header": null, "fields": [
                    {{"name": "x", "type": {{"kind": "simple", "type": "int", "optional": false, "format": {format}}}}}
                ]}}],
                "receivers": [],
                "getters": [],
                "errors": {{}}
            }}"#
            ));
            let schema = abi_to_schema(&abi);
            let TypeShape::Struct { fields } = &schema.types[0].shape else {
                panic!("the declared type is a struct");
            };
            assert_eq!(
                schema.types[fields[0].ty.0 as usize].shape,
                TypeShape::Int {
                    bits: None,
                    signed: true
                }
            );
        }
    }

    #[test]
    fn an_optional_field_wraps_in_nullable() {
        let abi = synthetic_abi(
            r#"{
                "name": "Synthetic",
                "types": [{"name": "S", "header": null, "fields": [
                    {"name": "x", "type": {"kind": "simple", "type": "int", "optional": true, "format": 257}}
                ]}],
                "receivers": [],
                "getters": [],
                "errors": {}
            }"#,
        );
        let schema = abi_to_schema(&abi);
        let s = schema
            .types
            .iter()
            .find(|t| t.name.as_deref() == Some("S"))
            .expect("S must exist");
        let TypeShape::Struct { fields } = &s.shape else {
            panic!("expected Struct")
        };
        match &schema.types[fields[0].ty.0 as usize].shape {
            TypeShape::Nullable { inner } => {
                assert_eq!(
                    schema.types[inner.0 as usize].shape,
                    TypeShape::Int {
                        bits: Some(257),
                        signed: true
                    }
                );
            }
            other => panic!("expected Nullable, got {other:?}"),
        }
    }

    #[test]
    fn text_receivers_are_dropped_typed_receivers_are_kept() {
        let abi = synthetic_abi(
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
        let schema = abi_to_schema(&abi);
        assert_eq!(
            schema.incoming_internal.len(),
            1,
            "the text receiver must not produce a MessageRef"
        );
        assert_eq!(
            schema.types[schema.incoming_internal[0].ty.0 as usize]
                .name
                .as_deref(),
            Some("Ping")
        );
    }

    #[test]
    fn errors_are_sorted_by_code_regardless_of_hashmap_iteration_order() {
        // Real regression: HashMap iteration order is randomized per
        // process, so this must be sorted deterministically by
        // convert_errors, not just "whatever order iter() happened to
        // produce this run" — see that function's own doc comment.
        let abi = synthetic_abi(
            r#"{
                "name": "Synthetic",
                "types": [],
                "receivers": [],
                "getters": [],
                "errors": {
                    "50": {"message": "e"},
                    "2": {"message": "a"},
                    "33": {"message": "c"},
                    "9": {"message": "b"}
                }
            }"#,
        );
        let schema = abi_to_schema(&abi);
        assert_eq!(
            schema.errors,
            vec![
                ErrorCode::Named {
                    name: "a".to_string(),
                    code: 2
                },
                ErrorCode::Named {
                    name: "b".to_string(),
                    code: 9
                },
                ErrorCode::Named {
                    name: "c".to_string(),
                    code: 33
                },
                ErrorCode::Named {
                    name: "e".to_string(),
                    code: 50
                },
            ]
        );
    }

    #[test]
    fn storage_resolves_via_the_dollar_data_naming_convention() {
        let abi = synthetic_abi(
            r#"{
                "name": "MyContract",
                "types": [{"name": "MyContract$Data", "header": null, "fields": []}],
                "receivers": [],
                "getters": [],
                "errors": {}
            }"#,
        );
        let schema = abi_to_schema(&abi);
        assert_eq!(schema.storage, Some(TypeRef(0)));
    }

    #[test]
    fn extracted_schema_always_satisfies_its_own_invariant() {
        let abi = synthetic_abi(
            r#"{"name": "S", "types": [], "receivers": [], "getters": [], "errors": {}}"#,
        );
        abi_to_schema(&abi)
            .validate()
            .expect("TactExtractor output must always satisfy ContractSchema::validate");
    }
}
