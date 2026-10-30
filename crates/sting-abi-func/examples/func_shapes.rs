//! Prints the tree the vendored `FunC` grammar produces for a handful of shapes.
//!
//! Written to answer one question the grammar's `node-types.json` does not: `FunC` has no
//! `assignment` node, and the detectors that track a write to a variable need to know what it does
//! have instead. Kept because the answer decides how a grammar-neutral surface is written, and a
//! claim about a parse tree that nobody can reproduce is worth nothing.
#![allow(
    clippy::print_stdout,
    clippy::expect_used,
    reason = "docs/ARCHITECTURE.md §3.2 scopes the no-expect policy to library code; this is an               example whose entire purpose is to print a parse tree, and a grammar that will not               load has nothing to print"
)]

use tree_sitter::{Node, Parser};

const SAMPLES: &[(&str, &str)] = &[
    (
        "assignment and augmented assignment",
        "() main() {\n  int a = 1;\n  a = 2;\n  a += 3;\n}\n",
    ),
    (
        "method call, mutating method call, plain call",
        "() main(slice s) {\n  int op = s~load_uint(32);\n  int x = s.preload_uint(8);\n  s.end_parse();\n  throw_unless(101, x > 0);\n}\n",
    ),
    (
        "conditional and return",
        "int f(int a) {\n  if (a > 0) { return 1; } else { return 0; }\n}\n",
    ),
    (
        "if with an else",
        "() f(slice s, int cond) {
  if (cond) { s.end_parse(); } else { s.end_parse(); }
}
",
    ),
    (
        "storage round trip",
        "() save() {\n  set_data(begin_cell().store_uint(1, 32).end_cell());\n}\n",
    ),
];

fn main() {
    let mut parser = Parser::new();
    parser
        .set_language(&tree_sitter_func::LANGUAGE.into())
        .expect("the vendored FunC grammar loads");
    for (label, source) in SAMPLES {
        println!("=== {label} ===");
        let tree = parser.parse(source, None).expect("the sample parses");
        dump(tree.root_node(), source, 0);
        println!();
    }
}

/// One line per named node: its kind, the fields it fills, and the text it covers.
fn dump(node: Node, source: &str, depth: usize) {
    if node.is_named() {
        let text = node
            .utf8_text(source.as_bytes())
            .unwrap_or("<not utf8>")
            .replace('\n', " ");
        let short = text.chars().take(48).collect::<String>();
        println!(
            "{:indent$}{} :: {}",
            "",
            node.kind(),
            short,
            indent = depth * 2
        );
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        dump(child, source, depth + 1);
    }
}
