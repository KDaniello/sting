//! Typed representation of `tact`'s `<file>_<Contract>.abi` artifact.
//! Confirmed against real compiler output (`@tact-lang/compiler` v1.6.13,
//! `vendor/tact/`) on three real contracts, not against the OTP-002 spec
//! text alone — see `tests/fixtures/SOURCE.md` for
//! the exact provenance. Deliberately does not model every field the OTP-002
//! spec describes, only what was actually observed, plus honest fallbacks
//! (`AbiReceiverMessage::Other`, `AbiTypeShape::Other`) for shapes this
//! module doesn't recognize yet — same "start narrow" discipline
//! `sting_tolkc::symbol_types`'s own doc comment already establishes for
//! the sibling Tolk artifact.

use std::collections::HashMap;

use serde::Deserialize;

/// The compiler's ABI description for one contract (one `.abi` file).
#[derive(Debug, Clone, Deserialize)]
pub struct ContractAbi {
    /// The `contract Name { ... }` block's name.
    pub name: String,
    /// Every struct/message type this contract references — both
    /// user-declared and compiler-injected stdlib types (`StateInit`,
    /// `Context`, `SendParameters`, ...).
    pub types: Vec<AbiType>,
    /// This contract's message receivers.
    pub receivers: Vec<AbiReceiver>,
    /// This contract's `get`-methods.
    pub getters: Vec<AbiGetter>,
    /// Every reachable exit code, keyed by its numeric value as a decimal
    /// string (confirmed real shape: JSON object, not array — Tolk's
    /// `thrown_errors` is an array, Tact's is not, not a typo).
    pub errors: HashMap<String, AbiError>,
    /// TEP-like introspection interface tags (e.g.
    /// `"org.ton.introspection.v0"`) — always non-empty on every real
    /// contract observed, no Tolk equivalent. **Not extracted into
    /// `ContractSchema` in v1** — see `crate`'s own doc comment for why.
    #[serde(default)]
    pub interfaces: Vec<String>,
}

/// One type this contract's ABI references.
#[derive(Debug, Clone, Deserialize)]
pub struct AbiType {
    /// Declared name.
    pub name: String,
    /// The TL-B 32-bit opcode prefix, for a message-shaped type — `None`
    /// for a plain struct with no opcode (storage types, stdlib helper
    /// types like `StateInit`).
    pub header: Option<i64>,
    /// Fields in declaration order.
    #[serde(default)]
    pub fields: Vec<AbiField>,
}

/// One field of an [`AbiType`], or one argument of an [`AbiGetter`] (same
/// real JSON shape for both — Tact's own `ABIField`/`ABIArgument` are
/// structurally identical, confirmed against real output).
#[derive(Debug, Clone, Deserialize)]
pub struct AbiField {
    /// Field/argument name as declared.
    pub name: String,
    /// This field's type.
    #[serde(rename = "type")]
    pub ty: AbiTypeRef,
}

/// A type reference as Tact's ABI expresses it — always by name for a
/// non-primitive (Tact has no index table the way Tolk's `abi.json` does;
/// a struct field pointing at another declared type repeats that type's
/// bare name as a string).
///
/// **Two structurally incompatible real shapes, not one** — found by
/// compiling a fourth/fifth real contract (`map_comparison.tact`,
/// `nested_struct.tact`) beyond the original three: a `map<K, V>`
/// field/argument serializes as `{"kind":"dict","key":"uint",
/// "keyFormat":8,"value":"bool"}` — no `"type"`/`"optional"`/`"format"`
/// keys at all. An earlier version of this type had `type`/`optional`/
/// `format` as directly-required fields on one flat struct; that would
/// have hard-failed deserializing *any* contract using a map anywhere in a
/// getter argument or message field — common enough in real Tact that this
/// was worth fixing before the first release, not after.
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AbiTypeRef {
    /// A non-map type — either a primitive keyword (`"int"`, `"uint"`,
    /// `"bool"`, `"cell"`, `"slice"`, `"builder"`, `"address"`,
    /// `"fixed-bytes"`, ...) or the bare name of another declared type in
    /// the same `ContractAbi::types`.
    Simple {
        /// The primitive keyword or referenced type name — see this
        /// variant's own doc comment.
        #[serde(rename = "type")]
        type_name: String,
        /// Whether this is a `T?` optional.
        #[serde(default)]
        optional: bool,
        /// Bit width (for `int`/`uint`), or `"remainder"` for an
        /// unbounded-trailing-slice field — polymorphic (number or
        /// string) in real output, so kept as a raw [`serde_json::Value`]
        /// rather than one fixed Rust type.
        #[serde(default)]
        format: Option<serde_json::Value>,
    },
    /// A `map<K, V>`.
    Dict {
        /// The key type's primitive keyword (e.g. `"int"`, `"uint"`,
        /// `"address"` — every real key type observed is a primitive, not
        /// a struct name, matching TL-B's own map-key restriction).
        key: String,
        /// The key's bit-width modifier (Tact's `map<Int as uint8, V>`
        /// syntax), if narrower than the key type's own default width.
        #[serde(default, rename = "keyFormat")]
        key_format: Option<serde_json::Value>,
        /// The value type's keyword or referenced type name.
        value: String,
        /// The value's bit-width modifier, mirroring `key_format`. Not
        /// observed on any real fixture yet (only `keyFormat` was), kept
        /// for symmetry rather than guessed at in detail.
        #[serde(default, rename = "valueFormat")]
        value_format: Option<serde_json::Value>,
    },
    /// A `kind` this module does not yet model — same "carried through,
    /// not guessed at" principle as `AbiReceiverMessage::Other`.
    #[serde(other)]
    Other,
}

/// One message receiver.
#[derive(Debug, Clone, Deserialize)]
pub struct AbiReceiver {
    /// `"internal"` on every real receiver observed so far; Tact's own
    /// language model documents `external()` receivers too (not seen in
    /// this crate's three fixtures) — carried through as a bare `String`
    /// rather than an enum with only one confirmed variant.
    pub receiver: String,
    /// What this receiver matches.
    pub message: AbiReceiverMessage,
}

/// What one [`AbiReceiver`] matches against an incoming message.
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AbiReceiverMessage {
    /// Matches a specific typed message (by name, resolved against
    /// `ContractAbi::types`).
    Typed {
        /// The message type's declared name.
        #[serde(rename = "type")]
        type_name: String,
    },
    /// Matches an exact string-comment payload (`receive("Hello") { ... }`)
    /// — **or** the catch-all string receiver (`receive(s: String) { ... }`,
    /// a real, idiomatic Tact pattern), which shares the same `"kind":
    /// "text"` tag but carries no `"text"` field at all (it matches any
    /// string, not one specific literal). Real crash found by a corpus run (`corpus-abi/clean/tact/text-message-receivers.tact`):
    /// an earlier version of this variant had `text: String` (required,
    /// no default), so deserializing *any* contract using the catch-all
    /// form failed outright with a `serde_json` error for the whole
    /// artifact — not a graceful per-field degradation, a total extraction
    /// failure from one common receiver style. `None` here means "matches
    /// any string," `Some(text)` means "matches exactly this literal."
    Text {
        /// The exact text this receiver matches, or `None` for the
        /// catch-all `receive(s: String)` form.
        #[serde(default)]
        text: Option<String>,
    },
    /// A `kind` this module does not yet model (e.g. `"empty"`/`"any"`
    /// receivers, real Tact concepts per the OTP-002 spec, just not
    /// present in any of the three real contracts this module was built
    /// against) — degrades to this instead of failing outright, same
    /// principle as `sting_tolkc::TypeDescriptor::Other`.
    #[serde(other)]
    Other,
}

/// One `get`-method.
#[derive(Debug, Clone, Deserialize)]
pub struct AbiGetter {
    /// Method name as declared.
    pub name: String,
    /// The TVM method id. **Signed** — confirmed real output includes
    /// negative values (`get(-pow2(18))` produces `methodId: -262144`;
    /// TON get-method ids are documented as 19-bit signed integers).
    #[serde(rename = "methodId")]
    pub method_id: i64,
    /// Named, typed argument list.
    #[serde(default)]
    pub arguments: Vec<AbiField>,
    /// Return type — kept `Option` defensively (every real getter sampled
    /// had one, including `bool`-returning getters, but a void-returning
    /// getter was never specifically tried).
    #[serde(default, rename = "returnType")]
    pub return_type: Option<AbiTypeRef>,
}

/// One reachable exit code's description.
#[derive(Debug, Clone, Deserialize)]
pub struct AbiError {
    /// Human-readable description. **Not a short symbolic name** the way
    /// Tolk's `ERROR_BAD_SEQNO`-style constants are — real values are full
    /// sentences (`"Stack underflow"`, `"Invalid standard address"`) —
    /// mapped into `sting_abi_core::ErrorCode::Named.name` as-is, not
    /// reformatted into a fake symbol.
    pub message: String,
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "docs/ARCHITECTURE.md §3.2 scopes the no-unwrap/expect/panic policy to library code outside tests"
)]
mod tests {
    use super::{AbiReceiverMessage, ContractAbi};

    /// Regression test for a real crash found by a corpus run
    /// (`corpus-abi/clean/tact/text-message-receivers.tact`): a contract
    /// using the catch-all string receiver (`receive(s: String) { ... }`)
    /// emits `{"kind":"text"}` with **no** `"text"` field — deserializing
    /// this used to fail the whole `ContractAbi` parse outright (a required
    /// `text: String` field), not just lose information about that one
    /// receiver.
    #[test]
    fn a_catch_all_string_receiver_deserializes_without_a_text_field() {
        let abi: ContractAbi = serde_json::from_str(
            r#"{
                "name": "Synthetic",
                "types": [],
                "receivers": [
                    {"receiver": "internal", "message": {"kind": "text", "text": "hello"}},
                    {"receiver": "internal", "message": {"kind": "text"}}
                ],
                "getters": [],
                "errors": {}
            }"#,
        )
        .expect("a bare {\"kind\":\"text\"} receiver must not fail the whole parse");

        match &abi.receivers[0].message {
            AbiReceiverMessage::Text { text } => assert_eq!(text.as_deref(), Some("hello")),
            other => panic!("expected Text{{text: Some(_)}}, got {other:?}"),
        }
        match &abi.receivers[1].message {
            AbiReceiverMessage::Text { text } => assert_eq!(*text, None),
            other => panic!("expected Text{{text: None}}, got {other:?}"),
        }
    }
}
