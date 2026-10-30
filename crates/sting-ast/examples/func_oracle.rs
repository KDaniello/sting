//! Reads real `FunC` and says what the grammar could not.
//!
//! The claim this answers is the milestone's first gate row: every `FunC` file in a published bug
//! bounty scope parses without an `ERROR` node. A grammar that reads most of a language is a
//! grammar that reports most of its defects, and "most" is not a number anyone can act on — so this
//! prints the count and exits non-zero the moment a file is read only in part.
#![allow(
    clippy::print_stdout,
    clippy::print_stderr,
    reason = "an oracle whose entire purpose is to print what it measured"
)]

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use sting_ast::{Language, first_unparsed, parse_as};

fn main() -> ExitCode {
    let roots: Vec<PathBuf> = std::env::args().skip(1).map(PathBuf::from).collect();
    if roots.is_empty() {
        eprintln!("usage: func_oracle <directory>...");
        return ExitCode::FAILURE;
    }

    let mut files = Vec::new();
    for root in &roots {
        if let Err(error) = collect(root, &mut files) {
            eprintln!("cannot read {}: {error}", root.display());
            return ExitCode::FAILURE;
        }
    }
    files.sort();

    let mut unread = 0;
    for file in &files {
        let source = match std::fs::read_to_string(file) {
            Ok(source) => source,
            Err(error) => {
                eprintln!("cannot read {}: {error}", file.display());
                return ExitCode::FAILURE;
            }
        };
        let tree = match parse_as(Language::Func, &source) {
            Ok(tree) => tree,
            Err(error) => {
                eprintln!("cannot parse {}: {error}", file.display());
                return ExitCode::FAILURE;
            }
        };
        if let Some(region) = first_unparsed(&tree) {
            unread += 1;
            println!(
                "{}:{}: {} node, {} in the file",
                file.display(),
                region.range.start_line,
                region.kind,
                region.total
            );
        }
    }

    println!("{} FunC files, {unread} read only in part", files.len());
    if unread == 0 {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

/// Every `FunC` file under `root`, in whichever of its two spellings — the question of which
/// extension belongs to which grammar is answered in one place and asked here.
fn collect(root: &Path, out: &mut Vec<PathBuf>) -> std::io::Result<()> {
    if root.is_file() {
        out.push(root.to_path_buf());
        return Ok(());
    }
    for entry in std::fs::read_dir(root)? {
        let path = entry?.path();
        if std::fs::symlink_metadata(&path).is_ok_and(|meta| meta.file_type().is_symlink()) {
            continue;
        }
        if path.is_dir() {
            collect(&path, out)?;
        } else if Language::of_path(&path) == Some(Language::Func) {
            out.push(path);
        }
    }
    Ok(())
}
