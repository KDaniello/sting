//! A diagnostic (docs/ARCHITECTURE.md): parses every
//! `.tolk` fixture under `corpus/fixtures/tolk-bench/` and reports `ERROR`/
//! `MISSING` nodes precisely, so the `contract {}` grammar gap can be
//! characterized instead of assumed.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "throwaway diagnostic binary, not shipped library code (docs/ARCHITECTURE.md §3.2)"
)]

use std::fs;
use std::path::Path;

fn main() {
    let fixtures_dir =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../corpus/fixtures/tolk-bench");
    let mut entries: Vec<_> = fs::read_dir(&fixtures_dir)
        .unwrap_or_else(|e| panic!("reading {}: {e}", fixtures_dir.display()))
        .filter_map(Result::ok)
        .filter(|e| e.path().extension().is_some_and(|ext| ext == "tolk"))
        .collect();
    entries.sort_by_key(std::fs::DirEntry::path);

    let mut any_errors = false;

    for entry in entries {
        let path = entry.path();
        let source =
            fs::read_to_string(&path).unwrap_or_else(|e| panic!("reading {}: {e}", path.display()));
        let tree = sting_ast::parse(&source).expect("grammar loads");
        let errors = sting_ast::find_error_nodes(&tree);

        println!("=== {} ===", path.file_name().unwrap().to_string_lossy());
        if errors.is_empty() {
            println!("  OK: no ERROR/MISSING nodes ({} bytes)", source.len());
            continue;
        }

        any_errors = true;
        println!("  {} ERROR/MISSING node(s):", errors.len());
        for node in &errors {
            let start = node.start_position();
            let end = node.end_position();
            let text = node
                .utf8_text(source.as_bytes())
                .unwrap_or("<invalid utf8>");
            let snippet: String = text.chars().take(60).collect();
            println!(
                "    kind={:?} missing={} @ {}:{}..{}:{}  text={:?}",
                node.kind(),
                node.is_missing(),
                start.row + 1,
                start.column + 1,
                end.row + 1,
                end.column + 1,
                snippet
            );
            // Print the immediate parent for context, since ERROR nodes are
            // often more informative alongside what they're embedded in.
            if let Some(parent) = node.parent() {
                println!("      parent kind={:?}", parent.kind());
            }
        }
    }

    if any_errors {
        println!("\nSUMMARY: grammar gap confirmed on real tolk-bench fixtures.");
    } else {
        println!("\nSUMMARY: all fixtures parsed cleanly, no gap detected.");
    }
}
