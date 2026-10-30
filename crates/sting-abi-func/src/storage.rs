//! Level 2 storage — two independent extractors, [`extract_storage`] for
//! the paired `load_data()`+`save_data(...)` wrapper-function form and
//! [`extract_inline_storage`] for the inline `get_data()`/`set_data()`
//! form (~45% of the research corpus) — see this
//! crate's own `lib.rs` module doc comment for how `lib.rs` orders the
//! two, and [`extract_inline_storage`]'s own doc comment for its exact
//! scope).
//!
//! For [`extract_storage`] specifically: field *types* come from
//! `load_data`'s `~load_*` method-call chain in
//! its return tuple (confirmed shape, real fixture
//! `tests/fixtures/jetton-wallet.fc`): `return (ds~load_coins(),
//! ds~load_msg_addr(), ...)` parses as a `tensor_expression` whose
//! children alternate `identifier` (the receiver, e.g. `ds` — unused here,
//! `method_call` has no receiver field of its own so it appears as a
//! preceding sibling instead) and `method_call` (the actual load, which
//! *is* used). Field *names* come from `save_data`'s named parameters,
//! matched positionally — `load_data`'s own return tuple never names its
//! fields. Arity mismatch between the two (or either function missing)
//! means no storage is recognized at all, not a guess.

use tree_sitter::Node;

use sting_abi_core::TypeShape;

use crate::type_text::map_load_call;

/// Returns `Some((field_name, field_shape))` pairs in declaration order if
/// `root` has a recognizable `load_data`/`save_data` pair with matching
/// arity; `None` if either function is absent, the return shape isn't a
/// plain `~load_*` chain, or the two functions' arities disagree.
#[must_use]
pub fn extract_storage(root: Node, source: &[u8]) -> Option<Vec<(String, TypeShape)>> {
    let load_data_fn = find_function_named(root, source, "load_data")?;
    let save_data_fn = find_function_named(root, source, "save_data")?;

    let field_shapes = extract_load_data_shapes(load_data_fn, source)?;
    let field_names = extract_parameter_names(save_data_fn, source);

    if field_shapes.is_empty() || field_shapes.len() != field_names.len() {
        return None;
    }

    Some(field_names.into_iter().zip(field_shapes).collect())
}

/// Real bug found by review: `FunC` allows a bodyless forward declaration
/// (`(int, slice) load_data();`) ahead of the real definition — a real,
/// if uncommon, style. Taking the *first* same-named `function_definition`
/// unconditionally would pick the bodyless declaration, whose missing
/// `body` field makes `extract_load_data_shapes` silently report "no
/// storage recognized" even though a fully-recognizable real definition
/// exists later in the same file. Prefers a match that actually has a
/// `body` field; falls back to the first match otherwise (still `None`
/// downstream, same as before this fix, for a genuinely bodyless-only
/// case such as an `asm`-only declaration).
fn find_function_named<'a>(root: Node<'a>, source: &[u8], name: &str) -> Option<Node<'a>> {
    let mut cursor = root.walk();
    let matches: Vec<Node<'a>> = root
        .children(&mut cursor)
        .filter(|node| node.kind() == "function_definition")
        .filter(|node| {
            node.child_by_field_name("name")
                .and_then(|n| n.utf8_text(source).ok())
                == Some(name)
        })
        .collect();
    matches
        .iter()
        .find(|node| node.child_by_field_name("body").is_some())
        .copied()
        .or_else(|| matches.into_iter().next())
}

fn extract_load_data_shapes(func: Node, source: &[u8]) -> Option<Vec<TypeShape>> {
    let body = func.child_by_field_name("body")?;
    let mut cursor = body.walk();
    let return_stmt = body
        .named_children(&mut cursor)
        .find(|node| node.kind() == "return_statement")?;

    // `return_statement`'s own children are a *flat* sequence, not always
    // one wrapped expression — confirmed empirically across three distinct
    // real shapes: a multi-field tuple return wraps cleanly in one
    // `tensor_expression`/`tuple_expression` child, but a single-field
    // return's `~load_*` receiver (e.g. `ds`) and its `method_call` are two
    // *separate*, un-wrapped children of `return_statement` directly when
    // written without redundant parens (`return ds~load_coins();`, the
    // idiomatic style with no tuple to group) — or two children of one
    // `parenthesized_expression` if a caller *did* write redundant parens
    // (`return (ds~load_coins());`). All three shapes reduce to "find
    // every `method_call` among the relevant flat child list" once a
    // single wrapping `parenthesized_expression` is unwrapped.
    let mut top_level_cursor = return_stmt.walk();
    let top_level: Vec<Node> = return_stmt.named_children(&mut top_level_cursor).collect();
    let search_node = match top_level.as_slice() {
        [single]
            if matches!(
                single.kind(),
                "parenthesized_expression" | "tensor_expression" | "tuple_expression"
            ) =>
        {
            *single
        }
        // The "assign, then bare `return <var>;`" idiom — confirmed real,
        // found on real, deployed USDT (`docs/domains/extractors.md`:
        // `var data = (ds~load_coins(), ...); ds.end_parse(); return data;`).
        // A bare `return_statement` with a single `identifier` child doesn't
        // match either arm above, so without this case the whole storage was
        // reported as unrecognized rather than partially recognized.
        [single] if single.kind() == "identifier" => {
            let name = single.utf8_text(source).ok()?;
            let assigned = resolve_local_assignment(body, return_stmt, name, source)?;
            unwrap_single_expression(assigned)
        }
        _ => return_stmt,
    };

    let mut calls_cursor = search_node.walk();
    let calls: Vec<Node> = search_node
        .named_children(&mut calls_cursor)
        .filter(|node| node.kind() == "method_call" && is_load_like_call(*node, source))
        .collect();

    if calls.is_empty() {
        // Any other return shape (a bare identifier, a plain function
        // call with no `~load_*` chain at all, ...) isn't the idiom this
        // heuristic recognizes — `None` here, not a partial/wrong guess.
        return None;
    }
    Some(
        calls
            .into_iter()
            .map(|call| method_call_to_shape(call, source))
            .collect(),
    )
}

/// Finds the value most recently assigned to a local variable named `name`
/// among `body`'s direct statement children preceding `return_stmt` — the
/// real shape confirmed by dumping the actual parse tree (`node-types.json`
/// has no `left`/`right`-style assignment node at all): `var data = (...)`
/// parses as one `expression_statement` with exactly two named children —
/// a `variable_declaration` (type + name only, no initializer field of its
/// own) immediately followed by the initializer expression as a sibling,
/// the `=` itself an anonymous token. Only resolves **one level** of
/// indirection (`var x = (...); return x;`) — not alias chains (`var a =
/// ...; var b = a; return b;`) — a conscious scope limit, not found
/// necessary on the real corpus yet. Tracks the *last* matching assignment
/// before `return_stmt`, so a real reassignment resolves correctly, not
/// just the first declaration.
fn resolve_local_assignment<'a>(
    body: Node<'a>,
    return_stmt: Node<'a>,
    name: &str,
    source: &[u8],
) -> Option<Node<'a>> {
    let mut cursor = body.walk();
    let mut resolved = None;
    for stmt in body.named_children(&mut cursor) {
        if stmt.id() == return_stmt.id() {
            break;
        }
        if stmt.kind() != "expression_statement" {
            continue;
        }
        let mut inner_cursor = stmt.walk();
        let children: Vec<Node> = stmt.named_children(&mut inner_cursor).collect();
        let [decl, value] = children.as_slice() else {
            continue;
        };
        if decl.kind() != "variable_declaration" {
            continue;
        }
        let Some(variable) = decl.child_by_field_name("variable") else {
            continue;
        };
        if variable.kind() == "identifier" && variable.utf8_text(source) == Ok(name) {
            resolved = Some(*value);
        }
    }
    resolved
}

/// Unwraps one level of redundant `parenthesized_expression` around a
/// resolved assignment's value — mirrors the same tolerance
/// `extract_load_data_shapes` already applies directly under `return`
/// (`return (ds~load_coins());` with redundant parens is real, observed
/// style elsewhere in this same heuristic).
fn unwrap_single_expression(node: Node) -> Node {
    if node.kind() == "parenthesized_expression" {
        let mut cursor = node.walk();
        let children: Vec<Node> = node.named_children(&mut cursor).collect();
        if let [single] = children.as_slice() {
            return *single;
        }
    }
    node
}

/// `true` if `call`'s method name looks like a stdlib load function
/// (`load_*`/`preload_*`). Real bug found by review: this grammar's
/// `method_call` node has no field distinguishing `~foo()` (a mutating
/// call, what a `~load_*` chain actually uses) from plain `.foo()` — both
/// parse identically — so without this filter, an unrelated dot-call
/// sharing the same tuple as a real load (e.g. `return (ds~load_coins(),
/// get_ref().begin_parse());`) was counted as a second, phantom field
/// (`begin_parse` mapped to `TypeShape::Unknown` and presented as a real,
/// recognized field — worse than the documented "ambiguous → `None`"
/// fallback, since the accidental arity match made it look trustworthy).
/// Name-prefix filtering is the only signal available without a real
/// `~`-vs-`.` distinction in the grammar.
fn is_load_like_call(call: Node, source: &[u8]) -> bool {
    call.child_by_field_name("method_name")
        .and_then(|n| n.utf8_text(source).ok())
        .is_some_and(|name| name.starts_with("load_") || name.starts_with("preload_"))
}

fn method_call_to_shape(call: Node, source: &[u8]) -> TypeShape {
    let method_name = call
        .child_by_field_name("method_name")
        .and_then(|n| n.utf8_text(source).ok())
        .unwrap_or("");
    let arg_literal = call
        .child_by_field_name("arguments")
        .and_then(|arg_node| number_literal_in(arg_node, source));
    map_load_call(method_name, arg_literal)
}

/// A `method_call`'s single `arguments` field is a bare `number` node only
/// for some grammar productions — for a real one-argument call like
/// `ds~load_uint(32)`, it is a `parenthesized_expression` *wrapping* a
/// `number` (confirmed empirically: `()` zero-arg calls parse `arguments`
/// as `unit_literal` directly, but `(32)` parses as `parenthesized_expression
/// (number)`, a real bug found and fixed while testing — the first synthetic
/// fixture with a
/// non-zero-arg `load_*` call caught it, none of the zero-arg real-corpus
/// calls exercised this path at all). Checks both shapes rather than
/// assuming either.
fn number_literal_in(node: Node, source: &[u8]) -> Option<u32> {
    let number_node = if node.kind() == "number" {
        Some(node)
    } else if node.kind() == "parenthesized_expression" {
        let mut cursor = node.walk();
        node.named_children(&mut cursor)
            .find(|n| n.kind() == "number")
    } else {
        None
    }?;
    number_node.utf8_text(source).ok()?.trim().parse().ok()
}

/// Level 2 storage, inline `get_data()`/`set_data()` form — the ~45% of
/// this crate's research corpus with no `load_data`/`save_data` wrapper
/// pair (this module's own doc comment; `lib.rs`'s module doc comment §
/// "Storage via inline `get_data()`/`set_data()`"). Real, canonical
/// example already vendored: `tests/fixtures/wallet3-code.fc`'s
/// `recv_external` — reads three named fields via `get_data().
/// begin_parse()` + tuple destructuring, later writes transformed
/// versions of the same three values back via `set_data(begin_cell()...
/// end_cell())`, same function, matching arity.
///
/// Returns `Some((field_name, field_shape))` pairs, names from the read
/// side's bound tuple names, types from its `~load_*` calls (the same
/// source of truth [`extract_storage`] uses for the paired form — never
/// from the `store_*` side, so there is exactly one place a field's type
/// comes from). `None` if no function has a same-function read+write pair
/// with matching arity, or if more than one function has a *disagreeing*
/// (different names/types/order) pair — an ambiguity this heuristic
/// reports rather than guesses through, the same "don't guess" discipline
/// [`extract_storage`] already applies to a load/save arity mismatch.
/// Deliberately does **not** attempt to reconcile multiple *partial*
/// readers of the same storage (e.g. `wallet3-code.fc`'s own `seqno()`/
/// `get_public_key()` getters, each reading only part of the struct
/// independently) — cross-call-site reconciliation is explicitly out of
/// v1 scope (`docs/domains/method-dictionary.md`).
#[must_use]
pub fn extract_inline_storage(root: Node, source: &[u8]) -> Option<Vec<(String, TypeShape)>> {
    let mut cursor = root.walk();
    let matches: Vec<Vec<(String, TypeShape)>> = root
        .children(&mut cursor)
        .filter(|node| node.kind() == "function_definition")
        .filter_map(|func| extract_inline_storage_from_function(func, source))
        .collect();

    match matches.split_first() {
        None => None,
        Some((first, rest)) => {
            if rest.iter().all(|candidate| candidate == first) {
                Some(first.clone())
            } else {
                // More than one function has a same-function read+write
                // pair, and they disagree — ambiguous, not guessed through.
                None
            }
        }
    }
}

/// One function's own inline-storage candidate, or `None` if it has no
/// same-function `get_data().begin_parse()` read bound to a name, no
/// tuple-destructure of that exact name via `~load_*` calls, or no
/// `set_data(begin_cell()...)` call whose `store_*` count matches the
/// read side's field count.
fn extract_inline_storage_from_function(
    func: Node,
    source: &[u8],
) -> Option<Vec<(String, TypeShape)>> {
    let body = func.child_by_field_name("body")?;
    let bound_name = find_get_data_parse_binding(body, source)?;
    let read_fields = find_matching_tuple_read(body, source, &bound_name)?;
    let store_arity = find_set_data_store_arity(body, source)?;
    if read_fields.is_empty() || read_fields.len() != store_arity {
        return None;
    }
    Some(read_fields)
}

/// Finds a statement of the exact shape `var <name> = get_data().
/// begin_parse();` — a plain single-identifier binding to that exact
/// two-call chain, flat (this grammar has no receiver field on
/// `method_call`, so the chain's pieces are siblings, not nested — see
/// [`extract_load_data_shapes`]'s own doc comment for the same
/// flattening, confirmed empirically for this exact chain while building
/// this function via a real parse-tree dump of `wallet3-code.fc`).
/// Returns the bound variable's name (e.g. `"ds"`) — other, more
/// elaborate ways of reaching the same slice (an extra hop, a
/// differently-ordered chain) are a conscious v1 scope limit, not
/// attempted.
fn find_get_data_parse_binding(body: Node, source: &[u8]) -> Option<String> {
    let mut cursor = body.walk();
    for stmt in body.named_children(&mut cursor) {
        if stmt.kind() != "expression_statement" {
            continue;
        }
        let mut inner_cursor = stmt.walk();
        let children: Vec<Node> = stmt.named_children(&mut inner_cursor).collect();
        let [decl, first, second] = children.as_slice() else {
            continue;
        };
        if decl.kind() != "variable_declaration" {
            continue;
        }
        let Some(variable) = decl.child_by_field_name("variable") else {
            continue;
        };
        if variable.kind() != "identifier" {
            continue;
        }
        let is_get_data = first.kind() == "function_application"
            && first
                .child_by_field_name("function")
                .and_then(|n| n.utf8_text(source).ok())
                == Some("get_data");
        let is_begin_parse = second.kind() == "method_call"
            && second
                .child_by_field_name("method_name")
                .and_then(|n| n.utf8_text(source).ok())
                == Some("begin_parse");
        if is_get_data && is_begin_parse {
            return variable.utf8_text(source).ok().map(str::to_string);
        }
    }
    None
}

/// Finds a tuple-destructure statement (`var (n1, ..., nK) = (<bound_name>
/// ~load_A(...), ...)`) whose every `~load_*` call's receiver identifier
/// is exactly `bound_name` — the receiver check is what keeps this from
/// picking up an unrelated tuple read from a *different* slice in the
/// same function (`wallet3-code.fc`'s own `recv_external` destructures
/// *two* tuples: one from the inbound message's `cs`, one from storage's
/// `ds` — only the latter must match). Reuses the same alternating
/// `identifier`-then-`method_call` shape [`extract_load_data_shapes`]
/// already recognizes for a return-tuple's flat children, applied here to
/// a `var (...) = (...)` statement's right-hand side instead.
fn find_matching_tuple_read(
    body: Node,
    source: &[u8],
    bound_name: &str,
) -> Option<Vec<(String, TypeShape)>> {
    let mut cursor = body.walk();
    for stmt in body.named_children(&mut cursor) {
        if stmt.kind() != "expression_statement" {
            continue;
        }
        let mut inner_cursor = stmt.walk();
        let children: Vec<Node> = stmt.named_children(&mut inner_cursor).collect();
        let [decl, rhs] = children.as_slice() else {
            continue;
        };
        if decl.kind() != "variable_declaration" {
            continue;
        }
        let Some(variable) = decl.child_by_field_name("variable") else {
            continue;
        };
        if !matches!(variable.kind(), "tensor_expression" | "tuple_expression") {
            continue;
        }
        let mut names_cursor = variable.walk();
        let field_names: Vec<String> = variable
            .named_children(&mut names_cursor)
            .filter_map(|n| {
                (n.kind() == "identifier")
                    .then(|| n.utf8_text(source).ok().map(str::to_string))
                    .flatten()
            })
            .collect();
        if field_names.is_empty() || field_names.len() != variable.named_child_count() {
            // Empty, or a non-identifier member (e.g. `_`) in the LHS
            // tuple — not the shape this heuristic recognizes.
            continue;
        }

        let rhs_node = unwrap_single_expression(*rhs);
        if !matches!(rhs_node.kind(), "tensor_expression" | "tuple_expression") {
            continue;
        }
        let mut rhs_cursor = rhs_node.walk();
        let rhs_children: Vec<Node> = rhs_node.named_children(&mut rhs_cursor).collect();
        if rhs_children.len() != field_names.len() * 2 {
            continue;
        }

        let mut field_shapes = Vec::with_capacity(field_names.len());
        let mut matched = true;
        let (pairs, _) = rhs_children.as_chunks::<2>();
        for [receiver, call] in pairs {
            let receiver_matches =
                receiver.kind() == "identifier" && receiver.utf8_text(source) == Ok(bound_name);
            let call_is_load = call.kind() == "method_call" && is_load_like_call(*call, source);
            if !receiver_matches || !call_is_load {
                matched = false;
                break;
            }
            field_shapes.push(method_call_to_shape(*call, source));
        }
        if matched {
            return Some(field_names.into_iter().zip(field_shapes).collect());
        }
    }
    None
}

/// Finds a `set_data(begin_cell()...store_*(...)...end_cell())` call and
/// returns how many `store_*` calls it makes — arity only, per
/// [`extract_inline_storage`]'s own doc comment on why field identity
/// isn't cross-checked against the read side beyond count (the same
/// "arity, not content" trust boundary [`extract_storage`] already uses
/// for the paired `load_data`/`save_data` form).
fn find_set_data_store_arity(body: Node, source: &[u8]) -> Option<usize> {
    let mut cursor = body.walk();
    for stmt in body.named_children(&mut cursor) {
        if stmt.kind() != "expression_statement" {
            continue;
        }
        let mut inner_cursor = stmt.walk();
        let children: Vec<Node> = stmt.named_children(&mut inner_cursor).collect();
        let [call] = children.as_slice() else {
            continue;
        };
        if call.kind() != "function_application" {
            continue;
        }
        let is_set_data = call
            .child_by_field_name("function")
            .and_then(|n| n.utf8_text(source).ok())
            == Some("set_data");
        if !is_set_data {
            continue;
        }
        let Some(chain) = call.child_by_field_name("arguments") else {
            continue;
        };
        let mut chain_cursor = chain.walk();
        let store_count = chain
            .named_children(&mut chain_cursor)
            .filter(|node| node.kind() == "method_call")
            .filter(|node| is_store_like_call(*node, source))
            .count();
        if store_count > 0 {
            return Some(store_count);
        }
    }
    None
}

/// `true` if `call`'s method name looks like a stdlib store function
/// (`store_*`) — the write-side mirror of [`is_load_like_call`].
fn is_store_like_call(call: Node, source: &[u8]) -> bool {
    call.child_by_field_name("method_name")
        .and_then(|n| n.utf8_text(source).ok())
        .is_some_and(|name| name.starts_with("store_"))
}

fn extract_parameter_names(func: Node, source: &[u8]) -> Vec<String> {
    let Some(args) = func.child_by_field_name("arguments") else {
        return Vec::new();
    };
    let mut cursor = args.walk();
    args.named_children(&mut cursor)
        .filter(|node| node.kind() == "parameter_declaration")
        .filter_map(|param| {
            param
                .child_by_field_name("name")
                .and_then(|n| n.utf8_text(source).ok())
                .map(str::to_string)
        })
        .collect()
}

/// Level 2 storage, paired **any-name** wrapper-function form — the third
/// real idiom found while validating against the mainnet walk
/// (`docs/domains/method-dictionary.md`): a `load_data`/
/// `save_data`-shaped pair of functions, structurally identical to what
/// [`extract_storage`] already recognizes, except under a **different**
/// name (real, deployed example: `load_storage`/`save_storage`,
/// `wallet-contract/func/simple-subscription-plugin.fc`, `ton-blockchain/
/// wallet-contract`). [`extract_storage`] can't see this at all — it looks
/// for the two literal names `load_data`/`save_data`; this function
/// recognizes the same *shape* under any names, structurally, the same way
/// [`extract_inline_storage`] already recognizes the inline form without
/// caring what the enclosing function is called.
///
/// Finds every function whose `return_statement` is a tuple of `~load_*`
/// calls on a name this same function bound via `get_data().begin_parse()`
/// (the get-storage signal — reuses [`find_get_data_parse_binding`], the
/// same receiver-identity check [`extract_inline_storage`]'s read side
/// already applies, so an unrelated function returning some other
/// `~load_*`-shaped tuple, e.g. one parsing a message body, is not
/// mistaken for a storage getter just because its return shape matches).
/// Separately finds every function whose body's main statement is
/// `set_data(begin_cell()...store_*(...)...end_cell())`, arity matching
/// its own named parameter count (the set-storage signal, reusing
/// [`find_set_data_store_arity`]). If **exactly one** of each exists in the
/// file, with matching arity, they are paired: names from the setter's
/// parameters, types from the getter's `~load_*` calls — the same "types
/// only ever come from the read side" trust boundary [`extract_storage`]
/// already uses. Zero or more than one candidate on either side is
/// ambiguous — `None`, not a guess, the same discipline
/// [`extract_inline_storage`]'s own multi-function case already applies.
#[must_use]
pub fn extract_named_pair_storage(root: Node, source: &[u8]) -> Option<Vec<(String, TypeShape)>> {
    let mut cursor = root.walk();
    let functions: Vec<Node> = root
        .children(&mut cursor)
        .filter(|node| node.kind() == "function_definition")
        .collect();

    let mut getters: Vec<Vec<TypeShape>> = Vec::new();
    for &func in &functions {
        if let Some(shapes) = extract_get_data_return_shapes(func, source) {
            getters.push(shapes);
        }
    }
    let [shapes] = getters.as_slice() else {
        return None;
    };

    let mut setters: Vec<Vec<String>> = Vec::new();
    for &func in &functions {
        let Some(body) = func.child_by_field_name("body") else {
            continue;
        };
        let Some(store_arity) = find_set_data_store_arity(body, source) else {
            continue;
        };
        let param_names = extract_parameter_names(func, source);
        if store_arity > 0 && param_names.len() == store_arity {
            setters.push(param_names);
        }
    }
    let [names] = setters.as_slice() else {
        return None;
    };

    if shapes.is_empty() || shapes.len() != names.len() {
        return None;
    }
    Some(names.iter().cloned().zip(shapes.iter().cloned()).collect())
}

/// A function's own storage-getter candidacy for
/// [`extract_named_pair_storage`]: `Some(shapes)` if its body binds
/// `get_data().begin_parse()` to a name (see
/// [`find_get_data_parse_binding`]) and its `return_statement` is a tuple
/// of `~load_*` calls all on that exact bound name — the same return-shape
/// unwrapping [`extract_load_data_shapes`] already handles (redundant
/// parens, the assign-then-return idiom), reused here rather than
/// duplicated, plus the receiver-identity check that function doesn't need
/// (its caller already trusts the literal `load_data` name).
fn extract_get_data_return_shapes(func: Node, source: &[u8]) -> Option<Vec<TypeShape>> {
    let body = func.child_by_field_name("body")?;
    let bound_name = find_get_data_parse_binding(body, source)?;

    let mut cursor = body.walk();
    let return_stmt = body
        .named_children(&mut cursor)
        .find(|node| node.kind() == "return_statement")?;

    let mut top_level_cursor = return_stmt.walk();
    let top_level: Vec<Node> = return_stmt.named_children(&mut top_level_cursor).collect();
    let search_node = match top_level.as_slice() {
        [single]
            if matches!(
                single.kind(),
                "parenthesized_expression" | "tensor_expression" | "tuple_expression"
            ) =>
        {
            *single
        }
        [single] if single.kind() == "identifier" => {
            let name = single.utf8_text(source).ok()?;
            let assigned = resolve_local_assignment(body, return_stmt, name, source)?;
            unwrap_single_expression(assigned)
        }
        _ => return_stmt,
    };

    let mut children_cursor = search_node.walk();
    let children: Vec<Node> = search_node.named_children(&mut children_cursor).collect();

    // Every identifier among the flat children must be the get_data()-bound
    // name — the signal that these load_* calls really read *this
    // function's own* storage slice, not some other slice entirely
    // (a message body, a different helper's local variable, ...).
    let every_identifier_is_bound_name = children
        .iter()
        .filter(|node| node.kind() == "identifier")
        .all(|node| node.utf8_text(source) == Ok(bound_name.as_str()));
    if !every_identifier_is_bound_name {
        return None;
    }

    let calls: Vec<Node> = children
        .into_iter()
        .filter(|node| node.kind() == "method_call" && is_load_like_call(*node, source))
        .collect();
    if calls.is_empty() {
        return None;
    }
    Some(
        calls
            .into_iter()
            .map(|call| method_call_to_shape(call, source))
            .collect(),
    )
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "docs/ARCHITECTURE.md §3.2 scopes the no-unwrap/expect/panic policy to library code outside tests"
)]
mod tests {
    use super::extract_storage;
    use sting_abi_core::TypeShape;

    fn parse(source: &str) -> tree_sitter::Tree {
        let mut parser = tree_sitter::Parser::new();
        parser
            .set_language(&tree_sitter_func::LANGUAGE.into())
            .expect("grammar must load");
        parser.parse(source, None).expect("must parse")
    }

    /// Locks in a real, fixed bug: a `load_uint(32)`-
    /// style call with a real numeric argument parses that argument as
    /// `parenthesized_expression(number)`, not a bare `number` — the
    /// zero-arg calls in this crate's real-corpus fixtures
    /// (`load_coins()`, `load_msg_addr()`) never exercised this path, only
    /// a hand-authored synthetic fixture with a width-bearing call caught
    /// it.
    #[test]
    fn width_bearing_load_calls_recover_the_real_bit_width() {
        let source = "(int, int) load_data() inline { slice ds = get_data().begin_parse(); return (ds~load_uint(32), ds~load_int(8)); } () save_data(int a, int b) impure inline { set_data(begin_cell().end_cell()); }";
        let tree = parse(source);
        let fields = extract_storage(tree.root_node(), source.as_bytes())
            .expect("paired form must be recognized");
        assert_eq!(
            fields[0],
            (
                "a".to_string(),
                TypeShape::Int {
                    bits: Some(32),
                    signed: false
                }
            )
        );
        assert_eq!(
            fields[1],
            (
                "b".to_string(),
                TypeShape::Int {
                    bits: Some(8),
                    signed: true
                }
            )
        );
    }

    /// Locks in the real grail-test defect, found on real, deployed USDT:
    /// `load_data`'s
    /// tuple assigned to an intermediate variable before a bare
    /// `return data;`, not returned directly. Byte-for-byte the same idiom
    /// (`ds~load_coins()`/`ds~load_msg_addr()`/`ds~load_ref()`-chain,
    /// `ds.end_parse()` between the assignment and the return), not a
    /// synthetic simplification.
    #[test]
    fn assign_then_return_storage_is_recognized() {
        let source = "(int, slice, slice, cell, cell) load_data() inline { \
            slice ds = get_data().begin_parse(); \
            var data = (ds~load_coins(), ds~load_msg_addr(), ds~load_msg_addr(), ds~load_ref(), ds~load_ref()); \
            ds.end_parse(); \
            return data; \
        } () save_data(int a, slice b, slice c, cell d, cell e) impure inline { set_data(begin_cell().end_cell()); }";
        let tree = parse(source);
        let fields = extract_storage(tree.root_node(), source.as_bytes())
            .expect("assign-then-return form must now be recognized");
        assert_eq!(
            fields,
            vec![
                ("a".to_string(), TypeShape::Coins),
                ("b".to_string(), TypeShape::Address),
                ("c".to_string(), TypeShape::Address),
                ("d".to_string(), TypeShape::Cell),
                ("e".to_string(), TypeShape::Cell),
            ]
        );
    }

    #[test]
    fn missing_save_data_means_no_storage_recognized() {
        let source = "(int) load_data() inline { slice ds = get_data().begin_parse(); return (ds~load_uint(32)); }";
        let tree = parse(source);
        assert!(extract_storage(tree.root_node(), source.as_bytes()).is_none());
    }

    #[test]
    fn arity_mismatch_between_load_and_save_means_no_storage_recognized() {
        let source = "(int, int) load_data() inline { slice ds = get_data().begin_parse(); return (ds~load_uint(32), ds~load_uint(32)); } () save_data(int a) impure inline { set_data(begin_cell().end_cell()); }";
        let tree = parse(source);
        assert!(
            extract_storage(tree.root_node(), source.as_bytes()).is_none(),
            "2 loaded fields vs 1 save_data param must not guess a mapping"
        );
    }

    #[test]
    fn single_field_storage_is_recognized() {
        let source = "(int) load_data() inline { slice ds = get_data().begin_parse(); return (ds~load_coins()); } () save_data(int balance) impure inline { set_data(begin_cell().end_cell()); }";
        let tree = parse(source);
        let fields = extract_storage(tree.root_node(), source.as_bytes())
            .expect("single-field paired form must be recognized");
        assert_eq!(fields, vec![("balance".to_string(), TypeShape::Coins)]);
    }

    /// Real bug found by review, reproduced with a CLI run against this
    /// exact source: an unrelated dot-call (`.begin_parse()`) sharing the
    /// tuple with a real `~load_coins()` was counted as a phantom second
    /// field (mapped to `Unknown`, presented as if genuinely recognized)
    /// because `method_call` has no grammar-level way to distinguish
    /// `~foo()` from `.foo()`. Fixed by requiring the method name itself
    /// look like a load call (`load_*`/`preload_*`).
    #[test]
    fn an_unrelated_dot_call_in_the_same_tuple_is_not_counted_as_a_field() {
        let source = "(int, slice) load_data() inline { slice ds = get_data().begin_parse(); return (ds~load_coins(), get_ref().begin_parse()); } () save_data(int balance, slice extra) impure inline { set_data(begin_cell().end_cell()); }";
        let tree = parse(source);
        assert!(
            extract_storage(tree.root_node(), source.as_bytes()).is_none(),
            "1 real load call vs 2 save_data params must not guess a mapping \
             (the whole point: begin_parse() must not be counted as a field)"
        );
    }

    /// Real bug found by review, reproduced with a CLI run: a bodyless
    /// forward declaration (`(int, slice) load_data();`, real `FunC`
    /// syntax) ahead of the real definition made `find_function_named`
    /// return the declaration (no `body` field) instead of the real
    /// definition later in the same file, silently reporting no storage
    /// at all despite a fully-recognizable pair being present.
    #[test]
    fn a_forward_declaration_does_not_shadow_the_real_definition() {
        let source = "(int) load_data(); () save_data(int); (int) load_data() inline { slice ds = get_data().begin_parse(); return (ds~load_coins()); } () save_data(int balance) impure inline { set_data(begin_cell().end_cell()); }";
        let tree = parse(source);
        let fields = extract_storage(tree.root_node(), source.as_bytes())
            .expect("the real, later definition must still be found");
        assert_eq!(fields, vec![("balance".to_string(), TypeShape::Coins)]);
    }

    mod inline_storage {
        use super::super::extract_inline_storage;
        use super::parse;
        use sting_abi_core::TypeShape;

        /// A smaller synthetic version of the real pattern this function
        /// exists for (`tests/fixtures/wallet3-code.fc`'s `recv_external`,
        /// pinned end-to-end in `tests/real_extraction.rs`'s
        /// `wallet3_recovers_two_get_methods_and_inline_storage`) — same
        /// shape, fewer fields, kept here as a fast, dependency-free unit
        /// test of the parser-level logic alone.
        #[test]
        fn a_same_function_read_write_pair_is_recognized() {
            let source = "() foo() impure { \
                var ds = get_data().begin_parse(); \
                var (a, b) = (ds~load_uint(32), ds~load_coins()); \
                set_data(begin_cell().store_uint(a, 32).store_coins(b).end_cell()); \
            }";
            let tree = parse(source);
            let fields = extract_inline_storage(tree.root_node(), source.as_bytes())
                .expect("same-function read+write pair with matching arity must be recognized");
            assert_eq!(
                fields,
                vec![
                    (
                        "a".to_string(),
                        TypeShape::Int {
                            bits: Some(32),
                            signed: false
                        }
                    ),
                    ("b".to_string(), TypeShape::Coins),
                ]
            );
        }

        #[test]
        fn arity_mismatch_between_read_and_write_means_no_storage_recognized() {
            let source = "() foo() impure { \
                var ds = get_data().begin_parse(); \
                var (a, b) = (ds~load_uint(32), ds~load_coins()); \
                set_data(begin_cell().store_uint(a, 32).end_cell()); \
            }";
            let tree = parse(source);
            assert!(
                extract_inline_storage(tree.root_node(), source.as_bytes()).is_none(),
                "2 read fields vs 1 store_* call must not guess a mapping"
            );
        }

        /// The exact real risk this function's own doc comment names:
        /// `wallet3-code.fc`'s `recv_external` destructures *two* tuples in
        /// one function, one from the inbound message (`cs`), one from
        /// storage (`ds`) — only a tuple-read from the `get_data()`-bound
        /// name must count.
        #[test]
        fn a_tuple_read_from_a_different_variable_is_not_mistaken_for_storage() {
            let source = "() foo() impure { \
                var ds = get_data().begin_parse(); \
                var (a, b) = (cs~load_uint(32), cs~load_coins()); \
                set_data(begin_cell().store_uint(a, 32).store_coins(b).end_cell()); \
            }";
            let tree = parse(source);
            assert!(
                extract_inline_storage(tree.root_node(), source.as_bytes()).is_none(),
                "a tuple read from `cs`, not the get_data()-bound `ds`, must not be recognized"
            );
        }

        #[test]
        fn a_read_with_no_set_data_at_all_means_no_storage_recognized() {
            let source = "() foo() impure { \
                var ds = get_data().begin_parse(); \
                var (a, b) = (ds~load_uint(32), ds~load_coins()); \
            }";
            let tree = parse(source);
            assert!(extract_inline_storage(tree.root_node(), source.as_bytes()).is_none());
        }

        /// Two functions with a genuinely *disagreeing* read+write pair —
        /// the ambiguity `extract_inline_storage`'s own doc comment says
        /// must be reported, not guessed through (mirrors
        /// `extract_storage`'s own arity-mismatch "don't guess" discipline
        /// for the paired form).
        #[test]
        fn two_functions_with_disagreeing_patterns_are_ambiguous() {
            let source = "() foo() impure { \
                var ds = get_data().begin_parse(); \
                var (a, b) = (ds~load_uint(32), ds~load_coins()); \
                set_data(begin_cell().store_uint(a, 32).store_coins(b).end_cell()); \
            } () bar() impure { \
                var ds = get_data().begin_parse(); \
                var (x, y, z) = (ds~load_uint(8), ds~load_coins(), ds~load_msg_addr()); \
                set_data(begin_cell().store_uint(x, 8).store_coins(y).store_slice(z).end_cell()); \
            }";
            let tree = parse(source);
            assert!(
                extract_inline_storage(tree.root_node(), source.as_bytes()).is_none(),
                "two functions recognizing different storage shapes must not silently pick one"
            );
        }

        /// The flip side of the above: two functions that happen to
        /// recognize the *same* shape agree, not conflict — this is not a
        /// contract this function's callers currently rely on (real
        /// contracts only ever have one such function), but pins the
        /// intended "disagreement is ambiguous, agreement is not" rule
        /// precisely rather than leaving it only implicit in the
        /// disagreement test above.
        #[test]
        fn two_functions_with_agreeing_patterns_are_not_ambiguous() {
            let source = "() foo() impure { \
                var ds = get_data().begin_parse(); \
                var (a, b) = (ds~load_uint(32), ds~load_coins()); \
                set_data(begin_cell().store_uint(a, 32).store_coins(b).end_cell()); \
            } () bar() impure { \
                var ds = get_data().begin_parse(); \
                var (a, b) = (ds~load_uint(32), ds~load_coins()); \
                set_data(begin_cell().store_uint(a, 32).store_coins(b).end_cell()); \
            }";
            let tree = parse(source);
            let fields = extract_inline_storage(tree.root_node(), source.as_bytes()).expect(
                "two functions agreeing on the same shape must not be treated as ambiguous",
            );
            assert_eq!(fields.len(), 2);
        }
    }

    mod named_pair_storage {
        use super::super::extract_named_pair_storage;
        use super::parse;
        use sting_abi_core::TypeShape;

        /// A smaller synthetic version of the real pattern this function
        /// exists for (`wallet-contract/func/simple-subscription-plugin.fc`'s
        /// real `load_storage()`/`save_storage(...)` pair, pinned end-to-end
        /// against the real file during this function's own development —
        /// not committed as an automated test, network-dependent).
        #[test]
        fn a_paired_wrapper_pair_under_any_name_is_recognized() {
            let source = "(int, int) load_storage() inline { \
                var ds = get_data().begin_parse(); \
                return (ds~load_uint(32), ds~load_coins()); \
            } () save_storage(int a, int b) impure inline { \
                set_data(begin_cell().store_uint(a, 32).store_coins(b).end_cell()); \
            }";
            let tree = parse(source);
            let fields = extract_named_pair_storage(tree.root_node(), source.as_bytes()).expect(
                "a load_storage/save_storage pair must be recognized like load_data/save_data",
            );
            assert_eq!(
                fields,
                vec![
                    (
                        "a".to_string(),
                        TypeShape::Int {
                            bits: Some(32),
                            signed: false
                        }
                    ),
                    ("b".to_string(), TypeShape::Coins),
                ]
            );
        }

        #[test]
        fn a_getter_returning_an_unrelated_slices_load_chain_is_not_mistaken_for_storage() {
            // The exact real risk this function's own doc comment names: a
            // function returning a `~load_*`-shaped tuple that reads some
            // *other* slice (here, a parameter), not this function's own
            // get_data()-bound storage — must not be recognized just
            // because the return shape matches.
            let source = "(int, int) parse_message(slice msg) inline { \
                return (msg~load_uint(32), msg~load_coins()); \
            } () save_storage(int a, int b) impure inline { \
                set_data(begin_cell().store_uint(a, 32).store_coins(b).end_cell()); \
            }";
            let tree = parse(source);
            assert!(
                extract_named_pair_storage(tree.root_node(), source.as_bytes()).is_none(),
                "no get_data()-bound getter exists — must not guess parse_message is one"
            );
        }

        #[test]
        fn arity_mismatch_between_getter_and_setter_means_no_storage_recognized() {
            let source = "(int, int) load_storage() inline { \
                var ds = get_data().begin_parse(); \
                return (ds~load_uint(32), ds~load_coins()); \
            } () save_storage(int a) impure inline { \
                set_data(begin_cell().store_uint(a, 32).end_cell()); \
            }";
            let tree = parse(source);
            assert!(
                extract_named_pair_storage(tree.root_node(), source.as_bytes()).is_none(),
                "2 read fields vs 1 setter param must not guess a mapping"
            );
        }

        #[test]
        fn two_candidate_getters_are_ambiguous() {
            let source = "(int) load_storage_a() inline { \
                var ds = get_data().begin_parse(); \
                return (ds~load_uint(32)); \
            } (int) load_storage_b() inline { \
                var ds = get_data().begin_parse(); \
                return (ds~load_uint(64)); \
            } () save_storage(int a) impure inline { \
                set_data(begin_cell().store_uint(a, 32).end_cell()); \
            }";
            let tree = parse(source);
            assert!(
                extract_named_pair_storage(tree.root_node(), source.as_bytes()).is_none(),
                "two functions both shaped like a storage getter must not silently pick one"
            );
        }

        #[test]
        fn no_setter_at_all_means_no_storage_recognized() {
            let source = "(int) load_storage() inline { \
                var ds = get_data().begin_parse(); \
                return (ds~load_uint(32)); \
            }";
            let tree = parse(source);
            assert!(extract_named_pair_storage(tree.root_node(), source.as_bytes()).is_none());
        }
    }
}
