//! `EPS` — Ensure PRG Seed .
//!
//! **Sting's own addition, not a [`sting_core::DefectClass`] member**
//! (**corrected 2026-07-22**, found stale by review,
//! : this previously said "same reasoning as
//! `SUA`/`UTA`," implying those two also sit outside `DefectClass` — they
//! don't, both are real members of `DefectClass::ALL`, see
//! `sting-core/src/lib.rs`; only `EPS` and `UDA` are not. The comment was
//! written before `Sua`/`Uta` were added to the enum and never revisited.
//! `DetectorId` — what actually drives detector registration/dispatch — is
//! a bare `&'static str` independent of `DefectClass` either way, so this
//! was cosmetic, not a functional bug): a real, narrow vulnerability class
//! found via prior-art research (Misti-style "ensure the PRG is seeded"
//! check), not this project's own corpus audit.
//!
//! **The security claim, mechanistically confirmed against `ton-blockchain/
//! ton`'s real C++ source, not assumed from documentation prose alone**
//! (research notes have the full trace):
//! `Transaction::prepare_rand_seed` (`crypto/block/transaction.cpp`) derives
//! a transaction's *initial* randomness seed as `SHA256(block_rand_seed .
//! addr)` — **deliberately excluding** the transaction's own logical time
//! (`trans_lt`); the TON developers' own comment at that exact line
//! discusses and rejects including it. Consequence: **every message that
//! lands on the same contract within the same block starts from an
//! identical seed.** `RANDU256`/`RAND` (`crypto/vm/tonops.cpp`, i.e.
//! `random.uint256()`/`random.range()` below) are a pure, deterministic
//! SHA512 chain from that seed — no hidden entropy source. `random.
//! initialize()` (`vendor/tolk/tolk-stdlib/common.tolk:723-724`, `asm
//! "LTIME" "ADDRAND"`) exists specifically to fix this: it mixes in
//! `trans_lt` (`ADDRAND`: `new_seed = SHA256(old_seed || LTIME)`), breaking
//! the same-block correlation between otherwise-identical seeds.
//!
//! **The real exploit shape**: an attacker able to get several of their own
//! messages processed against the victim contract within one block (an
//! ordinary burst-send, not a validator-collusion attack) observes the
//! "random" outcome of the first message and thereby learns the outcome of
//! every other message in that same block that reads randomness before any
//! seed-changing call — a real, practical lever against lotteries/loot
//! boxes/randomized minting, not a theoretical curiosity.
//!
//! **Honest severity ceiling, stated up front, not just in the remediation
//! text**: even a contract that calls `random.initialize()` correctly
//! before every use remains only as unpredictable as `block_rand_seed`
//! itself, which a colluding validator ultimately controls — TON's own
//! documentation rates that residual risk "Low". This detector (and its
//! fix) closes the same-block-replay hole specifically; it does not, and
//! cannot, promise resistance to a colluding-validator attack. Severity is
//! [`sting_core::Severity::Medium`] to reflect that ceiling honestly, not
//! [`sting_core::Severity::Critical`].
//!
//! **`DetectorKind::Ast` "with elements of a local CFG", same category as
//! `lep.rs`** — not `cfg.rs`'s generic `WorklistSolver` (`id`/`ubm`/`tlb`'s
//! machinery): the fact this detector tracks ("has a seed-changing call
//! happened yet on this specific path") only ever needs to flow forward
//! through straight-line code and fork into `if`/`match`/loop bodies, never
//! merge two paths back together the way `LEP`'s "on every path" question
//! does — a plain recursive walk threading a `bool` forward is a complete,
//! simpler substitute. A branch's own seed-changing call does **not**
//! propagate back out past that branch (we cannot prove the branch
//! executed) — the same conservative "no cross-branch leakage" default
//! `lep.rs`'s own doc comment documents as a *known gap* found after the
//! fact; here it is the deliberate design from the start, not a later fix.
//!
//! **Recognizes three real seeding calls, not just `random.initialize()`**
//! (confirmed against `vendor/tolk/tolk-stdlib/common.tolk`, not guessed by
//! analogy): `random.initialize()` (mixes in `LTIME`, the documented fix),
//! `random.initializeBy(...)` (`asm "ADDRAND"`, manual mixing with any
//! value), and `random.setSeed(...)` (`asm "SETRAND"`, an outright
//! overwrite) — all three genuinely change the seed away from the shared
//! same-block default, so all three count as "now seeded" for this
//! detector's purposes. `random.getSeed()` does **not** count: reading the
//! current seed changes nothing.
//!
//! **v1 scope, deliberately narrow, matching every other detector's own
//! "start narrow" precedent (`BR`/`PL`/`UR`/`UTA`)**: same-function only —
//! a helper function that calls `random.uint256()` without itself calling a
//! seeding function, relying on its caller having already seeded earlier in
//! the same transaction, is a known, undetected false-positive shape (no
//! interprocedural call-following, unlike `AC`/`UTA`). Not found in real
//! code yet, not solved speculatively — the one confirmed real organic use
//! in this project's own corpus
//! (`corpus/clean/acton-contracts/config/config-helpers.tolk`'s
//! `scanRandomProposal`) calls `random.initialize()` and `random.uint256()`
//! in the same function, so this gap has not actually been observed.

use sting_ast::{AstStore, Language};
use sting_core::{DetectorId, Finding, Severity};
use tree_sitter::Node;

use crate::support::{
    as_method_call, block_children, function_body, identifier_text, location_of, match_arm_body,
};
use crate::{Detector, DetectorContext, DetectorKind};

/// `random.<method>()` calls that generate a new pseudo-random value —
/// require a preceding seed change on the same path.
const RANDOM_GENERATION_METHODS: &[&str] = &["uint256", "range"];

/// `random.<method>()` calls that change the seed away from the shared
/// same-block default — any one of these makes subsequent generation calls
/// on the same path safe (see this module's own doc comment for why all
/// three, not just `initialize`).
const RANDOM_SEEDING_METHODS: &[&str] = &["initialize", "initializeBy", "setSeed"];

pub(crate) struct EpsDetector;

impl Detector for EpsDetector {
    fn id(&self) -> DetectorId {
        "EPS"
    }

    fn kind(&self) -> DetectorKind {
        DetectorKind::Ast
    }

    fn default_severity(&self) -> Severity {
        Severity::Medium
    }

    fn check(&self, store: &AstStore, ctx: &DetectorContext) -> Vec<Finding> {
        let source = ctx.source();
        let mut unseeded_uses = Vec::new();

        for function in store.functions() {
            let Some(body) = function_body(function, ctx) else {
                continue;
            };
            walk_block(&block_children(body), false, source, &mut unseeded_uses);
        }

        unseeded_uses
            .into_iter()
            .map(|use_node| Finding {
                detector_id: self.id(),
                severity: self.default_severity(),
                message: "`random.uint256()`/`random.range()` called with no preceding `random.initialize()`/`initializeBy()`/`setSeed()` on this path — the PRG seed starts identical for every message this contract processes within the same block".to_string(),
                remediation: "call `random.initialize()` before generating any value from `random.uint256()`/`random.range()` — it mixes in this transaction's logical time (LTIME), breaking the shared same-block seed; note this closes same-block replay specifically, it does not defend against a colluding validator, which TON's own documentation already rates as a separate, lower residual risk".to_string(),
                location: location_of(use_node, ctx),
                confidence: None,
                dynamic_verification: None,
                suggestion: None,
            })
            .collect()
    }
}

/// Walks `stmts` in source order, threading `seeded_in` forward across
/// straight-line statements and forking (without merging back) into `if`/
/// `match`/`while`/`do-while` bodies — see this module's own doc comment
/// for why no merge is needed here, unlike `LEP`'s "on every path"
/// question. Every unseeded `random.uint256()`/`random.range()` call found
/// anywhere is appended to `unseeded_uses`.
fn walk_block<'a>(
    stmts: &[Node<'a>],
    seeded_in: bool,
    source: &str,
    unseeded_uses: &mut Vec<Node<'a>>,
) {
    let mut seeded = seeded_in;
    for &stmt in stmts {
        match stmt.kind() {
            "if_statement" => {
                if let Some(body) = stmt.child_by_field_name("body") {
                    walk_block(&block_children(body), seeded, source, unseeded_uses);
                }
                match stmt.child_by_field_name("alternative") {
                    Some(alt) if alt.kind() == "if_statement" => {
                        walk_block(std::slice::from_ref(&alt), seeded, source, unseeded_uses);
                    }
                    Some(alt) => walk_block(&block_children(alt), seeded, source, unseeded_uses),
                    None => {}
                }
            }
            "match_statement" => {
                walk_match_arms(stmt, seeded, source, unseeded_uses);
            }
            "while_statement" => {
                if let Some(body) = stmt.child_by_field_name("body") {
                    walk_block(&block_children(body), seeded, source, unseeded_uses);
                }
            }
            "do_while_statement" => {
                // Same reasoning as `ac.rs`'s/`ur.rs`'s own do-while
                // handling: the condition is checked *after* the body on
                // every iteration, so entry cannot be gated by it — this
                // does not change how *this* detector treats the body
                // either way (it forks unconditionally like `while` does),
                // named here only for why both loop kinds are handled the
                // same way despite the semantic difference.
                if let Some(body) = stmt.child_by_field_name("body") {
                    walk_block(&block_children(body), seeded, source, unseeded_uses);
                }
            }
            _ => {
                collect_unseeded_uses_in_statement(stmt, seeded, source, unseeded_uses);
                if contains_seeding_call(stmt, source) {
                    seeded = true;
                }
            }
        }
    }
}

fn walk_match_arms<'a>(
    match_stmt: Node<'a>,
    seeded: bool,
    source: &str,
    unseeded_uses: &mut Vec<Node<'a>>,
) {
    let Some(arms_body) = match_stmt
        .named_child(0)
        .and_then(|match_expr| match_expr.child_by_field_name("body"))
    else {
        return;
    };
    let mut cursor = arms_body.walk();
    for arm in arms_body
        .named_children(&mut cursor)
        .filter(|c| c.kind() == "match_arm")
    {
        match match_arm_body(arm) {
            Some(b) if b.kind() == "block_statement" => {
                walk_block(&block_children(b), seeded, source, unseeded_uses);
            }
            Some(b) => walk_block(std::slice::from_ref(&b), seeded, source, unseeded_uses),
            None => {}
        }
    }
}

/// If `random_call`'s receiver is literally the `random` namespace (not an
/// unrelated type with a coincidentally same-named method).
fn is_random_namespace_call(obj: Node, source: &str) -> bool {
    identifier_text(obj, source).as_deref() == Some("random")
}

/// Appends every `random.uint256()`/`random.range()` call found anywhere in
/// `stmt`'s subtree to `unseeded_uses`, if `seeded` is `false` — a flat
/// `Preorder` scan over the whole statement is safe here (not just a
/// top-level check) because a seeding call *within the same statement* as a
/// generation call is not a real shape any observed code produces (both
/// calls in `config-helpers.tolk`'s own real, safe usage are separate
/// statements) — see this module's own doc comment's v1 scope note.
fn collect_unseeded_uses_in_statement<'a>(
    stmt: Node<'a>,
    seeded: bool,
    source: &str,
    unseeded_uses: &mut Vec<Node<'a>>,
) {
    if seeded {
        return;
    }
    for visited in sting_ast::Preorder::from_node(stmt) {
        let node = visited.node;
        if let Some(call) = as_method_call(node, source, Language::Tolk)
            && is_random_namespace_call(call.obj, source)
            && RANDOM_GENERATION_METHODS.contains(&call.method.as_str())
        {
            unseeded_uses.push(call.method_node);
        }
    }
}

fn contains_seeding_call(stmt: Node, source: &str) -> bool {
    sting_ast::Preorder::from_node(stmt)
        .map(|v| v.node)
        .filter_map(|n| as_method_call(n, source, Language::Tolk))
        .any(|call| {
            is_random_namespace_call(call.obj, source)
                && RANDOM_SEEDING_METHODS.contains(&call.method.as_str())
        })
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

    use super::EpsDetector;
    use crate::{Detector, DetectorContext};

    fn run(source: &str) -> Vec<sting_core::Finding> {
        let tree = sting_ast::parse(source).expect("grammar loads");
        let store = AstStore::build(&tree, source);
        let ctx = DetectorContext::new(PathBuf::from("contract.tolk"), tree, source.to_string());
        EpsDetector.check(&store, &ctx)
    }

    #[test]
    fn flags_random_uint256_with_no_preceding_seed_call() {
        let source = "fun f(): int {\n    return random.uint256();\n}\n";
        let findings = run(source);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].detector_id, "EPS");
        assert!(findings[0].message.contains("random.initialize"));
    }

    #[test]
    fn flags_random_range_with_no_preceding_seed_call() {
        let source = "fun f(): int {\n    return random.range(100);\n}\n";
        let findings = run(source);
        assert_eq!(findings.len(), 1);
    }

    #[test]
    fn does_not_flag_when_initialize_precedes_the_use_in_the_same_function() {
        // The one real, organic, already-safe shape in this project's own
        // corpus: corpus/clean/acton-contracts/config/config-helpers.tolk's
        // scanRandomProposal.
        let source = "fun f(): int {\n    random.initialize();\n    return random.uint256();\n}\n";
        assert!(run(source).is_empty());
    }

    #[test]
    fn does_not_flag_when_initialize_by_precedes_the_use() {
        let source =
            "fun f(): int {\n    random.initializeBy(42);\n    return random.uint256();\n}\n";
        assert!(run(source).is_empty());
    }

    #[test]
    fn does_not_flag_when_set_seed_precedes_the_use() {
        let source =
            "fun f(seed: int): int {\n    random.setSeed(seed);\n    return random.uint256();\n}\n";
        assert!(run(source).is_empty());
    }

    #[test]
    fn does_not_treat_get_seed_as_a_mitigation() {
        let source =
            "fun f(): int {\n    val s = random.getSeed();\n    return random.uint256();\n}\n";
        let findings = run(source);
        assert_eq!(
            findings.len(),
            1,
            "getSeed only reads the seed, it does not change it"
        );
    }

    #[test]
    fn flags_a_use_before_the_seed_call_even_if_one_follows_later() {
        let source = "fun f(): int {\n    val x = random.uint256();\n    random.initialize();\n    return x;\n}\n";
        let findings = run(source);
        assert_eq!(
            findings.len(),
            1,
            "the seed call comes too late to protect the earlier use"
        );
    }

    #[test]
    fn flags_a_use_inside_an_if_branch_when_no_seed_call_precedes_it() {
        let source = "fun f(cond: bool): int {\n    if (cond) {\n        return random.uint256();\n    }\n    return 0;\n}\n";
        let findings = run(source);
        assert_eq!(findings.len(), 1);
    }

    #[test]
    fn does_not_flag_a_use_inside_an_if_branch_when_seeded_before_the_branch() {
        let source = "fun f(cond: bool): int {\n    random.initialize();\n    if (cond) {\n        return random.uint256();\n    }\n    return 0;\n}\n";
        assert!(run(source).is_empty());
    }

    #[test]
    fn a_seed_call_inside_one_branch_does_not_protect_code_after_the_if() {
        // Conservative by design (not a discovered gap, see this module's
        // own doc comment): we cannot prove `cond` was true, so the seed
        // call inside the branch must not count for code after the `if`.
        let source = "fun f(cond: bool): int {\n    if (cond) {\n        random.initialize();\n    }\n    return random.uint256();\n}\n";
        let findings = run(source);
        assert_eq!(findings.len(), 1);
    }

    #[test]
    fn flags_a_use_inside_a_match_arm_with_no_preceding_seed_call() {
        let source = "fun f(x: int): int {\n    match (x) {\n        1 => { return random.uint256(); }\n        else => { return 0; }\n    }\n}\n";
        let findings = run(source);
        assert_eq!(findings.len(), 1);
    }

    #[test]
    fn does_not_flag_a_match_arm_use_when_seeded_before_the_match() {
        let source = "fun f(x: int): int {\n    random.initialize();\n    match (x) {\n        1 => { return random.uint256(); }\n        else => { return 0; }\n    }\n}\n";
        assert!(run(source).is_empty());
    }

    #[test]
    fn flags_a_use_inside_a_while_loop_with_no_preceding_seed_call() {
        let source = "fun f(cond: bool): int {\n    while (cond) {\n        return random.uint256();\n    }\n    return 0;\n}\n";
        let findings = run(source);
        assert_eq!(findings.len(), 1);
    }

    #[test]
    fn does_not_flag_an_unrelated_type_with_a_coincidentally_named_method() {
        let source = "fun f(x: MyRandom): int {\n    return x.uint256();\n}\n";
        assert!(run(source).is_empty());
    }

    #[test]
    fn kind_is_ast_and_severity_is_medium() {
        assert_eq!(EpsDetector.kind(), crate::DetectorKind::Ast);
        assert_eq!(EpsDetector.default_severity(), sting_core::Severity::Medium);
    }
}
