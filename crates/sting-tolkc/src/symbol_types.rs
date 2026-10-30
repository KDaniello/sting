//! Typed representation of `tolk`'s `<basename>.symbolTypes.json` artifact —
//! the compiler's own whole-project symbol table: every file with its
//! resolved import graph, every function/struct/alias/enum declaration,
//! each with a fully monomorphized name and a precise source location.
//!
//! Confirmed against real compiler output (`tolk` v1.4.1, run against
//! `corpus/clean/acton-contracts/config/Config.tolk`), not against any
//! public schema documentation, because none exists: the shape was recovered
//! from output.
//! Deliberately does not model every field this JSON could ever contain,
//! only what was actually observed, plus a safe fallback
//! ([`TypeDescriptor::Other`]) for any `unique_types` entry whose `kind`
//! this module doesn't recognize yet — the same "start narrow, extend only
//! once a real need appears" discipline this whole project's detectors
//! already follow (see e.g. `br.rs`/`uta.rs`'s own doc comments).

use serde::Deserialize;

/// The compiler's whole-project symbol table for one `tolk` invocation.
#[derive(Debug, Clone, Deserialize)]
pub struct SymbolTypes {
    /// Every file that was actually compiled (entrypoint, its transitive
    /// project-relative imports, and every `@stdlib/...` file pulled in).
    pub files: Vec<FileEntry>,
    /// A deduplicated table of every type shape referenced anywhere in the
    /// project, indexed by position — other fields point into this table
    /// by `*_ty_idx`, never by name.
    pub unique_types: Vec<TypeDescriptor>,
    /// Monomorphized instances of generic structs (e.g. `MapEntry<int,
    /// slice>`) actually used in this compilation.
    #[serde(default)]
    pub struct_instantiations: Vec<StructInstantiation>,
    /// Same idea as [`Self::struct_instantiations`] but for generic type
    /// aliases — never observed populated in practice, kept opaque rather
    /// than guessed at.
    #[serde(default)]
    pub alias_instantiations: Vec<serde_json::Value>,
    /// Every `struct`/`alias`/`enum` declared anywhere in the project.
    pub declarations: Vec<Declaration>,
    /// Every function in the project, including compiler-synthesized
    /// generic monomorphizations (e.g. `map<int32, cell>.get`).
    pub functions: Vec<FunctionEntry>,
}

/// One compiled source file.
#[derive(Debug, Clone, Deserialize)]
pub struct FileEntry {
    /// Index into [`SymbolTypes::files`] — what `*_loc`/`imports` entries
    /// elsewhere in this JSON refer to.
    pub file_id: u32,
    /// Absolute path as `tolk` resolved it (not necessarily the path this
    /// crate's caller originally passed in).
    pub file_name: String,
    /// Source length in characters — not currently consumed by this crate,
    /// kept for completeness.
    pub size_chars: u32,
    /// [`FileEntry::file_id`] of every file this one directly imports
    /// (not transitively closed — a file's own imports must be walked
    /// separately to reach its imports' imports).
    #[serde(default)]
    pub imports: Vec<u32>,
}

/// A `[file_id, start_line, start_col, end_line, end_col]` source span, as
/// `tolk` emits it. Deserializes directly from that raw 5-element array so
/// every other type in this module can use the named form.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(from = "[u32; 5]")]
pub struct SourceSpan {
    /// [`FileEntry::file_id`] this span is located in.
    pub file_id: u32,
    /// 1-indexed starting line (confirmed by measuring identifier length
    /// against `start_col`/`end_col` on a real compiled example).
    pub start_line: u32,
    /// 1-indexed starting column.
    pub start_col: u32,
    /// 1-indexed ending line.
    pub end_line: u32,
    /// 1-indexed ending column.
    pub end_col: u32,
}

impl From<[u32; 5]> for SourceSpan {
    fn from(a: [u32; 5]) -> Self {
        Self {
            file_id: a[0],
            start_line: a[1],
            start_col: a[2],
            end_line: a[3],
            end_col: a[4],
        }
    }
}

impl SourceSpan {
    /// `true` for the all-zero sentinel `tolk` emits when a declaration has
    /// no single physical source location — confirmed on real output for
    /// compiler-synthesized generic monomorphizations (e.g. `map<int32,
    /// cell>.get`, which exists only as an instantiation of stdlib code,
    /// not as a distinct written function). Unambiguous: every real span's
    /// line numbers are 1-indexed, so `start_line == 0` never occurs
    /// otherwise.
    #[must_use]
    pub fn is_synthetic(self) -> bool {
        self.file_id == 0
            && self.start_line == 0
            && self.start_col == 0
            && self.end_line == 0
            && self.end_col == 0
    }
}

/// One parameter of a [`FunctionEntry`] or [`crate::GetMethod`].
#[derive(Debug, Clone, Deserialize)]
pub struct Param {
    /// Parameter name as written (`"self"` for a method receiver).
    pub name: String,
    /// Index into [`SymbolTypes::unique_types`].
    pub ty_idx: u32,
}

/// One function in the project's flat function table.
#[derive(Debug, Clone, Deserialize)]
pub struct FunctionEntry {
    /// Index into [`SymbolTypes::functions`].
    pub f_idx: u32,
    /// Fully resolved, monomorphized name — e.g. `Storage.save`,
    /// `map<int32, cell>.get` — not just the bare identifier as written at
    /// the declaration site.
    pub name: String,
    /// Index into [`SymbolTypes::unique_types`] for the return type.
    pub return_ty_idx: u32,
    /// Parameter count, including an implicit `self` for methods.
    pub num_params: u32,
    /// Span of the function name at its declaration site. Check
    /// [`SourceSpan::is_synthetic`] before treating this as a real
    /// location — compiler-synthesized generic instantiations carry the
    /// all-zero sentinel.
    pub ident_loc: SourceSpan,
    /// Span of the function body's closing brace.
    pub end_loc: SourceSpan,
    /// Named, typed parameter list.
    #[serde(default)]
    pub params: Vec<Param>,
}

/// A monomorphized instance of a generic struct actually used in this
/// compilation (e.g. `MapEntry<uint256, slice>`).
#[derive(Debug, Clone, Deserialize)]
pub struct StructInstantiation {
    /// Index into [`SymbolTypes::unique_types`] for this specific
    /// instantiation.
    pub ty_idx: u32,
    /// Display name including its type arguments, e.g.
    /// `"MapEntry<uint256, slice>"`.
    pub struct_name: String,
    /// [`SymbolTypes::unique_types`] indices of this instantiation's own
    /// fields, in declaration order.
    #[serde(default)]
    pub monomorphic_fields_ty_idx: Vec<u32>,
}

/// A message/struct's TL-B-style opcode prefix.
#[derive(Debug, Clone, Copy, Deserialize)]
pub struct TlbPrefix {
    /// The prefix's bit pattern, as an integer.
    pub prefix_num: u64,
    /// How many of [`Self::prefix_num`]'s low bits are the actual prefix.
    pub prefix_len: u32,
}

/// One field of a [`Declaration::Struct`].
#[derive(Debug, Clone, Deserialize)]
pub struct FieldEntry {
    /// Field name as declared.
    pub name: String,
    /// Index into [`SymbolTypes::unique_types`].
    pub ty_idx: u32,
}

/// `alias`'s optional custom (de)serialization hook (e.g. `ConfigDict`'s
/// hand-written pack/unpack instead of the default derived one).
#[derive(Debug, Clone, Copy, Deserialize)]
pub struct CustomPackUnpack {
    /// Whether this alias defines its own `packToBuilder`.
    pub pack_to_builder: bool,
    /// Whether this alias defines its own `unpackFromSlice`.
    pub unpack_from_slice: bool,
}

/// One `struct`/`alias`/`enum` declared anywhere in the project.
///
/// **`ident_loc` is genuinely absent, not merely sometimes-null, in
/// `abi.json`** — confirmed on real output: `symbolTypes.json`'s
/// declarations always carry it, `abi.json`'s never do (that artifact
/// describes the contract's public interface for external tooling, not for
/// an IDE/debugger, so it has no reason to). This shared type is
/// deserialized from both artifacts, so every `ident_loc` here is
/// `Option`, not required — an earlier version of this module got this
/// wrong by assuming the two artifacts shared one identical `Declaration`
/// shape; a real compile failed loudly with a missing-field JSON error and
/// corrected it.
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Declaration {
    /// A `type Name = ...` alias.
    Alias {
        /// Declared name.
        name: String,
        /// Index into [`SymbolTypes::unique_types`] for this alias itself.
        ty_idx: u32,
        /// Location of the alias name in its `type` declaration — present
        /// in `symbolTypes.json`, absent in `abi.json`.
        #[serde(default)]
        ident_loc: Option<SourceSpan>,
        /// Index into [`SymbolTypes::unique_types`] for the aliased type.
        target_ty_idx: u32,
        /// Present only for aliases with hand-written (de)serialization.
        #[serde(default)]
        custom_pack_unpack: Option<CustomPackUnpack>,
        /// The alias's own doc comment, present only in `abi.json`.
        #[serde(default)]
        description: Option<String>,
    },
    /// A `struct Name { ... }` declaration.
    Struct {
        /// Declared name (without generic type arguments).
        name: String,
        /// Index into [`SymbolTypes::unique_types`].
        ty_idx: u32,
        /// Location of the struct name in its declaration — present in
        /// `symbolTypes.json`, absent in `abi.json`.
        #[serde(default)]
        ident_loc: Option<SourceSpan>,
        /// Generic type parameter names, if this is a generic struct.
        #[serde(default)]
        type_params: Vec<String>,
        /// TL-B opcode prefix, present for structs used as a message body.
        #[serde(default)]
        prefix: Option<TlbPrefix>,
        /// Fields in declaration order.
        #[serde(default)]
        fields: Vec<FieldEntry>,
        /// The struct's own doc comment, present only in `abi.json`.
        #[serde(default)]
        description: Option<String>,
    },
    /// An `enum Name { ... }` declaration.
    Enum {
        /// Declared name.
        name: String,
        /// Index into [`SymbolTypes::unique_types`].
        ty_idx: u32,
        /// Location of the enum name in its declaration — present in
        /// `symbolTypes.json`, absent in `abi.json`.
        #[serde(default)]
        ident_loc: Option<SourceSpan>,
        /// Index into [`SymbolTypes::unique_types`] for this enum's
        /// underlying encoded representation.
        encoded_as_ty_idx: u32,
        /// Member list — real per-member field shape not yet confirmed
        /// against compiler output, kept opaque rather than guessed.
        #[serde(default)]
        members: Vec<serde_json::Value>,
    },
}

/// One arm of a [`TypeDescriptor::Union`] (a TL-B-style tagged union, e.g.
/// the set of structs a message field can hold).
#[derive(Debug, Clone, Deserialize)]
pub struct UnionVariant {
    /// Index into [`SymbolTypes::unique_types`] for this arm's type.
    pub variant_ty_idx: u32,
    /// This arm's discriminating prefix bit pattern.
    pub prefix_num: u64,
    /// How many low bits of [`Self::prefix_num`] are the actual prefix.
    pub prefix_len: u32,
    /// `true` when the prefix is implicit (e.g. a plain `bool`-shaped
    /// union) rather than a written opcode.
    #[serde(default)]
    pub is_prefix_implicit: bool,
    /// Internal TVM stack-slot type id — opaque, not currently consumed.
    pub stack_type_id: u32,
    /// How many TVM stack slots this arm occupies.
    pub stack_width: u32,
}

/// One entry of [`SymbolTypes::unique_types`] — every type shape the
/// compiler encountered anywhere in the project, referenced elsewhere only
/// by index. Variant names are `PascalCase` per Rust convention; the
/// `#[serde(rename)]` on each carries the real, inconsistently-cased JSON
/// tag `tolk` actually emits (a mix of `lowercase`, `camelCase`, and
/// `PascalCase` — not a convention this crate invented, transcribed as
/// observed).
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "kind")]
pub enum TypeDescriptor {
    /// The TVM `null`-shaped void return type.
    #[serde(rename = "void")]
    Void,
    /// An unsized TVM integer.
    #[serde(rename = "int")]
    Int,
    /// A TVM slice.
    #[serde(rename = "slice")]
    Slice,
    /// A TVM cell.
    #[serde(rename = "cell")]
    Cell,
    /// A TVM builder.
    #[serde(rename = "builder")]
    Builder,
    /// A boolean (TVM `-1`/`0` integer under the hood).
    #[serde(rename = "bool")]
    Bool,
    /// The `coins` (`VarUInteger 16`) primitive.
    #[serde(rename = "coins")]
    Coins,
    /// A TVM/TON address.
    #[serde(rename = "address")]
    Address,
    /// A short-form nullable address (`address?` in Tolk source) — the
    /// compiler emits a bare `{"kind":"addressOpt"}`, not the general
    /// `{"kind":"nullable","inner_ty_idx":...}` wrapper every other
    /// nullable type uses. Confirmed real, not hypothetical: found on
    /// multiple real, deployed contracts (`docs/domains/extractors.md`
    /// — `MinterStorage.adminAddress`/`nextAdminAddress`,
    /// `NftDataReply.ownerAddress`), and confirmed by a systematic audit of
    /// this module's own coverage against the real corpus
    /// (`docs/domains/service.md`) before
    /// this variant was added — not the only short form found, see
    /// [`Self::AddressAny`].
    #[serde(rename = "addressOpt")]
    AddressOpt,
    /// The full TL-B `MsgAddress` union (`AddrNone | AddrExtern | AddrStd |
    /// AddrVar`) as opposed to [`Self::Address`]'s narrower, more common
    /// internal-address case — found by the same systematic corpus audit
    /// that found [`Self::AddressOpt`], not previously modeled at all
    /// (silently degraded to [`Self::Other`]). Confirmed as a **real
    /// struct field type**, not just an internal compiler artifact — e.g.
    /// `CollectionDataReply.ownerAddress`, `IntMsgInfoRelaxed.src` in
    /// `tolk-stdlib/common.tolk`'s own message-envelope structs, which real
    /// contracts reference. This IR does not yet distinguish "any address"
    /// from "internal address" as separate shapes (that would be a new
    /// `TypeShape` variant, a bigger change than this fix's scope) — mapped
    /// to the same `sting_abi_core::TypeShape::Address` as
    /// [`Self::Address`], a documented simplification, not silently lost.
    #[serde(rename = "addressAny")]
    AddressAny,
    /// The type of the bare `null` literal expression itself (e.g. an
    /// optional field's default-value expression) — found by the same
    /// audit, confirmed real in `tolk-stdlib/common.tolk`, but **not**
    /// observed as any real struct/message field's type across the entire
    /// audited corpus (only [`Self::AddressOpt`]/[`Self::AddressAny`] were).
    /// Modeled explicitly anyway, rather than left to fall into
    /// [`Self::Other`], so this module's coverage of the real `tolk` output
    /// surface stays complete and auditable — not because it's known to
    /// affect any `ContractSchema` field today.
    #[serde(rename = "nullLiteral")]
    NullLiteral,
    /// A callable/function-reference type (Tolk supports higher-order
    /// functions) — real semantics/JSON sub-shape still unconfirmed against
    /// live output (never observed paired with actual field data across
    /// every real contract sampled building `sting-abi-tolk`), same
    /// "carried through, not guessed at" status as when this variant was
    /// first added. **Not** what represents a TVM tuple, despite an earlier
    /// version of this doc comment claiming so — confirmed empirically
    /// (`docs/domains/precision-sources.md`) that Tolk's stdlib `tuple` type
    /// is really `alias tuple =
    /// arrayOf<unknown>` (see [`Self::ArrayOf`]/[`Self::Unknown`]), with no
    /// `"kind":"callable"` involved at all.
    #[serde(rename = "callable")]
    Callable,
    /// A language-level UTF-8 string (Tolk's `string` keyword) — not a TVM
    /// primitive (represented as a `Slice`/`Cell` under the hood), but a
    /// distinct ABI-level kind the real compiler emits, confirmed on real
    /// output (a corpus run over
    /// `corpus/clean/acton-contracts/nft-v1.1`'s `content`-shaped storage
    /// fields).
    #[serde(rename = "string")]
    String,
    /// Whatever bits/refs are left unread in a slice (`RemainingBitsAndRefs`
    /// shape, e.g. a message's trailing payload).
    #[serde(rename = "remaining")]
    Remaining,
    /// A type the compiler could not resolve more specifically (e.g. an
    /// untyped tuple element before a cast).
    #[serde(rename = "unknown")]
    Unknown,
    /// A fixed-width signed integer.
    #[serde(rename = "intN")]
    IntN {
        /// Bit width.
        n: u32,
    },
    /// A fixed-width unsigned integer.
    #[serde(rename = "uintN")]
    UintN {
        /// Bit width.
        n: u32,
    },
    /// A TL-B `VarUInteger n`-style variable-width unsigned integer.
    #[serde(rename = "varuintN")]
    VarUintN {
        /// Maximum bit width of the length-prefixed encoding.
        n: u32,
    },
    /// A fixed-width opaque bitstring.
    #[serde(rename = "bitsN")]
    BitsN {
        /// Bit width.
        n: u32,
    },
    /// A reference to a named `type` alias — resolve via
    /// [`SymbolTypes::declarations`]'s matching [`Declaration::Alias`].
    #[serde(rename = "AliasRef")]
    AliasRef {
        /// The alias's declared name.
        alias_name: String,
    },
    /// A reference to a named `struct` — resolve via
    /// [`SymbolTypes::declarations`]'s matching [`Declaration::Struct`].
    #[serde(rename = "StructRef")]
    StructRef {
        /// The struct's declared name (without generic arguments).
        struct_name: String,
        /// [`SymbolTypes::unique_types`] indices of the concrete type
        /// arguments, present only for a generic struct reference.
        #[serde(default)]
        type_args_ty_idx: Vec<u32>,
    },
    /// A reference to a named `enum` — resolve via
    /// [`SymbolTypes::declarations`]'s matching [`Declaration::Enum`].
    #[serde(rename = "EnumRef")]
    EnumRef {
        /// The enum's declared name.
        enum_name: String,
    },
    /// A `T?` optional/nullable type.
    #[serde(rename = "nullable")]
    Nullable {
        /// Index into [`SymbolTypes::unique_types`] for `T`.
        inner_ty_idx: u32,
        /// Internal TVM stack-slot type id — opaque, not currently
        /// consumed.
        #[serde(default)]
        stack_type_id: Option<u32>,
        /// How many TVM stack slots this occupies.
        #[serde(default)]
        stack_width: Option<u32>,
    },
    /// A `cell<T>`-shaped "T packed into a cell" type.
    #[serde(rename = "cellOf")]
    CellOf {
        /// Index into [`SymbolTypes::unique_types`] for `T`.
        inner_ty_idx: u32,
    },
    /// A TVM tuple typed as a homogeneous array of `T`.
    #[serde(rename = "arrayOf")]
    ArrayOf {
        /// Index into [`SymbolTypes::unique_types`] for the element type.
        inner_ty_idx: u32,
    },
    /// A lisp-style cons-list of `T` (`tolk-stdlib/lisp-lists.tolk`).
    #[serde(rename = "lispListOf")]
    LispListOf {
        /// Index into [`SymbolTypes::unique_types`] for the element type.
        inner_ty_idx: u32,
    },
    /// A `map<K, V>`.
    #[serde(rename = "mapKV")]
    MapKv {
        /// Index into [`SymbolTypes::unique_types`] for `K`.
        key_ty_idx: u32,
        /// Index into [`SymbolTypes::unique_types`] for `V`.
        value_ty_idx: u32,
    },
    /// A fixed tuple of heterogeneous element types.
    #[serde(rename = "tensor")]
    Tensor {
        /// [`SymbolTypes::unique_types`] indices of each element, in order.
        items_ty_idx: Vec<u32>,
    },
    /// An as-yet-unresolved generic type parameter (e.g. `T` inside a
    /// generic function body before monomorphization).
    #[serde(rename = "genericT")]
    GenericT {
        /// The type parameter's name as written.
        name_t: String,
    },
    /// A TL-B-style tagged union — e.g. the set of possible message bodies
    /// a field can hold, or `bool` under the hood.
    #[serde(rename = "union")]
    Union {
        /// Each possible arm.
        variants: Vec<UnionVariant>,
        /// How many TVM stack slots this union's widest arm occupies.
        stack_width: u32,
    },
    /// A `kind` this module does not yet model — parsing degrades to this
    /// instead of failing outright, since `tolk`'s own type-shape surface
    /// is not documented anywhere and may grow with new compiler versions.
    #[serde(other)]
    Other,
}
