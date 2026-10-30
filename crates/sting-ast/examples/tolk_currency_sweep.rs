//! Measures the gap between the grammar and the language.
//!
//! `tree-sitter-tolk 0.0.5` knows Tolk 1.2 while the compiler is at 1.4.2. How
//! badly that breaks parsing is a question that has to be answered with a
//! **number** rather than a feeling: a detector that does not understand a
//! construct does not fail, it silently finds nothing, and no test sees it.
//!
//! Three different things are counted, because each gives a different diagnosis:
//!
//! - **the share of clean files** — how many parse without a single `ERROR` or
//!   `MISSING`;
//! - **the share of bytes under `ERROR`** — one `ERROR` in a rare place and an
//!   `ERROR` over half a file are not the same thing, and counted by file they
//!   are indistinguishable;
//! - **the breakdown by construct** — which token each `ERROR` span starts at,
//!   which is the list of what the grammar does not know.
//!
//! Run: `cargo run -p sting-ast --example tolk_currency_sweep -- <path>...`
//! Without arguments it takes the real corpus and the vendored compiler's
//! standard library.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "a diagnostic binary rather than shipped library code (docs/ARCHITECTURE.md §3.2)"
)]

use std::cmp::Reverse;
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

/// What one file measured.
struct FileReport {
    path: PathBuf,
    bytes: usize,
    /// Bytes covered by top-level `ERROR` and `MISSING` nodes; a nested one is
    /// not counted twice.
    error_bytes: usize,
    error_nodes: usize,
    /// The first significant token of each `ERROR` span — the name of the
    /// construct that was not understood.
    constructs: Vec<String>,
    /// `ERROR` spans holding a declaration of executable code (`fun` or
    /// `get`). This is the question that matters: does the stale grammar lose
    /// code the detectors have to see, or only the declarative wrapping?
    swallowed_code: Vec<String>,
    /// Top-level constructs parsed: a named node that is neither `ERROR` nor
    /// `comment`.
    top_level_ok: usize,
    /// Top-level constructs not parsed: `ERROR` or `MISSING` directly at the
    /// root.
    top_level_bad: usize,
}

fn main() {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    // `--dump` prints every ERROR span line by line, which is what separates
    // "the grammar did not understand the header" from "the grammar did not
    // understand the whole block" — by byte share the two are identical.
    let dump = args.iter().any(|a| a == "--dump");
    args.retain(|a| a != "--dump");
    let roots: Vec<PathBuf> = if args.is_empty() {
        default_roots()
    } else {
        args.into_iter().map(PathBuf::from).collect()
    };

    let mut reports = Vec::new();
    for root in &roots {
        if !root.exists() {
            eprintln!("skipped, not on disk: {}", root.display());
            continue;
        }
        collect_tolk_files(root, &mut reports);
    }
    reports.sort();

    let mut per_group: BTreeMap<String, Vec<FileReport>> = BTreeMap::new();
    for path in reports {
        let report = analyze(&path);
        if dump && report.error_nodes > 0 {
            dump_spans(&path);
        }
        per_group.entry(group_of(&path)).or_default().push(report);
    }
    if dump {
        return;
    }

    let mut all = Vec::new();
    println!("# How current the Tolk grammar is\n");
    for (group, files) in &per_group {
        print_group(group, files);
        all.extend(files.iter());
    }

    println!("\n## The whole measurement\n");
    print_totals(&all);
    print_constructs(&all);
}

/// The sources of real, current Tolk the repository already carries.
fn default_roots() -> Vec<PathBuf> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    vec![
        root.join("corpus/clean/acton-contracts"),
        root.join("corpus/clean/tolk-bench"),
        root.join("corpus/clean/converted"),
        root.join("corpus/clean/synthetic"),
        root.join("vendor/tolk/tolk-stdlib"),
        root.join("corpus/fixtures"),
    ]
}

/// The reporting group: the source directory's name rather than the full path.
fn group_of(path: &Path) -> String {
    let text = path.to_string_lossy().replace('\\', "/");
    for marker in [
        "corpus/clean/acton-contracts",
        "corpus/clean/tolk-bench",
        "corpus/clean/converted",
        "corpus/clean/synthetic",
        "vendor/tolk/tolk-stdlib",
        "corpus/fixtures",
    ] {
        if text.contains(marker) {
            return marker.to_string();
        }
    }
    "other".to_string()
}

fn collect_tolk_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.filter_map(Result::ok) {
        let path = entry.path();
        if path.is_dir() {
            collect_tolk_files(&path, out);
        } else if path.extension().is_some_and(|ext| ext == "tolk") {
            out.push(path);
        }
    }
}

fn analyze(path: &Path) -> FileReport {
    let source =
        fs::read_to_string(path).unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()));
    let tree = sting_ast::parse(&source).expect("the Tolk grammar loads");
    let error_nodes = sting_ast::find_error_nodes(&tree);

    // An ERROR nested inside another is not counted twice: the byte share would
    // otherwise pass 100% and stop meaning anything.
    let mut spans: Vec<(usize, usize)> = error_nodes
        .iter()
        .map(|n| (n.start_byte(), n.end_byte()))
        .collect();
    spans.sort_unstable();
    let mut error_bytes = 0;
    let mut last_end = 0;
    for (start, end) in &spans {
        let start = (*start).max(last_end);
        if end > &start {
            error_bytes += end - start;
            last_end = *end;
        }
    }

    let constructs = error_nodes
        .iter()
        .map(|node| construct_name(&source, node.start_byte(), node.end_byte()))
        .collect();

    let swallowed_code = error_nodes
        .iter()
        .filter_map(|node| {
            let text = source.get(node.start_byte()..node.end_byte())?;
            let hides_code = text.lines().any(|line| {
                let line = line.trim();
                line.starts_with("fun ")
                    || line.starts_with("get ")
                    || line.starts_with("get fun ")
                    || line.contains(") {")
            });
            hides_code.then(|| {
                format!(
                    "{}:{}",
                    path.file_name().unwrap_or_default().to_string_lossy(),
                    node.start_position().row + 1
                )
            })
        })
        .collect();

    // Top-level constructs, because the gate asks for the share of
    // *constructs* rather than of files: one ERROR in a rare place and an ERROR
    // in every function are indistinguishable by file.
    let mut cursor = tree.root_node().walk();
    let mut top_level_ok = 0;
    let mut top_level_bad = 0;
    for child in tree.root_node().named_children(&mut cursor) {
        if child.is_error() || child.is_missing() {
            top_level_bad += 1;
        } else if child.kind() != "comment" {
            top_level_ok += 1;
        }
    }

    FileReport {
        path: path.to_path_buf(),
        bytes: source.len(),
        error_bytes,
        error_nodes: error_nodes.len(),
        constructs,
        swallowed_code,
        top_level_ok,
        top_level_bad,
    }
}

/// The name of the construct that was not understood: the first significant
/// token of the `ERROR` span. Comments and blank lines are skipped — they fall
/// into the span as the previous node's tail and say nothing about it.
fn construct_name(source: &str, start: usize, end: usize) -> String {
    let text = source.get(start..end).unwrap_or("");
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with("//") || line.starts_with('*') {
            continue;
        }
        let first: String = line
            .chars()
            .take_while(|c| c.is_alphanumeric() || *c == '_' || *c == '@')
            .collect();
        return if first.is_empty() {
            let symbol: String = line.chars().take(2).collect();
            format!("<symbol {symbol}>")
        } else {
            first
        };
    }
    "<empty>".to_string()
}

fn print_group(group: &str, files: &[FileReport]) {
    let clean = files.iter().filter(|f| f.error_nodes == 0).count();
    let bytes: usize = files.iter().map(|f| f.bytes).sum();
    let error_bytes: usize = files.iter().map(|f| f.error_bytes).sum();
    println!("## {group}\n");
    println!(
        "Files: **{}**, of them without ERROR: **{}** ({:.1}%). Bytes under ERROR: **{:.1}%** ({error_bytes} of {bytes}).\n",
        files.len(),
        clean,
        percent(clean, files.len()),
        percent(error_bytes, bytes),
    );
    let mut dirty: Vec<&FileReport> = files.iter().filter(|f| f.error_nodes > 0).collect();
    dirty.sort_by_key(|file| Reverse(file.error_bytes));
    for file in dirty.iter().take(10) {
        println!(
            "- `{}` — {} ERROR, {:.1}% of bytes",
            file.path.file_name().unwrap_or_default().to_string_lossy(),
            file.error_nodes,
            percent(file.error_bytes, file.bytes),
        );
    }
    if dirty.len() > 10 {
        println!("- …and {} more", dirty.len() - 10);
    }
    println!();
}

fn print_totals(files: &[&FileReport]) {
    let clean = files.iter().filter(|f| f.error_nodes == 0).count();
    let bytes: usize = files.iter().map(|f| f.bytes).sum();
    let error_bytes: usize = files.iter().map(|f| f.error_bytes).sum();
    let error_nodes: usize = files.iter().map(|f| f.error_nodes).sum();
    println!("- files in total: **{}**", files.len());
    println!(
        "- parsed without a single ERROR or MISSING: **{clean}** ({:.1}%)",
        percent(clean, files.len())
    );
    println!(
        "- source text under ERROR: **{:.1}%** ({error_bytes} bytes of {bytes})",
        percent(error_bytes, bytes)
    );
    println!("- ERROR and MISSING nodes in total: **{error_nodes}**");

    let ok: usize = files.iter().map(|f| f.top_level_ok).sum();
    let bad: usize = files.iter().map(|f| f.top_level_bad).sum();
    println!(
        "- **top-level constructs parsed: {ok} of {} ({:.1}%)** — the gate's number",
        ok + bad,
        percent(ok, ok + bad)
    );

    let swallowed: Vec<&String> = files.iter().flat_map(|f| f.swallowed_code.iter()).collect();
    if swallowed.is_empty() {
        println!(
            "- **not one byte of executable code sits under an ERROR**: every ERROR span \
             holds declarative wrapping alone, and none swallowed a `fun` or `get` declaration"
        );
    } else {
        println!(
            "- **ERROR spans that swallowed a code declaration: {}**",
            swallowed.len()
        );
        for place in swallowed.iter().take(20) {
            println!("  - {place}");
        }
    }
}

fn print_constructs(files: &[&FileReport]) {
    let mut counts: BTreeMap<&str, usize> = BTreeMap::new();
    for file in files {
        for construct in &file.constructs {
            *counts.entry(construct.as_str()).or_default() += 1;
        }
    }
    if counts.is_empty() {
        println!("\nNo construct went unparsed.");
        return;
    }
    let mut sorted: Vec<(&&str, &usize)> = counts.iter().collect();
    sorted.sort_by(|a, b| b.1.cmp(a.1).then(a.0.cmp(b.0)));
    println!("\n### Which token an ERROR span starts at\n");
    println!("| Token | Times |");
    println!("|---|---|");
    for (name, count) in sorted {
        println!("| `{name}` | {count} |");
    }
}

/// Prints every `ERROR` span of a file: its lines, its size and its first line
/// of text. Answers what a byte share hides — whether a header or a body is
/// covered.
fn dump_spans(path: &Path) {
    let source =
        fs::read_to_string(path).unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()));
    let tree = sting_ast::parse(&source).expect("the Tolk grammar loads");
    let total_lines = source.lines().count();
    println!("\n=== {} ({total_lines} lines) ===", path.display());
    for node in sting_ast::find_error_nodes(&tree) {
        let start = node.start_position();
        let end = node.end_position();
        let text = source.get(node.start_byte()..node.end_byte()).unwrap_or("");
        let first = text.lines().next().unwrap_or("").trim();
        let spanned = end.row.saturating_sub(start.row) + 1;
        println!(
            "  {}: lines {}..{} ({spanned} of {total_lines}), {} bytes | {first}",
            node.kind(),
            start.row + 1,
            end.row + 1,
            node.end_byte() - node.start_byte(),
        );
    }
}

fn percent(part: usize, whole: usize) -> f64 {
    if whole == 0 {
        return 0.0;
    }
    #[allow(
        clippy::cast_precision_loss,
        reason = "the counters here are far below 2^53"
    )]
    {
        part as f64 * 100.0 / whole as f64
    }
}
