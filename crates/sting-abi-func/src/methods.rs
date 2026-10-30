//! Level 1: `get`-method signatures — `method_id`-specified top-level
//! `function_definition`s, the same subset `ton-blockchain/verifier`'s
//! `getterParser.ts` recovers (`docs/abi-layer/blocks/
//! 04-func-heuristic-extractor.md` §4.2). `tvm_method_id` is computed by
//! [`crate::default_method_id`] (TON's standard CRC16 convention) unless
//! the source gives an explicit `method_id(N)` override.

use tree_sitter::Node;

use crate::default_method_id;
use crate::type_text::{map_type_text, split_top_level_components};

/// One recovered get-method, before its types are interned into a
/// [`sting_abi_core::ContractSchema`]'s type table (that step needs a
/// mutable interner shared across the whole extraction, so it stays in
/// `lib.rs`, not here).
pub struct RawGetMethod {
    pub name: String,
    pub tvm_method_id: i64,
    /// Parameter `(name, raw type text)` pairs, in declaration order.
    pub parameters: Vec<(String, sting_abi_core::TypeShape)>,
    /// The return type's top-level components, already split (see
    /// [`crate::type_text::split_top_level_components`]) — empty for a
    /// bare `()`/unit return, one element for a plain single-value return,
    /// more than one for a tensor return (`lib.rs` wraps that case in a
    /// [`sting_abi_core::TypeShape::Tensor`] once each component is
    /// interned, which needs the shared type table this module doesn't
    /// have access to).
    pub return_shapes: Vec<sting_abi_core::TypeShape>,
}

/// Walks every top-level `function_definition` under `root` and returns
/// the ones carrying a `method_id` specifier. Only top-level definitions —
/// `FunC` has no nested function definitions, matching `getterParser.ts`'s
/// own scope.
#[must_use]
pub fn extract_get_methods(root: Node, source: &[u8]) -> Vec<RawGetMethod> {
    let mut cursor = root.walk();
    root.children(&mut cursor)
        .filter(|node| node.kind() == "function_definition")
        .filter_map(|node| extract_one(node, source))
        .collect()
}

fn extract_one(node: Node, source: &[u8]) -> Option<RawGetMethod> {
    let method_id_node = specifier_method_id(node)?;

    let name_node = node.child_by_field_name("name")?;
    let name = node_text(name_node, source)?.to_string();

    let tvm_method_id = explicit_method_id_value(method_id_node, source)
        .unwrap_or_else(|| default_method_id(&name));

    let parameters = node
        .child_by_field_name("arguments")
        .map(|args| extract_parameters(args, source))
        .unwrap_or_default();

    let return_shapes = field_span_text(node, "return_type", source)
        .map(|text| {
            split_top_level_components(text)
                .into_iter()
                .map(map_type_text)
                .collect()
        })
        .unwrap_or_default();

    Some(RawGetMethod {
        name,
        tvm_method_id,
        parameters,
        return_shapes,
    })
}

/// Finds the `method_id` specifier node among `function_definition`'s
/// `specifiers` field children, if present — a function without one isn't
/// a get-method by this extractor's definition (matches `getterParser.ts`'s
/// own filter).
fn specifier_method_id(function_def: Node) -> Option<Node> {
    let mut cursor = function_def.walk();
    function_def
        .children_by_field_name("specifiers", &mut cursor)
        .find(|node| node.kind() == "method_id")
}

/// `method_id`'s own optional `number` child — an explicit numeric
/// override (`method_id(100)`), distinct from the bare `method_id`
/// specifier (no child at all), which falls back to the CRC16 default.
/// Hex literals (`0x...`) parse via `i64::from_str_radix`, matching
/// `FunC`'s own numeric literal syntax.
fn explicit_method_id_value(method_id_node: Node, source: &[u8]) -> Option<i64> {
    let mut cursor = method_id_node.walk();
    let number_node = method_id_node
        .children(&mut cursor)
        .find(|node| node.kind() == "number")?;
    let text = node_text(number_node, source)?;
    parse_func_integer(text)
}

/// Parses a `FunC` integer literal — decimal or `0x`/`0X`-prefixed hex, the
/// two forms actually used for op-codes/method-ids in real source (see this
/// block's own pre-implementation research). A leading `-` (`method_id(-5)`)
/// *is* part of this grammar's own `number` token text (confirmed via
/// `to_sexp()` — not a separate unary-minus node), and both `i64::parse`/
/// `i64::from_str_radix` below already handle it correctly without any
/// special-casing; every real `method_id(N)` override actually observed in
/// the research corpus happened to be non-negative, and nothing here assumes
/// that.
fn parse_func_integer(text: &str) -> Option<i64> {
    let trimmed = text.trim();
    if let Some(hex) = trimmed
        .strip_prefix("0x")
        .or_else(|| trimmed.strip_prefix("0X"))
    {
        i64::from_str_radix(hex, 16).ok()
    } else {
        trimmed.parse::<i64>().ok()
    }
}

fn extract_parameters(
    parameter_list: Node,
    source: &[u8],
) -> Vec<(String, sting_abi_core::TypeShape)> {
    let mut cursor = parameter_list.walk();
    parameter_list
        .named_children(&mut cursor)
        .filter(|node| node.kind() == "parameter_declaration")
        .map(|param| {
            let name = param
                .child_by_field_name("name")
                .and_then(|n| node_text(n, source))
                .unwrap_or("_")
                .to_string();
            let shape = field_span_text(param, "type", source)
                .map_or(sting_abi_core::TypeShape::Unknown, map_type_text);
            (name, shape)
        })
        .collect()
}

fn node_text<'a>(node: Node, source: &'a [u8]) -> Option<&'a str> {
    node.utf8_text(source).ok()
}

/// Combines the byte span of every child matched by a "multiple" field
/// (`type`/`return_type`, which can spread across several tokens for a
/// tensor/tuple type, e.g. `(int, int)`) into one text slice — `tree-sitter`
/// only gives per-node access, not a pre-joined span, for `multiple` fields.
fn field_span_text<'a>(node: Node, field: &str, source: &'a [u8]) -> Option<&'a str> {
    let mut cursor = node.walk();
    let mut start = None;
    let mut end = None;
    for child in node.children_by_field_name(field, &mut cursor) {
        start = Some(start.map_or(child.start_byte(), |s: usize| s.min(child.start_byte())));
        end = Some(end.map_or(child.end_byte(), |e: usize| e.max(child.end_byte())));
    }
    let (start, end) = (start?, end?);
    std::str::from_utf8(&source[start..end]).ok()
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "docs/ARCHITECTURE.md §3.2 scopes the no-unwrap/expect/panic policy to library code outside tests"
)]
mod tests {
    use super::extract_get_methods;
    use sting_abi_core::TypeShape;

    fn parse(source: &str) -> tree_sitter::Tree {
        let mut parser = tree_sitter::Parser::new();
        parser
            .set_language(&tree_sitter_func::LANGUAGE.into())
            .expect("grammar must load");
        parser.parse(source, None).expect("must parse")
    }

    #[test]
    fn bare_method_id_uses_the_crc16_default() {
        let source = "int seqno() method_id { return 1; }";
        let tree = parse(source);
        let methods = extract_get_methods(tree.root_node(), source.as_bytes());
        assert_eq!(methods.len(), 1);
        assert_eq!(methods[0].tvm_method_id, crate::default_method_id("seqno"));
    }

    /// Real bug class this locks in: `method_id`'s own numeric-override
    /// child is a *direct* `number` child (confirmed via `to_sexp()`),
    /// structurally different from `method_call`'s single-argument shape
    /// (`parenthesized_expression(number)`, the actual bug found and fixed
    /// in `storage.rs`'s own call-argument parsing) — this test confirms
    /// the two are not conflated.
    #[test]
    fn explicit_method_id_override_is_used_verbatim() {
        let source = "int seqno() method_id(100) { return 1; }";
        let tree = parse(source);
        let methods = extract_get_methods(tree.root_node(), source.as_bytes());
        assert_eq!(methods.len(), 1);
        assert_eq!(methods[0].tvm_method_id, 100);
    }

    #[test]
    fn hex_explicit_method_id_override_parses() {
        let source = "int seqno() method_id(0x85143) { return 1; }";
        let tree = parse(source);
        let methods = extract_get_methods(tree.root_node(), source.as_bytes());
        assert_eq!(methods[0].tvm_method_id, 0x85143);
    }

    #[test]
    fn function_without_method_id_is_not_a_get_method() {
        let source = "int helper() { return 1; }";
        let tree = parse(source);
        assert!(extract_get_methods(tree.root_node(), source.as_bytes()).is_empty());
    }

    #[test]
    fn multi_value_tensor_return_splits_into_separate_components() {
        let source = "(int, slice, cell) get_data_tuple() method_id { return (1, \"\", null()); }";
        let tree = parse(source);
        let methods = extract_get_methods(tree.root_node(), source.as_bytes());
        assert_eq!(methods[0].return_shapes.len(), 3);
        assert_eq!(
            methods[0].return_shapes[0],
            TypeShape::Int {
                bits: None,
                signed: true
            }
        );
        assert_eq!(methods[0].return_shapes[1], TypeShape::Slice);
        assert_eq!(methods[0].return_shapes[2], TypeShape::Cell);
    }

    #[test]
    fn parameters_are_recovered_with_names_and_types() {
        let source =
            "slice get_wallet_address(slice owner_address) method_id { return owner_address; }";
        let tree = parse(source);
        let methods = extract_get_methods(tree.root_node(), source.as_bytes());
        assert_eq!(methods[0].parameters.len(), 1);
        assert_eq!(methods[0].parameters[0].0, "owner_address");
        assert_eq!(methods[0].parameters[0].1, TypeShape::Slice);
    }
}
