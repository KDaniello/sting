//! `UTA` — Unchecked Transfer Amount (
//! sting's own addition past the `TONScanner` taxonomy, found via real
//! external validation — `hacktheton.com`'s `token_level`, not this
//! project's own corpus). Flags a function that debits one tracked quantity
//! and credits a *different* one by the same amount (`a -= amount; b +=
//! amount;`, the universal "transfer" shape) where only one side of the
//! transfer is validated against going negative — real, exploitable when
//! `amount` can itself be negative (read via `.loadInt(...)`, TON's
//! *signed* slice-read primitive, not `.loadUint(...)`): a negative amount
//! *increases* the "debited" side and *decreases* the "credited" side,
//! silently inverting the transfer and minting value from nothing while
//! sailing straight through a check that only ever looked at the side that
//! grew.
//!
//! **Real motivating case**, confirmed on `hacktheton.com`'s actual,
//! currently-live level source:
//! ```text
//! fun transfer(balances: cell, from: slice, to: slice, amount: int): cell {
//!     var fromBalance: int = getBalance(balances, from);
//!     var toBalance: int = getBalance(balances, to);
//!     fromBalance -= amount;
//!     toBalance += amount;
//!     assert(fromBalance > 0, 502);   // only `fromBalance` is checked
//!     balances.setBalance(from, fromBalance);
//!     balances.setBalance(to, toBalance);
//!     return balances;
//! }
//! ```
//! called as `transfer(ctxBalances, senderAddress, to, amount)` from
//! `onInternalMessage`, where `val amount: int = inMsgBody.loadInt(256);` —
//! the signed read and the asymmetric check are in **different** functions,
//! not one. `amount = -N` makes `fromBalance += N` (passes the `> 0` check
//! trivially) and `toBalance -= N` (never checked at all) — the sender
//! prints themselves `N` tokens for free.
//!
//! **Deliberately narrow v1 scope**, one real source, calibrated the same
//! corpus-first way as every other detector in this taxonomy
//! (`docs/domains/detectors.md`):
//! - Only a *bare-identifier* compound assignment (`x -= amount` /
//!   `y += amount`) is recognized — `set_assignment` nodes whose `right` is
//!   a plain `identifier`, matched by shared name across a `-=` and a `+=`
//!   with **different** `left` targets. An arbitrary expression on either
//!   side is out of scope; this is a syntactic pattern, not a dataflow
//!   proof of aliasing.
//! - "Signed-suspect" requires *proving* `amount` traces to a `.loadInt(...)`
//!   call — either bound by a `val`/`var` in the very same function
//!   ([`bound_from_load_int_in_body`]), or, matching the real motivating
//!   case above, `amount` is this function's own parameter and **some**
//!   same-file call site passes an argument at that parameter's position
//!   that is itself `.loadInt(...)` (directly, or one hop back through a
//!   `val`/`var` bound in *that call site's own enclosing function* —
//!   [`call_site_supplies_signed_argument`]). Genuinely interprocedural, but
//!   deliberately shallow: one hop each direction, no recursion, no
//!   memoized whole-program summary the way `ac.rs`'s `classify_function`
//!   is for its own (same-file, but multi-hop) question.
//! - The finding only fires when *exactly one* of the two sides has a
//!   later `assert`/bail-out-`if` mentioning it — not "neither is checked"
//!   (a plausible but not yet independently observed shape; over-eager here
//!   risks flagging ordinary, correctly-unchecked-because-truly-unsigned
//!   arithmetic) and not "both are checked" (safe).
//!
//! `DetectorKind::Ast`, no dataflow/CFG: the same "local pattern-match plus
//! one shallow same-file hop" shape `ac.rs`'s delegated-call handling
//! already uses, applied to a different question.

use std::collections::HashMap;

use sting_ast::{AstStore, FunctionDef, Language};
use sting_core::{DetectorId, Finding, Severity};
use tree_sitter::Node;

use crate::support::{
    CrossFileCache, as_method_call, bound_names, function_body, identifier_text, location_of,
    subtree_references_identifier,
};
use crate::{Detector, DetectorContext, DetectorKind};

pub(crate) struct UtaDetector;

impl Detector for UtaDetector {
    fn id(&self) -> DetectorId {
        "UTA"
    }

    fn kind(&self) -> DetectorKind {
        DetectorKind::Ast
    }

    fn default_severity(&self) -> Severity {
        Severity::High
    }

    fn check(&self, store: &AstStore, ctx: &DetectorContext) -> Vec<Finding> {
        let source = ctx.source();
        let mut findings = Vec::new();

        for function in store.functions() {
            let Some(body) = function_body(function, ctx) else {
                continue;
            };
            for transfer in find_asymmetric_transfers(body, source) {
                if !is_signed_suspect(&transfer.amount_name, function, body, store, ctx) {
                    continue;
                }
                let debit_checked = has_check_after(
                    body,
                    transfer.credit_node.end_byte(),
                    &transfer.debit_target,
                    source,
                ) || has_check_after(
                    body,
                    transfer.debit_node.end_byte(),
                    &transfer.debit_target,
                    source,
                );
                let credit_checked = has_check_after(
                    body,
                    transfer.debit_node.end_byte(),
                    &transfer.credit_target,
                    source,
                ) || has_check_after(
                    body,
                    transfer.credit_node.end_byte(),
                    &transfer.credit_target,
                    source,
                );
                if debit_checked == credit_checked {
                    continue;
                }
                let (checked, unchecked, anchor) = if debit_checked {
                    (
                        &transfer.debit_target,
                        &transfer.credit_target,
                        transfer.credit_node,
                    )
                } else {
                    (
                        &transfer.credit_target,
                        &transfer.debit_target,
                        transfer.debit_node,
                    )
                };
                findings.push(Finding {
                    detector_id: self.id(),
                    severity: self.default_severity(),
                    message: format!(
                        "function `{}` adjusts both `{}` and `{}` by the same `{}`, but only `{checked}` is validated afterwards — `{}` is never checked and `{}` may be a signed (negative) amount",
                        function.name,
                        transfer.debit_target,
                        transfer.credit_target,
                        transfer.amount_name,
                        unchecked,
                        transfer.amount_name
                    ),
                    remediation: format!(
                        "also validate `{unchecked}` after the adjustment (e.g. `assert ({unchecked} >= 0) throw ...;`), or reject a negative `{}` up front right after it is read (`assert ({} >= 0) throw ...;`) so neither side can be driven negative by a signed input",
                        transfer.amount_name, transfer.amount_name
                    ),
                    location: location_of(anchor, ctx),
                    confidence: None,
                    dynamic_verification: None,
                    suggestion: None,
                });
            }
        }
        findings
    }
}

/// One candidate "transfer" shape found in a function body: a `-=` and a
/// `+=` compound assignment sharing the same bare-identifier right-hand
/// side (`amount_name`) but different left-hand targets.
struct AsymmetricTransfer<'tree> {
    amount_name: String,
    debit_target: String,
    debit_node: Node<'tree>,
    credit_target: String,
    credit_node: Node<'tree>,
}

/// Collects every `set_assignment` in `body` (`x -= y` / `x += y` — Tolk's
/// own node kind for compound assignment, confirmed via a real parse: `left`/
/// `right` are named fields, but the `-=`/`+=` operator itself is not — its
/// text is read from the anonymous second child instead), buckets them by
/// operator and by their bare-identifier `right` side, then pairs up any
/// `-=`/`+=` sharing the same right-hand name but different `left` targets.
fn find_asymmetric_transfers<'tree>(
    body: Node<'tree>,
    source: &str,
) -> Vec<AsymmetricTransfer<'tree>> {
    let mut debits: HashMap<String, Vec<(Node<'tree>, String)>> = HashMap::new();
    let mut credits: HashMap<String, Vec<(Node<'tree>, String)>> = HashMap::new();

    for visited in sting_ast::Preorder::from_node(body) {
        let node = visited.node;
        if node.kind() != "set_assignment" {
            continue;
        }
        let Some(operator) = node.child(1).map(|n| n.kind()) else {
            continue;
        };
        let Some(left) = node
            .child_by_field_name("left")
            .and_then(|n| identifier_text(n, source))
        else {
            continue;
        };
        let Some(right) = node
            .child_by_field_name("right")
            .and_then(|n| identifier_text(n, source))
        else {
            continue;
        };
        match operator {
            "-=" => debits.entry(right).or_default().push((node, left)),
            "+=" => credits.entry(right).or_default().push((node, left)),
            _ => {}
        }
    }

    let mut found = Vec::new();
    for (amount_name, debit_list) in &debits {
        let Some(credit_list) = credits.get(amount_name) else {
            continue;
        };
        for (debit_node, debit_target) in debit_list {
            for (credit_node, credit_target) in credit_list {
                if debit_target != credit_target {
                    found.push(AsymmetricTransfer {
                        amount_name: amount_name.clone(),
                        debit_target: debit_target.clone(),
                        debit_node: *debit_node,
                        credit_target: credit_target.clone(),
                        credit_node: *credit_node,
                    });
                }
            }
        }
    }
    found
}

/// Whether an `assert`/bail-out `if` mentioning `target` appears anywhere in
/// `body` after `after_byte` — deliberately the same "purely syntactic,
/// textual-identifier, no branch-sensitivity" precision level as the rest of
/// this module, not a proof the check runs on every path.
fn has_check_after(body: Node, after_byte: usize, target: &str, source: &str) -> bool {
    sting_ast::Preorder::from_node(body).any(|visited| {
        let node = visited.node;
        if node.start_byte() <= after_byte {
            return false;
        }
        let condition = match node.kind() {
            "assert_statement" | "if_statement" => node.child_by_field_name("condition"),
            _ => None,
        };
        condition.is_some_and(|c| subtree_references_identifier(c, target, source))
    })
}

/// Whether `amount_name` can be proven to trace to a `.loadInt(...)` call —
/// see this module's doc comment for the two recognized shapes.
fn is_signed_suspect(
    amount_name: &str,
    function: &FunctionDef,
    body: Node,
    store: &AstStore,
    ctx: &DetectorContext,
) -> bool {
    let source = ctx.source();
    if bound_from_load_int_in_body(body, amount_name, source) {
        return true;
    }
    let Some(param_index) = function
        .parameters
        .iter()
        .position(|p| p.name == amount_name)
    else {
        return false;
    };
    call_site_supplies_signed_argument(store, ctx, &function.name, param_index)
}

/// Whether `body` contains `val/var <amount_name> = <expr>.loadInt(...);`
/// (a fresh declaration only — this module's own "amount" is always a
/// freshly-read value, never reassigned after the fact in the real case
/// this was built from).
fn bound_from_load_int_in_body(body: Node, amount_name: &str, source: &str) -> bool {
    sting_ast::Preorder::from_node(body).any(|visited| {
        let node = visited.node;
        if node.kind() != "expression_statement" {
            return false;
        }
        let Some(expr) = node.named_child(0) else {
            return false;
        };
        if expr.kind() != "assignment" {
            return false;
        }
        let Some(left) = expr.child_by_field_name("left") else {
            return false;
        };
        if left.kind() != "var_declaration_lhs" {
            return false;
        }
        let bound_to_amount = match bound_names(left, source).as_slice() {
            [(_, name)] => name == amount_name,
            _ => false,
        };
        if !bound_to_amount {
            return false;
        }
        expr.child_by_field_name("right")
            .is_some_and(|r| is_load_int_call(r, source))
    })
}

/// `true` if `expr` is (structurally) a `<slice>.loadInt(...)` call.
fn is_load_int_call(expr: Node, source: &str) -> bool {
    as_method_call(expr, source, Language::Tolk).is_some_and(|call| call.method == "loadInt")
}

/// Whether any call to a function named `callee_name` — in `store`/`ctx`'s
/// own file, plus, when `ctx` carries a compiled `ProjectStore`, every other
/// file in that same compiled project — supplies, at argument position
/// `param_index`, an
/// expression that is itself `.loadInt(...)`, or a bare identifier bound via
/// `.loadInt(...)` inside that call site's own enclosing function — the
/// one-hop interprocedural case this module's doc comment describes (real
/// shape: `onInternalMessage` reads `amount` via `.loadInt`, then passes it
/// to `transfer(...)`).
///
/// **A different shape of cross-file gap than `ac.rs`/`id.rs`/`tlb.rs`'s
/// own, in two ways.** First, the underlying question: those resolve "what
/// does *this* call site's callee refer to" —
/// [`sting_tolkc::ProjectStore::resolve_call`] answers exactly that,
/// forward, from a known caller. This function asks the *reverse* question,
/// "who calls *me*", which `ProjectStore` has no index for at all —
/// `symbolTypesJson` is a flat signature table, not a call graph (see
/// `sting-tolkc`'s own top-level doc comment: function bodies/call sites are
/// structurally outside what it captures) — so this reuses the exact same
/// `Preorder` tree-walk this module already runs for its own file, once per
/// other file loaded through [`crate::support::CrossFileCache`].
///
/// Second, and the reason this scans **every file in the whole project**
/// (`project.symbol_types().files`), not
/// [`sting_tolkc::ProjectStore::reachable_files`] from `store`/`ctx`'s own
/// file: `reachable_files` follows `import` *forward* — what a file itself
/// can reach — which is the right direction for "what does my callee
/// resolve to", but the *wrong* direction for "who calls me". A real caller
/// of `callee_name` can live in any file that imports (directly or
/// transitively) `callee_name`'s own file, which is not generally the same
/// set `callee_name`'s own file itself imports — caught by a real fixture
/// during this same phase's implementation, not anticipated in advance:
/// `helper.tolk` (defining `transfer`, itself importing nothing) reported
/// zero reachable files besides itself via `reachable_files`, silently
/// missing `Main.tolk`, the real caller, entirely. Every file in
/// `symbol_types().files` is, by construction, already reachable from
/// whichever entrypoint `tolk` itself was given to produce this specific
/// `project` (`tolk`'s own `symbolTypes.json` only ever lists files it
/// needed to resolve that one entrypoint) — safe to scan all of them.
fn call_site_supplies_signed_argument(
    store: &AstStore,
    ctx: &DetectorContext,
    callee_name: &str,
    param_index: usize,
) -> bool {
    if scan_one_file_for_signed_argument(store, ctx, callee_name, param_index) {
        return true;
    }
    let Some(project) = ctx.project_store() else {
        return false;
    };
    let Some(own_file_id) = ctx.file_id() else {
        return false;
    };
    // Deliberately every file in the whole compiled project
    // (`ProjectStore::symbol_types().files`), **not**
    // `ProjectStore::reachable_files(own_file_id)` — `reachable_files`
    // follows `import` *forward* (what this file itself can reach), which
    // answers "what can I call", the direction `ac.rs`/`id.rs`/`tlb.rs`
    // already need. This function asks the reverse question, "who calls
    // me" — a caller of `callee_name` can be *any* file in the project that
    // itself imports (directly or transitively) the file `callee_name` is
    // defined in, which is not generally the same set as what
    // `callee_name`'s own file imports (confirmed the hard way: a real
    // fixture where the callee's own file imports nothing at all —
    // `reachable_files(own_file_id)` was empty besides itself, silently
    // missing the caller). Every file in `symbol_types().files` is, by
    // construction, already reachable from whichever entrypoint `tolk`
    // itself was given to produce this specific `project` — safe to scan
    // all of them.
    let mut cross_file = CrossFileCache::default();
    for file in &project.symbol_types().files {
        if file.file_id == own_file_id {
            continue;
        }
        let Some(path) = project.path_for_file_id(file.file_id) else {
            continue;
        };
        let Some(loaded) = cross_file.get_or_load(&path, project) else {
            continue;
        };
        if scan_one_file_for_signed_argument(&loaded.0, &loaded.1, callee_name, param_index) {
            return true;
        }
    }
    false
}

/// The single-file search [`call_site_supplies_signed_argument`] runs
/// against `store`/`ctx`'s own file, and (unchanged) against every
/// cross-file candidate it reaches — the exact walk this module ran before
/// Factored out only so it can be pointed at a different
/// `(store, ctx)` pair without duplicating the walk itself.
fn scan_one_file_for_signed_argument(
    store: &AstStore,
    ctx: &DetectorContext,
    callee_name: &str,
    param_index: usize,
) -> bool {
    let source = ctx.source();
    for visited in sting_ast::Preorder::from_node(ctx.tree().root_node()) {
        let node = visited.node;
        if node.kind() != "function_call" {
            continue;
        }
        let Some(callee) = node.child_by_field_name("callee") else {
            continue;
        };
        if callee.kind() != "identifier"
            || identifier_text(callee, source).as_deref() != Some(callee_name)
        {
            continue;
        }
        let Some(args) = node.child_by_field_name("arguments") else {
            continue;
        };
        let Some(arg) = nth_call_argument_node(args, param_index) else {
            continue;
        };
        if is_load_int_call(arg, source) {
            return true;
        }
        let Some(arg_name) = identifier_text(arg, source) else {
            continue;
        };
        let Some(enclosing) = enclosing_function(store, node.start_byte()) else {
            continue;
        };
        let Some(enclosing_body) = function_body(enclosing, ctx) else {
            continue;
        };
        if bound_from_load_int_in_body(enclosing_body, &arg_name, source) {
            return true;
        }
    }
    false
}

/// The `index`-th (0-based) `call_argument`'s expression node inside an
/// `argument_list` node — the `Node`-returning counterpart to
/// `support::nth_call_argument_text`, needed here because this module
/// re-inspects the argument's own structure (`is_load_int_call`), not just
/// its text.
fn nth_call_argument_node(args: Node, index: usize) -> Option<Node> {
    let mut cursor = args.walk();
    args.named_children(&mut cursor)
        .filter(|c| c.kind() == "call_argument")
        .nth(index)
        .and_then(|ca| ca.child_by_field_name("expr"))
}

/// The function whose declaration span contains `byte_pos` — used to find
/// "which function is this call site written inside", for the one-hop
/// interprocedural case (see [`call_site_supplies_signed_argument`]).
fn enclosing_function(store: &AstStore, byte_pos: usize) -> Option<&FunctionDef> {
    store
        .functions()
        .find(|f| f.range.start_byte <= byte_pos && byte_pos < f.range.end_byte)
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

    use super::UtaDetector;
    use crate::{Detector, DetectorContext};

    fn run(source: &str) -> Vec<sting_core::Finding> {
        let tree = sting_ast::parse(source).expect("grammar loads");
        let store = AstStore::build(&tree, source);
        let ctx = DetectorContext::new(PathBuf::from("contract.tolk"), tree, source.to_string());
        UtaDetector.check(&store, &ctx)
    }

    #[test]
    fn flags_an_asymmetric_check_on_a_same_function_signed_amount() {
        let source = "fun transfer(from: slice, to: slice) {\n    val amount: int = inMsgBody.loadInt(256);\n    var fromBalance: int = 10;\n    var toBalance: int = 0;\n    fromBalance -= amount;\n    toBalance += amount;\n    assert(fromBalance > 0, 502);\n}\n";
        let findings = run(source);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].detector_id, "UTA");
        assert!(findings[0].message.contains("toBalance"));
    }

    #[test]
    fn flags_the_real_cross_function_shape() {
        // The actual hacktheton.com `token_level` shape: the signed read and
        // the asymmetric check are in different functions, linked only by
        // `transfer`'s own parameter being passed the loaded value at the
        // call site.
        let source = "fun transfer(balances: cell, from: slice, to: slice, amount: int): cell {\n    var fromBalance: int = getBalance(balances, from);\n    var toBalance: int = getBalance(balances, to);\n    fromBalance -= amount;\n    toBalance += amount;\n    assert(fromBalance > 0, 502);\n    return balances;\n}\nfun onInternalMessage(myBalance: int, msgValue: int, inMsgFull: cell, inMsgBody: slice) {\n    val amount: int = inMsgBody.loadInt(256);\n    ctxBalances = transfer(ctxBalances, senderAddress, to, amount);\n}\n";
        let findings = run(source);
        assert_eq!(findings.len(), 1);
        assert!(findings[0].message.contains("transfer"));
    }

    #[test]
    fn does_not_flag_when_the_amount_is_unsigned() {
        let source = "fun transfer(from: slice, to: slice) {\n    val amount: int = inMsgBody.loadUint(256);\n    var fromBalance: int = 10;\n    var toBalance: int = 0;\n    fromBalance -= amount;\n    toBalance += amount;\n    assert(fromBalance > 0, 502);\n}\n";
        assert!(run(source).is_empty());
    }

    #[test]
    fn does_not_flag_when_both_sides_are_checked() {
        let source = "fun transfer(from: slice, to: slice) {\n    val amount: int = inMsgBody.loadInt(256);\n    var fromBalance: int = 10;\n    var toBalance: int = 0;\n    fromBalance -= amount;\n    toBalance += amount;\n    assert(fromBalance > 0, 502);\n    assert(toBalance > 0, 503);\n}\n";
        assert!(run(source).is_empty());
    }

    #[test]
    fn does_not_flag_when_neither_side_is_checked() {
        // Deliberately out of v1 scope — see this module's doc comment.
        let source = "fun transfer(from: slice, to: slice) {\n    val amount: int = inMsgBody.loadInt(256);\n    var fromBalance: int = 10;\n    var toBalance: int = 0;\n    fromBalance -= amount;\n    toBalance += amount;\n}\n";
        assert!(run(source).is_empty());
    }

    #[test]
    fn does_not_flag_unrelated_compound_assignments_to_the_same_target() {
        let source = "fun f() {\n    val amount: int = inMsgBody.loadInt(256);\n    var total: int = 0;\n    total -= amount;\n    total += amount;\n    assert(total > 0, 1);\n}\n";
        assert!(run(source).is_empty());
    }

    #[test]
    fn does_not_flag_when_the_amount_cannot_be_traced_to_a_signed_load() {
        let source = "fun transfer(from: slice, to: slice, amount: int) {\n    var fromBalance: int = 10;\n    var toBalance: int = 0;\n    fromBalance -= amount;\n    toBalance += amount;\n    assert(fromBalance > 0, 502);\n}\n";
        assert!(run(source).is_empty());
    }

    #[test]
    fn kind_is_ast_and_severity_is_high() {
        assert_eq!(UtaDetector.kind(), crate::DetectorKind::Ast);
        assert_eq!(UtaDetector.default_severity(), sting_core::Severity::High);
    }

    #[test]
    fn flags_the_real_cross_function_shape_across_files() {
        // Same real hacktheton.com `token_level` shape as
        // flags_the_real_cross_function_shape above, but the caller that
        // supplies the signed `.loadInt` argument now lives in a real,
        // different file, resolved only through a hand-built `ProjectStore`
        // `call_site_supplies_signed_argument` used to
        // only ever see same-file call sites.
        let dir = tempfile::tempdir().expect("tempdir");
        let caller_path = dir.path().join("caller.tolk");
        std::fs::write(
            &caller_path,
            "fun onInternalMessage(myBalance: int, msgValue: int, inMsgFull: cell, inMsgBody: slice) {\n    val amount: int = inMsgBody.loadInt(256);\n    ctxBalances = transfer(ctxBalances, senderAddress, to, amount);\n}\n",
        )
        .expect("write caller.tolk");

        let project =
            std::sync::Arc::new(sting_tolkc::ProjectStore::new(sting_tolkc::SymbolTypes {
                files: vec![
                    sting_tolkc::FileEntry {
                        file_id: 0,
                        file_name: "main.tolk".to_string(),
                        size_chars: 0,
                        imports: vec![1],
                    },
                    sting_tolkc::FileEntry {
                        file_id: 1,
                        file_name: caller_path.display().to_string(),
                        size_chars: 0,
                        imports: vec![],
                    },
                ],
                unique_types: Vec::new(),
                struct_instantiations: Vec::new(),
                alias_instantiations: Vec::new(),
                declarations: Vec::new(),
                functions: Vec::new(),
            }));

        let source = "fun transfer(balances: cell, from: slice, to: slice, amount: int): cell {\n    var fromBalance: int = getBalance(balances, from);\n    var toBalance: int = getBalance(balances, to);\n    fromBalance -= amount;\n    toBalance += amount;\n    assert(fromBalance > 0, 502);\n    return balances;\n}\n";
        let tree = sting_ast::parse(source).expect("grammar loads");
        let store = AstStore::build(&tree, source);
        let ctx = DetectorContext::new("main.tolk".into(), tree, source.to_string())
            .with_project(project, 0);

        let findings = UtaDetector.check(&store, &ctx);
        assert_eq!(findings.len(), 1);
        assert!(findings[0].message.contains("transfer"));
    }

    #[test]
    fn does_not_flag_cross_file_when_no_project_is_attached() {
        // Same `transfer` body as the flagging test above, but through
        // `lint_source`'s exact shape (no `.with_project` call) — the
        // caller supplying the signed argument is not even written anywhere
        // in this test at all, so this also doubles as confirming the
        // same-file-only search alone finds nothing here, matching every
        // the earlier same-file-only baseline.
        let source = "fun transfer(balances: cell, from: slice, to: slice, amount: int): cell {\n    var fromBalance: int = getBalance(balances, from);\n    var toBalance: int = getBalance(balances, to);\n    fromBalance -= amount;\n    toBalance += amount;\n    assert(fromBalance > 0, 502);\n    return balances;\n}\n";
        assert!(run(source).is_empty());
    }
}
