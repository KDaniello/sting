//! `BR` — Bad Randomness. One of the three classes deferred rather than
//! abandoned (`docs/domains/detectors.md`), it was
//! `TONScanner` codes; implemented later, direction
//! 4). Using the transaction's logical time (or block Unix time) as a seed
//! for what looks like a random pick — `blockchain.now() % 100` to pick a
//! lottery winner, an NFT trait, a reward amount. Both values are set by
//! whichever validator produces the block, not by anything unpredictable to
//! them — a validator who also controls (or colludes with) a transaction
//! that depends on this "randomness" can bias the outcome in their favor by
//! choosing when to include it.
//!
//! Purely syntactic (`DetectorKind::Ast`, same as `GVR`/`PL`): a
//! `binary_operator` node whose operator is `%` (modulo — the standard
//! "reduce a large number to a small range" idiom for turning any value into
//! a bounded pick) and whose left or right operand is itself a direct call
//! to `blockchain.now()` or `blockchain.logicalTime()`. No per-function or
//! per-contract scoping needed, same reasoning as `PL`.
//!
//! **Recognizes exactly two calls**, both confirmed as the real, current
//! Tolk idiom by grep across this workspace's own corpus (`corpus/`; no bare
//! `now()`/`curLt()` builtin appears anywhere, only the `blockchain.`-
//! qualified methods): `blockchain.now()` (Unix time) and
//! `blockchain.logicalTime()` (transaction logical time) — both receiver
//! *and* method name are checked, not method name alone, so an unrelated
//! `.now()` on some other type is not accidentally matched.
//!
//! **Scoped to a *direct* call as the modulo operand** — not a variable that
//! was assigned from one earlier (`val seed = blockchain.now(); val pick =
//! seed % 100;` is not flagged in v1). The real corpus's own `blockchain.now()`
//! usage is overwhelmingly for expiry/deadline comparisons, never modulo, so
//! this narrow direct-operand scope already covers the shape `TONScanner`'s
//! own definition describes without needing dataflow tracking through an
//! intermediate variable — the same "start narrow, broaden only once a real
//! missed case is found" discipline `ID`/`TLB`/`PL` all document for their
//! own v1 boundaries.
//!
//! **Not flagged**: `random.uint256()`/`random.initialize()` (a real,
//! separate stdlib module for actual pseudorandomness, used correctly
//! elsewhere in this corpus's own `corpus/clean/acton-contracts/config/
//! config-helpers.tolk`'s `scanRandomProposal`) — different receiver
//! (`random`, not `blockchain`) and different method names entirely, so
//! this detector's own method/receiver check already excludes it structurally,
//! not via a special-cased exception.

use sting_ast::{AstStore, Language};
use sting_core::{DetectorId, Finding, Severity};
use tree_sitter::Node;

use crate::support::{as_method_call, identifier_text, location_of};
use crate::{Detector, DetectorContext, DetectorKind};

pub(crate) struct BrDetector;

impl Detector for BrDetector {
    fn id(&self) -> DetectorId {
        "BR"
    }

    fn kind(&self) -> DetectorKind {
        DetectorKind::Ast
    }

    fn default_severity(&self) -> Severity {
        Severity::Medium
    }

    fn check(&self, _store: &AstStore, ctx: &DetectorContext) -> Vec<Finding> {
        let source = ctx.source();
        let mut findings = Vec::new();
        for visited in sting_ast::Preorder::from_node(ctx.tree().root_node()) {
            let node = visited.node;
            if node.kind() != "binary_operator" || operator_kind(node) != Some("%") {
                continue;
            }
            let (Some(left), Some(right)) = (node.named_child(0), node.named_child(1)) else {
                continue;
            };
            let culprit = time_source_method(unwrap_parens(left), source)
                .or_else(|| time_source_method(unwrap_parens(right), source));
            if let Some(method) = culprit {
                findings.push(Finding {
                    detector_id: self.id(),
                    severity: self.default_severity(),
                    message: format!(
                        "`{}` uses `blockchain.{method}()` as a modulo seed — block-producer-controlled time is not unpredictable to the validator producing it, so this \"random\" pick can be biased",
                        node_text(node, source).unwrap_or("<expr>")
                    ),
                    remediation: format!(
                        "replace `blockchain.{method}() % ...` with a source the validator producing this block cannot bias — e.g. `random.uint256()` after `random.initialize()`, or a commit-reveal scheme if the outcome must be unpredictable to participants too"
                    ),
                    location: location_of(node, ctx),
                    confidence: None,
                    dynamic_verification: None,
                    suggestion: None,
                });
            }
        }
        findings
    }
}

/// If `node` is a direct call to `blockchain.now()`/`blockchain.logicalTime()`,
/// the method name (`"now"`/`"logicalTime"`) — see this module's doc comment
/// for why both the receiver and the method name are checked.
fn time_source_method<'a>(node: Node, source: &'a str) -> Option<&'a str> {
    let call = as_method_call(node, source, Language::Tolk)?;
    if identifier_text(call.obj, source).as_deref() != Some("blockchain") {
        return None;
    }
    matches!(call.method.as_str(), "now" | "logicalTime").then(|| {
        // `call.method` borrows `source` for as long as `node` does, so
        // re-reading the method node's own text (rather than returning
        // `call.method` directly, an owned `String`) keeps this function's
        // return type a borrow instead of forcing an allocation on every
        // `binary_operator` visited, most of which are not even a `%`.
        call.method_node.utf8_text(source.as_bytes()).unwrap_or("")
    })
}

fn unwrap_parens(mut node: Node) -> Node {
    while node.kind() == "parenthesized_expression"
        && let Some(inner) = node.child_by_field_name("inner")
    {
        node = inner;
    }
    node
}

/// A `binary_operator` node's operator symbol — see `pl.rs`'s own
/// `operator_kind` doc comment for why this reads `child(1)` (an anonymous
/// token), not a named child.
fn operator_kind(node: Node) -> Option<&'static str> {
    (node.child(1)?.kind() == "%").then_some("%")
}

fn node_text<'a>(node: Node, source: &'a str) -> Option<&'a str> {
    node.utf8_text(source.as_bytes()).ok()
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

    use super::BrDetector;
    use crate::{Detector, DetectorContext};

    fn run(source: &str) -> Vec<sting_core::Finding> {
        let tree = sting_ast::parse(source).expect("grammar loads");
        let store = AstStore::build(&tree, source);
        let ctx = DetectorContext::new(PathBuf::from("contract.tolk"), tree, source.to_string());
        BrDetector.check(&store, &ctx)
    }

    #[test]
    fn flags_now_used_as_a_modulo_seed() {
        let source = "fun pickWinner(participantCount: int): int {\n    return blockchain.now() % participantCount;\n}\n";
        let findings = run(source);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].detector_id, "BR");
        assert!(findings[0].message.contains("now"));
    }

    #[test]
    fn flags_logical_time_used_as_a_modulo_seed() {
        let source = "fun pickWinner(participantCount: int): int {\n    return blockchain.logicalTime() % participantCount;\n}\n";
        let findings = run(source);
        assert_eq!(findings.len(), 1);
        assert!(findings[0].message.contains("logicalTime"));
    }

    #[test]
    fn flags_the_time_call_as_the_right_operand() {
        let source = "fun pickWinner(participantCount: int): int {\n    return participantCount % blockchain.now();\n}\n";
        assert_eq!(run(source).len(), 1);
    }

    #[test]
    fn flags_a_parenthesized_time_call() {
        let source = "fun pickWinner(participantCount: int): int {\n    return (blockchain.now()) % participantCount;\n}\n";
        assert_eq!(run(source).len(), 1);
    }

    #[test]
    fn does_not_flag_now_used_outside_a_modulo() {
        let source =
            "fun isExpired(deadline: int): bool {\n    return blockchain.now() > deadline;\n}\n";
        assert!(run(source).is_empty());
    }

    #[test]
    fn does_not_flag_a_modulo_with_no_time_call() {
        let source =
            "fun wrapSeqno(seqno: int, modulo: int): int {\n    return seqno % modulo;\n}\n";
        assert!(run(source).is_empty());
    }

    #[test]
    fn does_not_flag_the_real_random_module() {
        let source = "fun pickEntry(bound: int): int {\n    random.initialize();\n    return random.uint256() % bound;\n}\n";
        assert!(run(source).is_empty());
    }

    #[test]
    fn does_not_flag_an_unrelated_receiver_with_the_same_method_name() {
        let source = "fun f(bound: int): int {\n    return timer.now() % bound;\n}\n";
        assert!(run(source).is_empty());
    }

    #[test]
    fn kind_is_ast_and_severity_is_medium() {
        assert_eq!(BrDetector.kind(), crate::DetectorKind::Ast);
        assert_eq!(BrDetector.default_severity(), sting_core::Severity::Medium);
    }
}
