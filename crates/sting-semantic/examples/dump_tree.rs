//! Prints a Tolk file's parse tree with node kinds and text.
//!
//! It exists because `tree-sitter-tolk`'s node names are documented nowhere,
//! and the inference rules in [`sting_semantic`] have to match real node kinds
//! rather than guessed ones. Every new rule starts with a run of this dumper
//! over real code.
//!
//! Run: `cargo run -p sting-semantic --example dump_tree -- <file> [depth]`
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "a development diagnostic rather than shipped library code (docs/ARCHITECTURE.md §3.2)"
)]

fn main() {
    let mut args = std::env::args().skip(1);
    let path = args.next().expect("give the path to a .tolk file");
    let max_depth: usize = args
        .next()
        .and_then(|d| d.parse().ok())
        .unwrap_or(usize::MAX);

    let source = std::fs::read_to_string(&path).expect("the file reads");
    let tree = sting_ast::parse(&source).expect("the Tolk grammar loads");

    for visited in sting_ast::Preorder::new(&tree) {
        if visited.depth > max_depth {
            continue;
        }
        let node = visited.node;
        let text = source
            .get(node.start_byte()..node.end_byte())
            .unwrap_or("")
            .lines()
            .next()
            .unwrap_or("")
            .trim();
        let text = if text.chars().count() > 60 {
            let cut: String = text.chars().take(57).collect();
            format!("{cut}...")
        } else {
            text.to_string()
        };
        let named = if node.is_named() { "" } else { " (anonymous)" };
        println!(
            "{:indent$}{}{}  [{}:{}]  {text}",
            "",
            node.kind(),
            named,
            node.start_position().row + 1,
            node.start_position().column + 1,
            indent = visited.depth * 2,
        );
    }
}
