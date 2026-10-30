//! [`SemanticModel`] — the three questions this layer answers.
//!
//! A **project**-level model rather than a file-level one: the compiler's
//! artefact describes a whole run, and "what is reachable from this entry
//! point" always crosses files on a real contract. A query is still made in the
//! context of one file ([`SemanticModel::file`]), because a `tree-sitter` node
//! does not carry which tree it came from.
//!
//! The order of trust is followed literally: **ask the compiler first, infer
//! only where it is silent.** Every rule below is marked with where its answer
//! comes from.

use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};

use sting_tolkc::{Declaration, FunctionEntry, SymbolTypes};
use tree_sitter::{Node, Tree};

use crate::types::{ResolvedType, TypeRef, UnknownReason};

/// One parsed file of the project.
pub struct SourceUnit<'a> {
    /// The path the file was handed to the compiler under.
    pub path: PathBuf,
    /// The parse tree.
    pub tree: &'a Tree,
    /// The source text, the same one the tree was built from.
    pub source: &'a str,
}

/// A function as the compiler knows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FunctionRef {
    /// The full name: `foo` for a free function, `Storage.save` for a method.
    pub name: String,
    /// The index of the result type in `unique_types` where the compiler knows
    /// it. `None` means the function was found only in the AST (see
    /// [`FunctionRef::from_ast`]).
    pub return_ty_idx: Option<u32>,
    /// The function is known only from source: the compiler inlined it and did
    /// not surface it in `symbolTypes.json`.
    pub from_ast: bool,
}

/// Why a call did not resolve.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UnresolvedCall {
    /// The node is not a call.
    NotACall,
    /// The receiver's type is unknown, so where the method goes is unknown too.
    ReceiverTypeUnknown(UnknownReason),
    /// The receiver's type is known and is not a struct.
    ReceiverNotAStruct {
        /// The receiver type's name.
        type_name: String,
    },
    /// The name is in neither the compiler's table nor the project's
    /// declarations. Usually a standard-library function — an honest "I do not
    /// know".
    NoSuchFunction {
        /// The full name that was looked for.
        looked_for: String,
    },
}

/// The layer's answer to "which function does this call call".
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResolvedCall {
    /// The call resolved uniquely.
    Known(FunctionRef),
    /// More than one candidate.
    Ambiguous(Vec<FunctionRef>),
    /// Unresolved, with the reason named.
    Unresolved(UnresolvedCall),
}

impl ResolvedCall {
    /// The function, where there is exactly one.
    #[must_use]
    pub fn known(&self) -> Option<&FunctionRef> {
        match self {
            Self::Known(f) => Some(f),
            _ => None,
        }
    }
}

/// A function found in source: the fallback for one the compiler inlined.
///
/// Needed for more than resolving calls. The compiler's table is
/// **post-inlining**, so an inlined function has no span either, and the nodes
/// of its body would otherwise come back as "outside any function". Measured
/// over the corpus: 1400 such nodes, close to forty per cent of every
/// refusal.
struct AstFunction<'a> {
    unit: usize,
    body: Option<Node<'a>>,
    /// The type name from the return annotation (`fun Storage.load(): Storage`).
    return_type_name: Option<String>,
    /// `(parameter name, type name)` from the source's annotations.
    params: Vec<(String, String)>,
}

struct UnitIndex<'a> {
    path: PathBuf,
    tree: &'a Tree,
    source: &'a str,
    file_id: Option<u32>,
    /// `(first line, last line, index into symbols.functions)`, narrow to
    /// wide.
    functions_here: Vec<(u32, u32, usize)>,
    /// The same over the source's declarations: `(first, last, full name)`.
    /// Used where the compiler inlined the function and gave no span.
    ast_here: Vec<(u32, u32, String)>,
}

/// The semantics of a project compiled by one run of `tolk`.
pub struct SemanticModel<'a> {
    symbols: &'a SymbolTypes,
    units: Vec<UnitIndex<'a>>,
    by_name: HashMap<String, Vec<usize>>,
    ast_functions: HashMap<String, AstFunction<'a>>,
    declared_types: HashMap<String, u32>,
    /// `global name: Type;` — the compiler does not surface globals in
    /// `symbolTypes.json` at all, and their type is written in the declaration
    /// itself. Measured over the corpus: 1239 refusals for this reason.
    globals: HashMap<String, String>,
    /// `struct Name { field: Type ... }` from source. The compiler does not
    /// surface the standard library's structs in `declarations` even though it
    /// compiles them: `InMessage`, with its `senderAddress` and `valueCoins`
    /// fields, is declared in `tolk-stdlib/common.tolk` and absent from the
    /// artefact.
    ast_structs: HashMap<String, Vec<(String, String)>>,
}

impl<'a> SemanticModel<'a> {
    /// Builds the model from the compiler's artefact and the project's parsed
    /// files.
    #[must_use]
    pub fn build(symbols: &'a SymbolTypes, units: &[SourceUnit<'a>]) -> Self {
        let mut by_name: HashMap<String, Vec<usize>> = HashMap::new();
        for (index, f) in symbols.functions.iter().enumerate() {
            by_name.entry(f.name.clone()).or_default().push(index);
        }

        let mut declared_types = HashMap::new();
        for decl in &symbols.declarations {
            match decl {
                Declaration::Alias { name, ty_idx, .. }
                | Declaration::Struct { name, ty_idx, .. }
                | Declaration::Enum { name, ty_idx, .. } => {
                    declared_types.insert(name.clone(), *ty_idx);
                }
            }
        }

        let mut indexed = Vec::with_capacity(units.len());
        let mut ast_functions = HashMap::new();
        let mut globals = HashMap::new();
        let mut ast_structs = HashMap::new();
        for (unit_index, unit) in units.iter().enumerate() {
            index_globals(unit, &mut globals);
            index_structs(unit, &mut ast_structs);
            let file_id = file_id_of(symbols, &unit.path);
            let mut functions_here: Vec<(u32, u32, usize)> = file_id
                .map(|id| {
                    symbols
                        .functions
                        .iter()
                        .enumerate()
                        .filter(|(_, f)| f.ident_loc.file_id == id)
                        .map(|(i, f)| (f.ident_loc.start_line, f.end_loc.end_line, i))
                        .collect()
                })
                .unwrap_or_default();
            functions_here.sort_by_key(|(start, end, _)| (end - start, *start));

            let mut ast_here = index_ast_functions(unit, unit_index, &mut ast_functions);
            ast_here.sort_by_key(|(start, end, _)| (end - start, *start));

            indexed.push(UnitIndex {
                path: unit.path.clone(),
                tree: unit.tree,
                source: unit.source,
                file_id,
                functions_here,
                ast_here,
            });
        }

        Self {
            symbols,
            units: indexed,
            by_name,
            ast_functions,
            declared_types,
            globals,
            ast_structs,
        }
    }

    /// A type by name: a declared struct or alias, or a primitive.
    fn type_by_name(&self, name: &str) -> Option<TypeRef> {
        if let Some(idx) = self.declared_types.get(name) {
            return TypeRef::from_index(self.symbols, *idx);
        }
        let primitive = (0..self.symbols.unique_types.len()).find_map(|idx| {
            let ty = TypeRef::from_index(self.symbols, u32::try_from(idx).ok()?)?;
            (ty.display_name() == name).then_some(ty)
        });
        if primitive.is_some() {
            return primitive;
        }
        // A struct the compiler stayed silent about and which is declared in
        // the sources handed in, the standard library included.
        self.ast_structs
            .contains_key(name)
            .then(|| TypeRef::from_source_struct(name))
    }

    /// A view of the model in one file's context.
    #[must_use]
    pub fn file(&self, path: &Path) -> Option<FileSemantics<'_, 'a>> {
        let target = path.file_name()?;
        let unit = self
            .units
            .iter()
            .position(|u| u.path.file_name() == Some(target))?;
        Some(FileSemantics { model: self, unit })
    }

    /// The project's files the compiler did not see — handed in by the caller
    /// and not part of this entry point's compilation.
    #[must_use]
    pub fn units_without_compiler_data(&self) -> Vec<&Path> {
        self.units
            .iter()
            .filter(|u| u.file_id.is_none())
            .map(|u| u.path.as_path())
            .collect()
    }

    // Question three: what is reachable.

    /// **"What is reachable from this entry point?"** — transitively and
    /// across every file of the project, not one hop.
    ///
    /// The entry point itself is not in the result. The order is
    /// deterministic.
    #[must_use]
    pub fn reachable_from(&self, entrypoint: &str) -> BTreeSet<String> {
        let mut seen = BTreeSet::new();
        let mut frontier = vec![entrypoint.to_string()];
        while let Some(current) = frontier.pop() {
            for callee in self.direct_calls_of(&current) {
                if seen.insert(callee.clone()) {
                    frontier.push(callee);
                }
            }
        }
        seen
    }

    /// The functions called directly from `name`'s body.
    #[must_use]
    pub fn direct_calls_of(&self, name: &str) -> Vec<String> {
        let Some((unit, body)) = self.body_of(name) else {
            return Vec::new();
        };
        let view = FileSemantics { model: self, unit };
        let mut out = Vec::new();
        for visited in sting_ast::Preorder::from_node(body) {
            let node = visited.node;
            if node.kind() != "function_call" {
                continue;
            }
            if let ResolvedCall::Known(f) = view.resolve_call(node)
                && !out.contains(&f.name)
            {
                out.push(f.name);
            }
        }
        out
    }

    /// A function's body by its full name: the compiler's spans first, then
    /// the AST declaration, for the inlined ones.
    fn body_of(&self, name: &str) -> Option<(usize, Node<'a>)> {
        if let Some(ast) = self.ast_functions.get(name) {
            return ast.body.map(|body| (ast.unit, body));
        }
        None
    }

    fn lookup(&self, full_name: &str) -> ResolvedCall {
        // 1. Ask the compiler.
        if let Some(indices) = self.by_name.get(full_name) {
            let refs: Vec<FunctionRef> = indices
                .iter()
                .filter_map(|i| self.symbols.functions.get(*i))
                .map(|f| FunctionRef {
                    name: f.name.clone(),
                    return_ty_idx: Some(f.return_ty_idx),
                    from_ast: false,
                })
                .collect();
            return match refs.len() {
                0 => ResolvedCall::Unresolved(UnresolvedCall::NoSuchFunction {
                    looked_for: full_name.to_string(),
                }),
                1 => refs.into_iter().next().map_or_else(
                    || unreachable!("the length is checked above"),
                    ResolvedCall::Known,
                ),
                _ => ResolvedCall::Ambiguous(refs),
            };
        }

        // 2. The compiler was silent, so look in the source. Not a guess: the
        //    `symbolTypes.json` table is **post-inlining**, and one-line
        //    functions such as `fun Storage.load(): Storage` never reach it
        //    even though they are declared and called. Measured on the real
        //    corpus.
        if let Some(ast) = self.ast_functions.get(full_name) {
            let return_ty_idx = ast
                .return_type_name
                .as_deref()
                .and_then(|n| self.declared_types.get(n))
                .copied();
            return ResolvedCall::Known(FunctionRef {
                name: full_name.to_string(),
                return_ty_idx,
                from_ast: true,
            });
        }

        ResolvedCall::Unresolved(UnresolvedCall::NoSuchFunction {
            looked_for: full_name.to_string(),
        })
    }

    fn field_type(&self, struct_name: &str, field: &str) -> Option<TypeRef> {
        // 1. Ask the compiler.
        if let Some(fields) = self
            .symbols
            .declarations
            .iter()
            .find_map(|decl| match decl {
                Declaration::Struct { name, fields, .. } if name == struct_name => Some(fields),
                _ => None,
            })
            && let Some(entry) = fields.iter().find(|f| f.name == field)
        {
            return TypeRef::from_index(self.symbols, entry.ty_idx);
        }
        // 2. It stays silent about the standard library's structs even though
        //    it compiles them: `struct InMessage` is declared in
        //    `tolk-stdlib/common.tolk` and is absent from the artefact's
        //    `declarations`. So the fields are read from source, where the
        //    source was handed to the model.
        let fields = self.ast_structs.get(struct_name)?;
        let (_, type_name) = fields.iter().find(|(name, _)| name == field)?;
        self.type_by_name(type_name)
    }

    fn primitive(&self, name: &str) -> ResolvedType {
        let found = (0..self.symbols.unique_types.len()).find_map(|idx| {
            let ty = TypeRef::from_index(self.symbols, u32::try_from(idx).ok()?)?;
            (ty.display_name() == name).then_some(ty)
        });
        found.map_or(
            ResolvedType::Unknown(UnknownReason::UninferrableExpression {
                node_kind: format!("the primitive {name} is absent from the project's type table"),
            }),
            ResolvedType::Known,
        )
    }
}

/// The enclosing function: as the compiler knows it, or — where it inlined the
/// function — as the source declares it.
enum Enclosing<'m, 'a> {
    Compiler(&'a FunctionEntry),
    Ast(&'m AstFunction<'a>),
}

/// The model in one file's context: queries about the nodes of its own tree.
pub struct FileSemantics<'m, 'a> {
    model: &'m SemanticModel<'a>,
    unit: usize,
}

impl<'a> FileSemantics<'_, 'a> {
    fn unit(&self) -> &UnitIndex<'a> {
        &self.model.units[self.unit]
    }

    fn text(&self, node: Node) -> &'a str {
        self.unit()
            .source
            .get(node.start_byte()..node.end_byte())
            .unwrap_or("")
    }

    // Question one: the type of a value.

    /// **"What type is this expression?"**
    ///
    /// Takes any expression node rather than an identifier alone: the type of
    /// `storage.counter` is as legitimate a question as the type of
    /// `storage`.
    #[must_use]
    pub fn type_of(&self, node: Node) -> ResolvedType {
        let Some(function) = self.enclosing_function(node) else {
            return ResolvedType::Unknown(UnknownReason::OutsideAnyFunction);
        };
        self.infer(node, &function, 0)
    }

    fn infer(&self, node: Node, function: &Enclosing<'_, 'a>, depth: u32) -> ResolvedType {
        const MAX_DEPTH: u32 = 32;
        if depth > MAX_DEPTH {
            return ResolvedType::Unknown(UnknownReason::UninferrableExpression {
                node_kind: "<cyclic dependency>".to_string(),
            });
        }
        match node.kind() {
            "number_literal" => self.model.primitive("int"),
            "string_literal" => self.model.primitive("string"),
            "boolean_literal" => self.model.primitive("bool"),
            // `lazy expr` does not change the type: it tells the compiler how
            // to load, not what to convert.
            "lazy_expression" | "parenthesized_expression" | "expression_statement" => {
                node.named_child(0).map_or(
                    ResolvedType::Unknown(UnknownReason::NoInitializer),
                    |inner| self.infer(inner, function, depth + 1),
                )
            }
            "function_call" => self.type_of_call_result(node, function, depth),
            "dot_access" => self.type_of_dot_access(node, function, depth),
            "identifier" => self.type_of_identifier(node, function, depth),
            kind => ResolvedType::Unknown(UnknownReason::UninferrableExpression {
                node_kind: kind.to_string(),
            }),
        }
    }

    fn type_of_identifier(
        &self,
        node: Node,
        function: &Enclosing<'_, 'a>,
        depth: u32,
    ) -> ResolvedType {
        let name = self.text(node);

        // 1. Ask the compiler: a function parameter.
        if let Some(ty) = self.param_of(function, name) {
            return ResolvedType::Known(ty);
        }

        // 2. Narrowing in a `match` arm. The compiler does not report this,
        //    and the rule is exact rather than heuristic: an arm of a `match`
        //    over a union declares the scrutinee's type across its whole
        //    body.
        if let Some(ty) = self.match_arm_narrowing(node, name) {
            return ResolvedType::Known(ty);
        }

        // 3. A local `val` or `var`: infer from the initializer.
        if let Some(initializer) = self.local_initializer(node, function, name) {
            return self.infer(initializer, function, depth + 1);
        }

        // 4. A global. The compiler does not surface them in
        //    `symbolTypes.json` at all, and the type is written in the
        //    `global name: Type;` declaration itself — reading it costs less
        //    than refusing.
        if let Some(ty) = self
            .model
            .globals
            .get(name)
            .and_then(|ty| self.model.type_by_name(ty))
        {
            return ResolvedType::Known(ty);
        }

        ResolvedType::Unknown(UnknownReason::NotDeclaredInFunction)
    }

    fn type_of_dot_access(
        &self,
        node: Node,
        function: &Enclosing<'_, 'a>,
        depth: u32,
    ) -> ResolvedType {
        // 1. Ask the compiler about the whole path. It lowers parameters into
        //    access paths: `fun onInternalMessage(in: InMessage)` becomes a
        //    parameter named `in.body`. The compiler reports the path's type
        //    directly, while the type of `in` itself it does not.
        if let Some(ty) = self.param_of(function, self.text(node)) {
            return ResolvedType::Known(ty);
        }

        let (Some(receiver), Some(member)) = (node.named_child(0), last_identifier(node)) else {
            return ResolvedType::Unknown(UnknownReason::UninferrableExpression {
                node_kind: "dot_access".to_string(),
            });
        };
        let field = self.text(member);

        // 2. A struct field, whose type the compiler knows exactly.
        let receiver_type = self.infer(receiver, function, depth + 1);
        let Some(receiver_type) = receiver_type.known() else {
            return receiver_type;
        };
        let Some(struct_name) = receiver_type.struct_name() else {
            return ResolvedType::Unknown(UnknownReason::UninferrableExpression {
                node_kind: format!("a field on the non-struct {}", receiver_type.display_name()),
            });
        };
        self.model.field_type(struct_name, field).map_or(
            ResolvedType::Unknown(UnknownReason::NoSuchField {
                struct_name: struct_name.to_string(),
                field: field.to_string(),
            }),
            ResolvedType::Known,
        )
    }

    fn type_of_call_result(
        &self,
        call: Node,
        function: &Enclosing<'_, 'a>,
        depth: u32,
    ) -> ResolvedType {
        match self.resolve_call_within(call, function, depth) {
            ResolvedCall::Known(f) => f
                .return_ty_idx
                .and_then(|idx| TypeRef::from_index(self.model.symbols, idx))
                .map_or(
                    ResolvedType::Unknown(UnknownReason::UnresolvedCall),
                    ResolvedType::Known,
                ),
            ResolvedCall::Ambiguous(candidates) => ResolvedType::from_candidates(
                candidates
                    .iter()
                    .filter_map(|f| f.return_ty_idx)
                    .filter_map(|idx| TypeRef::from_index(self.model.symbols, idx))
                    .collect(),
                UnknownReason::UnresolvedCall,
            ),
            ResolvedCall::Unresolved(_) => ResolvedType::Unknown(UnknownReason::UnresolvedCall),
        }
    }

    // Question two: the target of a call.

    /// **"Which function does this call call?"**
    ///
    /// Resolution goes by the receiver's type rather than by a bare name
    /// matching — exactly what `AC` lacks with its "exactly one name match in
    /// the whole project, otherwise surrender".
    #[must_use]
    pub fn resolve_call(&self, call: Node) -> ResolvedCall {
        let Some(function) = self.enclosing_function(call) else {
            return ResolvedCall::Unresolved(UnresolvedCall::ReceiverTypeUnknown(
                UnknownReason::OutsideAnyFunction,
            ));
        };
        self.resolve_call_within(call, &function, 0)
    }

    fn resolve_call_within(
        &self,
        call: Node,
        function: &Enclosing<'_, 'a>,
        depth: u32,
    ) -> ResolvedCall {
        if call.kind() != "function_call" {
            return ResolvedCall::Unresolved(UnresolvedCall::NotACall);
        }
        let Some(callee) = call.named_child(0) else {
            return ResolvedCall::Unresolved(UnresolvedCall::NotACall);
        };

        match callee.kind() {
            "identifier" => self.model.lookup(self.text(callee)),
            "dot_access" => {
                let (Some(receiver), Some(member)) =
                    (callee.named_child(0), last_identifier(callee))
                else {
                    return ResolvedCall::Unresolved(UnresolvedCall::NotACall);
                };
                let method = self.text(member);

                // A static call on a type: `Storage.load()`. It differs from
                // a method call in that the receiver is a declared type's name
                // rather than a variable.
                if receiver.kind() == "identifier" {
                    let receiver_name = self.text(receiver);
                    if self.model.declared_types.contains_key(receiver_name) {
                        return self.model.lookup(&format!("{receiver_name}.{method}"));
                    }
                }

                let receiver_type = self.infer(receiver, function, depth + 1);
                match &receiver_type {
                    ResolvedType::Known(ty) => ty.struct_name().map_or_else(
                        || {
                            ResolvedCall::Unresolved(UnresolvedCall::ReceiverNotAStruct {
                                type_name: ty.display_name().to_string(),
                            })
                        },
                        |struct_name| self.model.lookup(&format!("{struct_name}.{method}")),
                    ),
                    // The receiver's type is ambiguous, which is **not** a
                    // reason to give up: the method is checked against every
                    // candidate. This is exactly where the layer beats today's
                    // fail-safe.
                    ResolvedType::Ambiguous(candidates) => {
                        let found: Vec<FunctionRef> = candidates
                            .iter()
                            .filter_map(TypeRef::struct_name)
                            .filter_map(|s| {
                                self.model.lookup(&format!("{s}.{method}")).known().cloned()
                            })
                            .collect();
                        match found.len() {
                            0 => ResolvedCall::Unresolved(UnresolvedCall::NoSuchFunction {
                                looked_for: method.to_string(),
                            }),
                            1 => found.into_iter().next().map_or_else(
                                || unreachable!("the length is checked above"),
                                ResolvedCall::Known,
                            ),
                            _ => ResolvedCall::Ambiguous(found),
                        }
                    }
                    ResolvedType::Unknown(reason) => ResolvedCall::Unresolved(
                        UnresolvedCall::ReceiverTypeUnknown(reason.clone()),
                    ),
                }
            }
            _ => ResolvedCall::Unresolved(UnresolvedCall::NotACall),
        }
    }

    // Helpers.

    /// A node's enclosing function: the compiler's spans first, then the
    /// source's declarations.
    ///
    /// The second path is not a luxury: the compiler's table is post-inlining,
    /// and without it every node in an inlined function's body would answer
    /// `OutsideAnyFunction` — 1400 of them measured over the corpus.
    fn enclosing_function(&self, node: Node) -> Option<Enclosing<'_, 'a>> {
        let line = u32::try_from(node.start_position().row).ok()? + 1;
        let unit = self.unit();
        if let Some(entry) = unit
            .functions_here
            .iter()
            .find(|(start, end, _)| *start <= line && line <= *end)
            .and_then(|(_, _, index)| self.model.symbols.functions.get(*index))
        {
            return Some(Enclosing::Compiler(entry));
        }
        let name = unit
            .ast_here
            .iter()
            .find(|(start, end, _)| *start <= line && line <= *end)
            .map(|(_, _, name)| name)?;
        self.model.ast_functions.get(name).map(Enclosing::Ast)
    }

    /// The type of an enclosing function's parameter, by name.
    fn param_of(&self, enclosing: &Enclosing<'_, 'a>, name: &str) -> Option<TypeRef> {
        let from_source = |ast: &AstFunction<'a>| {
            ast.params
                .iter()
                .find(|(param, _)| param == name)
                .and_then(|(_, ty)| self.model.type_by_name(ty))
        };
        match enclosing {
            Enclosing::Compiler(entry) => {
                // The compiler lowers parameters: for
                // `fun onInternalMessage(in: InMessage)` the table holds only
                // `in.body`, the path that really arrives on the stack. `in`
                // itself is not there even though `in.senderAddress` is used in
                // the body. So on a miss the source's annotation is asked
                // instead: 90 unresolved `in.senderAddress` were measured for
                // exactly this reason.
                param_type(self.model.symbols, entry, name).or_else(|| {
                    self.model
                        .ast_functions
                        .get(&entry.name)
                        .and_then(from_source)
                })
            }
            Enclosing::Ast(ast) => from_source(ast),
        }
    }

    fn body_of_enclosing(&self, enclosing: &Enclosing<'_, 'a>) -> Option<Node<'a>> {
        match enclosing {
            Enclosing::Ast(ast) => ast.body,
            Enclosing::Compiler(function) => {
                let target_row = function.ident_loc.start_line.checked_sub(1)?;
                for visited in sting_ast::Preorder::new(self.unit().tree) {
                    let node = visited.node;
                    if !matches!(node.kind(), "function_declaration" | "method_declaration") {
                        continue;
                    }
                    if u32::try_from(node.start_position().row).ok()? == target_row {
                        return body_of_declaration(node);
                    }
                }
                None
            }
        }
    }

    fn local_initializer(
        &self,
        use_site: Node,
        function: &Enclosing<'_, 'a>,
        name: &str,
    ) -> Option<Node<'a>> {
        let body = self.body_of_enclosing(function)?;
        let mut best: Option<Node<'a>> = None;
        for visited in sting_ast::Preorder::from_node(body) {
            let node = visited.node;
            if node.kind() != "assignment" {
                continue;
            }
            // The declaration has to precede the use, or this is a different,
            // later assignment to the same name.
            if node.start_byte() >= use_site.start_byte() {
                continue;
            }
            let Some(lhs) = node.named_child(0) else {
                continue;
            };
            if lhs.kind() != "var_declaration_lhs" || !self.declares(lhs, name) {
                continue;
            }
            if let Some(rhs) = node.named_child(1) {
                best = Some(rhs);
            }
        }
        best
    }

    fn declares(&self, lhs: Node, name: &str) -> bool {
        sting_ast::Preorder::from_node(lhs)
            .any(|v| v.node.kind() == "identifier" && self.text(v.node) == name)
    }

    fn match_arm_narrowing(&self, node: Node, name: &str) -> Option<TypeRef> {
        let mut current = node;
        while let Some(parent) = current.parent() {
            if parent.kind() == "match_arm" {
                let arm_type = parent
                    .named_children(&mut parent.walk())
                    .find(|c| c.kind() == "type_identifier")?;
                // What narrows is the very expression `match` scrutinises.
                if self.match_subject(parent)? != name {
                    return None;
                }
                let ty_idx = *self.model.declared_types.get(self.text(arm_type))?;
                return TypeRef::from_index(self.model.symbols, ty_idx);
            }
            current = parent;
        }
        None
    }

    fn match_subject(&self, arm: Node) -> Option<&'a str> {
        let mut current = arm;
        while let Some(parent) = current.parent() {
            if parent.kind() == "match_expression" {
                let subject = parent
                    .named_children(&mut parent.walk())
                    .find(|c| c.kind() != "match_body")?;
                return Some(self.text(subject));
            }
            current = parent;
        }
        None
    }
}

/// Indexes a file's function and method declarations by full name.
fn index_ast_functions<'a>(
    unit: &SourceUnit<'a>,
    unit_index: usize,
    out: &mut HashMap<String, AstFunction<'a>>,
) -> Vec<(u32, u32, String)> {
    let mut spans = Vec::new();
    for visited in sting_ast::Preorder::new(unit.tree) {
        let node = visited.node;
        let Some(name) = declaration_name(node, unit.source) else {
            continue;
        };
        let start_line = u32::try_from(node.start_position().row).unwrap_or(0) + 1;
        let end_line = u32::try_from(node.end_position().row).unwrap_or(0) + 1;
        spans.push((start_line, end_line, name.clone()));
        out.entry(name).or_insert(AstFunction {
            unit: unit_index,
            body: body_of_declaration(node),
            return_type_name: return_type_name(node, unit.source),
            params: declared_params(node, unit.source),
        });
    }
    spans
}

/// A declaration's parameters with their type annotations.
fn declared_params(node: Node, source: &str) -> Vec<(String, String)> {
    let text = |n: Node| source.get(n.start_byte()..n.end_byte()).unwrap_or("");
    let Some(list) = node
        .named_children(&mut node.walk())
        .find(|c| c.kind() == "parameter_list")
    else {
        return Vec::new();
    };
    list.named_children(&mut list.walk())
        .filter(|c| c.kind() == "parameter_declaration")
        .filter_map(|param| {
            let name = param
                .named_children(&mut param.walk())
                .find(|c| c.kind() == "identifier")?;
            let ty = param
                .named_children(&mut param.walk())
                .find(|c| c.kind() == "type_identifier")?;
            Some((text(name).to_string(), text(ty).to_string()))
        })
        .collect()
}

/// `struct Name { field: Type ... }` — the fields of structs declared in
/// source.
fn index_structs(unit: &SourceUnit<'_>, out: &mut HashMap<String, Vec<(String, String)>>) {
    let text = |n: Node| unit.source.get(n.start_byte()..n.end_byte()).unwrap_or("");
    for visited in sting_ast::Preorder::new(unit.tree) {
        let node = visited.node;
        if node.kind() != "struct_declaration" {
            continue;
        }
        let Some(name) = node
            .named_children(&mut node.walk())
            .find(|c| c.kind() == "identifier")
        else {
            continue;
        };
        let Some(body) = node
            .named_children(&mut node.walk())
            .find(|c| c.kind() == "struct_body")
        else {
            continue;
        };
        let fields: Vec<(String, String)> = body
            .named_children(&mut body.walk())
            .filter(|c| c.kind() == "struct_field_declaration")
            .filter_map(|field| {
                let field_name = field
                    .named_children(&mut field.walk())
                    .find(|c| c.kind() == "identifier")?;
                let field_type = field
                    .named_children(&mut field.walk())
                    .find(|c| c.kind() == "type_identifier")?;
                Some((text(field_name).to_string(), text(field_type).to_string()))
            })
            .collect();
        out.entry(text(name).to_string()).or_insert(fields);
    }
}

/// `global name: Type;` — the type table for globals.
fn index_globals(unit: &SourceUnit<'_>, out: &mut HashMap<String, String>) {
    let text = |n: Node| unit.source.get(n.start_byte()..n.end_byte()).unwrap_or("");
    for visited in sting_ast::Preorder::new(unit.tree) {
        let node = visited.node;
        if node.kind() != "global_var_declaration" {
            continue;
        }
        let (Some(name), Some(ty)) = (
            node.named_children(&mut node.walk())
                .find(|c| c.kind() == "identifier"),
            node.named_children(&mut node.walk())
                .find(|c| c.kind() == "type_identifier"),
        ) else {
            continue;
        };
        out.entry(text(name).to_string())
            .or_insert_with(|| text(ty).to_string());
    }
}

/// A declaration's full name: `foo` or `Storage.load`.
fn declaration_name(node: Node, source: &str) -> Option<String> {
    let text = |n: Node| source.get(n.start_byte()..n.end_byte()).unwrap_or("");
    match node.kind() {
        "function_declaration" => node
            .named_children(&mut node.walk())
            .find(|c| c.kind() == "identifier")
            .map(|n| text(n).to_string()),
        "method_declaration" => {
            let receiver = node
                .named_children(&mut node.walk())
                .find(|c| c.kind() == "method_receiver")?;
            let receiver_type = receiver
                .named_children(&mut receiver.walk())
                .find(|c| c.kind() == "type_identifier")?;
            let method = node
                .named_children(&mut node.walk())
                .find(|c| c.kind() == "identifier")?;
            Some(format!("{}.{}", text(receiver_type), text(method)))
        }
        _ => None,
    }
}

/// The type name from the return annotation, where there is one.
fn return_type_name(node: Node, source: &str) -> Option<String> {
    let annotation = node
        .named_children(&mut node.walk())
        .filter(|c| c.kind() == "type_identifier")
        .last()?;
    // On a method the first `type_identifier` is the receiver and sits inside
    // `method_receiver`, so it never reaches here; the last one at the
    // declaration's top level is the return type.
    Some(
        source
            .get(annotation.start_byte()..annotation.end_byte())?
            .to_string(),
    )
}

fn body_of_declaration(node: Node) -> Option<Node> {
    node.named_children(&mut node.walk())
        .find(|c| c.kind() == "block_statement")
}

fn param_type(symbols: &SymbolTypes, function: &FunctionEntry, name: &str) -> Option<TypeRef> {
    let param = function.params.iter().find(|p| p.name == name)?;
    TypeRef::from_index(symbols, param.ty_idx)
}

/// A file's `file_id` in the compiler's artefact.
///
/// Compared by file name rather than by full path: `tolk` records the path as
/// it resolved it, and that need not match what the caller handed in — which
/// `sting_tolkc::FileEntry::file_name` documents outright.
fn file_id_of(symbols: &SymbolTypes, path: &Path) -> Option<u32> {
    let target = path.file_name()?;
    symbols
        .files
        .iter()
        .find(|f| Path::new(&f.file_name).file_name() == Some(target))
        .map(|f| f.file_id)
}

/// The last `identifier` in a `dot_access` — the field's or method's name.
fn last_identifier(node: Node) -> Option<Node> {
    node.named_children(&mut node.walk())
        .filter(|c| c.kind() == "identifier")
        .last()
}
