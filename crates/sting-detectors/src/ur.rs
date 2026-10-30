//! `UR` — Unchecked Return. One of the three classes deferred rather than
//! abandoned (`docs/domains/detectors.md`), it was
//! `TONScanner` codes; implemented last of the three, after `BR`/`PL` —
//!). A dict/map lookup (`.get(key)`,
//! `.findKeyGreater(key)`, ... — Tolk's stdlib returns a lookup-result value
//! carrying an `isFound` field for all of these, not just `.get`) is read
//! via `.loadValue()` without first checking `isFound` — an unfound
//! lookup's "value" is garbage/undefined, not a safe default.
//!
//! `DetectorKind::Ast` "with elements of local CFG", the same shape `LEP`
//! documents for itself: recursive descent over block/branch structure,
//! tracking which lookup names are *confirmed found* on the current path,
//! rather than `sting_dataflow`'s generic `WorklistSolver` (Tolk's lack of
//! `goto` makes structured recursion a complete substitute here too — see
//! `lep.rs`'s own doc comment for the fuller argument). Unlike `LEP`
//! (a single whole-function "was X called by the end, on every path"
//! question), this is a **per-call-site** question — a `.loadValue()` call
//! is judged by what is confirmed at *that specific point*, so the
//! "confirmed" set grows and shrinks as the walk enters/leaves branches,
//! rather than being reduced to one boolean per function.
//!
//! **Every guard form below was found by testing against this workspace's
//! own real corpus** (`corpus/clean/acton-contracts/`, 40+ real
//! `.loadValue()` call sites), not designed upfront and assumed sufficient —
//! the initial three-form design (plain `if`, negated early-return,
//! ternary) missed more than half of them on first real contact, each
//! subsequent form added only after finding a real site it did not yet
//! explain:
//! 1. `if (x.isFound) { ...x.loadValue()... }` — `x` confirmed inside the
//!    `then` branch only.
//! 2. `if (!x.isFound) { return/throw ...; } ...x.loadValue()...` — an
//!    early-return guard; `x` becomes confirmed for the rest of the
//!    *enclosing* block once the guard's `then` branch is a single
//!    `return`/`throw` statement, see [`is_simple_exit_block`].
//! 3. `x.isFound ? x.loadValue() : <default>` — the ternary's own
//!    `consequence` branch is exempted when its `condition` is exactly
//!    `x.isFound`; the `alternative` branch is deliberately **not**
//!    exempted (a `.loadValue()` written backwards, in the "not found"
//!    branch, is exactly as wrong as no guard at all).
//! 4. `assert (x.isFound) throw N;` — confirms `x` for the rest of the
//!    enclosing block once seen (an `assert_statement` *is* inherently the
//!    "pass through or throw" shape, no separate body to check).
//! 5. `while (x.isFound) { ...x.loadValue()... }` — `x` confirmed only
//!    *inside* the loop body (the loop may run zero times).
//! 6. `x.isFound && <rest>` / `<rest> && x.isFound` — a compound `&&`
//!    condition, checked on an `if`/`while`/`assert`; `x.isFound` may be
//!    either conjunct.
//! 7. `NAME = x.isFound;` (a plain reassignment, not `var`/`val`) makes
//!    `NAME` an alias usable in a later `if (NAME)`/`while (NAME)` exactly
//!    as `x.isFound` itself would be (not, however, chained through a
//!    *second* alias) — the `do`/`while`-with-a-boolean-flag idiom
//!    (`corpus/clean/acton-contracts/elector/credits.tolk`'s
//!    `unfreezeWithoutBonuces`: `searching = freezeLookup.isFound; if
//!    (searching) { ...freezeLookup.loadValue()... }`).
//! 8. A guard (of any form above) *inside* one `match` arm's own body is
//!    only visible within that same arm — every arm is walked
//!    independently from the state entering the `match`, matching
//!    `onInternalMessage`'s universal op-code-dispatch shape
//!    (`corpus/clean/acton-contracts/multisig-v2.1/Order.tolk`).
//!
//! **Not attempted in v1** (documented boundaries, not silent gaps): a
//! guard on a field/index access rather than a plain local identifier
//! (`self.lookup.isFound`), an alias chained through a *second* alias, and
//! full all-paths-exit analysis for the early-return form (only a
//! single-statement `return`/`throw` body is recognized, not a longer block
//! that still unconditionally exits).

use std::collections::{HashMap, HashSet};

use sting_ast::{AstStore, Language};
use sting_core::{DetectorId, Finding, Severity};
use tree_sitter::Node;

use crate::support::{
    as_method_call, block_children, bound_names, function_body, identifier_text,
    is_simple_exit_block, location_of, match_arm_body, unwrap_parens,
};
use crate::{Detector, DetectorContext, DetectorKind};

pub(crate) struct UrDetector;

impl Detector for UrDetector {
    fn id(&self) -> DetectorId {
        "UR"
    }

    fn kind(&self) -> DetectorKind {
        DetectorKind::Ast
    }

    fn default_severity(&self) -> Severity {
        Severity::High
    }

    fn check(&self, store: &AstStore, ctx: &DetectorContext) -> Vec<Finding> {
        let mut findings = Vec::new();
        for function in store.functions() {
            let Some(body) = function_body(function, ctx) else {
                continue;
            };
            let env = Env {
                source: ctx.source(),
                ctx,
                function_name: &function.name,
            };
            walk_block(
                &block_children(body),
                &HashSet::new(),
                &HashMap::new(),
                &env,
                &mut findings,
            );
        }
        findings
    }
}

/// The parts of a `walk_block` call that never change across one function's
/// whole recursive walk — bundled so every helper below takes one shared
/// reference instead of three, on top of the two pieces of state
/// (`confirmed`/`bool_aliases`) that genuinely do change per branch.
struct Env<'a> {
    source: &'a str,
    ctx: &'a DetectorContext,
    function_name: &'a str,
}

/// Walks `stmts` (one block's direct children) in source order, threading
/// two pieces of state: `confirmed` (lookup names known "found" at each
/// point) and `bool_aliases` (plain boolean variable names known to mean
/// "the same as `<lookup>.isFound`" — guard form 7 in this module's doc
/// comment). Both grow inside a positive guard's own branch and shrink back
/// out once that branch's `walk_block` call returns (each branch clones its
/// own copy), except where a guard form specifically says otherwise (the
/// early-return/`assert` forms confirm for the *rest of the current block*).
fn walk_block(
    stmts: &[Node],
    confirmed_in: &HashSet<String>,
    bool_aliases_in: &HashMap<String, String>,
    env: &Env,
    findings: &mut Vec<Finding>,
) {
    let mut confirmed = confirmed_in.clone();
    let mut bool_aliases = bool_aliases_in.clone();
    for &stmt in stmts {
        if let Some((alias, lookup)) = bool_alias_assignment(stmt, env.source) {
            bool_aliases.insert(alias, lookup);
            continue;
        }
        if stmt.kind() == "match_statement" {
            walk_match_arms(stmt, &confirmed, &bool_aliases, env, findings);
            continue;
        }
        if stmt.kind() == "assert_statement" {
            if let Some(condition) = stmt.child_by_field_name("condition")
                && let Some(name) = is_found_check(condition, &bool_aliases, env.source)
            {
                confirmed.insert(name);
            }
            continue;
        }
        // Unconditional wrappers — see `walk_unconditional_blocks`.
        if matches!(
            stmt.kind(),
            "block_statement" | "repeat_statement" | "try_catch_statement"
        ) {
            walk_unconditional_blocks(stmt, &confirmed, &bool_aliases, env, findings);
            continue;
        }
        if matches!(stmt.kind(), "while_statement" | "do_while_statement") {
            walk_loop_statement(stmt, &confirmed, &bool_aliases, env, findings);
            continue;
        }
        if stmt.kind() != "if_statement" {
            check_statement(stmt, &confirmed, env, findings);
            continue;
        }
        let Some(condition) = stmt.child_by_field_name("condition") else {
            continue;
        };
        let then_body = stmt.child_by_field_name("body");
        let alternative = stmt.child_by_field_name("alternative");

        if let Some(name) = is_found_check(condition, &bool_aliases, env.source) {
            if let Some(b) = then_body {
                let mut then_confirmed = confirmed.clone();
                then_confirmed.insert(name);
                walk_block(
                    &block_children(b),
                    &then_confirmed,
                    &bool_aliases,
                    env,
                    findings,
                );
            }
            if let Some(alt) = alternative {
                walk_alternative(alt, &confirmed, &bool_aliases, env, findings);
            }
        } else if let Some(name) = negated_is_found_check(condition, &bool_aliases, env.source) {
            if let Some(b) = then_body {
                walk_block(&block_children(b), &confirmed, &bool_aliases, env, findings);
                if is_simple_exit_block(b) {
                    confirmed.insert(name.clone());
                }
            }
            if let Some(alt) = alternative {
                let mut else_confirmed = confirmed.clone();
                else_confirmed.insert(name);
                walk_alternative(alt, &else_confirmed, &bool_aliases, env, findings);
            }
        } else {
            if let Some(b) = then_body {
                walk_block(&block_children(b), &confirmed, &bool_aliases, env, findings);
            }
            if let Some(alt) = alternative {
                walk_alternative(alt, &confirmed, &bool_aliases, env, findings);
            }
        }
    }
}

fn walk_alternative(
    alt: Node,
    confirmed: &HashSet<String>,
    bool_aliases: &HashMap<String, String>,
    env: &Env,
    findings: &mut Vec<Finding>,
) {
    if alt.kind() == "if_statement" {
        walk_block(
            std::slice::from_ref(&alt),
            confirmed,
            bool_aliases,
            env,
            findings,
        );
    } else {
        walk_block(&block_children(alt), confirmed, bool_aliases, env, findings);
    }
}

/// Walks every arm of `match_stmt` independently, each starting from the
/// same state (as of entering the `match`) — see guard form 8 in this
/// module's doc comment.
/// Walks a loop body. A `while`'s entry condition can confirm a name inside the
/// body; a `do`/`while`'s cannot, because it is tested **after** the body and
/// gates no first iteration. Every real guarded `do`/`while` in the corpus
/// (`corpus/clean/acton-contracts/elector/credits.tolk`,
/// `unfreezeWithoutBonuces`) puts the guard inside the body through a boolean
/// alias, so an ordinary recursive walk carrying the current state is enough.
fn walk_loop_statement(
    stmt: Node,
    confirmed: &HashSet<String>,
    bool_aliases: &HashMap<String, String>,
    env: &Env,
    findings: &mut Vec<Finding>,
) {
    let Some(body) = stmt.child_by_field_name("body") else {
        return;
    };
    let mut body_confirmed = confirmed.clone();
    if stmt.kind() == "while_statement"
        && let Some(condition) = stmt.child_by_field_name("condition")
        && let Some(name) = is_found_check(condition, bool_aliases, env.source)
    {
        body_confirmed.insert(name);
    }
    walk_block(
        &block_children(body),
        &body_confirmed,
        bool_aliases,
        env,
        findings,
    );
}

/// Walks the bodies of constructs that execute **unconditionally**: a bare
/// scope block, `repeat`, and `try`/`catch`. None of them carries an entry
/// condition, so a confirmation in force outside stays in force inside, while
/// one established inside does not escape — [`walk_block`] clones the state,
/// which is exactly a block's scoping rule.
///
/// Without this branch those bodies were not walked at all, and any
/// `.loadValue()` inside counted as unguarded — even one standing directly in
/// the body of `if (x.isFound) { ... }`. Found on
/// `smartcontractkit/chainlink-ton` (`mcms/mcms.tolk`): **11 false positives in
/// one file**. The cause was isolated by a minimal reproduction — the same code
/// gave 0 findings unwrapped and 1 wrapped — so the two-source rule was not
/// applied here: a deterministic reproduction proves a mechanical defect more
/// strongly than a second source would.
///
/// `repeat` surfaced **after** the bare block was fixed, on a re-run against
/// the same source. That is precisely why a fix is checked against the source
/// rather than against the reproduction alone.
fn walk_unconditional_blocks(
    stmt: Node,
    confirmed: &HashSet<String>,
    bool_aliases: &HashMap<String, String>,
    env: &Env,
    findings: &mut Vec<Finding>,
) {
    let blocks: Vec<Node> = if stmt.kind() == "block_statement" {
        vec![stmt]
    } else {
        stmt.named_children(&mut stmt.walk())
            .filter(|c| c.kind() == "block_statement")
            .collect()
    };
    for block in blocks {
        walk_block(
            &block_children(block),
            confirmed,
            bool_aliases,
            env,
            findings,
        );
    }
}

fn walk_match_arms(
    match_stmt: Node,
    confirmed: &HashSet<String>,
    bool_aliases: &HashMap<String, String>,
    env: &Env,
    findings: &mut Vec<Finding>,
) {
    let Some(arms_body) = match_stmt
        .named_child(0)
        .and_then(|match_expr| match_expr.child_by_field_name("body"))
    else {
        return;
    };
    let mut cursor = arms_body.walk();
    let arms: Vec<Node> = arms_body
        .named_children(&mut cursor)
        .filter(|c| c.kind() == "match_arm")
        .collect();
    for arm in arms {
        let Some(body) = match_arm_body(arm) else {
            continue;
        };
        if body.kind() == "block_statement" {
            walk_block(
                &block_children(body),
                confirmed,
                bool_aliases,
                env,
                findings,
            );
        } else {
            walk_block(
                std::slice::from_ref(&body),
                confirmed,
                bool_aliases,
                env,
                findings,
            );
        }
    }
}

/// Scans one non-control-flow statement's whole subtree for
/// `<name>.loadValue()` calls, flagging any whose `name` is not in
/// `confirmed` and is not itself exempted by the ternary form (guard form 3).
fn check_statement(
    stmt: Node,
    confirmed: &HashSet<String>,
    env: &Env,
    findings: &mut Vec<Finding>,
) {
    for visited in sting_ast::Preorder::from_node(stmt) {
        let node = visited.node;
        let Some(call) = as_method_call(node, env.source, Language::Tolk) else {
            continue;
        };
        if call.method != "loadValue" {
            continue;
        }
        let Some(name) = identifier_text(call.obj, env.source) else {
            continue;
        };
        if confirmed.contains(&name) || is_ternary_guarded_consequence(node, &name, env.source) {
            continue;
        }
        findings.push(Finding {
            detector_id: "UR",
            severity: Severity::High,
            message: format!(
                "function `{}` calls `{name}.loadValue()` without checking `{name}.isFound` first — an unfound lookup's value is garbage, not a safe default",
                env.function_name
            ),
            remediation: format!(
                "guard the call with `if ({name}.isFound) {{ ... }}`, an early `if (!{name}.isFound) {{ return/throw ...; }}`, or `assert ({name}.isFound) throw ...;` before reading `{name}.loadValue()`"
            ),
            location: location_of(node, env.ctx),
            confidence: None,
            dynamic_verification: None,
            suggestion: None,
        });
    }
}

/// If `stmt` is a plain (non-`var`/`val`) reassignment `NAME = <rhs>;` or a
/// fresh `var`/`val NAME = <rhs>;` declaration, and `<rhs>` is exactly
/// `<lookup>.isFound` (not itself resolved through `bool_aliases` — chaining
/// an alias of an alias is out of v1's scope, see this module's doc
/// comment), the pair `(NAME, lookup)` — guard form 7.
fn bool_alias_assignment(stmt: Node, source: &str) -> Option<(String, String)> {
    if stmt.kind() != "expression_statement" {
        return None;
    }
    let expr = stmt.named_child(0)?;
    if expr.kind() != "assignment" {
        return None;
    }
    let left = expr.child_by_field_name("left")?;
    let right = expr.child_by_field_name("right")?;
    let lookup_name = is_found_check(right, &HashMap::new(), source)?;
    let alias_name = if left.kind() == "var_declaration_lhs" {
        match bound_names(left, source).as_slice() {
            [(_, name)] => Some(name.clone()),
            _ => None,
        }
    } else {
        identifier_text(left, source)
    }?;
    Some((alias_name, lookup_name))
}

/// If `node` is `<name>.isFound`, a compound `x.isFound && <rest>`, or a
/// bare identifier resolving through `bool_aliases` to one of those — the
/// name. Receiver must be a plain local identifier (see this module's doc
/// comment for why a field/index receiver is out of v1's scope).
fn is_found_check(
    node: Node,
    bool_aliases: &HashMap<String, String>,
    source: &str,
) -> Option<String> {
    let node = unwrap_parens(node);
    if node.kind() == "identifier"
        && let Some(name) = identifier_text(node, source)
        && let Some(lookup) = bool_aliases.get(&name)
    {
        return Some(lookup.clone());
    }
    if node.kind() == "binary_operator" && node.child(1).map(|c| c.kind()) == Some("&&") {
        let left = node.named_child(0)?;
        let right = node.named_child(1)?;
        return is_found_check(left, bool_aliases, source)
            .or_else(|| is_found_check(right, bool_aliases, source));
    }
    if node.kind() != "dot_access" {
        return None;
    }
    let obj = node.child_by_field_name("obj")?;
    let field = node.child_by_field_name("field")?;
    if field.utf8_text(source.as_bytes()).ok()? != "isFound" {
        return None;
    }
    identifier_text(obj, source)
}

/// If `node` is `!<is_found_check-shaped-expression>`, the name.
fn negated_is_found_check(
    node: Node,
    bool_aliases: &HashMap<String, String>,
    source: &str,
) -> Option<String> {
    if node.kind() != "unary_operator" {
        return None;
    }
    if node.child_by_field_name("operator_name")?.kind() != "!" {
        return None;
    }
    is_found_check(node.child_by_field_name("argument")?, bool_aliases, source)
}

/// Whether `node` (a `function_call` for `<name>.loadValue()`) is exactly
/// the `consequence` branch of an enclosing `ternary_operator` whose own
/// `condition` is `<name>.isFound` — guard form 3. The `alternative` branch
/// is deliberately never exempted this way, see this module's doc comment.
fn is_ternary_guarded_consequence(node: Node, name: &str, source: &str) -> bool {
    let Some(parent) = node.parent() else {
        return false;
    };
    if parent.kind() != "ternary_operator" {
        return false;
    }
    let Some(consequence) = parent.child_by_field_name("consequence") else {
        return false;
    };
    if consequence.id() != node.id() {
        return false;
    }
    let Some(condition) = parent.child_by_field_name("condition") else {
        return false;
    };
    is_found_check(condition, &HashMap::new(), source).as_deref() == Some(name)
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "docs/ARCHITECTURE.md §3.2 scopes the no-unwrap/expect policy to library code outside tests"
)]
mod tests {
    use std::path::PathBuf;

    use sting_ast::AstStore;

    use super::UrDetector;
    use crate::{Detector, DetectorContext};

    fn run(source: &str) -> Vec<sting_core::Finding> {
        let tree = sting_ast::parse(source).expect("grammar loads");
        let store = AstStore::build(&tree, source);
        let ctx = DetectorContext::new(PathBuf::from("contract.tolk"), tree, source.to_string());
        UrDetector.check(&store, &ctx)
    }

    #[test]
    fn flags_load_value_with_no_guard_at_all() {
        let source = "fun f(config: ConfigDict, idx: int32): cell {\n    val lookup = config.get(idx);\n    return lookup.loadValue();\n}\n";
        let findings = run(source);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].detector_id, "UR");
        assert!(findings[0].message.contains("lookup"));
    }

    #[test]
    fn does_not_flag_a_positive_if_guard() {
        let source = "fun f(config: ConfigDict, idx: int32): cell? {\n    val lookup = config.get(idx);\n    if (lookup.isFound) {\n        return lookup.loadValue();\n    }\n    return null;\n}\n";
        assert!(run(source).is_empty());
    }

    #[test]
    fn does_not_flag_a_negated_early_return_guard() {
        // corpus/clean/acton-contracts/config/storage.tolk's real
        // ConfigDict.getById shape.
        let source = "fun f(config: ConfigDict, idx: int32): cell? {\n    val lookup = config.get(idx);\n    if (!lookup.isFound) {\n        return null;\n    }\n    return lookup.loadValue();\n}\n";
        assert!(run(source).is_empty());
    }

    #[test]
    fn does_not_flag_a_negated_early_return_guard_preceded_by_a_comment() {
        // corpus/clean/acton-contracts/elector/Elector.tolk's real
        // recoverStake shape — a `//` comment as the guard body's first
        // "statement" broke `is_simple_exit_block` until this was tested
        // against the real corpus.
        let source = "fun f(config: ConfigDict, idx: int32): cell? {\n    val lookup = config.get(idx);\n    if (!lookup.isFound) {\n        // not found, bail\n        return null;\n    }\n    return lookup.loadValue();\n}\n";
        assert!(run(source).is_empty());
    }

    #[test]
    fn does_not_flag_a_ternary_guard() {
        let source = "fun f(config: ConfigDict, idx: int32): cell {\n    val lookup = config.get(idx);\n    return lookup.isFound ? lookup.loadValue() : null;\n}\n";
        assert!(run(source).is_empty());
    }

    #[test]
    fn flags_load_value_written_backwards_in_the_ternarys_alternative() {
        // `lookup.isFound` is checked, but `.loadValue()` is called in the
        // *not found* branch — exactly as wrong as no guard at all.
        let source = "fun f(config: ConfigDict, idx: int32): cell {\n    val lookup = config.get(idx);\n    return lookup.isFound ? null : lookup.loadValue();\n}\n";
        let findings = run(source);
        assert_eq!(findings.len(), 1);
    }

    #[test]
    fn flags_load_value_inside_the_positive_guards_else_branch() {
        // `lookup.isFound` is false inside the `else` of `if (lookup.isFound)`.
        let source = "fun f(config: ConfigDict, idx: int32): cell {\n    val lookup = config.get(idx);\n    if (lookup.isFound) {\n        return null;\n    } else {\n        return lookup.loadValue();\n    }\n}\n";
        let findings = run(source);
        assert_eq!(findings.len(), 1);
    }

    #[test]
    fn flags_only_the_unguarded_lookup_when_two_are_present() {
        let source = "fun f(a: ConfigDict, b: ConfigDict, idx: int32): cell {\n    val lookupA = a.get(idx);\n    val lookupB = b.get(idx);\n    if (lookupA.isFound) {\n        val x = lookupA.loadValue();\n    }\n    return lookupB.loadValue();\n}\n";
        let findings = run(source);
        assert_eq!(findings.len(), 1);
        assert!(findings[0].message.contains("lookupB"));
    }

    #[test]
    fn does_not_flag_an_unrelated_if_condition() {
        let source = "fun f(config: ConfigDict, idx: int32, cond: bool): cell? {\n    val lookup = config.get(idx);\n    if (cond) {\n        return null;\n    }\n    if (!lookup.isFound) {\n        return null;\n    }\n    return lookup.loadValue();\n}\n";
        assert!(run(source).is_empty());
    }

    #[test]
    fn does_not_flag_a_function_with_no_load_value_calls() {
        let source = "fun f(config: ConfigDict, idx: int32) {\n    config.set(idx, 0);\n}\n";
        assert!(run(source).is_empty());
    }

    #[test]
    fn does_not_flag_an_assert_throw_guard() {
        // corpus/clean/acton-contracts/config/config-helpers.tolk's real
        // getCurrentValidatorContext shape.
        let source = "fun f(config: ConfigDict, idx: int32): cell {\n    val lookup = config.get(idx);\n    assert (lookup.isFound) throw 61;\n    return lookup.loadValue();\n}\n";
        assert!(run(source).is_empty());
    }

    #[test]
    fn does_not_flag_a_while_condition_guard() {
        let source = "fun f(config: ConfigDict): int {\n    var total = 0;\n    var entry = config.findFirst();\n    while (entry.isFound) {\n        total += entry.loadValue();\n        entry = config.findNext();\n    }\n    return total;\n}\n";
        assert!(run(source).is_empty());
    }

    #[test]
    fn does_not_flag_a_compound_and_condition() {
        // corpus/clean/acton-contracts/multisig-v2.1/order-helpers.tolk's
        // real findSignerByAddress shape.
        let source = "fun f(config: ConfigDict, stop: bool): int {\n    var entry = config.findFirst();\n    while (entry.isFound && !stop) {\n        val v = entry.loadValue();\n        entry = config.findNext();\n    }\n    return 0;\n}\n";
        assert!(run(source).is_empty());
    }

    #[test]
    fn does_not_flag_a_boolean_alias_of_is_found() {
        // corpus/clean/acton-contracts/elector/credits.tolk's real
        // unfreezeWithoutBonuces shape: `searching = freezeLookup.isFound;`
        // then `if (searching) { ...freezeLookup.loadValue()... }`.
        let source = "fun f(config: ConfigDict): int {\n    var searching = true;\n    var lookup = config.findFirst();\n    do {\n        lookup = config.findNext();\n        searching = lookup.isFound;\n        if (searching) {\n            val v = lookup.loadValue();\n        }\n    } while (searching);\n    return 0;\n}\n";
        assert!(run(source).is_empty());
    }

    #[test]
    fn flags_a_guard_missing_inside_one_match_arm_but_present_in_another() {
        let source = "fun f(config: ConfigDict, op: int): int {\n    val lookup = config.get(op);\n    match (op) {\n        1 => {\n            if (lookup.isFound) {\n                return lookup.loadValue();\n            }\n            return 0;\n        }\n        else => {\n            return lookup.loadValue();\n        }\n    }\n}\n";
        let findings = run(source);
        assert_eq!(
            findings.len(),
            1,
            "the else arm's unguarded loadValue must still be flagged even though the 1 arm guards its own"
        );
    }

    #[test]
    fn does_not_flag_a_guard_inside_a_match_arm() {
        let source = "fun f(config: ConfigDict, op: int): int {\n    match (op) {\n        1 => {\n            val lookup = config.get(op);\n            assert (lookup.isFound) throw 1;\n            return lookup.loadValue();\n        }\n        else => {\n            return 0;\n        }\n    }\n}\n";
        assert!(run(source).is_empty());
    }

    #[test]
    fn kind_is_ast_and_severity_is_high() {
        assert_eq!(UrDetector.kind(), crate::DetectorKind::Ast);
        assert_eq!(UrDetector.default_severity(), sting_core::Severity::High);
    }

    /// A bare block `{ ... }` is a scope rather than a branch, and a guard
    /// from outside keeps holding inside it.
    ///
    /// Found on `smartcontractkit/chainlink-ton` (`mcms/mcms.tolk`): 11 false
    /// positives in one file. The cause was isolated by a minimal reproduction
    /// — the same code gave 0 findings unwrapped and 1 wrapped — so the
    /// two-source rule was not applied here: a deterministic reproduction
    /// proves a mechanical defect more strongly than a second source would.
    #[test]
    fn ur_guard_survives_a_bare_scope_block() {
        let source = "fun f(m: map<int, int>) {
    var out = 0;
    {
        var v = m.get(1);
        if (v.isFound) {
            out = v.loadValue();
        }
    }
    return out;
}
";
        assert!(
            run(source).is_empty(),
            "a check inside a bare block must count as a guard"
        );
    }

    /// The same for `repeat`: its body is an ordinary block and carries no
    /// entry condition. This case survived the bare-block fix and was caught by
    /// a re-run against the source rather than against the reproduction.
    #[test]
    fn ur_guard_survives_a_repeat_body() {
        let source = "fun f(m: map<int, int>) {
    var out = 0;
    repeat(4) {
        var v = m.get(1);
        if (v.isFound) {
            out = v.loadValue();
        }
    }
    return out;
}
";
        assert!(
            run(source).is_empty(),
            "a check inside a repeat body must count as a guard"
        );
    }

    /// The other side: an unguarded read inside the same wrappers must still
    /// be found. A test that only checks for the absence of false positives
    /// would pass on a detector that is silent always.
    #[test]
    fn ur_still_fires_on_an_unguarded_read_inside_a_block_or_repeat() {
        let in_block = "fun f(m: map<int, int>) {
    var out = 0;
    {
        var v = m.get(1);
        out = v.loadValue();
    }
    return out;
}
";
        assert_eq!(
            run(in_block).len(),
            1,
            "an unguarded read in a bare block must be found"
        );
        let in_repeat = "fun f(m: map<int, int>) {
    var out = 0;
    repeat(4) {
        var v = m.get(1);
        out = v.loadValue();
    }
    return out;
}
";
        assert_eq!(
            run(in_repeat).len(),
            1,
            "an unguarded read in a repeat body must be found"
        );
    }
}
