//! AST parsing for the two languages the chain runs on: `tree-sitter-tolk` and
//! `tree-sitter-func` wrappers, [`AstStore`], traversal.
//!
//! The two grammars describe the same kinds of program and describe them differently, so an
//! [`AstStore`] carries the [`Language`] it was built from. A detector that asks what a construct
//! *is* gets an answer; a detector that asks what a grammar *calls* it gets one answer per grammar
//! and no way to tell which it is holding.

use tree_sitter::{Node, Parser, Tree};

/// Which grammar a tree came from.
///
/// Carried rather than inferred from the tree: a `tree-sitter` tree is nodes and names, and the same
/// question — is this a call, does it write to its receiver — has a different answer per grammar.
/// Inferring it from node kinds would work until two grammars share a kind name, and then it would
/// be wrong silently.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Language {
    /// The language the toolchain is moving to.
    Tolk,
    /// The language the chain runs on today.
    Func,
}

impl Language {
    /// Which grammar reads a file with this extension, if any.
    ///
    /// The answer lives here because it is a fact about grammars and this is the crate that owns
    /// them. Every caller asks rather than repeating the list: a scan deciding what to walk, a
    /// corpus deciding what a fixture is written in, a reach table deciding what it can count as
    /// readable. Two spellings of the same rule drift, and the day they do, one of them counts a
    /// file the other never opens.
    ///
    /// `FunC` answers to two extensions. Both occur inside one published scope, so reading only one
    /// would leave files unexamined while reporting a clean run.
    #[must_use]
    pub fn of_extension(extension: &str) -> Option<Self> {
        match extension {
            "tolk" => Some(Self::Tolk),
            "func" | "fc" => Some(Self::Func),
            _ => None,
        }
    }

    /// Which grammar reads `path`, if any.
    #[must_use]
    pub fn of_path(path: &std::path::Path) -> Option<Self> {
        Self::of_extension(path.extension()?.to_str()?)
    }

    /// How the language is written in a report.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Tolk => "Tolk",
            Self::Func => "FunC",
        }
    }
}

/// Parses Tolk source text into a `tree-sitter` [`Tree`].
///
/// # Errors
/// Returns an error if the `tree-sitter-tolk` grammar fails to load into the
/// parser. Malformed/incomplete Tolk source is *not* an error here: tree-sitter
/// performs error-recovery parsing and represents unparseable regions as
/// `ERROR`/`MISSING` nodes in an otherwise-valid tree (see [`find_error_nodes`]).
pub fn parse(source: &str) -> Result<Tree, ParseError> {
    parse_as(Language::Tolk, source)
}

/// Parses source text in `language` into a `tree-sitter` [`Tree`].
///
/// # Errors
/// Returns an error if the grammar fails to load into the parser. Malformed source is *not* an
/// error here, for the reason [`parse`] gives.
pub fn parse_as(language: Language, source: &str) -> Result<Tree, ParseError> {
    let mut parser = Parser::new();
    let grammar = match language {
        Language::Tolk => tree_sitter_tolk::LANGUAGE,
        Language::Func => tree_sitter_func::LANGUAGE,
    };
    parser
        .set_language(&grammar.into())
        .map_err(ParseError::LanguageLoad)?;
    parser.parse(source, None).ok_or(ParseError::NoTreeProduced)
}

/// Errors from [`parse`].
#[derive(Debug, thiserror::Error)]
pub enum ParseError {
    /// The `tree-sitter-tolk` grammar could not be loaded into the parser
    /// (ABI version mismatch between the `tree-sitter` and `tree-sitter-tolk`
    /// crate versions in use).
    #[error("failed to load the tolk grammar into the parser: {0}")]
    LanguageLoad(#[source] tree_sitter::LanguageError),
    /// `tree-sitter` produced no tree at all (only possible with an explicit
    /// parse timeout/cancellation flag, which this wrapper does not set).
    #[error("tree-sitter returned no tree")]
    NoTreeProduced,
}

/// One node visited during a [`Preorder`] traversal, with its depth from the
/// tree root (root itself is depth 0).
#[derive(Debug, Clone, Copy)]
pub struct VisitedNode<'tree> {
    /// The visited node.
    pub node: Node<'tree>,
    /// Distance from the tree root along parent links.
    pub depth: usize,
}

/// Preorder (parent-before-children, depth-first) traversal over every node of
/// a `tree-sitter` tree, including anonymous/hidden nodes visible to a cursor.
///
/// Hand-rolled instead of depending on `tree-sitter-traversal`: that crate has
/// not been published since 2021 and its nominal successor,
/// `tree-sitter-traversal2`, is itself over a year stale (see
/// `docs/GLOSSARY.md#rust-tooling`). `tree_sitter::TreeCursor` already
/// exposes everything a preorder walk needs.
pub struct Preorder<'tree> {
    cursor: tree_sitter::TreeCursor<'tree>,
    depth: usize,
    done: bool,
}

impl<'tree> Preorder<'tree> {
    /// Starts a preorder traversal at the root of `tree`.
    #[must_use]
    pub fn new(tree: &'tree Tree) -> Self {
        Self::from_node(tree.root_node())
    }

    /// Starts a preorder traversal scoped to `node`'s own subtree (`node`
    /// itself, then every descendant) instead of a whole tree's root.
    ///
    /// The detectors (`docs/domains/detectors.md`) need this to
    /// inspect one function's body in isolation — e.g. `GVR` walking a
    /// single function looking for shadowing `var` declarations — without
    /// re-walking the whole file per function or hand-tracking "am I still
    /// inside the function I started from" during a whole-tree walk.
    #[must_use]
    pub fn from_node(node: Node<'tree>) -> Self {
        Self {
            cursor: node.walk(),
            depth: 0,
            done: false,
        }
    }
}

impl<'tree> Iterator for Preorder<'tree> {
    type Item = VisitedNode<'tree>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.done {
            return None;
        }

        let node = self.cursor.node();
        let depth = self.depth;

        if self.cursor.goto_first_child() {
            self.depth += 1;
        } else {
            while !self.cursor.goto_next_sibling() {
                if self.cursor.goto_parent() {
                    self.depth -= 1;
                } else {
                    self.done = true;
                    break;
                }
            }
        }

        Some(VisitedNode { node, depth })
    }
}

/// Every `ERROR` or `MISSING` node in `tree`, in preorder.
///
/// Used to diagnose grammar-coverage gaps against real Tolk source (see
/// `docs/ARCHITECTURE.md`) and to decide whether
/// a file is safe to hand to the detector framework at all.
#[must_use]
pub fn find_error_nodes(tree: &Tree) -> Vec<Node<'_>> {
    Preorder::new(tree)
        .map(|visited| visited.node)
        .filter(|node| node.is_error() || node.is_missing())
        .collect()
}

/// The upstream release `vendor/tree-sitter-tolk` was forked from.
///
/// Named rather than derived from the crate's own metadata: what is compiled in
/// is a fork, and reporting only the published number would be a half-truth
/// (`vendor/tree-sitter-tolk/README.md` lists every change).
pub const GRAMMAR_UPSTREAM_VERSION: &str = "tree-sitter-tolk 0.0.5";

/// The first region of a file the grammar could not read.
///
/// A parse that leaves an `ERROR` or `MISSING` node still returns a tree, and
/// every detector then runs over whatever did parse. Saying nothing about the
/// rest is the defect this type exists to prevent: a file the grammar read
/// half of must never be reportable as a file with nothing in it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnparsedRegion {
    /// The node's kind as `tree-sitter` names it: `ERROR` or `MISSING`.
    pub kind: &'static str,
    /// Where it starts and ends, lines and columns 1-indexed.
    pub range: SourceRange,
    /// How many such regions the whole file carries. One position is what a
    /// reader acts on; the count is what says whether the gap is a corner or
    /// half the file.
    pub total: usize,
}

/// The first [`UnparsedRegion`] of `tree`, or `None` when the grammar read the
/// whole file.
#[must_use]
pub fn first_unparsed(tree: &Tree) -> Option<UnparsedRegion> {
    let mut unreadable = Preorder::new(tree)
        .map(|visited| visited.node)
        .filter(|node| node.is_error() || node.is_missing());
    let first = unreadable.next()?;
    Some(UnparsedRegion {
        kind: first.kind(),
        range: SourceRange::from_node(&first),
        total: 1 + unreadable.count(),
    })
}

/// A source span, captured once at [`AstStore`]-build time from a live
/// `tree-sitter` [`Node`] so nothing downstream needs to re-derive line/column
/// from byte offsets. Lines/columns are 1-indexed, matching SARIF's `region`
/// convention (`docs/ARCHITECTURE.md` §6) — tree-sitter itself is 0-indexed,
/// so that conversion happens exactly once, here.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SourceRange {
    /// Start byte offset (0-indexed, tree-sitter convention — useful for
    /// `Node::utf8_text`-style slicing, unlike the line/column fields).
    pub start_byte: usize,
    /// End byte offset (exclusive), 0-indexed.
    pub end_byte: usize,
    /// First line, 1-indexed.
    pub start_line: u32,
    /// First column, 1-indexed.
    pub start_column: u32,
    /// Last line, 1-indexed.
    pub end_line: u32,
    /// Last column, 1-indexed.
    pub end_column: u32,
}

impl SourceRange {
    /// Captures `node`'s span.
    #[must_use]
    pub fn from_node(node: &Node) -> Self {
        let start = node.start_position();
        let end = node.end_position();
        let row_to_line = |row: usize| u32::try_from(row).unwrap_or(u32::MAX).saturating_add(1);
        let col_to_column = |col: usize| u32::try_from(col).unwrap_or(u32::MAX).saturating_add(1);
        Self {
            start_byte: node.start_byte(),
            end_byte: node.end_byte(),
            start_line: row_to_line(start.row),
            start_column: col_to_column(start.column),
            end_line: row_to_line(end.row),
            end_column: col_to_column(end.column),
        }
    }

    /// Builds a [`sting_core::Location`] by attaching a file path — the only
    /// piece of information a [`SourceRange`] does not carry itself (an
    /// `AstStore` is built from a single file, so its ranges don't repeat the
    /// path internally; see [`AstStore::build`]).
    #[must_use]
    pub fn with_file(self, file: std::path::PathBuf) -> sting_core::Location {
        sting_core::Location {
            file,
            start_line: self.start_line,
            start_column: self.start_column,
            end_line: self.end_line,
            end_column: self.end_column,
        }
    }

    /// Re-locates the live `tree-sitter` [`Node`] this range was captured
    /// from, given the same `tree` it came from — the inverse of
    /// [`SourceRange::from_node`].
    ///
    /// `AstStore` deliberately stores signatures (`FunctionDef`, `StructDef`,
    /// ...) without borrowed `Node`s, so it stays owned/`Send + Sync`
    /// (`AstStore`'s own doc comment). The detectors (`docs/domains/detectors.md`)
    /// need statement-level structure `AstStore` doesn't capture (e.g. GVR
    /// needs every `var` declaration inside a function's body) — this is the
    /// bridge back from an `AstStore`-held range to a live [`Node`] a
    /// [`Preorder::from_node`] walk can then descend into.
    #[must_use]
    pub fn locate<'tree>(&self, tree: &'tree Tree) -> Option<Node<'tree>> {
        tree.root_node()
            .descendant_for_byte_range(self.start_byte, self.end_byte)
    }
}

/// One parameter of a [`FunctionDef`]. `type_text` is `None` for an implicit
/// `self` receiver parameter (`fun Foo.bar(self)`), which has no `type` field
/// in the grammar (confirmed against real `method_declaration` nodes — see
/// `docs/domains/dataflow-cfg.md`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParamDef {
    /// Parameter name.
    pub name: String,
    /// Raw source text of the declared type, unparsed (typed representation
    /// is out of scope until a detector actually needs it).
    pub type_text: Option<String>,
    /// Whether the parameter is declared `mutate` (by-reference) — confirmed
    /// against a real parse tree (`parameter_declaration`'s first child is an
    /// anonymous `mutate` token, a sibling of `name`/`type`, not part of
    /// either field) during a post-Block-09 corpus review: a `mutate
    /// s: slice` parameter is a fundamentally different calling convention
    /// from a plain `s: slice` one — the callee is a cooperating sub-parser
    /// expected to consume only *part* of `s`, leaving the rest for the
    /// caller to keep reading after the call returns, not the sole/final
    /// consumer. `LepDetector` uses this to skip `mutate` parameters
    /// entirely (see `lep.rs`) instead of wrongly demanding `.assertEnd()`
    /// on a slice the callee was never meant to fully consume.
    pub is_mutate: bool,
}

/// Reads a `FunC` `parameter_list` into [`ParamDef`]s.
///
/// The type is the raw text of the `type` field: `slice`, `int`, `cell`, or a tensor or tuple
/// spelled out. Left unparsed for the same reason Tolk's is — a typed representation is worth
/// building when a detector needs one and not before.
fn func_parameters(function: &Node, source: &str) -> Vec<ParamDef> {
    let Some(list) = function.child_by_field_name("arguments") else {
        return Vec::new();
    };
    let mut parameters = Vec::new();
    let mut cursor = list.walk();
    for child in list.named_children(&mut cursor) {
        if child.kind() != "parameter_declaration" {
            continue;
        }
        let Some(name) = child
            .child_by_field_name("name")
            .and_then(|n| n.utf8_text(source.as_bytes()).ok())
        else {
            continue;
        };
        parameters.push(ParamDef {
            name: name.trim().to_string(),
            type_text: child
                .child_by_field_name("type")
                .and_then(|t| t.utf8_text(source.as_bytes()).ok())
                .map(|t| t.trim().to_string()),
            is_mutate: false,
        });
    }
    parameters
}

/// A `fun` declaration — either a free function (`function_declaration`) or a
/// method (`method_declaration`, `fun Type.name(...)`), unified here because
/// The detectors need to look up both the same way (e.g. finding a
/// `struct`'s `.load()`/`.save()` pair for the `ID` detector).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FunctionDef {
    /// Function/method name.
    pub name: String,
    /// `Some("MinterStorage")` for `fun MinterStorage.load()`, `None` for a
    /// free function.
    pub receiver_type: Option<String>,
    /// Declared parameters, in source order.
    pub parameters: Vec<ParamDef>,
    /// Span of the whole declaration, including its body.
    pub range: SourceRange,
}

/// One field of a [`StructDef`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FieldDef {
    /// Field name.
    pub name: String,
    /// Raw source text of the declared type, unparsed.
    pub type_text: String,
}

/// A `struct` declaration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StructDef {
    /// Struct name.
    pub name: String,
    /// Declared fields, in source order.
    pub fields: Vec<FieldDef>,
    /// Span of the whole declaration.
    pub range: SourceRange,
}

/// A `contract Name { ... }` block, read from its own node.
///
/// It carried a text scan until the vendored grammar gained the rule: the block
/// arrived as one opaque `ERROR` node, and two adjacent blocks arrived as one,
/// which lost the second contract entirely.
#[derive(Debug, Clone, PartialEq)]
pub struct ContractDef {
    /// Contract name.
    pub name: String,
    /// `key: value` fields from the block body — `author`,
    /// `incomingMessages`, `storage`, and any others. The set is not fixed by
    /// the language.
    pub fields: Vec<(String, String)>,
    /// Span of the whole header block.
    pub range: SourceRange,
}

/// An ID-indexed store of a single Tolk file's top-level definitions —
/// functions/methods, structs and contract blocks — built once per parse and
/// handed to detectors
/// by shared reference, mirroring Misti's `AstStore` (`docs/domains/dataflow-cfg.md`
/// §1.1; see that doc for why this owns its data instead of detectors walking
/// `tree_sitter::Node<'a>` directly: a `'static`-free, trivially `Send + Sync`
/// store is what lets the pipeline parallelise detectors over files with `rayon`
/// without every detector signature fighting the tree's borrow).
///
/// No explicit `NodeId`/slotmap here (unlike the sketch in
/// `docs/domains/dataflow-cfg.md`): nothing in the parser's or the detectors'
/// planned detectors needs to reference a definition indirectly (by id)
/// rather than just iterating/looking up by name, so an id type would be
/// unused, speculative API surface. Add one when a real consumer needs it.
/// Deliberately not [`Default`]: an empty store would have to claim a language it was never built
/// from, and a detector that branches on the wrong one is wrong quietly.
#[derive(Debug, Clone)]
pub struct AstStore {
    language: Language,
    functions: Vec<FunctionDef>,
    structs: Vec<StructDef>,
    contracts: Vec<ContractDef>,
}

impl AstStore {
    /// Builds a store from an already-parsed `tree` and its `source` text.
    ///
    /// Walks the *whole* tree (via [`Preorder`]), not just root's direct
    /// children: nothing in the grammar rules out a nested declaration, and
    /// scanning the full tree costs little for file-sized inputs while
    /// staying correct if that assumption ever turns out to be wrong.
    #[must_use]
    pub fn build(tree: &Tree, source: &str) -> Self {
        Self::build_as(Language::Tolk, tree, source)
    }

    /// Builds a store from a tree of `language`.
    ///
    /// # Panics
    /// Never: the match below is total over [`Language`].
    #[must_use]
    pub fn build_as(language: Language, tree: &Tree, source: &str) -> Self {
        match language {
            Language::Tolk => Self::build_tolk(tree, source),
            Language::Func => Self::build_func(tree, source),
        }
    }

    /// Which grammar this store was built from.
    #[must_use]
    pub const fn language(&self) -> Language {
        self.language
    }

    /// Builds a store from a `FunC` tree.
    ///
    /// `FunC` declares far less than Tolk does, and the store says so by leaving the rest empty
    /// rather than by inventing it. There are no structs and no contract blocks in the language, so
    /// those stay empty; there is no receiver type, because a method is a free function whose first
    /// parameter happens to be written to the left of the dot at the call site; and no parameter is
    /// ever `mutate`, because `FunC` decides whether a call writes back to its receiver at the call
    /// site — `~` against `.` — and not in the declaration.
    fn build_func(tree: &Tree, source: &str) -> Self {
        let mut functions = Vec::new();
        for visited in Preorder::new(tree) {
            let node = visited.node;
            if node.kind() != "function_definition" {
                continue;
            }
            let Some(name) = node
                .child_by_field_name("name")
                .and_then(|n| n.utf8_text(source.as_bytes()).ok())
            else {
                continue;
            };
            functions.push(FunctionDef {
                name: name.trim().to_string(),
                receiver_type: None,
                parameters: func_parameters(&node, source),
                range: SourceRange::from_node(&node),
            });
        }
        Self {
            language: Language::Func,
            functions,
            structs: Vec::new(),
            contracts: Vec::new(),
        }
    }

    /// Builds a store from a Tolk tree.
    fn build_tolk(tree: &Tree, source: &str) -> Self {
        let mut functions = Vec::new();
        let mut structs = Vec::new();
        let mut contracts = Vec::new();

        for visited in Preorder::new(tree) {
            let node = visited.node;
            match node.kind() {
                "function_declaration" => {
                    if let Some(def) = build_function_def(&node, source, None) {
                        functions.push(def);
                    }
                }
                "method_declaration" => {
                    let receiver_type = node
                        .child_by_field_name("receiver")
                        .and_then(|r| r.child_by_field_name("receiver_type"))
                        .and_then(|t| t.utf8_text(source.as_bytes()).ok())
                        .map(str::to_string);
                    if let Some(def) = build_function_def(&node, source, receiver_type) {
                        functions.push(def);
                    }
                }
                "struct_declaration" => {
                    if let Some(def) = build_struct_def(&node, source) {
                        structs.push(def);
                    }
                }
                "contract_declaration" => {
                    if let Some(def) = build_contract_def(&node, source) {
                        contracts.push(def);
                    }
                }
                _ => {}
            }
        }

        Self {
            language: Language::Tolk,
            functions,
            structs,
            contracts,
        }
    }

    /// All functions and methods, in the order they were encountered.
    pub fn functions(&self) -> impl Iterator<Item = &FunctionDef> {
        self.functions.iter()
    }

    /// The first free function or method named `name` (methods are matched by
    /// their own name only, not `Receiver.name` — use
    /// [`AstStore::method_by_receiver_and_name`] to disambiguate).
    #[must_use]
    pub fn function_by_name(&self, name: &str) -> Option<&FunctionDef> {
        self.functions.iter().find(|f| f.name == name)
    }

    /// The method named `name` on receiver type `receiver_type`
    /// (`fun {receiver_type}.{name}(...)`).
    #[must_use]
    pub fn method_by_receiver_and_name(
        &self,
        receiver_type: &str,
        name: &str,
    ) -> Option<&FunctionDef> {
        self.functions
            .iter()
            .find(|f| f.name == name && f.receiver_type.as_deref() == Some(receiver_type))
    }

    /// All structs, in the order they were encountered.
    pub fn structs(&self) -> impl Iterator<Item = &StructDef> {
        self.structs.iter()
    }

    /// The first struct named `name`.
    #[must_use]
    pub fn struct_by_name(&self, name: &str) -> Option<&StructDef> {
        self.structs.iter().find(|s| s.name == name)
    }

    /// Every `contract Name { ... }` block in this file, usually zero or one.
    pub fn contracts(&self) -> impl Iterator<Item = &ContractDef> {
        self.contracts.iter()
    }
}

fn build_function_def(
    node: &Node,
    source: &str,
    receiver_type: Option<String>,
) -> Option<FunctionDef> {
    let name = node
        .child_by_field_name("name")?
        .utf8_text(source.as_bytes())
        .ok()?
        .to_string();
    let parameters = node
        .child_by_field_name("parameters")
        .map(|params| {
            let mut cursor = params.walk();
            params
                .children(&mut cursor)
                .filter(|c| c.kind() == "parameter_declaration")
                .filter_map(|p| {
                    let pname = p
                        .child_by_field_name("name")?
                        .utf8_text(source.as_bytes())
                        .ok()?
                        .to_string();
                    let type_text = p
                        .child_by_field_name("type")
                        .and_then(|t| t.utf8_text(source.as_bytes()).ok())
                        .map(str::to_string);
                    let is_mutate = {
                        let mut cursor = p.walk();
                        p.children(&mut cursor).any(|c| c.kind() == "mutate")
                    };
                    Some(ParamDef {
                        name: pname,
                        type_text,
                        is_mutate,
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    Some(FunctionDef {
        name,
        receiver_type,
        parameters,
        range: SourceRange::from_node(node),
    })
}

/// Reads a `contract Name { key: value ... }` block from its own node.
///
/// Every field is kept as the text of its name and the text of its value: the
/// set is not fixed by the language, and a detector asks for one by name
/// (`storage`, `incomingMessages`) rather than iterating a schema.
fn build_contract_def(node: &Node, source: &str) -> Option<ContractDef> {
    let name = node
        .child_by_field_name("name")?
        .utf8_text(source.as_bytes())
        .ok()?
        .to_string();
    let fields = node
        .child_by_field_name("body")
        .map(|body| {
            let mut cursor = body.walk();
            body.children(&mut cursor)
                .filter(|c| c.kind() == "contract_field_declaration")
                .filter_map(|field| {
                    let key = field
                        .child_by_field_name("name")?
                        .utf8_text(source.as_bytes())
                        .ok()?
                        .to_string();
                    let value = field
                        .child_by_field_name("value")?
                        .utf8_text(source.as_bytes())
                        .ok()?
                        .to_string();
                    Some((key, value))
                })
                .collect()
        })
        .unwrap_or_default();

    Some(ContractDef {
        name,
        fields,
        range: SourceRange::from_node(node),
    })
}

fn build_struct_def(node: &Node, source: &str) -> Option<StructDef> {
    let name = node
        .child_by_field_name("name")?
        .utf8_text(source.as_bytes())
        .ok()?
        .to_string();
    let fields = node
        .child_by_field_name("body")
        .map(|body| {
            let mut cursor = body.walk();
            body.children(&mut cursor)
                .filter(|c| c.kind() == "struct_field_declaration")
                .filter_map(|f| {
                    let fname = f
                        .child_by_field_name("name")?
                        .utf8_text(source.as_bytes())
                        .ok()?
                        .to_string();
                    let type_text = f
                        .child_by_field_name("type")?
                        .utf8_text(source.as_bytes())
                        .ok()?
                        .to_string();
                    Some(FieldDef {
                        name: fname,
                        type_text,
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    Some(StructDef {
        name,
        fields,
        range: SourceRange::from_node(node),
    })
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "docs/ARCHITECTURE.md §3.2 scopes the no-unwrap/expect policy to library code outside tests"
)]
mod tests {
    use tree_sitter::Node;

    use super::{AstStore, Language, Preorder, SourceRange, find_error_nodes, parse, parse_as};

    /// The store has to say which grammar it came from: a detector that branches on the wrong one
    /// is wrong quietly, and the two grammars share some of their node names.
    #[test]
    fn a_store_remembers_its_language() {
        let tolk_source = "fun f(s: slice) {}";
        let tolk = parse(tolk_source).expect("tolk parses");
        assert_eq!(
            AstStore::build(&tolk, tolk_source).language(),
            Language::Tolk
        );

        let func_source = "() f(slice s) {}";
        let func = parse_as(Language::Func, func_source).expect("func parses");
        assert_eq!(
            AstStore::build_as(Language::Func, &func, func_source).language(),
            Language::Func
        );
    }

    /// The shape every `FunC` entrypoint has, and the one a slice-reading detector starts from.
    #[test]
    fn a_func_function_yields_its_name_and_typed_parameters() {
        let source = "() recv_internal(int my_balance, int msg_value, cell in_msg_full, slice in_msg_body) {\n}\n";
        let tree = parse_as(Language::Func, source).expect("func parses");
        let store = AstStore::build_as(Language::Func, &tree, source);
        let functions: Vec<_> = store.functions().collect();
        assert_eq!(functions.len(), 1);
        assert_eq!(functions[0].name, "recv_internal");
        assert_eq!(functions[0].parameters.len(), 4);
        assert_eq!(functions[0].parameters[3].name, "in_msg_body");
        assert_eq!(
            functions[0].parameters[3].type_text.as_deref(),
            Some("slice")
        );
    }

    /// `FunC` decides at the call site whether a call writes back to its receiver, so no parameter is
    /// ever `mutate`. Reporting one as mutate would make a detector skip a slice it should read.
    #[test]
    fn no_func_parameter_is_declared_mutate() {
        let source = "(slice, int) load_op(slice s) {\n  return (s, s~load_uint(32));\n}\n";
        let tree = parse_as(Language::Func, source).expect("func parses");
        let store = AstStore::build_as(Language::Func, &tree, source);
        let function = store.functions().next().expect("one function");
        assert!(function.parameters.iter().all(|p| !p.is_mutate));
    }

    /// A method in `FunC` is a free function; nothing declares a receiver type, and claiming one
    /// would invent a Tolk shape the language does not have.
    #[test]
    fn a_func_function_declares_no_receiver_type() {
        let source = "int f(slice s) {\n  return 1;\n}\n";
        let tree = parse_as(Language::Func, source).expect("func parses");
        let store = AstStore::build_as(Language::Func, &tree, source);
        assert!(store.functions().all(|f| f.receiver_type.is_none()));
        assert_eq!(store.structs().count(), 0);
        assert_eq!(store.contracts().count(), 0);
    }

    #[test]
    fn parses_trivial_source_without_errors() {
        let tree = parse("fun main() { return; }").expect("grammar loads");
        assert!(
            find_error_nodes(&tree).is_empty(),
            "trivial source should not produce ERROR/MISSING nodes"
        );
    }

    #[test]
    fn preorder_visits_root_first_at_depth_zero() {
        let tree = parse("fun main() { return; }").expect("grammar loads");
        let first = Preorder::new(&tree)
            .next()
            .expect("tree has at least one node");
        assert_eq!(first.depth, 0);
        assert_eq!(first.node.id(), tree.root_node().id());
    }

    /// Counts every descendant of `node` (`node` included), named or
    /// anonymous, via `Node::child`/`child_count` directly — a genuinely
    /// different traversal API from the `TreeCursor` `Preorder` is built on,
    /// used as an independent oracle in
    /// `preorder_visits_every_descendant_exactly_once` below.
    fn count_all(node: Node) -> usize {
        let mut total = 1;
        for i in 0..node.child_count() {
            let i = u32::try_from(i).expect("child index fits in u32");
            if let Some(child) = node.child(i) {
                total += count_all(child);
            }
        }
        total
    }

    #[test]
    fn preorder_visits_every_descendant_exactly_once() {
        // Strengthened during a full project review: the original version
        // only cross-checked *named* node counts against the number of `(`
        // in `to_sexp()` — that neither proves "exactly once" (two
        // independent miscounts could cancel out) nor covers anonymous
        // nodes, despite this function's own doc comment claiming both.
        let source = "fun main(x: int) {\n    if (x > 0) {\n        return;\n    }\n}";
        let tree = parse(source).expect("grammar loads");
        let visited: Vec<_> = Preorder::new(&tree).collect();

        // "Exactly once": no node id appears twice.
        let mut seen = std::collections::HashSet::new();
        for v in &visited {
            assert!(
                seen.insert(v.node.id()),
                "node {:?} (kind {:?}) was visited more than once",
                v.node.id(),
                v.node.kind()
            );
        }

        // "Every descendant", named or anonymous.
        assert_eq!(visited.len(), count_all(tree.root_node()));
    }

    #[test]
    fn detects_a_deliberately_malformed_snippet() {
        let tree = parse("fun main( { return; }").expect("grammar loads");
        assert!(
            !find_error_nodes(&tree).is_empty(),
            "malformed source must surface as ERROR/MISSING nodes, not silently parse"
        );
    }

    // Adversarial `parse_contract_headers_text` cases — added during a full
    // project review, none of these were previously exercised by any test.
    // The function is deliberately "best-effort", not a real parser (see its
    // own doc comment): every case here must degrade to recovering as many
    // *complete* blocks as possible and silently stop, never panic.

    #[test]
    fn ast_store_extracts_functions_structs_and_contracts_from_real_source() {
        // Assembled from the exact shapes confirmed in
        // docs/domains/dataflow-cfg.md against real tolk-bench fixtures:
        // function_declaration (free fn), method_declaration (receiver +
        // implicit `self` param, no `type` field), struct_declaration, and a
        // contract{} header, recovered through the text fallback.
        let source = r#"
struct MinterStorage {
    totalSupply: coins
    adminAddress: address
}

fun MinterStorage.save(self) {
    contract.setData(self.toCell())
}

fun onInternalMessage(in: InMessage) {
    return;
}

contract JettonMinter {
    author: "The Tolk Team"
    storage: MinterStorage
}
"#;
        let tree = parse(source).expect("grammar loads");
        let store = AstStore::build(&tree, source);

        let structs: Vec<_> = store.structs().collect();
        assert_eq!(structs.len(), 1);
        assert_eq!(structs[0].name, "MinterStorage");
        assert_eq!(
            structs[0].fields,
            vec![
                super::FieldDef {
                    name: "totalSupply".to_string(),
                    type_text: "coins".to_string()
                },
                super::FieldDef {
                    name: "adminAddress".to_string(),
                    type_text: "address".to_string()
                },
            ]
        );
        assert_eq!(
            store.struct_by_name("MinterStorage").map(|s| &s.name),
            Some(&"MinterStorage".to_string())
        );
        assert!(store.struct_by_name("NoSuchStruct").is_none());

        let functions: Vec<_> = store.functions().collect();
        assert_eq!(functions.len(), 2);

        let free_fn = store
            .function_by_name("onInternalMessage")
            .expect("free function found");
        assert_eq!(free_fn.receiver_type, None);
        assert_eq!(free_fn.parameters.len(), 1);
        assert_eq!(free_fn.parameters[0].name, "in");
        assert_eq!(
            free_fn.parameters[0].type_text.as_deref(),
            Some("InMessage")
        );

        let method = store
            .method_by_receiver_and_name("MinterStorage", "save")
            .expect("method found by receiver+name");
        assert_eq!(method.receiver_type.as_deref(), Some("MinterStorage"));
        assert_eq!(method.parameters.len(), 1);
        assert_eq!(method.parameters[0].name, "self");
        assert_eq!(
            method.parameters[0].type_text, None,
            "implicit `self` has no `type` field in the grammar"
        );
        assert!(
            store
                .method_by_receiver_and_name("WalletStorage", "save")
                .is_none()
        );

        let contracts: Vec<_> = store.contracts().collect();
        assert_eq!(contracts.len(), 1);
        assert_eq!(contracts[0].name, "JettonMinter");
        assert_eq!(
            contracts[0].fields[1],
            ("storage".to_string(), "MinterStorage".to_string())
        );
    }

    #[test]
    fn preorder_from_node_visits_only_the_given_subtree() {
        let source = "fun f() {\n    var owner = 5;\n}\nfun g() {\n    var owner = 6;\n}";
        let tree = parse(source).expect("grammar loads");
        let store = AstStore::build(&tree, source);
        let g = store.function_by_name("g").expect("g is declared");
        let g_node = g.range.locate(&tree).expect("range resolves to a node");

        let var_decls_in_g: Vec<_> = Preorder::from_node(g_node)
            .map(|v| v.node)
            .filter(|n| n.kind() == "var_declaration")
            .collect();

        assert_eq!(
            var_decls_in_g.len(),
            1,
            "a scoped walk from g's node must not see f's var declaration"
        );
    }

    #[test]
    fn locate_recovers_the_original_node_a_range_was_captured_from() {
        let source = "fun f() {\n    return;\n}\nfun g() {\n    return;\n}";
        let tree = parse(source).expect("grammar loads");
        let store = AstStore::build(&tree, source);
        let g = store.function_by_name("g").expect("g is declared");

        let node = g.range.locate(&tree).expect("range resolves to a node");

        assert_eq!(
            node.kind(),
            "function_declaration",
            "locate must recover the exact declaration node the range was captured from, not a child/parent"
        );
        assert_eq!(SourceRange::from_node(&node), g.range);
    }

    #[test]
    fn locate_clamps_an_out_of_bounds_range_to_the_root_instead_of_panicking() {
        // Verified empirically, not assumed: tree-sitter's own
        // `descendant_for_byte_range` does not return `None` for an
        // out-of-range request, it clamps to the smallest node that still
        // contains as much of the requested range as exists (the whole tree,
        // in this case) — `locate` inherits that behavior rather than
        // panicking or fabricating a `None` the underlying API doesn't
        // actually produce here. `Option` stays the right return type: it is
        // `descendant_for_byte_range`'s own signature, and a genuinely empty
        // tree is a real (if unlikely) case where it returns `None`.
        let source = "fun f() { return; }";
        let tree = parse(source).expect("grammar loads");
        let out_of_bounds = SourceRange {
            start_byte: source.len() + 100,
            end_byte: source.len() + 100,
            start_line: 99,
            start_column: 1,
            end_line: 99,
            end_column: 1,
        };
        let node = out_of_bounds
            .locate(&tree)
            .expect("tree-sitter clamps rather than returning None here");
        assert_eq!(node.id(), tree.root_node().id());
    }
}
