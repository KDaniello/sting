//! Confirms real `FunC` source parses cleanly through a Rust-side
//! `tree-sitter` binding. `tree-sitter-func` v1.0.0 on crates.io cannot be
//! used directly — it pins `tree-sitter ~0.20.10`, incompatible with this
//! workspace's `tree-sitter = "0.26.11"` (confirmed by trying; see
//! `vendor/tree-sitter-func/README.md` for the exact error and the fix:
//! regenerating the grammar's C tables with a current `tree-sitter-cli` and
//! rewriting the Rust binding to the `tree-sitter-language`-mediated
//! pattern `tree-sitter-tolk` already uses).
//!
//! Fixture provenance: `tests/fixtures/SOURCE.md`.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "docs/ARCHITECTURE.md §3.2 scopes the no-unwrap/expect/panic policy to library code outside tests"
)]

use std::fs;
use std::path::Path;

fn count_error_nodes(node: tree_sitter::Node) -> usize {
    // `is_missing()` matters as much as `is_error()` here: a `MISSING` node
    // is how tree-sitter's error-recovery marks "the grammar expected a
    // token here and synthesized one to keep parsing" — it never becomes an
    // `ERROR`-kinded node, but it's exactly the same class of "this source
    // didn't really parse cleanly" signal `has_error()` aggregates over a
    // whole (sub)tree. Counting only `is_error()` would silently miss that
    // recovery case; doesn't change today's result (all fixtures score 0
    // either way, confirmed), but is the stricter, correct check for
    // whatever real FunC source gets added to the fixtures later.
    let mut count = usize::from(node.is_error() || node.is_missing());
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        count += count_error_nodes(child);
    }
    count
}

#[test]
fn real_func_fixtures_parse_without_error_nodes() {
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&tree_sitter_func::LANGUAGE.into())
        .expect("the vendored FunC grammar must load");

    let fixtures_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let mut checked = 0;

    for entry in fs::read_dir(&fixtures_dir).expect("fixtures directory must exist") {
        let path = entry.expect("readable directory entry").path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("fc") {
            continue;
        }

        let source = fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("reading fixture {}: {e}", path.display()));
        let tree = parser
            .parse(&source, None)
            .unwrap_or_else(|| panic!("parser must produce a tree for {}", path.display()));
        let errors = count_error_nodes(tree.root_node());

        assert_eq!(
            errors,
            0,
            "{} produced {errors} ERROR node(s) — real FunC source must parse cleanly \
             — which is the whole point of parsing it at all)",
            path.display()
        );
        checked += 1;
    }

    assert!(
        checked >= 3,
        "at least 3 real FunC files must parse (docs/domains/abi-ir.md); \
         found only {checked} in tests/fixtures/"
    );
}
