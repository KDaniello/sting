//! The normalized type table — [`ContractSchema::types`](crate::ContractSchema::types)
//! and everything that indexes into it. Deliberately **not** a re-export of
//! any single source artifact's type-shape enum (Tolk's `TypeDescriptor` has
//! ~20 variants, several existing only to support *symbolic* by-name
//! resolution against a separate `declarations` table). Once extraction has
//! run, that resolution is already done — a normalized [`NamedType`] table
//! only ever needs to point at *other entries in the same table*, by index,
//! never by name. See `docs/domains/abi-ir.md`

/// An index into [`ContractSchema::types`](crate::ContractSchema::types).
/// Meaningless outside the `ContractSchema` it was produced alongside —
/// never shared across two separately extracted schemas.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(transparent)]
pub struct TypeRef(pub u32);

/// One entry of the deduplicated type table.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct NamedType {
    /// Declared name, for a `struct`/`alias`/`enum` — `None` for an
    /// anonymous/structural type (e.g. a bare `int` or an inline tensor)
    /// that exists in the table only because something else references it.
    pub name: Option<String>,
    /// The source declaration's own doc comment, if the source language and
    /// this particular extractor tier carry one through (Tolk `abi.json`
    /// does; `FunC` heuristic extraction generally will not).
    pub description: Option<String>,
    /// The TL-B-style discriminating prefix, if this struct has one.
    /// **Not always a 32-bit message opcode** — confirmed empirically
    /// against real `tolk` output (`corpus/clean/acton-contracts/config`):
    /// a top-level message like `SetNextValidatorSetRequest` does carry a
    /// full 32-bit `prefix_len`, but a struct embedded as one arm of a
    /// tagged union nested *inside* a message (e.g. that same corpus's
    /// `ConfigProposal`, embedded in `NewVotingProposalRequest.proposal`)
    /// carries an 8-bit prefix instead — an earlier version of this field
    /// was a bare `Option<u32>` and would have silently truncated or
    /// misrepresented that case. Present for any struct with a TL-B prefix
    /// at all (message body or nested tagged variant), absent for
    /// storage-only or purely structural types.
    pub prefix: Option<TlbPrefix>,
    /// What shape this type actually is.
    pub shape: TypeShape,
}

/// One named, typed field of a [`TypeShape::Struct`] — also reused as-is for
/// [`crate::GetMethod`] parameters ([`crate::Param`] is kept as a distinct
/// type rather than a type alias for this one: a method parameter and a
/// struct field are the same shape today but answer different questions,
/// and have already diverged once in the source artifacts this is modeled
/// on — Tolk's own `Param` and `FieldEntry` are two separate types for
/// exactly that reason).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct FieldRef {
    /// Field name as declared.
    pub name: String,
    /// Index into [`ContractSchema::types`](crate::ContractSchema::types).
    pub ty: TypeRef,
}

/// A TL-B-style discriminating bit-prefix — direct normalized form of
/// Tolk's own `TlbPrefix` (`crates/sting-tolkc/src/symbol_types.rs`).
/// Deliberately not assumed to always be 32 bits wide: confirmed
/// empirically (real `tolk` output, `corpus/clean/acton-contracts/config`)
/// that a struct used as one arm of a nested tagged union can carry an
/// 8-bit prefix, not just the 32-bit opcode a top-level message has.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct TlbPrefix {
    /// The prefix's bit pattern, as an integer.
    pub prefix_num: u64,
    /// How many of `prefix_num`'s low bits are the actual prefix.
    pub prefix_len: u32,
}

/// One arm of a [`TypeShape::Union`] — direct normalized form of Tolk's own
/// `UnionVariant` (`crates/sting-tolkc/src/symbol_types.rs`), minus
/// `stack_type_id`/`stack_width`: TVM-stack-layout implementation detail
/// that crate's own doc comment already calls "opaque, not currently
/// consumed" — not ABI-relevant to a consumer decoding a message off the
/// wire, so deliberately not carried into this IR.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct UnionVariant {
    /// Index into [`ContractSchema::types`](crate::ContractSchema::types)
    /// for this arm's type.
    pub ty: TypeRef,
    /// This arm's discriminating prefix.
    pub prefix: TlbPrefix,
    /// `true` when the prefix is implicit (e.g. a plain `bool`-shaped
    /// union) rather than a written opcode.
    pub is_prefix_implicit: bool,
}

/// The shape of one [`NamedType`] table entry. Covers the value shapes
/// actually observed in real `tolk` compiler output
/// (`crates/sting-tolkc/src/symbol_types.rs`'s `TypeDescriptor`) at the time
/// this was written, collapsed to their language-neutral normalized form —
/// extend only once another language's extractor actually needs a shape not
/// covered here, never speculatively ahead of that need. **Known deliberate gap, not an oversight**: `tolk`'s own
/// `Declaration::Enum::members` is kept as opaque, unconfirmed JSON even in
/// `sting-tolkc` itself ("real per-member field shape not yet confirmed
/// against compiler output, kept opaque rather than guessed" — that crate's
/// own doc comment) — [`Self::Enum`] below carries member *names* (confirmed
/// safe to assume every member has one) but not per-member values, for the
/// same reason: guessing an encoding neither this crate nor `sting-tolkc`
/// has actually observed would be worse than admitting the gap.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TypeShape {
    /// The TVM `null`-shaped void return type.
    Void,
    /// A TVM integer. `bits: None` for an unsized `int`; `Some(n)` for a
    /// fixed-width `intN`/`uintN`.
    Int {
        /// Bit width, if fixed.
        bits: Option<u16>,
        /// `false` for `uintN`/an inherently unsigned primitive.
        signed: bool,
    },
    /// A boolean (TVM `-1`/`0` integer under the hood).
    Bool,
    /// The `coins` (`VarUInteger 16`) primitive.
    Coins,
    /// A TVM/TON address.
    Address,
    /// A TVM cell.
    Cell,
    /// A TVM slice.
    Slice,
    /// A TVM builder.
    Builder,
    /// A language-level UTF-8 string (Tolk's `string`, Tact's `String`) —
    /// **not** a TVM primitive (the VM itself has no string type; both
    /// languages represent one as a `Slice`/`Cell` under the hood), but a
    /// distinct, first-class type at the ABI level in both source
    /// languages, confirmed real and common on real output (a corpus run
    /// against `corpus/clean/acton-contracts/nft-v1.1` and
    /// `corpus-abi/clean/tact/local-type-inference`: every jetton/NFT `content`/
    /// `name`-shaped storage field and getter that returns one degraded to
    /// [`Self::Unknown`] before this variant existed — a real, previously
    /// undiscovered structural-completeness gap, not a hypothetical one).
    Str,
    /// A fixed-width opaque bitstring.
    Bits {
        /// Bit width.
        width: u16,
    },
    /// Whatever bits/refs are left unread in a slice (a message's trailing
    /// payload, typically).
    Remaining,
    /// A `T?` optional/nullable type.
    Nullable {
        /// Index into [`ContractSchema::types`](crate::ContractSchema::types) for `T`.
        inner: TypeRef,
    },
    /// A homogeneous array of `T`.
    Array {
        /// Index into [`ContractSchema::types`](crate::ContractSchema::types) for the element type.
        inner: TypeRef,
    },
    /// `T` packed into its own `cell` (Tolk's `cell<T>`) — distinct from a
    /// bare [`Self::Cell`]: this one still names what's packed inside.
    /// Added after finding it in every non-trivial real Tolk contract
    /// sampled (Config/JettonMinter/NftItem/WalletV5/DnsItem/Elector) —
    /// missing this variant entirely in an earlier draft of this enum was
    /// found by compiling real fixtures for the golden set, not assumed
    /// complete from the first small sample.
    CellPacked {
        /// Index into [`ContractSchema::types`](crate::ContractSchema::types) for `T`.
        inner: TypeRef,
    },
    /// A lisp-style cons-list of `T` (Tolk stdlib's `list<T>`) — same
    /// discovery story as [`Self::CellPacked`] (seen in real governance
    /// contracts, `Config` and `Elector`, not in the smaller first sample).
    LispList {
        /// Index into [`ContractSchema::types`](crate::ContractSchema::types) for `T`.
        inner: TypeRef,
    },
    /// A `type Name = T` alias — resolving to this rather than being
    /// transparently flattened away, because real Tolk output shows other
    /// declarations addressing the alias's own table slot directly (e.g. a
    /// struct field's `ty_idx` pointing straight at the alias, not at what
    /// it resolves to) — collapsing it away at extraction time would lose
    /// that addressable node.
    Alias {
        /// Index into [`ContractSchema::types`](crate::ContractSchema::types)
        /// for the type this aliases.
        target: TypeRef,
    },
    /// A `map<K, V>`.
    Map {
        /// Index into [`ContractSchema::types`](crate::ContractSchema::types) for `K`.
        key: TypeRef,
        /// Index into [`ContractSchema::types`](crate::ContractSchema::types) for `V`.
        value: TypeRef,
    },
    /// A struct with named fields — the shape backing most storage types
    /// and every message body.
    Struct {
        /// Fields in declaration order.
        fields: Vec<FieldRef>,
    },
    /// A TL-B-style tagged union — e.g. the set of possible message bodies a
    /// field can hold.
    Union {
        /// Each possible arm, with the wire-level prefix that disambiguates
        /// it from the others.
        variants: Vec<UnionVariant>,
    },
    /// A named `enum` with an integer underlying representation. Member
    /// *values* are deliberately not modeled — see this enum's own doc
    /// comment for why.
    Enum {
        /// Index into [`ContractSchema::types`](crate::ContractSchema::types)
        /// for the underlying encoded representation (an [`Self::Int`] in
        /// every real case observed so far).
        encoded_as: TypeRef,
        /// Member names, in declaration order.
        member_names: Vec<String>,
    },
    /// A fixed tuple of heterogeneous element types.
    Tensor {
        /// [`ContractSchema::types`](crate::ContractSchema::types) indices of each element, in order.
        items: Vec<TypeRef>,
    },
    /// A type the extractor could not resolve more specifically — carried
    /// through rather than dropped, so a partial extraction is still
    /// visible to the consumer instead of silently missing a field.
    Unknown,
}
