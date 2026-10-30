//! A type as the compiler knows it, and three ways of not knowing it.
//!
//! Full Tolk type inference means rewriting the compiler's front end, and it is
//! not part of this layer. The layer therefore has to **answer "I do not know"
//! honestly** — not through `Option::None`, indistinguishable from "there is no
//! type at all", but through a variant of its own carrying the reason.
//!
//! [`ResolvedType::Ambiguous`] is deliberately separate from
//! [`ResolvedType::Unknown`]: for a consumer these are different situations. In
//! the first the candidates are finite and all of them can be checked; in the
//! second nothing can. Today's `AC` collapses both into one fail-safe and pays
//! for it in precision
//! ([detectors.md §5](../../../docs/domains/detectors.md)).

use sting_tolkc::{Declaration, SymbolTypes, TypeDescriptor};

/// A reference to a type in the compiler's `unique_types` table.
///
/// Holds the index rather than the name: the table is deduplicated, and the
/// index is the only identifier every other field of the artefact refers to a
/// type by. The name is computed for a human and for comparison with structs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypeRef {
    /// The index in `unique_types` where the compiler knows the type. `None`
    /// means it was read from source: the compiler's `declarations` do not hold
    /// the standard library's structs even though it compiled them, and
    /// `InMessage` is exactly that case.
    idx: Option<u32>,
    display: String,
    struct_name: Option<String>,
}

impl TypeRef {
    /// Builds a type reference from an index into the compiler's table.
    ///
    /// Returns `None` when the index is outside the table. The compiler's
    /// artefact is internally consistent, so that would mean somebody else's
    /// `symbolTypes.json` was handed in, not that a type is "simply missing".
    #[must_use]
    pub fn from_index(symbols: &SymbolTypes, idx: u32) -> Option<Self> {
        let descriptor = symbols.unique_types.get(idx as usize)?;
        Some(Self {
            idx: Some(idx),
            display: describe(symbols, descriptor),
            struct_name: struct_name_of(symbols, descriptor),
        })
    }

    /// A struct known only from source.
    ///
    /// Needed for the standard library's structs: the compiler compiles them
    /// and does not surface them in the artefact's `declarations`. Without this
    /// the type of `in.senderAddress` — the one `AC` rests on — stays unknown,
    /// even though the declaration sits in `tolk-stdlib/common.tolk`.
    #[must_use]
    pub fn from_source_struct(name: &str) -> Self {
        Self {
            idx: None,
            display: name.to_string(),
            struct_name: Some(name.to_string()),
        }
    }

    /// The index in `unique_types`, where the compiler knows the type.
    #[must_use]
    pub fn index(&self) -> Option<u32> {
        self.idx
    }

    /// A human-readable name, for messages and tests rather than comparison.
    #[must_use]
    pub fn display_name(&self) -> &str {
        &self.display
    }

    /// The struct's name where the type is a struct, an alias included.
    ///
    /// This is what methods resolve by: a method's function sits in the
    /// compiler's table under `<Struct>.<method>`.
    #[must_use]
    pub fn struct_name(&self) -> Option<&str> {
        self.struct_name.as_deref()
    }
}

/// Why a type is undetermined.
///
/// The reason is part of the contract rather than diagnostics: a consumer
/// behaves differently when the answer is "we cannot infer this shape" and when
/// it is "the compiler never ran".
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UnknownReason {
    /// The node lies outside any function body — for instance inside the
    /// `ERROR` span of a `contract {}` block, which grammar 0.0.5 does not
    /// parse.
    OutsideAnyFunction,
    /// The name was found neither among the parameters nor among the
    /// function's local declarations. Most often a global:
    /// `symbolTypes.json` does not hold them at all.
    NotDeclaredInFunction,
    /// The declaration was found and carries neither an initializer nor a type
    /// annotation, so there is nothing to infer from.
    NoInitializer,
    /// The initializer's shape is not covered by the inference rules. A
    /// **normal** answer rather than a defect: inference is never widened
    /// speculatively, only against a real consumer's real miss.
    UninferrableExpression {
        /// The tree-sitter node kind inference stopped at, which is what
        /// shows which rule to add if the miss turns out to be real.
        node_kind: String,
    },
    /// The call did not resolve, so its result type is unknown too.
    UnresolvedCall,
    /// The receiver's type is known and has no such field.
    NoSuchField {
        /// The struct the field was looked for in.
        struct_name: String,
        /// The field's name.
        field: String,
    },
}

/// The layer's answer to "what type is this".
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResolvedType {
    /// The type is determined uniquely.
    Known(TypeRef),
    /// More than one candidate. A consumer is entitled to check all of them,
    /// which is fundamentally better than giving up the way `AC` does today.
    Ambiguous(Vec<TypeRef>),
    /// The type is undetermined, with the reason named.
    Unknown(UnknownReason),
}

impl ResolvedType {
    /// The type, where there is exactly one.
    #[must_use]
    pub fn known(&self) -> Option<&TypeRef> {
        match self {
            Self::Known(ty) => Some(ty),
            _ => None,
        }
    }

    /// Every candidate: one for [`Self::Known`], however many for
    /// [`Self::Ambiguous`], none for [`Self::Unknown`].
    #[must_use]
    pub fn candidates(&self) -> &[TypeRef] {
        match self {
            Self::Known(ty) => std::slice::from_ref(ty),
            Self::Ambiguous(types) => types,
            Self::Unknown(_) => &[],
        }
    }

    /// Collapses a candidate list into an answer: empty gives `Unknown` with
    /// the stated reason, one gives `Known`, more gives `Ambiguous`.
    #[must_use]
    pub fn from_candidates(mut candidates: Vec<TypeRef>, if_empty: UnknownReason) -> Self {
        candidates.dedup_by(|a, b| a.idx == b.idx && a.display == b.display);
        match candidates.len() {
            0 => Self::Unknown(if_empty),
            1 => Self::Known(candidates.remove(0)),
            _ => Self::Ambiguous(candidates),
        }
    }
}

/// The struct name behind a type, directly or through an alias.
///
/// An alias is unwrapped because `type AllowedMessage = A | B` and a
/// `StructRef` are the same thing to a consumer: it is asking what to call a
/// method on.
fn struct_name_of(symbols: &SymbolTypes, descriptor: &TypeDescriptor) -> Option<String> {
    match descriptor {
        TypeDescriptor::StructRef { struct_name, .. } => Some(struct_name.clone()),
        TypeDescriptor::AliasRef { alias_name, .. } => {
            // An alias points at its target through `declarations` rather
            // than through the descriptor, so unwrapping goes by that table.
            let target = symbols.declarations.iter().find_map(|decl| match decl {
                Declaration::Alias {
                    name,
                    target_ty_idx,
                    ..
                } if name == alias_name => Some(*target_ty_idx),
                _ => None,
            })?;
            let target_descriptor = symbols.unique_types.get(target as usize)?;
            // One level of unwrapping: chains of alias upon alias have not
            // been observed in real compiler output, and recursion here would
            // risk a cycle for a shape that does not exist.
            match target_descriptor {
                TypeDescriptor::StructRef { struct_name, .. } => Some(struct_name.clone()),
                _ => None,
            }
        }
        _ => None,
    }
}

/// A human-readable name for a type.
fn describe(symbols: &SymbolTypes, descriptor: &TypeDescriptor) -> String {
    match descriptor {
        TypeDescriptor::Void => "void".to_string(),
        TypeDescriptor::Int => "int".to_string(),
        TypeDescriptor::Slice => "slice".to_string(),
        TypeDescriptor::Cell => "cell".to_string(),
        TypeDescriptor::Builder => "builder".to_string(),
        TypeDescriptor::Bool => "bool".to_string(),
        TypeDescriptor::Coins => "coins".to_string(),
        TypeDescriptor::Address => "address".to_string(),
        TypeDescriptor::AddressOpt => "address?".to_string(),
        TypeDescriptor::AddressAny => "MsgAddress".to_string(),
        TypeDescriptor::NullLiteral => "null".to_string(),
        TypeDescriptor::Callable => "callable".to_string(),
        TypeDescriptor::String => "string".to_string(),
        TypeDescriptor::Remaining => "RemainingBitsAndRefs".to_string(),
        TypeDescriptor::Unknown => "unknown".to_string(),
        TypeDescriptor::IntN { n } => format!("int{n}"),
        TypeDescriptor::UintN { n } => format!("uint{n}"),
        TypeDescriptor::StructRef { struct_name, .. } => struct_name.clone(),
        TypeDescriptor::AliasRef { alias_name, .. } => alias_name.clone(),
        other => describe_composite(symbols, other),
    }
}

/// Composite and not-yet-modelled kinds live apart so that [`describe`] reads
/// as a table of primitives.
fn describe_composite(symbols: &SymbolTypes, descriptor: &TypeDescriptor) -> String {
    match descriptor {
        TypeDescriptor::Nullable { inner_ty_idx, .. } => symbols
            .unique_types
            .get(*inner_ty_idx as usize)
            .map_or_else(
                || "?".to_string(),
                |inner| format!("{}?", describe(symbols, inner)),
            ),
        TypeDescriptor::Union { .. } => "union".to_string(),
        _ => "<not modelled>".to_string(),
    }
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "docs/ARCHITECTURE.md §3.2 scopes the no-unwrap/expect rule to library code outside tests"
)]
mod tests {
    use super::{ResolvedType, TypeRef, UnknownReason};
    use sting_tolkc::SymbolTypes;

    /// A minimal symbol table: two primitives to build a `TypeRef` from and
    /// nothing else, since collapsing candidates does not depend on which types
    /// they are.
    fn symbols() -> SymbolTypes {
        serde_json::from_str(
            r#"{"files":[],"unique_types":[{"kind":"int"},{"kind":"slice"}],
                "declarations":[],"functions":[]}"#,
        )
        .expect("the minimal artefact parses")
    }

    #[test]
    fn no_candidates_becomes_unknown_with_the_given_reason() {
        let resolved = ResolvedType::from_candidates(Vec::new(), UnknownReason::UnresolvedCall);
        assert_eq!(
            resolved,
            ResolvedType::Unknown(UnknownReason::UnresolvedCall)
        );
        assert!(resolved.candidates().is_empty());
        assert!(resolved.known().is_none());
    }

    #[test]
    fn one_candidate_becomes_known() {
        let symbols = symbols();
        let int = TypeRef::from_index(&symbols, 0).expect("type 0 exists");
        let resolved = ResolvedType::from_candidates(vec![int], UnknownReason::UnresolvedCall);
        assert_eq!(
            resolved.known().map(TypeRef::display_name),
            Some("int"),
            "one candidate is knowledge, not ambiguity"
        );
        assert_eq!(resolved.candidates().len(), 1);
    }

    /// `Ambiguous` has to be distinguishable from `Unknown`: a consumer is
    /// entitled to check every candidate, and that is exactly what today's
    /// `AC` gives up with its "none or two or more, so surrender".
    #[test]
    fn two_candidates_become_ambiguous_not_unknown() {
        let symbols = symbols();
        let int = TypeRef::from_index(&symbols, 0).expect("type 0 exists");
        let slice = TypeRef::from_index(&symbols, 1).expect("type 1 exists");
        let resolved =
            ResolvedType::from_candidates(vec![int, slice], UnknownReason::UnresolvedCall);
        assert!(
            matches!(resolved, ResolvedType::Ambiguous(_)),
            "two different types are ambiguity, not ignorance: {resolved:?}"
        );
        assert_eq!(resolved.candidates().len(), 2);
        assert!(
            resolved.known().is_none(),
            "ambiguity does not pass itself off as knowledge"
        );
    }

    /// The same type arriving by two paths is not ambiguity.
    #[test]
    fn duplicate_candidates_collapse_to_known() {
        let symbols = symbols();
        let a = TypeRef::from_index(&symbols, 0).expect("type 0 exists");
        let b = TypeRef::from_index(&symbols, 0).expect("type 0 exists");
        let resolved = ResolvedType::from_candidates(vec![a, b], UnknownReason::UnresolvedCall);
        assert_eq!(resolved.known().map(TypeRef::display_name), Some("int"));
    }

    #[test]
    fn a_type_index_outside_the_compiler_table_is_no_type_at_all() {
        let symbols = symbols();
        assert!(
            TypeRef::from_index(&symbols, 999).is_none(),
            "an index outside the table means a foreign artefact, not a missing type"
        );
    }
}
