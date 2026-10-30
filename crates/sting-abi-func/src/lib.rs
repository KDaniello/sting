//! Tier 2 extractor: parses `FunC` source with `tree-sitter` (grammar
//! confirmed working — see `tests/parse_real_func_samples.rs` and
//! `vendor/tree-sitter-func/README.md`) and heuristically builds a
//! `sting_abi_core::ContractSchema` with an attached
//! `sting_abi_core::ExtractionConfidence`.
//!
//! **The research this extractor stands on**, conducted before any of its
//! code, on 2026-07-21 against a 38-file real corpus (5 already-vendored
//! fixtures + `ton-blockchain/token-contract` + `ton-blockchain/
//! wallet-contract` + `ton-blockchain/ton`'s `crypto/smartcont/`). Findings
//! that shaped this crate's scope, in full:
//! - No macro-based dispatch found anywhere (0/38) — the block spec's own
//!   flagged risk, refuted by evidence, not assumed.
//! - `load_data()`/`save_data()` wrapper functions exist in only ~55% of
//!   contracts with storage; the rest access `get_data()`/`set_data()`
//!   inline at multiple call sites with no single recoverable field list.
//! - Real `recv_internal` message-body field parsing is almost universally
//!   delegated to a helper function one call away (`send_tokens(...)`,
//!   `receive_tokens(...)`) — recovering it would need interprocedural
//!   resolution, the same structural limit this project's security-branch
//!   v1 detectors already accept for other reasons.
//!
//! **What v1 extracts** (`docs/abi-layer/blocks/
//! 04-func-heuristic-extractor.md` §4.2-4.3):
//! - **Level 1 — get-methods**: every top-level `method_id`-specified
//!   `function_definition`, mirroring `ton-blockchain/verifier`'s
//!   `getterParser.ts` precedent at the same precision (raw type text, not
//!   a resolved type system — `FunC` doesn't have one beyond a handful of
//!   primitives). High confidence (proven precedent), not `1.0`.
//! - **Level 2 — storage**: two forms, tried in order, never both at once
//!   (`docs/domains/method-dictionary.md` added the
//!   second, 2026-07-22). **Paired `load_data()`+`save_data(...)`**
//!   wrapper form first (the ~55% of the research corpus that has it) —
//!   field types come from `load_data`'s `~load_*` call chain, field
//!   *names* from `save_data`'s named parameters (matched positionally;
//!   `load_data` alone never names its returned fields). If no paired
//!   form is found, **inline `get_data()`/`set_data()`**
//!   (`storage::extract_inline_storage`) is tried as a fallback: a
//!   same-function read (`get_data().begin_parse()` bound to a name, then
//!   tuple-destructured from that exact name via `~load_*` calls) paired
//!   with a same-function write (`set_data(begin_cell()...store_*(...)
//!   ...end_cell())`) with matching arity — field names from the read
//!   side's bound tuple names, types from its `~load_*` calls, same "types
//!   only from the load side" trust boundary as the paired form. Deferred
//!   entirely (not a guess) when a same-function pair isn't found, or
//!   when more than one function has a *disagreeing* pair — see that
//!   function's own doc comment. Markedly lower confidence than Level 1
//!   either way, and the inline form is markedly lower confidence again
//!   than the paired form (a structurally weaker signal — an ordinary
//!   function that also does other work, not two functions whose whole
//!   purpose is storage).
//!
//! **What this does not extract, and why** — the list is explicit because a
//! silent gap is indistinguishable from an absent problem:
//! - **Message-body fields** (`incoming_internal`/`incoming_external`
//!   stay empty) — the research above found real per-op field parsing is
//!   almost always one interprocedural hop away; even a narrower "just the
//!   op-code dispatch table, no fields" attempt would need matching flat,
//!   un-nested `if`/`elseif` condition token sequences (this grammar has
//!   no `binary_expression` node — `if_statement`'s `condition` field is a
//!   flat sequence of operand/operator tokens, confirmed by reading
//!   `vendor/tree-sitter-func/src/node-types.json`), for output of
//!   markedly lower value than storage fields. Scoped out entirely, not
//!   attempted at low confidence.
//! - **Cross-call-site reconciliation of partial storage reads** — e.g. a
//!   contract whose only storage access is two different get-methods each
//!   reading a *different slice* of the same layout independently (real
//!   example already vendored: `tests/fixtures/wallet3-code.fc`'s
//!   `seqno()`/`get_public_key()`) — `storage::extract_inline_storage`
//!   only recognizes a same-function read+write pair, deliberately, per
//!   `docs/domains/method-dictionary.md`'s own v1 scope
//!   limit.
//! - **Storage via `impure` global variables as primary state** — the
//!   block spec's own flagged candidate exclusion; not observed at all in
//!   the research corpus (1/38 files used `global`, and only for
//!   transient scratch state alongside a proper `load_data`/`set_data`
//!   pair) — narrowly relevant, not a real v1 gap in practice.
//! - **Interprocedural resolution of any kind** — every heuristic here is
//!   single-function, matching this project's existing v1 structural
//!   limit for exactly this class of analysis.

mod method_id;
mod methods;
mod storage;
mod type_text;

pub use method_id::default_method_id;

use std::path::Path;

use sting_abi_core::{
    ContractMetadata, ContractSchema, ExtractError, ExtractionConfidence, ExtractionTier,
    Extractor, FieldConfidence, GetMethod, NamedType, Param, SourceLanguage, TypeRef, TypeShape,
};

/// Confidence assigned to every Level 1 (get-method) extraction — high
/// (this crate mirrors a proven, production precedent at the same
/// precision) but deliberately not `1.0`: a pure heuristic can't offer a
/// compiler's guarantee. Calibrated against a labelled corpus rather than
/// asserted final here.
const GET_METHODS_CONFIDENCE: f32 = 0.85;

/// Confidence assigned to a recognized Level 2 storage form — markedly
/// lower than [`GET_METHODS_CONFIDENCE`], because a get method is knowable
/// more confidently than a storage field: there is no compiler guarantee at
/// all, and the `load_data`/`save_data` field-name and type
/// correlation is itself a heuristic, not a language-level guarantee.
const STORAGE_CONFIDENCE: f32 = 0.5;

/// Confidence assigned to storage recognized via a paired wrapper-function
/// form under names *other than* the literal `load_data`/`save_data`
/// ([`storage::extract_named_pair_storage`]) — between
/// [`STORAGE_CONFIDENCE`] and [`INLINE_STORAGE_CONFIDENCE`]: still two
/// *dedicated* functions, one for read, one for write (the same structural
/// strength as the literal-name form), but without the specific-name
/// convention's own confirming signal. **Not yet calibrated** — a starting
/// value, same caveat as [`INLINE_STORAGE_CONFIDENCE`].
const NAMED_PAIR_STORAGE_CONFIDENCE: f32 = 0.45;

/// Confidence assigned to storage recognized via the inline
/// `get_data()`/`set_data()` form ([`storage::extract_inline_storage`]),
/// deliberately lower than [`STORAGE_CONFIDENCE`] — that form correlates
/// two *named* functions (`load_data`/`save_data`) whose whole purpose is
/// storage; this form infers the same thing from an ordinary function
/// that also does other work, a structurally weaker signal. **Not yet
/// calibrated against a labeled corpus** — a starting value, per
/// `docs/domains/method-dictionary.md`, explicitly
/// flagged for revisiting once real accuracy numbers exist (the same
/// `ExtractionConfidence` discipline this crate already applies to
/// [`GET_METHODS_CONFIDENCE`]/[`STORAGE_CONFIDENCE`]).
const INLINE_STORAGE_CONFIDENCE: f32 = 0.4;

/// Builds an [`ExtractionConfidence`] from a value this crate itself
/// controls (a fixed module constant) — never from anything derived at
/// runtime, which is what makes the `unreachable!` genuinely unreachable
/// rather than a disguised `.unwrap()`. A unit test below pins both module
/// constants as in-range, so a future typo (e.g. `1.5`) fails in CI, not
/// silently here.
fn fixed_confidence(value: f32) -> ExtractionConfidence {
    ExtractionConfidence::new(value)
        .unwrap_or_else(|_| unreachable!("{value} must be a valid literal in [0.0, 1.0]"))
}

/// Deduplicates [`TypeShape`]s into one [`ContractSchema::types`] table by
/// linear scan (`TypeShape` isn't `Hash`, and this crate only ever interns
/// a handful of distinct shapes per contract — a `HashMap`-based cache
/// would be pure overhead here).
#[derive(Default)]
struct TypeInterner {
    types: Vec<NamedType>,
}

impl TypeInterner {
    fn intern(&mut self, shape: TypeShape) -> TypeRef {
        if let Some(index) = self.types.iter().position(|existing| {
            existing.name.is_none()
                && existing.description.is_none()
                && existing.prefix.is_none()
                && existing.shape == shape
        }) {
            return TypeRef(u32::try_from(index).unwrap_or(u32::MAX));
        }
        let index = self.types.len();
        self.types.push(NamedType {
            name: None,
            description: None,
            prefix: None,
            shape,
        });
        TypeRef(u32::try_from(index).unwrap_or(u32::MAX))
    }

    /// Adds a *named* type (e.g. the storage struct) without deduplicating
    /// against anonymous entries — a named type is never interchangeable
    /// with a same-shaped anonymous one.
    fn push_named(&mut self, name: String, shape: TypeShape) -> TypeRef {
        let index = self.types.len();
        self.types.push(NamedType {
            name: Some(name),
            description: None,
            prefix: None,
            shape,
        });
        TypeRef(u32::try_from(index).unwrap_or(u32::MAX))
    }
}

/// Interns a get-method's already-split return-type components (see
/// [`methods::RawGetMethod::return_shapes`]) into `interner`, producing the
/// single [`TypeRef`] a [`GetMethod::return_ty`] needs — `None` for an
/// empty list (unit return), the lone component's own ref for exactly one,
/// or a freshly-interned [`TypeShape::Tensor`] wrapping every component's
/// ref for more than one (a `FunC` get-method returning multiple stack
/// values, confirmed common in the research corpus).
fn intern_return_shapes(interner: &mut TypeInterner, shapes: Vec<TypeShape>) -> Option<TypeRef> {
    match shapes.len() {
        0 => None,
        1 => shapes
            .into_iter()
            .next()
            .map(|shape| interner.intern(shape)),
        _ => {
            let items = shapes
                .into_iter()
                .map(|shape| interner.intern(shape))
                .collect();
            Some(interner.intern(TypeShape::Tensor { items }))
        }
    }
}

/// The `FunC` Tier 2 heuristic extractor — see this module's own doc
/// comment for the full, research-backed scope of what it does and does
/// not recover.
#[derive(Debug, Clone, Copy, Default)]
pub struct FuncExtractor;

/// Three storage forms, tried in order, never more than one at once — a
/// schema never carries two disagreeing storage guesses. Literal
/// `load_data`/`save_data` first (highest confidence, ~55% of the research
/// corpus); then the same paired-function shape under any *other* name
/// ([`storage::extract_named_pair_storage`], the `load_storage` and
/// `save_storage` finding); the inline `get_data()`/`set_data()` form
/// ([`storage::extract_inline_storage`]) is the last-resort fallback.
fn extract_storage_with_confidence(
    root: tree_sitter::Node,
    source_bytes: &[u8],
) -> (Option<Vec<(String, TypeShape)>>, f32) {
    if let Some(fields) = storage::extract_storage(root, source_bytes) {
        return (Some(fields), STORAGE_CONFIDENCE);
    }
    if let Some(fields) = storage::extract_named_pair_storage(root, source_bytes) {
        return (Some(fields), NAMED_PAIR_STORAGE_CONFIDENCE);
    }
    (
        storage::extract_inline_storage(root, source_bytes),
        INLINE_STORAGE_CONFIDENCE,
    )
}

impl Extractor for FuncExtractor {
    fn language(&self) -> SourceLanguage {
        SourceLanguage::Func
    }

    fn extract(
        &self,
        project_dir: &Path,
        entrypoint: &Path,
    ) -> Result<ContractSchema, ExtractError> {
        let full_path = project_dir.join(entrypoint);
        let source =
            std::fs::read_to_string(&full_path).map_err(|source_err| ExtractError::Failed {
                entrypoint: entrypoint.to_path_buf(),
                message: format!("failed to read {}: {source_err}", full_path.display()),
                source: Some(Box::new(source_err)),
            })?;

        let mut parser = tree_sitter::Parser::new();
        parser
            .set_language(&tree_sitter_func::LANGUAGE.into())
            .map_err(|err| ExtractError::Failed {
                entrypoint: entrypoint.to_path_buf(),
                message: format!("failed to load the FunC grammar: {err}"),
                source: Some(Box::new(err)),
            })?;
        let tree = parser
            .parse(&source, None)
            .ok_or_else(|| ExtractError::Failed {
                entrypoint: entrypoint.to_path_buf(),
                message: "tree-sitter produced no parse tree".to_string(),
                source: None,
            })?;

        let mut interner = TypeInterner::default();
        let source_bytes = source.as_bytes();

        let raw_methods = methods::extract_get_methods(tree.root_node(), source_bytes);
        let has_get_methods = !raw_methods.is_empty();
        let get_methods: Vec<GetMethod> = raw_methods
            .into_iter()
            .map(|raw| GetMethod {
                tvm_method_id: Some(raw.tvm_method_id),
                name: raw.name,
                parameters: raw
                    .parameters
                    .into_iter()
                    .map(|(name, shape)| Param {
                        name,
                        ty: interner.intern(shape),
                    })
                    .collect(),
                return_ty: intern_return_shapes(&mut interner, raw.return_shapes),
            })
            .collect();

        let (raw_storage, storage_confidence) =
            extract_storage_with_confidence(tree.root_node(), source_bytes);
        let storage = raw_storage.map(|fields| {
            let field_refs = fields
                .into_iter()
                .map(|(name, shape)| sting_abi_core::FieldRef {
                    name,
                    ty: interner.intern(shape),
                })
                .collect();
            interner.push_named(
                "Storage".to_string(),
                TypeShape::Struct { fields: field_refs },
            )
        });

        let field_confidence = FieldConfidence {
            get_methods: has_get_methods.then(|| fixed_confidence(GET_METHODS_CONFIDENCE)),
            storage: storage
                .is_some()
                .then(|| fixed_confidence(storage_confidence)),
        };
        // The lower of whichever categories were actually populated — a
        // schema with only high-confidence get-methods reports that
        // directly; one that also recognized storage is only as confident
        // as its least-confident recognized category, never inflated by
        // averaging a low-confidence category away.
        let overall_confidence = [field_confidence.get_methods, field_confidence.storage]
            .into_iter()
            .flatten()
            .min_by(|a, b| a.value().total_cmp(&b.value()))
            .unwrap_or_else(|| fixed_confidence(0.0));

        let schema = ContractSchema {
            ir_schema_version: sting_abi_core::IR_SCHEMA_VERSION.to_string(),
            source_language: SourceLanguage::Func,
            extraction_tier: ExtractionTier::Heuristic,
            contract_name: None,
            metadata: ContractMetadata::default(),
            storage,
            types: interner.types,
            incoming_internal: Vec::new(),
            incoming_external: Vec::new(),
            outgoing: Vec::new(),
            get_methods,
            errors: Vec::new(),
            confidence: Some(overall_confidence),
            field_confidence: Some(field_confidence),
        };

        schema
            .validate()
            .map_err(|invariant_error| ExtractError::Failed {
                entrypoint: entrypoint.to_path_buf(),
                message: format!(
                    "internal bug: extracted schema violates its own invariant: {invariant_error}"
                ),
                source: None,
            })?;

        Ok(schema)
    }
}
