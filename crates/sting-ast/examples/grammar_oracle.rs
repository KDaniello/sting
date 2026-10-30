//! Every file the compiler reads whole and the grammar does not.
//!
//! The grammar's own report can only say what the grammar knows. It cannot say
//! whether a file it choked on is really malformed or merely written in a part
//! of the language it has never seen. Only the compiler can answer that, so the
//! compiler is the oracle: the vendored `tolk` runs over the same corpus, and
//! the divergence between the two answers is the list of what has to close.
//!
//! **The two answers are not symmetric.** A file the grammar reads whole and
//! the compiler refuses is a file with a real defect in it, or one whose
//! meaning needs more than syntax; that is not this tool's subject. A file the
//! compiler reads and the grammar refuses is the grammar's blindness, and every
//! one of them is named here.
//!
//! **How a syntax refusal is told from any other.** The compiler is asked
//! through `TolkCompiler::check`, which runs it in check-only mode with
//! structured errors, and every parse error it reports opens with `expected `.
//! An unknown type or an undefined symbol is a file it read and then disagreed
//! with, and a fixture full of undeclared names is exactly that: valid syntax
//! the compiler will not accept as a program. Only a parse error counts as a
//! refusal of the syntax.
//!
//! Needs the vendored compiler under `vendor/tolk/`, which is gitignored; the
//! run skips with a message when it is absent, the same discipline every
//! real-toolchain test here follows.
//!
//! Run: `cargo run -p sting-ast --example grammar_oracle`
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "a diagnostic binary rather than shipped library code (docs/ARCHITECTURE.md §3.2)"
)]

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use sting_tolkc::{CheckError, TolkCompiler};

/// What the compiler said about a file's syntax.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CompilerVerdict {
    /// It read the file: either compiled it, or refused it for a reason other
    /// than syntax.
    Read,
    /// It refused the syntax, with a position.
    Refused,
}

fn main() {
    let root = repo_root();
    let exe = root.join("vendor/tolk/tolk.exe");
    let exe = if exe.is_file() {
        exe
    } else {
        root.join("vendor/tolk/tolk")
    };
    if !exe.is_file() {
        eprintln!(
            "skipped: no vendored compiler at {} (vendor/tolk/ is gitignored)",
            exe.display()
        );
        return;
    }
    let compiler = TolkCompiler::new(exe);

    let mut files = Vec::new();
    collect_tolk(&root.join("corpus"), &mut files);
    files.sort();

    let mut diverging: Vec<(PathBuf, usize)> = Vec::new();
    let mut both_refuse = 0_usize;
    let mut grammar_clean = 0_usize;
    let mut too_permissive: Vec<PathBuf> = Vec::new();

    for path in &files {
        let Ok(source) = std::fs::read_to_string(path) else {
            continue;
        };
        let tree = sting_ast::parse(&source).expect("the Tolk grammar loads");
        let unparsed = sting_ast::first_unparsed(&tree);

        match (&unparsed, compiler_verdict(&compiler, path)) {
            (None, CompilerVerdict::Read) => grammar_clean += 1,
            (None, CompilerVerdict::Refused) => {
                grammar_clean += 1;
                too_permissive.push(path.clone());
            }
            (Some(region), CompilerVerdict::Read) => {
                diverging.push((path.clone(), region.range.start_line as usize));
            }
            (Some(_), CompilerVerdict::Refused) => both_refuse += 1,
        }
    }

    println!("# The compiler against the grammar\n");
    println!("- corpus files: **{}**", files.len());
    println!("- the grammar reads whole: **{grammar_clean}**");
    println!("- both refuse: **{both_refuse}** — a real defect, not our blindness");
    println!(
        "- **the compiler reads and the grammar refuses: {}**",
        diverging.len()
    );
    println!(
        "- the grammar reads and the compiler refuses: **{}** — the other direction, \
         a grammar too permissive rather than too narrow",
        too_permissive.len()
    );

    if !too_permissive.is_empty() {
        println!("\n## What the grammar reads and the compiler will not\n");
        for path in &too_permissive {
            println!("- `{}`", relative(&root, path));
        }
    }

    if diverging.is_empty() {
        println!("\nNothing diverges. The grammar reads everything the compiler reads.");
        return;
    }

    println!("\n## What the grammar cannot read\n");
    let mut by_construct: BTreeMap<String, Vec<&PathBuf>> = BTreeMap::new();
    for (path, _) in &diverging {
        let source = std::fs::read_to_string(path).unwrap_or_default();
        let tree = sting_ast::parse(&source).expect("the Tolk grammar loads");
        by_construct
            .entry(first_token(&source, &tree))
            .or_default()
            .push(path);
    }
    println!("| Construct | Files |");
    println!("|---|---|");
    let mut rows: Vec<(&String, &Vec<&PathBuf>)> = by_construct.iter().collect();
    rows.sort_by_key(|(name, files)| (std::cmp::Reverse(files.len()), (*name).clone()));
    for (name, files) in rows {
        println!("| `{name}` | {} |", files.len());
    }

    println!("\n## By name\n");
    for (path, line) in &diverging {
        println!("- `{}`:{line}", relative(&root, path));
    }
}

/// Asks the compiler whether it can read `path`'s syntax.
///
/// An integration failure — no binary, a timeout, output that will not parse —
/// says nothing about the source, so it counts as read rather than inventing a
/// refusal the compiler never made.
fn compiler_verdict(compiler: &TolkCompiler, path: &Path) -> CompilerVerdict {
    let project = path.parent().unwrap_or(Path::new("."));
    let entrypoint = Path::new(path.file_name().unwrap_or_default());
    match compiler.check(project, entrypoint) {
        Ok(errors) if errors.iter().any(CheckError::is_parse_error) => CompilerVerdict::Refused,
        _ => CompilerVerdict::Read,
    }
}

/// The first significant token of the file's first unreadable region — the name
/// of the construct the grammar does not know.
fn first_token(source: &str, tree: &tree_sitter::Tree) -> String {
    let Some(region) = sting_ast::first_unparsed(tree) else {
        return "<none>".to_string();
    };
    let text = source
        .get(region.range.start_byte..region.range.end_byte)
        .unwrap_or_default();
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
            line.chars().take(2).collect()
        } else {
            first
        };
    }
    "<empty>".to_string()
}

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn relative(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

fn collect_tolk(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.filter_map(Result::ok) {
        let path = entry.path();
        if path.is_dir() {
            collect_tolk(&path, out);
        } else if path.extension().is_some_and(|ext| ext == "tolk") {
            out.push(path);
        }
    }
}
