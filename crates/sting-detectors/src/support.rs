//! Small tree-walking helpers shared by more than one
//! detector (`docs/domains/detectors.md`) — promoted here once a
//! second detector needed the same shape of lookup, not written speculatively
//! ahead of a real second consumer.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;

use sting_ast::{AstStore, ContractDef, FunctionDef, Language, SourceRange};
use sting_tolkc::ProjectStore;
use tree_sitter::Node;

use crate::DetectorContext;

/// Re-locates `function`'s `block_statement` body node via `ctx`'s tree.
/// `None` for a declaration with no body (`asm`/`builtin`) or one that, for
/// whatever reason, cannot be re-located (should not happen for a range
/// `AstStore` itself produced from this same tree, but detectors must not
/// panic on untrusted input either way — `docs/ARCHITECTURE.md` §9).
pub(crate) fn function_body<'tree>(
    function: &FunctionDef,
    ctx: &'tree DetectorContext,
) -> Option<Node<'tree>> {
    function
        .range
        .locate(ctx.tree())?
        .child_by_field_name("body")
        .filter(|b| b.kind() == "block_statement")
}

/// The direct statement children of a `block_statement` node, in source
/// order. `_statement`/`_statement_require_semicolon_unless_last` are hidden
/// (`_`-prefixed) grammar rules, so a `block_statement`'s named children
/// already *are* the actual statement nodes (`if_statement`,
/// `assert_statement`, `expression_statement`, ...) — confirmed against real
/// parses, confirmed by inspecting the real trees these detectors run over.
pub(crate) fn block_children(block: Node<'_>) -> Vec<Node<'_>> {
    let mut cursor = block.walk();
    block.named_children(&mut cursor).collect()
}

/// One `<obj>.<method>(<args>)` call, unwound from its
/// `function_call(callee: dot_access(obj, field))` shape (confirmed via the
/// same diagnostic as [`block_children`]; `dot_access` is also how plain
/// field reads parse, so this only matches when the call is actually present
/// one level up).
pub(crate) struct MethodCall<'tree> {
    /// The receiver expression (left of the dot).
    pub obj: Node<'tree>,
    /// The method name, e.g. `"assertEnd"`.
    pub method: String,
    /// The method-name identifier node itself — usually the most precise
    /// span to attach a [`sting_core::Finding`] to.
    pub method_node: Node<'tree>,
    /// The call's `argument_list` node.
    pub args: Node<'tree>,
    /// Whether the call writes its result back into the receiver, when the grammar says so at the
    /// call site. `Some` in `FunC`, where `~f(…)` writes back and `.f(…)` does not; `None` in Tolk,
    /// where the answer lives in the callee's declaration — `mutate self` — and a call site cannot
    /// be read for it.
    pub receiver_written: Option<bool>,
}

/// Unwinds a method call in whichever grammar `language` names.
///
/// The two shape it differently enough that one reader cannot serve both. Tolk nests the receiver
/// inside the call, as the `obj` of a `dot_access` callee. `FunC` does not nest it at all: a
/// `method_call` node carries only the name and the arguments, and the receiver is the sibling
/// immediately before it — which is also what makes a chain work, since the receiver of the second
/// call in `begin_cell().store_uint(…).end_cell()` is the first call.
///
/// `None` for anything that is not a method call, including a plain free-function call.
pub(crate) fn as_method_call<'tree>(
    node: Node<'tree>,
    source: &str,
    language: Language,
) -> Option<MethodCall<'tree>> {
    match language {
        Language::Tolk => as_tolk_method_call(node, source),
        Language::Func => as_func_method_call(node, source),
    }
}

/// Tolk: `<obj>.<method>(<args>)` is a `function_call` whose callee is a `dot_access`.
fn as_tolk_method_call<'tree>(node: Node<'tree>, source: &str) -> Option<MethodCall<'tree>> {
    if node.kind() != "function_call" {
        return None;
    }
    let callee = node.child_by_field_name("callee")?;
    if callee.kind() != "dot_access" {
        return None;
    }
    let obj = callee.child_by_field_name("obj")?;
    let method_node = callee.child_by_field_name("field")?;
    let method = method_node.utf8_text(source.as_bytes()).ok()?.to_string();
    let args = node.child_by_field_name("arguments")?;
    Some(MethodCall {
        obj,
        method,
        method_node,
        args,
        receiver_written: None,
    })
}

/// `FunC`: a `method_call` node holding the name and the arguments, preceded by its receiver.
///
/// The leading character of the node's own text is the whole of what the language says about
/// mutation: `~` assigns the call's first result back into the receiver, `.` leaves it alone.
fn as_func_method_call<'tree>(node: Node<'tree>, source: &str) -> Option<MethodCall<'tree>> {
    if node.kind() != "method_call" {
        return None;
    }
    let obj = node.prev_named_sibling()?;
    let method_node = node.child_by_field_name("method_name")?;
    let method = method_node.utf8_text(source.as_bytes()).ok()?.to_string();
    let args = node.child_by_field_name("arguments")?;
    let receiver_written = node
        .utf8_text(source.as_bytes())
        .ok()
        .map(|text| text.starts_with('~'));
    Some(MethodCall {
        obj,
        method,
        method_node,
        args,
        receiver_written,
    })
}

/// The `index`-th (0-based) `call_argument`'s expression text inside an
/// `argument_list` node, trimmed — `None` if there is no argument at that
/// index or its text cannot be read. Generalizes what `id.rs`'s original
/// `first_arg_text` did for index `0` only; `crate::store_load` also needs
/// index `1` (a `storeUint(value, width)` call's width) once `TLB` needed to
/// inspect a second argument no existing caller had needed before.
pub(crate) fn nth_call_argument_text(args: Node, index: usize, source: &str) -> Option<String> {
    let mut cursor = args.walk();
    args.named_children(&mut cursor)
        .filter(|c| c.kind() == "call_argument")
        .nth(index)
        .and_then(|ca| ca.child_by_field_name("expr"))
        .and_then(|e| e.utf8_text(source.as_bytes()).ok())
        .map(|s| s.trim().to_string())
}

/// Whether `name` is referenced anywhere inside `node`'s subtree, as a plain
/// identifier — used wherever a detector needs to confirm some later
/// expression actually derives from a specific earlier-bound name, not just
/// a same-named-but-unrelated thing (`gvr.rs`'s write-back exemption,
/// `uta.rs`'s asymmetric-check detection; promoted here once the second
/// detector needed the identical shape, this module's own stated norm).
pub(crate) fn subtree_references_identifier(node: Node, name: &str, source: &str) -> bool {
    sting_ast::Preorder::from_node(node)
        .map(|v| v.node)
        .any(|n| identifier_text(n, source).as_deref() == Some(name))
}

/// If `node` is a call to `createMessage(...)`, its (sole) `object_literal`
/// argument.
///
/// Deliberately **not** "any `object_literal` seen" — a message body is
/// itself an `object_literal` (`body: SomeMessage { queryId: 1 }`, see
/// `ubm.rs`'s golden fixtures), nested inside `createMessage`'s own argument
/// literal. Matching on `object_literal` unconditionally made a `Preorder`
/// walk visit that inner literal right after the real one and overwrite
/// `last_message_is_safe` back to "unsafe" (it has no `bounce` field) even
/// when the real `createMessage({ bounce: BounceMode.NoBounce, ... })`
/// literal was correctly read moments earlier — caught by
/// `corpus/fixtures/detectors/UBM/negative/02-no-bounce-flag-set.tolk`
/// wrongly firing during `ubm.rs`'s implementation, not assumed away.
/// Promoted here once `deprecated.rs` needed the identical shape (its own
/// `bounce: false` — the deprecated `OldBounceMode` spelling — check).
pub(crate) fn create_message_argument<'tree>(
    node: Node<'tree>,
    source: &str,
) -> Option<Node<'tree>> {
    if node.kind() != "function_call" {
        return None;
    }
    let callee = node.child_by_field_name("callee")?;
    if identifier_text(callee, source).as_deref() != Some("createMessage") {
        return None;
    }
    let args = node.child_by_field_name("arguments")?;
    let mut cursor = args.walk();
    args.named_children(&mut cursor)
        .find(|c| c.kind() == "call_argument")
        .and_then(|ca| ca.child_by_field_name("expr"))
        .filter(|e| e.kind() == "object_literal")
}

/// `node`'s text if `node` is a plain `identifier` — `None` for anything
/// else (a `dot_access`, a literal, ...), since those need their own
/// unwinding, not a text dump.
pub(crate) fn identifier_text(node: Node, source: &str) -> Option<String> {
    if node.kind() != "identifier" {
        return None;
    }
    node.utf8_text(source.as_bytes())
        .ok()
        .map(ToString::to_string)
}

/// Every `var_declaration` name bound anywhere inside `scope` (typically a
/// `var_declaration_lhs` node), paired with its name node for precise
/// [`sting_core::Location`]s. Walking from `var_declaration_lhs` rather than
/// matching `var_declaration` directly across a whole function handles both
/// `var x = ...` (one direct child) and destructuring `var (a, b) = ...`
/// (multiple `var_declaration`s nested under a `tensor_vars_declaration`)
/// uniformly — confirmed shapes, see this module's top-level doc comment.
pub(crate) fn bound_names<'tree>(scope: Node<'tree>, source: &str) -> Vec<(Node<'tree>, String)> {
    sting_ast::Preorder::from_node(scope)
        .map(|v| v.node)
        .filter(|n| n.kind() == "var_declaration")
        .filter_map(|n| {
            let name_node = n.child_by_field_name("name")?;
            let text = name_node.utf8_text(source.as_bytes()).ok()?.to_string();
            Some((name_node, text))
        })
        .collect()
}

/// The `kind` field ("var" or "val") of a `var_declaration_lhs` node.
pub(crate) fn declaration_kind(var_decl_lhs: Node, source: &str) -> Option<String> {
    var_decl_lhs
        .child_by_field_name("kind")?
        .utf8_text(source.as_bytes())
        .ok()
        .map(ToString::to_string)
}

/// A `match_arm`'s body node, whatever shape it took.
///
/// **Not** `arm.child_by_field_name("body")` — despite `grammar.js` nesting
/// `field("body", choice(field("block", ...), field("return", ...),
/// field("throw", ...), field("expr", ...)))`, tree-sitter only actually
/// registers the *innermost* field name for a node wrapped by nested
/// `field()` calls like this, not every enclosing one. Confirmed against a
/// real parse (`arm.child_by_field_name("body")` is `None`,
/// `arm.child_by_field_name("block")` is `Some`) via
/// `crates/sting-ast/examples/check_match_arm_fields.rs` during this
/// block's implementation, after `cfg.rs`'s CFG builder silently dropped
/// every match arm's statements by trusting the (wrong) `"body"` name —
/// caught by this crate's own CFG-shape tests, not assumed away.
pub(crate) fn match_arm_body(arm: Node<'_>) -> Option<Node<'_>> {
    arm.child_by_field_name("block")
        .or_else(|| arm.child_by_field_name("return"))
        .or_else(|| arm.child_by_field_name("throw"))
        .or_else(|| arm.child_by_field_name("expr"))
}

/// Builds a [`sting_core::Location`] for `node`'s own span (as opposed to
/// [`DetectorContext::location`], which takes an already-captured
/// [`SourceRange`]) — the common case for a detector pointing at a
/// specific live node it just found mid-walk.
pub(crate) fn location_of(node: Node, ctx: &DetectorContext) -> sting_core::Location {
    ctx.location(SourceRange::from_node(&node))
}

/// Strips redundant parentheses (`((x.isFound))` → `x.isFound`) — needed
/// wherever a condition might be parenthesized (`if`/`while`/`assert`
/// conditions, `&&` operands). Promoted here from `ur.rs` once `ac.rs` needed
/// the identical shape (this module's own doc comment's stated precedent).
pub(crate) fn unwrap_parens(mut node: Node) -> Node {
    while node.kind() == "parenthesized_expression"
        && let Some(inner) = node.child_by_field_name("inner")
    {
        node = inner;
    }
    node
}

/// Whether `node` unconditionally exits via a single `return`/`throw`
/// statement — either directly, or as a block's *only* statement (`comment`
/// nodes aside — `comment` is a *named* node in this grammar, so it must be
/// filtered before counting "how many statements", or the single most
/// common real early-return-guard style in this corpus — a `//` comment
/// immediately before the `return` — would never be recognized).
/// Deliberately narrow otherwise: a longer block that still exits on every
/// path (e.g. ending in an `if`/`else` that both return) is not recognized.
/// Promoted here from `ur.rs` once `ac.rs` needed the identical shape.
pub(crate) fn is_simple_exit_block(node: Node) -> bool {
    match node.kind() {
        "return_statement" | "throw_statement" => true,
        "block_statement" => {
            let mut real_statements = block_children(node)
                .into_iter()
                .filter(|s| s.kind() != "comment");
            matches!(
                (real_statements.next(), real_statements.next()),
                (Some(stmt), None) if matches!(stmt.kind(), "return_statement" | "throw_statement")
            )
        }
        _ => false,
    }
}

/// Every contract header recovered from this file, sorted ascending by
/// header start byte — the candidate list [`nearest_contract_for`] picks
/// from. Every per-contract detector (`GVR`, `ID`, `UBM`) needs this exact
/// same ordering; factored out here (rather than duplicated per detector)
/// after `GVR`'s own inline version turned out to need the identical shape
/// for `ID` and `UBM` too — the same cross-contract-contamination bug class,
/// found independently in all three during a post-Block-09 corpus review.
pub(crate) fn contracts_in_source_order(store: &AstStore) -> Vec<&ContractDef> {
    let mut contracts: Vec<&ContractDef> = store.contracts().collect();
    contracts.sort_by_key(|contract| contract.range.start_byte);
    contracts
}

/// The contract whose header most closely *precedes* `position` in source
/// order — that byte offset's presumed owning contract. `None` if no
/// contract header appears before `position` at all (an unusual file
/// structure, not observed anywhere in this corpus, or a file with no
/// `contract{}` header whatsoever — e.g. a shared helper/library file).
/// `contracts_in_order` must already be sorted ascending by
/// `range.start_byte` ([`contracts_in_source_order`]).
///
/// Sound specifically because `sting-bench`'s `discover_merged_projects`
/// (`crates/sting-bench/src/corpus.rs`) concatenates whole files in source
/// order, and every real multi-contract directory in this corpus keeps each
/// contract's own `contract{}` header immediately before that contract's own
/// handler functions, inside that contract's own file (see `gvr.rs`'s
/// module doc for the concrete corpus example this was found against). A
/// function declared *before* any `contract{}` header at all has no
/// preceding contract to attribute it to and is left unattributed
/// (`None`) — the same "not silently wrong" choice made throughout this
/// module for other AST gaps, not a guess.
pub(crate) fn nearest_contract_for<'a>(
    position: usize,
    contracts_in_order: &[&'a ContractDef],
) -> Option<&'a ContractDef> {
    contracts_in_order
        .iter()
        .rfind(|contract| contract.range.start_byte <= position)
        .copied()
}

/// Loads and parses a *different* file than the one a detector's own
/// [`DetectorContext`] was built for — the cross-file counterpart of what
/// `sting-pipeline::LintController` already builds for the file actually
/// being linted, built on demand here for a callee a call site resolved
/// into via [`sting_tolkc::ProjectStore::resolve_call`]
/// Generalizes the
/// identical same-file-only lookup three detectors
/// (`ac.rs::classify_function`, `uta.rs`'s one-hop call-site walker,
/// `store_load.rs::classify_function`) already hand-roll — `ac.rs` is this
/// cache's first real consumer; the other two stay same-file-only, unaffected
/// by this cache existing.
///
/// Caches every file loaded during one top-level detector walk, keyed by
/// absolute path, so a helper file imported/reached more than once is read
/// and parsed from disk at most once. `None` is cached too, for a path that
/// failed to load or parse, so a broken import is not retried on every
/// reference either — matches every other gap in this crate's cross-file
/// story: fail closed to "unresolvable", never panic on untrusted input
/// (`docs/ARCHITECTURE.md` §9).
#[derive(Default)]
pub(crate) struct CrossFileCache {
    loaded: HashMap<PathBuf, Option<Rc<(AstStore, DetectorContext)>>>,
}

impl CrossFileCache {
    /// Loads (or returns the already-cached) `(AstStore, DetectorContext)`
    /// pair for `path`. The returned `DetectorContext` itself carries
    /// `project` forward (via [`DetectorContext::with_project`]), so a
    /// caller can keep resolving *further* cross-file calls from within the
    /// newly loaded file — cross-file resolution is not artificially capped
    /// at one hop, it recurses as far as real, unambiguous name resolution
    /// actually reaches.
    ///
    /// `None` if `path` cannot be read, cannot be parsed
    /// ([`sting_ast::parse`] only fails on a grammar-load error, not
    /// malformed source — see its own doc comment), or is not itself a
    /// known file in `project` (should not happen for a path
    /// [`sting_tolkc::ProjectStore::path_for_file_id`] itself just
    /// reported, but a caller must not assume a project snapshot can never
    /// be stale either way).
    pub(crate) fn get_or_load(
        &mut self,
        path: &Path,
        project: &Arc<ProjectStore>,
    ) -> Option<Rc<(AstStore, DetectorContext)>> {
        self.loaded
            .entry(path.to_path_buf())
            .or_insert_with(|| load_cross_file(path, project))
            .clone()
    }
}

fn load_cross_file(
    path: &Path,
    project: &Arc<ProjectStore>,
) -> Option<Rc<(AstStore, DetectorContext)>> {
    let source = std::fs::read_to_string(path).ok()?;
    let tree = sting_ast::parse(&source).ok()?;
    let store = AstStore::build(&tree, &source);
    let file_id = project.file_id_for_path(path)?;
    let ctx = DetectorContext::new(path.to_path_buf(), tree, source)
        .with_project(Arc::clone(project), file_id);
    Some(Rc::new((store, ctx)))
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "docs/ARCHITECTURE.md §3.2 scopes the no-unwrap/expect policy to library code outside tests"
)]
mod cross_file_cache_tests {
    use std::rc::Rc;
    use std::sync::Arc;

    use sting_tolkc::{FileEntry, ProjectStore, SymbolTypes};

    use super::CrossFileCache;

    fn project_with_one_file(path: &std::path::Path) -> Arc<ProjectStore> {
        Arc::new(ProjectStore::new(SymbolTypes {
            files: vec![FileEntry {
                file_id: 0,
                file_name: path.display().to_string(),
                size_chars: 0,
                imports: Vec::new(),
            }],
            unique_types: Vec::new(),
            struct_instantiations: Vec::new(),
            alias_instantiations: Vec::new(),
            declarations: Vec::new(),
            functions: Vec::new(),
        }))
    }

    #[test]
    fn get_or_load_reads_and_parses_a_real_file() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("helper.tolk");
        std::fs::write(&path, "fun helperFn() { return; }\n").expect("write");
        let project = project_with_one_file(&path);

        let mut cache = CrossFileCache::default();
        let loaded = cache
            .get_or_load(&path, &project)
            .expect("a real, valid file must load");
        assert!(loaded.0.function_by_name("helperFn").is_some());
        assert!(loaded.1.project_store().is_some());
    }

    #[test]
    fn get_or_load_caches_the_second_call() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("helper.tolk");
        std::fs::write(&path, "fun helperFn() { return; }\n").expect("write");
        let project = project_with_one_file(&path);

        let mut cache = CrossFileCache::default();
        let first = cache.get_or_load(&path, &project).expect("loads");
        let second = cache.get_or_load(&path, &project).expect("cached");
        assert!(
            Rc::ptr_eq(&first, &second),
            "the second call must return the same cached Rc, not re-read/re-parse the file"
        );
    }

    #[test]
    fn get_or_load_returns_none_for_a_missing_file() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("does-not-exist.tolk");
        let project = project_with_one_file(&path);

        let mut cache = CrossFileCache::default();
        assert!(cache.get_or_load(&path, &project).is_none());
    }

    #[test]
    fn get_or_load_returns_none_when_the_path_is_unknown_to_the_project() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("helper.tolk");
        std::fs::write(&path, "fun helperFn() { return; }\n").expect("write");
        // A project snapshot that knows nothing about this path at all.
        let empty_project = Arc::new(ProjectStore::new(SymbolTypes {
            files: Vec::new(),
            unique_types: Vec::new(),
            struct_instantiations: Vec::new(),
            alias_instantiations: Vec::new(),
            declarations: Vec::new(),
            functions: Vec::new(),
        }));

        let mut cache = CrossFileCache::default();
        assert!(cache.get_or_load(&path, &empty_project).is_none());
    }
}
