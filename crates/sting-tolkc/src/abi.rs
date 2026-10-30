//! Typed representation of `tolk`'s `<basename>.abi.json` artifact — the
//! contract's public interface: storage shape, incoming/outgoing message
//! types, get-methods, and the catalog of named error constants actually
//! thrown anywhere in the contract.
//!
//! Confirmed against real compiler output (`tolk` v1.4.1, run against
//! `corpus/clean/acton-contracts/config/Config.tolk`) the same way as
//! `symbol_types.rs` — see that module's doc comment for the shared
//! methodology and the `TypeDescriptor`/`Declaration`/`StructInstantiation`
//! types this module reuses. **Not an identical shape in both artifacts**:
//! `abi.json`'s `Declaration` entries never carry `ident_loc` (that field
//! exists purely for `symbolTypes.json`'s IDE/debugger use case) but may
//! carry a `description` (the declaration's own doc comment) that
//! `symbolTypes.json` was never observed to include — see
//! [`crate::Declaration`]'s own doc comment for how this was discovered.

use serde::Deserialize;

use crate::symbol_types::{Declaration, Param, StructInstantiation, TypeDescriptor};

/// The compiler's ABI description for one contract.
#[derive(Debug, Clone, Deserialize)]
pub struct AbiJson {
    /// ABI format version, as `tolk` stamps it (e.g. `"1.0"`).
    pub abi_schema_version: String,
    /// The `contract Name { ... }` block's name.
    pub contract_name: String,
    /// The contract header's optional `author: "..."` field.
    #[serde(default)]
    pub author: Option<String>,
    /// The contract header's optional `version: "..."` field.
    #[serde(default)]
    pub version: Option<String>,
    /// The contract header's optional `description: "..."` field.
    #[serde(default)]
    pub description: Option<String>,
    /// Same deduplicated type table as
    /// [`crate::SymbolTypes::unique_types`] — indices are only meaningful
    /// within this `AbiJson`, not shared with a separately parsed
    /// `SymbolTypes` from the same compile.
    pub unique_types: Vec<TypeDescriptor>,
    /// Same shape as [`crate::SymbolTypes::struct_instantiations`].
    #[serde(default)]
    pub struct_instantiations: Vec<StructInstantiation>,
    /// Same shape as [`crate::SymbolTypes::alias_instantiations`] — never
    /// observed populated, kept opaque.
    #[serde(default)]
    pub alias_instantiations: Vec<serde_json::Value>,
    /// Same shape as [`crate::SymbolTypes::declarations`].
    pub declarations: Vec<Declaration>,
    /// The contract's persistent storage type.
    pub storage: StorageRef,
    /// Message bodies accepted by `onInternalMessage`.
    #[serde(default)]
    pub incoming_messages: Vec<MessageRef>,
    /// Message bodies accepted by `onExternalMessage`.
    #[serde(default)]
    pub incoming_external: Vec<MessageRef>,
    /// Message bodies this contract is declared to send.
    #[serde(default)]
    pub outgoing_messages: Vec<MessageRef>,
    /// Declared TVM log events — real per-entry shape not yet confirmed
    /// against compiler output (empty in every example seen so far), kept
    /// opaque rather than guessed.
    #[serde(default)]
    pub emitted_events: Vec<serde_json::Value>,
    /// The contract's `get fun` methods.
    #[serde(default)]
    pub get_methods: Vec<GetMethod>,
    /// Every named/plain-integer error code actually reachable via a
    /// `throw`/`assert` anywhere in the contract.
    #[serde(default)]
    pub thrown_errors: Vec<ThrownError>,
    /// Always `"tolk"` for this crate's own vendored compiler — kept as a
    /// plain string rather than assumed, in case a future artifact
    /// version widens this.
    pub compiler_name: String,
    /// The exact `tolk` version that produced this artifact — compare
    /// against [`crate::TolkVersion`] if cross-checking is ever needed.
    pub compiler_version: String,
}

/// Points at the contract's storage type. `None` for a project with no
/// `contract{}` header declaring a `storage:` type at all — confirmed real,
/// not hypothetical: `tolk` still compiles such a project (the low-level
/// `global`-variable style `ac.rs`'s own module doc comment describes, and
/// this crate's `corpus/fixtures/cross-file/ac-unguarded-delegate` fixture)
/// and emits a genuinely empty `"storage": {}` object with no
/// `storage_ty_idx` field at all, not a placeholder value — caught by that
/// fixture's own real compile failing to parse before this field was made
/// optional, not assumed in advance (the same discovery shape as
/// [`crate::Declaration`]'s `ident_loc` — see its own doc comment).
#[derive(Debug, Clone, Copy, Deserialize)]
pub struct StorageRef {
    /// Index into [`AbiJson::unique_types`].
    #[serde(default)]
    pub storage_ty_idx: Option<u32>,
}

/// Points at one message body type, as referenced from
/// [`AbiJson::incoming_messages`]/[`AbiJson::incoming_external`]/[`AbiJson::outgoing_messages`].
#[derive(Debug, Clone, Copy, Deserialize)]
pub struct MessageRef {
    /// Index into [`AbiJson::unique_types`].
    pub body_ty_idx: u32,
}

/// One `get fun` method.
#[derive(Debug, Clone, Deserialize)]
pub struct GetMethod {
    /// The TVM method id (CRC16-derived from the name, per TON's own
    /// get-method convention) that off-chain callers invoke by.
    pub tvm_method_id: u32,
    /// Method name as declared.
    pub name: String,
    /// Named, typed parameter list.
    #[serde(default)]
    pub parameters: Vec<Param>,
    /// Index into [`AbiJson::unique_types`] for the return type.
    pub return_ty_idx: u32,
}

/// One error code reachable via `throw`/`assert` in the contract, as
/// [`AbiJson::thrown_errors`] catalogs them. The `Constant`/`EnumMember` vs.
/// `PlainInt` split is exactly the distinction a human-readable debugger UI
/// would want (`docs/ROADMAP.md` §9's "TVM debugger/simulator" idea —
/// decode `ERROR_BAD_SEQNO` instead of showing a bare `33`) — this crate
/// exists primarily for the static-analysis cross-file work, but this
/// specific type doubles as exactly what that future direction would need
/// too.
///
/// **`EnumMember` added after the fact, not designed upfront**: this type
/// originally had only `PlainInt`/`Constant` (`const ERROR_X = 33`-style
/// declarations) — real compiler output on `corpus/clean/acton-contracts/
/// nft-v1.1/NftItem.tolk` and `.../w5.2/WalletV5.tolk` (both throw via an
/// `enum { ... }` member, e.g. `throw(Errors.InvalidWorkchain)`) failed to
/// deserialize at all with a hard `unknown variant` error — `tolk` tags
/// that case `"enum_member"`, a third, previously unobserved `kind`, not a
/// `"constant"`. Same shape as `Constant` (`name` + `err_code`), so this
/// adds a variant rather than changing the existing two — real contracts
/// using either declaration style now both parse.
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ThrownError {
    /// A `throw(33)`-style bare integer literal — no name to recover.
    PlainInt {
        /// The literal exit code.
        err_code: u32,
    },
    /// A `throw(Errors.BadSeqno)`-style named constant declared via
    /// `const`.
    Constant {
        /// The constant's declared name (e.g. `"ERROR_BAD_SEQNO"`).
        name: String,
        /// The constant's resolved integer value.
        err_code: u32,
    },
    /// A `throw(Errors.InvalidWorkchain)`-style named constant declared as
    /// a member of an `enum { ... }` block — same shape as [`Self::Constant`],
    /// distinct `kind` tag only because `tolk` tracks the two declaration
    /// styles separately internally.
    EnumMember {
        /// The member's fully-qualified name (e.g.
        /// `"Errors.InvalidWorkchain"` — unlike [`Self::Constant`]'s bare
        /// name, this one is prefixed with its enum's own name, confirmed
        /// on real output).
        name: String,
        /// The member's resolved integer value.
        err_code: u32,
    },
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "docs/ARCHITECTURE.md §3.2 scopes the no-unwrap/expect/panic policy to library code outside tests"
)]
mod tests {
    use super::{StorageRef, ThrownError};

    #[test]
    fn thrown_error_parses_a_real_enum_member_variant() {
        // Real, byte-for-byte output from compiling
        // corpus/clean/acton-contracts/nft-v1.1/NftItem.tolk (2026-07-21) —
        // this exact shape previously failed with a hard `unknown variant`
        // deserialize error, see this type's own doc comment.
        let parsed: ThrownError = serde_json::from_str(
            r#"{"kind":"enum_member","name":"Errors.InvalidWorkchain","err_code":333}"#,
        )
        .expect("tolk's real enum_member shape must parse");
        match parsed {
            ThrownError::EnumMember { name, err_code } => {
                assert_eq!(name, "Errors.InvalidWorkchain");
                assert_eq!(err_code, 333);
            }
            other => panic!("expected EnumMember, got {other:?}"),
        }
    }

    #[test]
    fn storage_ref_parses_a_populated_storage_ty_idx() {
        let parsed: StorageRef = serde_json::from_str(r#"{"storage_ty_idx": 12}"#)
            .expect("a real contract{} header's storage produces this shape");
        assert_eq!(parsed.storage_ty_idx, Some(12));
    }

    #[test]
    fn storage_ref_parses_a_genuinely_empty_object() {
        // Real, confirmed output for a project with no `contract{}` header
        // at all (see this struct's own doc comment) — `tolk` emits `{}`,
        // not a placeholder `storage_ty_idx`.
        let parsed: StorageRef =
            serde_json::from_str("{}").expect("a project with no contract{} header still compiles");
        assert_eq!(parsed.storage_ty_idx, None);
    }
}
