//! How much the layer actually answers — over the whole real corpus rather
//! than over the one contract the gate checks.
//!
//! The gate checks the quality of the answers on one contract, by hand. What it
//! cannot check is **coverage**: a layer that answers `Unknown` nine times out
//! of ten passes the gate and is useless. That is the risk — a layer that is
//! built and does not suit the detectors, discovered far too late.
//!
//! Counts per corpus project: how many expressions got a type, how many calls
//! resolved, and which refusal reasons dominate. The reason matters more than
//! the share: it says which inference rule to add next.
//!
//! Run: `cargo run -p sting-semantic --example semantic_coverage`
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "a diagnostic binary rather than shipped library code (docs/ARCHITECTURE.md §3.2)"
)]

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use sting_semantic::{ResolvedCall, ResolvedType, SemanticModel, SourceUnit, UnknownReason};
use sting_tolkc::TolkCompiler;
use tree_sitter::Tree;

#[derive(Default)]
struct Tally {
    known: usize,
    ambiguous: usize,
    unknown: usize,
    reasons: BTreeMap<String, usize>,
}

impl Tally {
    fn total(&self) -> usize {
        self.known + self.ambiguous + self.unknown
    }

    fn add(&mut self, resolved: &ResolvedType) {
        match resolved {
            ResolvedType::Known(_) => self.known += 1,
            ResolvedType::Ambiguous(_) => self.ambiguous += 1,
            ResolvedType::Unknown(reason) => {
                self.unknown += 1;
                *self.reasons.entry(name_of(reason)).or_default() += 1;
            }
        }
    }
}

fn name_of(reason: &UnknownReason) -> String {
    match reason {
        UnknownReason::OutsideAnyFunction => {
            "outside a function (a contract{} ERROR span)".to_string()
        }
        UnknownReason::NotDeclaredInFunction => {
            "not declared in the function (usually a global)".to_string()
        }
        UnknownReason::NoInitializer => "no initializer".to_string(),
        UnknownReason::UninferrableExpression { node_kind } => format!("shape: {node_kind}"),
        UnknownReason::UnresolvedCall => "the call did not resolve".to_string(),
        UnknownReason::NoSuchField { .. } => "no such field".to_string(),
    }
}

#[allow(
    clippy::too_many_lines,
    reason = "a linear report: cutting it into functions for a line count would make the order of the output less obvious than it is"
)]
fn main() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let exe = root
        .join("vendor/tolk")
        .join(sting_tolkc::platform_executable_filename());
    if !exe.is_file() {
        eprintln!("vendor/tolk is needed and is gitignored: {}", exe.display());
        return;
    }
    let compiler = TolkCompiler::new(exe);

    let mut types = Tally::default();
    let mut calls_known = 0_usize;
    let mut calls_total = 0_usize;
    let mut call_failures: BTreeMap<String, usize> = BTreeMap::new();
    let mut undeclared: BTreeMap<String, usize> = BTreeMap::new();
    let mut projects_ok = 0_usize;
    let mut projects_skipped = Vec::new();

    println!("# The semantic layer's coverage over the real corpus\n");
    println!("| Project | expressions | type known | calls | resolved |");
    println!("|---|---|---|---|---|");

    for project in projects(&root) {
        let Some(entry) = entrypoint(&project) else {
            projects_skipped.push((project.clone(), "no entry point".to_string()));
            continue;
        };
        let symbols = match compiler.compile(&project, &entry) {
            Ok(output) => output.symbol_types,
            Err(err) => {
                projects_skipped.push((project.clone(), format!("does not compile: {err}")));
                continue;
            }
        };

        // The standard library's sources are handed in with the project: the
        // compiler compiles them (`common.tolk` is file_id 0) and does not
        // surface their structs in the artefact's `declarations`. Without them
        // there is nowhere to get the type of `in.senderAddress` from, even
        // though `struct InMessage` is declared exactly there.
        let parsed: Vec<(PathBuf, String, Tree)> = tolk_files(&project)
            .into_iter()
            .chain(tolk_files(&root.join("vendor/tolk/tolk-stdlib")))
            .filter_map(|path| {
                let source = std::fs::read_to_string(&path).ok()?;
                let tree = sting_ast::parse(&source).ok()?;
                Some((path, source, tree))
            })
            .collect();
        let units: Vec<SourceUnit<'_>> = parsed
            .iter()
            .map(|(path, source, tree)| SourceUnit {
                path: path.clone(),
                tree,
                source,
            })
            .collect();
        let model = SemanticModel::build(&symbols, &units);

        let mut project_types = Tally::default();
        let (mut project_calls_known, mut project_calls) = (0_usize, 0_usize);

        for (path, _, tree) in &parsed {
            // The standard library is given to the model as a source of
            // declarations, and only the project's own code is measured:
            // coverage over somebody else's library says nothing about the
            // layer's use to the detectors.
            if path.starts_with(root.join("vendor")) {
                continue;
            }
            let Some(file) = model.file(path) else {
                continue;
            };
            // Only expressions INSIDE function bodies. Asking the type of an
            // identifier in a struct declaration or in an `import` is
            // meaningless: the first version of this measurement did exactly
            // that and reported 16.7% "coverage", where more than half the
            // refusals were "outside a function" — it was measuring something
            // other than what it claimed.
            for body in function_bodies(tree) {
                for visited in sting_ast::Preorder::from_node(body) {
                    let node = visited.node;
                    match node.kind() {
                        // The expressions a detector really asks the type of.
                        "identifier" | "dot_access" => {
                            // An identifier inside a `dot_access` is already
                            // counted as part of the path; otherwise the same
                            // question would be counted twice with different
                            // answers.
                            if node.parent().is_some_and(|p| {
                                p.kind() == "dot_access" || p.kind() == "function_call"
                            }) {
                                continue;
                            }
                            let resolved = file.type_of(node);
                            if matches!(
                                resolved,
                                ResolvedType::Unknown(UnknownReason::NotDeclaredInFunction)
                            ) {
                                let text = std::fs::read_to_string(path).unwrap_or_default();
                                let name = text
                                    .get(node.start_byte()..node.end_byte())
                                    .unwrap_or("?")
                                    .to_string();
                                *undeclared.entry(name).or_default() += 1;
                            }
                            project_types.add(&resolved);
                        }
                        "function_call" => {
                            project_calls += 1;
                            match file.resolve_call(node) {
                                ResolvedCall::Known(_) | ResolvedCall::Ambiguous(_) => {
                                    project_calls_known += 1;
                                }
                                ResolvedCall::Unresolved(reason) => {
                                    *call_failures
                                        .entry(
                                            format!("{reason:?}")
                                                .split('{')
                                                .next()
                                                .unwrap_or("?")
                                                .trim()
                                                .to_string(),
                                        )
                                        .or_default() += 1;
                                }
                            }
                        }
                        _ => {}
                    }
                }
            }
        }

        println!(
            "| `{}` | {} | {} ({:.0}%) | {} | {} ({:.0}%) |",
            project
                .strip_prefix(&root)
                .unwrap_or(&project)
                .display()
                .to_string()
                .replace('\\', "/"),
            project_types.total(),
            project_types.known + project_types.ambiguous,
            percent(
                project_types.known + project_types.ambiguous,
                project_types.total()
            ),
            project_calls,
            project_calls_known,
            percent(project_calls_known, project_calls),
        );

        types.known += project_types.known;
        types.ambiguous += project_types.ambiguous;
        types.unknown += project_types.unknown;
        for (reason, count) in project_types.reasons {
            *types.reasons.entry(reason).or_default() += count;
        }
        calls_known += project_calls_known;
        calls_total += project_calls;
        projects_ok += 1;
    }

    println!("\n## The total\n");
    println!("- projects measured: **{projects_ok}**");
    println!(
        "- expressions: **{}**, of them typed: **{}** ({:.1}%), ambiguous: {}",
        types.total(),
        types.known,
        percent(types.known, types.total()),
        types.ambiguous
    );
    println!(
        "- calls: **{calls_total}**, resolved: **{calls_known}** ({:.1}%)",
        percent(calls_known, calls_total)
    );

    println!("\n### Why a type is undetermined\n");
    println!("| Reason | Times |");
    println!("|---|---|");
    let mut reasons: Vec<_> = types.reasons.iter().collect();
    reasons.sort_by(|a, b| b.1.cmp(a.1));
    for (reason, count) in reasons {
        println!("| {reason} | {count} |");
    }

    println!("\n### Which names do not resolve (top 20)\n");
    println!("| Name | Times |");
    println!("|---|---|");
    let mut names: Vec<_> = undeclared.iter().collect();
    names.sort_by(|a, b| b.1.cmp(a.1));
    for (name, count) in names.into_iter().take(20) {
        println!("| `{name}` | {count} |");
    }

    println!("\n### Why a call did not resolve\n");
    println!("| Reason | Times |");
    println!("|---|---|");
    let mut failures: Vec<_> = call_failures.iter().collect();
    failures.sort_by(|a, b| b.1.cmp(a.1));
    for (reason, count) in failures {
        println!("| {reason} | {count} |");
    }

    if !projects_skipped.is_empty() {
        println!("\n### Skipped\n");
        for (project, why) in &projects_skipped {
            println!(
                "- `{}` — {}",
                project.file_name().unwrap_or_default().to_string_lossy(),
                why.lines().next().unwrap_or("")
            );
        }
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

/// The corpus's projects: directories of the form
/// `corpus/clean/<set>/<project>/`.
fn projects(root: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for set in ["acton-contracts", "tolk-bench"] {
        let dir = root.join("corpus/clean").join(set);
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.filter_map(Result::ok) {
            if entry.path().is_dir() {
                out.push(entry.path());
            }
        }
    }
    out.sort();
    out
}

/// A project's entry point: the file with `onInternalMessage` or `main`.
fn entrypoint(project: &Path) -> Option<PathBuf> {
    tolk_files(project).into_iter().find(|path| {
        std::fs::read_to_string(path).is_ok_and(|s| {
            s.contains("fun onInternalMessage") || s.contains("fun onExternalMessage")
        })
    })
}

/// Every function and method body in the file.
fn function_bodies(tree: &Tree) -> Vec<tree_sitter::Node<'_>> {
    sting_ast::Preorder::new(tree)
        .map(|v| v.node)
        .filter(|n| matches!(n.kind(), "function_declaration" | "method_declaration"))
        .filter_map(|n| {
            n.named_children(&mut n.walk())
                .find(|c| c.kind() == "block_statement")
        })
        .collect()
}

fn tolk_files(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.filter_map(Result::ok) {
            let path = entry.path();
            if path.extension().is_some_and(|e| e == "tolk") {
                out.push(path);
            }
        }
    }
    out.sort();
    out
}
