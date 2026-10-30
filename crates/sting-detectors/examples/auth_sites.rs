//! Enumerates **the places authorization happens** — functions reachable from
//! `onInternalMessage` that mutate persistent state — together with the
//! features that separate a guarded one from an unguarded one.
//!
//! Why a separate tool rather than reusing `AcDetector`: the hypotheses are
//! checked **against** `AC`'s recognizer, and features extracted by its own
//! code would inherit its blind spots, degrading the check into "the detector
//! agrees with itself". The walk here is its own and deliberately naive — it
//! can be wrong, but it is wrong independently.
//!
//! What counts as a mutation is the same set `AC` uses (`ac.rs`'s
//! `DATA_MUTATION_METHODS`, `CODE_MUTATION_METHOD` and `LEGACY_*_FUNCTIONS`):
//! a divergence here would mean labelling a different set from the one
//! precision is measured over.
//!
//! Output is JSONL on stdout, one line per site, so that labelling is done on
//! top of a machine enumeration rather than on a retelling of one.
//!
//! Run: `cargo run -p sting-detectors --example auth_sites -- <dir>...`
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "a diagnostic binary rather than shipped library code (docs/ARCHITECTURE.md §3.2)"
)]

use std::collections::{BTreeSet, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::{env, fs};

use sting_ast::{AstStore, Preorder};
use tree_sitter::{Node, Tree};

/// The same names `AC` recognizes. Kept as a list here rather than imported:
/// `ac.rs`'s privacy is deliberate, and four duplicated lines cost less than
/// widening a public API for one diagnostic binary.
const MUTATION_METHODS: [&str; 2] = ["save", "setData"];
const MUTATION_FUNCTIONS: [&str; 3] = [
    "setCodePostponed",
    "setContractData",
    "setContractCodePostponed",
];

/// The entry points depth is counted from.
const ENTRYPOINTS: [&str; 3] = ["onInternalMessage", "onExternalMessage", "onBouncedMessage"];

fn main() {
    let roots: Vec<PathBuf> = env::args_os().skip(1).map(PathBuf::from).collect();
    assert!(!roots.is_empty(), "usage: auth_sites <dir>...");

    let mut files = Vec::new();
    for root in &roots {
        collect_tolk(root, &mut files);
    }
    files.sort();

    for path in files {
        let Ok(source) = fs::read_to_string(&path) else {
            continue;
        };
        let Ok(tree) = sting_ast::parse(&source) else {
            continue;
        };
        emit_file(&path, &source, &tree);
    }
}

fn collect_tolk(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.filter_map(Result::ok) {
        let path = entry.path();
        if path.is_dir() {
            if path.file_name().is_some_and(|n| n == ".git") {
                continue;
            }
            collect_tolk(&path, out);
        } else if path.extension().is_some_and(|e| e == "tolk") {
            out.push(path);
        }
    }
}

fn emit_file(path: &Path, source: &str, tree: &Tree) {
    let store = AstStore::build(tree, source);

    // A file outside `AC`'s scope: the detector only ever starts at
    // `onInternalMessage`.
    if store.function_by_name("onInternalMessage").is_none() {
        return;
    }

    let bodies = function_bodies(tree, source);
    let calls: HashMap<String, BTreeSet<String>> = bodies
        .iter()
        .map(|(name, body)| (name.clone(), callees(*body, source)))
        .collect();

    let depths = depths_from_entrypoints(&calls, &bodies);
    let file_reads_sender = source.contains("in.senderAddress");

    for (name, body) in &bodies {
        let Some(&depth) = depths.get(name) else {
            continue; // unreachable from an entry point, so outside `AC`'s scope
        };
        // A delegated mutation is enumerated too: `AC` reports it as well
        // (`delegated_finding`), and at the call site rather than inside the
        // callee.
        let delegates: BTreeSet<&str> = calls
            .get(name)
            .into_iter()
            .flatten()
            .filter(|c| {
                bodies.iter().any(|(n, _)| n == *c) && mutation_kind(&calls, c, &bodies).is_some()
            })
            .map(String::as_str)
            .collect();

        // A persistence wrapper (`fun Storage.save(self) {
        // contract.setData(self.toCell()) }`, `fun saveData() {
        // setContractData(...) }`) is the write primitive itself rather than
        // the place an authorization decision is made. Asking whether it
        // should be guarded is meaningless: what gets guarded is its caller,
        // enumerated separately as `delegated`. Found while labelling
        // `orderbook`, `ton-ctfs` and `converted` — the same shape in three
        // independent sources.
        if is_persistence_wrapper(*body, source) {
            continue;
        }

        let body_lines = body.end_position().row - body.start_position().row + 1;
        let mut sites = Vec::new();
        collect_sites(
            *body,
            source,
            &delegates,
            &mut Vec::new(),
            &mut Vec::new(),
            &mut sites,
        );

        for site in sites {
            println!(
                r#"{{"file":{},"function":{},"line":{},"call":{},"kind":{},"depth":{},"guard_depth":{},"guards":{},"path_has_address_comparison":{},"path_has_sender":{},"file_reads_sender":{},"body_lines":{},"wrappers":{},"label":null,"note":null}}"#,
                json_str(&path.display().to_string().replace('\\', "/")),
                json_str(name),
                site.line,
                json_str(&site.call),
                json_str(site.kind),
                depth,
                site.guards.len(),
                json_list(&site.guards),
                site.guards.iter().any(|g| looks_like_address_check(g)),
                site.guards.iter().any(|g| g.contains("senderAddress")),
                file_reads_sender,
                body_lines,
                json_list(
                    &site
                        .wrappers
                        .iter()
                        .map(|w| (*w).to_string())
                        .collect::<Vec<_>>()
                ),
            );
        }
    }
}

/// The body is exactly one statement and it contains a mutating call, which
/// is what a persistence wrapper looks like. Deliberately narrow: a function
/// with any branch, or with a second statement, is already making a decision
/// and stays in the set.
fn is_persistence_wrapper(body: Node, source: &str) -> bool {
    let mut cursor = body.walk();
    let statements: Vec<Node> = body.named_children(&mut cursor).collect();
    let [only] = statements[..] else { return false };
    let mut stack = vec![only];
    while let Some(node) = stack.pop() {
        if node.kind() == "function_call"
            && let Some(callee) = node.child_by_field_name("callee")
        {
            let name = if callee.kind() == "dot_access" {
                callee.child_by_field_name("field")
            } else {
                Some(callee)
            };
            if name.is_some_and(|n| is_mutation_name(body_text(n, source))) {
                return true;
            }
        }
        let mut cursor = node.walk();
        stack.extend(node.named_children(&mut cursor));
    }
    false
}

/// One mutation site: the call, its line, and the conditions guarding the
/// path to it.
struct Site {
    line: usize,
    call: String,
    kind: &'static str,
    guards: Vec<String>,
    /// The syntactic wrappers on the path from the function body to the
    /// site, outside inwards: `try`, `repeat`, a bare block. Recorded apart
    /// from `guards` because they guard nothing — and yet it is at their
    /// boundaries that the analyzer lost the "a check happened" state (see
    /// `ac.rs`'s `handle_unconditional_wrapper`). Without this field there is
    /// no way to answer whether the corpus contains the shape the detector got
    /// wrong.
    wrappers: Vec<&'static str>,
}

/// If `node` is a mutating call or a call to a delegate, records the site with
/// the conditions and wrappers in force. Split out of [`collect_sites`] to keep
/// that function inside the length limit.
fn record_site_if_mutation(
    node: Node,
    source: &str,
    delegates: &BTreeSet<&str>,
    guards: &[String],
    wrappers: &[&'static str],
    out: &mut Vec<Site>,
) {
    if node.kind() != "function_call" {
        return;
    }
    let Some(callee) = node.child_by_field_name("callee") else {
        return;
    };
    let name = if callee.kind() == "dot_access" {
        callee
            .child_by_field_name("field")
            .map(|n| body_text(n, source))
    } else {
        Some(body_text(callee, source))
    };
    let Some(name) = name else { return };

    let kind = if is_mutation_name(name) {
        "direct"
    } else if delegates.contains(name) {
        "delegated"
    } else {
        return;
    };
    out.push(Site {
        line: node.start_position().row + 1,
        call: name.to_string(),
        kind,
        guards: guards.to_vec(),
        wrappers: wrappers.to_vec(),
    });
}

/// Descends the body, accumulating the conditions along the path. An `assert`
/// expression, and the condition of an `if` a mutation sits inside, are exactly
/// what `AC` tries to recognize as "a check happened".
fn collect_sites(
    node: Node,
    source: &str,
    delegates: &BTreeSet<&str>,
    guards: &mut Vec<String>,
    wrappers: &mut Vec<&'static str>,
    out: &mut Vec<Site>,
) {
    // Wrappers that guard nothing and yet lose the analyzer's state.
    if let Some(kind) = match node.kind() {
        "try_catch_statement" => Some("try"),
        "repeat_statement" => Some("repeat"),
        _ => None,
    } {
        wrappers.push(kind);
        let mut cursor = node.walk();
        for child in node.named_children(&mut cursor) {
            collect_sites(child, source, delegates, guards, wrappers, out);
        }
        wrappers.pop();
        return;
    }

    record_site_if_mutation(node, source, delegates, guards, wrappers, out);

    // An `assert`, and a **call statement**, guard everything after them in
    // the same block. Every call is recorded rather than only the check-shaped
    // ones: the dominant authorization idiom in this corpus is
    // `assertSenderIsAdmin(in.senderAddress, storage.adminAddress);`, an
    // ordinary call, and any lexical filter here would mean the tool had
    // already decided what counts as a check. That decision is made during
    // analysis, over the raw list.
    if node.kind() == "block_statement" {
        let mut cursor = node.walk();
        let mut accumulated = Vec::new();
        for child in node.named_children(&mut cursor) {
            // The mutating call is a call statement too; swallowing it as a
            // "guard" would lose the very site everything is counted for.
            if (child.kind() == "assert_statement" || is_bare_call_statement(child))
                && !mentions_mutation(child, source, delegates)
            {
                accumulated.push(condense(body_text(child, source)));
                continue;
            }
            guards.extend(accumulated.iter().cloned());
            let added = accumulated.len();
            collect_sites(child, source, delegates, guards, wrappers, out);
            guards.truncate(guards.len() - added);
        }
        return;
    }

    // A `match` arm names the operation the mutation belongs to. For manual
    // labelling this is the most informative context there is:
    // `ChangeMinterAdmin` and `TopUpTons` differ in intent, not in shape.
    if node.kind() == "match_arm" {
        // An arm has no `pattern` field: the body sits in `block`, `return`,
        // `throw` or `expr` (see `support.rs::match_arm_body`), and the pattern
        // is the first named child before it.
        let body_id = ["block", "return", "throw", "expr"]
            .iter()
            .find_map(|f| node.child_by_field_name(f))
            .map(|b| b.id());
        let pattern = node
            .named_child(0)
            .filter(|p| Some(p.id()) != body_id)
            .map(|p| format!("=> {}", condense(body_text(p, source))));
        if let Some(pattern) = pattern {
            guards.push(pattern);
            let mut cursor = node.walk();
            for child in node.named_children(&mut cursor) {
                collect_sites(child, source, delegates, guards, wrappers, out);
            }
            guards.pop();
            return;
        }
    }

    let condition = match node.kind() {
        "if_statement" | "while_statement" | "do_while_statement" => node
            .child_by_field_name("condition")
            .map(|c| condense(body_text(c, source))),
        _ => None,
    };

    if let Some(condition) = condition {
        guards.push(condition);
        let mut cursor = node.walk();
        for child in node.named_children(&mut cursor) {
            collect_sites(child, source, delegates, guards, wrappers, out);
        }
        guards.pop();
        return;
    }

    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        collect_sites(child, source, delegates, guards, wrappers, out);
    }
}

/// Whether the subtree holds a mutating call or a call to a delegate.
fn mentions_mutation(node: Node, source: &str, delegates: &BTreeSet<&str>) -> bool {
    let mut stack = vec![node];
    while let Some(node) = stack.pop() {
        if node.kind() == "function_call"
            && let Some(callee) = node.child_by_field_name("callee")
        {
            let name = if callee.kind() == "dot_access" {
                callee.child_by_field_name("field")
            } else {
                Some(callee)
            };
            if let Some(name) = name.map(|n| body_text(n, source))
                && (is_mutation_name(name) || delegates.contains(name))
            {
                return true;
            }
        }
        let mut cursor = node.walk();
        stack.extend(node.named_children(&mut cursor));
    }
    false
}

/// An expression statement that is entirely a call: `foo(a, b);`. An
/// assignment (`var x = foo();`) is not one — it computes a value rather than
/// guarding the path by not having thrown.
fn is_bare_call_statement(node: Node) -> bool {
    node.kind() == "expression_statement"
        && node.named_child(0).is_some_and(|e| {
            let inner = if e.kind() == "parenthesized_expression" {
                e.named_child(0).unwrap_or(e)
            } else {
                e
            };
            inner.kind() == "function_call"
        })
}

/// A condition on one line: multi-line `match` subjects and wrapped text are
/// unreadable in JSONL during manual labelling.
fn condense(text: &str) -> String {
    let joined = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if joined.chars().count() > 160 {
        joined.chars().take(157).collect::<String>() + "..."
    } else {
        joined
    }
}

fn looks_like_address_check(guard: &str) -> bool {
    (guard.contains("==") || guard.contains("!="))
        && (guard.contains("address")
            || guard.contains("Address")
            || guard.contains("owner")
            || guard.contains("Owner")
            || guard.contains("admin")
            || guard.contains("Admin"))
}

fn json_list(items: &[String]) -> String {
    let inner: Vec<String> = items.iter().map(|s| json_str(s)).collect();
    format!("[{}]", inner.join(","))
}

/// Every function and method body in the file, by name. Same-named ones
/// overwrite each other — the same coarsening `AC`'s `function_by_name` makes.
fn function_bodies<'t>(tree: &'t Tree, source: &str) -> Vec<(String, Node<'t>)> {
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    for visited in Preorder::new(tree) {
        let node = visited.node;
        if !matches!(node.kind(), "function_declaration" | "method_declaration") {
            continue;
        }
        let Some(name) = node
            .child_by_field_name("name")
            .and_then(|n| n.utf8_text(source.as_bytes()).ok())
        else {
            continue;
        };
        let Some(body) = node
            .child_by_field_name("body")
            .filter(|b| b.kind() == "block_statement")
        else {
            continue;
        };
        if seen.insert(name.to_string()) {
            out.push((name.to_string(), body));
        }
    }
    out
}

/// The names of every function and method called in the subtree, without
/// resolving the receiver: for reachability the name is enough.
fn callees(body: Node, source: &str) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    let mut stack = vec![body];
    while let Some(node) = stack.pop() {
        // `function_call(callee: dot_access(obj, field))` for a method and
        // `function_call(callee: identifier)` for a free function — the shape
        // `support.rs::as_method_call` confirms.
        if node.kind() == "function_call"
            && let Some(callee) = node.child_by_field_name("callee")
        {
            let name = if callee.kind() == "dot_access" {
                callee.child_by_field_name("field")
            } else {
                Some(callee)
            };
            if let Some(text) = name.and_then(|n| n.utf8_text(source.as_bytes()).ok()) {
                out.insert(text.to_string());
            }
        }
        let mut cursor = node.walk();
        stack.extend(node.named_children(&mut cursor));
    }
    out
}

/// The shortest depth from any entry point, breadth-first over the call graph.
fn depths_from_entrypoints(
    calls: &HashMap<String, BTreeSet<String>>,
    bodies: &[(String, Node)],
) -> HashMap<String, usize> {
    let defined: HashSet<&str> = bodies.iter().map(|(n, _)| n.as_str()).collect();
    let mut depths = HashMap::new();
    let mut queue: Vec<(String, usize)> = ENTRYPOINTS
        .iter()
        .filter(|e| defined.contains(*e))
        .map(|e| ((*e).to_string(), 0))
        .collect();

    while let Some((name, depth)) = queue.pop() {
        if depths.get(&name).is_some_and(|&d| d <= depth) {
            continue;
        }
        depths.insert(name.clone(), depth);
        if let Some(next) = calls.get(&name) {
            for callee in next {
                if defined.contains(callee.as_str()) {
                    queue.push((callee.clone(), depth + 1));
                }
            }
        }
    }
    depths
}

/// `Some("direct")` when the mutation is in the body itself,
/// `Some("delegated")` when it is only reached through a function called in the
/// same file, and `None` when nothing mutates.
fn mutation_kind(
    calls: &HashMap<String, BTreeSet<String>>,
    name: &str,
    bodies: &[(String, Node)],
) -> Option<&'static str> {
    let own = calls.get(name)?;
    if own.iter().any(|c| is_mutation_name(c)) {
        return Some("direct");
    }
    let defined: HashSet<&str> = bodies.iter().map(|(n, _)| n.as_str()).collect();
    let mut seen: HashSet<String> = HashSet::from([name.to_string()]);
    let mut stack: Vec<String> = own.iter().cloned().collect();
    while let Some(next) = stack.pop() {
        if !defined.contains(next.as_str()) || !seen.insert(next.clone()) {
            continue;
        }
        let Some(inner) = calls.get(&next) else {
            continue;
        };
        if inner.iter().any(|c| is_mutation_name(c)) {
            return Some("delegated");
        }
        stack.extend(inner.iter().cloned());
    }
    None
}

fn is_mutation_name(name: &str) -> bool {
    MUTATION_METHODS.contains(&name) || MUTATION_FUNCTIONS.contains(&name)
}

fn body_text<'a>(node: Node, source: &'a str) -> &'a str {
    node.utf8_text(source.as_bytes()).unwrap_or("")
}

fn json_str(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for ch in s.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}
