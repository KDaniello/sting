//! `PL` — Precision Loss. One of the three classes deferred rather than
//! abandoned (`docs/domains/detectors.md`), it was
//! `TONScanner` codes; implemented later, direction
//! 4). Division before multiplication in the same expression:
//! `(a / b) * c` truncates the fractional remainder of `a / b` *before* `c`
//! ever has a chance to preserve it — `(a * c) / b` computes the same ratio
//! with a single truncation, at the end, on the largest possible numerator.
//! Real-world impact per the taxonomy: rounding-driven asset loss in
//! DeFi-shaped fee/share calculations.
//!
//! Purely syntactic (`DetectorKind::Ast`, same as `GVR`): a `binary_operator`
//! node whose operator is `*` and whose left or right operand is itself a
//! `binary_operator` node whose own operator is `/`. No per-function or
//! per-contract scoping needed — unlike `GVR`/`ID`/`UBM`, this pattern is
//! entirely local to one expression, nothing to attribute across a file.
//!
//! **One deliberate exclusion**: `(a / b) * b` — dividing by `b`, then
//! multiplying back by that *same* `b` — is not this bug. It is the common
//! "round `a` down to the nearest multiple of `b`" idiom (gas/storage-duration
//! granularity, TON amounts rounded to a step), and the result is exactly
//! what the code intends, not an accidental precision loss. Detected by
//! comparing the division's divisor text against the multiplication's other
//! operand text; if they match exactly, this expression is skipped. Anywhere
//! this textual comparison cannot be read cleanly, the expression is still
//! flagged (the primary div-before-mul shape is confirmed either way — an
//! unprovable exclusion must not silently swallow a confirmed finding, the
//! same asymmetry `ID`/`TLB` apply to their own ambiguous cases, just in the
//! opposite direction: there the *shape itself* is ambiguous so nothing
//! fires, here the shape is unambiguous and only a narrow safe-idiom
//! exception could suppress it).
//!
//! **v1 recognizes only the plain `/` operator**, not FunC/Tolk's rounding
//! variants `~/` (round) / `^/` (ceiling) — confirmed via grep across this
//! entire workspace's own corpus (`corpus/`): neither operator appears
//! anywhere in it, so there is no real usage to validate detection against.
//! Not guessed at; a documented v1 boundary, not an oversight.

use sting_ast::AstStore;
use sting_core::{DetectorId, Finding, Severity};
use tree_sitter::Node;

use crate::support::location_of;
use crate::{Detector, DetectorContext, DetectorKind};

pub(crate) struct PlDetector;

impl Detector for PlDetector {
    fn id(&self) -> DetectorId {
        "PL"
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
            if node.kind() != "binary_operator" || operator_kind(node) != Some("*") {
                continue;
            }
            let (Some(left), Some(right)) = (node.named_child(0), node.named_child(1)) else {
                continue;
            };
            let (left, right) = (unwrap_parens(left), unwrap_parens(right));
            if is_risky_division(left, right, source) || is_risky_division(right, left, source) {
                findings.push(Finding {
                    detector_id: self.id(),
                    severity: self.default_severity(),
                    message: format!(
                        "`{}` divides before multiplying — the fractional remainder is truncated before the multiplication can preserve it; reorder to multiply first, then divide, to lose precision only once",
                        node_text(node, source).unwrap_or("<expr>")
                    ),
                    remediation: "swap the operator order so the multiplication runs first (`a * b / c` instead of `a / c * b`) — if this really is a deliberate round-down-to-a-multiple-of-the-divisor idiom, make the divisor and multiplier textually identical so this detector's own exclusion recognizes it".to_string(),
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

/// `candidate` is a `/`-shaped division whose result feeds `other` via
/// multiplication, and it is not the "round down to a multiple" idiom (see
/// this module's doc comment) — `other`'s text does not exactly match
/// `candidate`'s own divisor.
fn is_risky_division(candidate: Node, other: Node, source: &str) -> bool {
    if candidate.kind() != "binary_operator" || operator_kind(candidate) != Some("/") {
        return false;
    }
    let Some(divisor) = candidate.named_child(1) else {
        return true;
    };
    let divisor = unwrap_parens(divisor);
    let other = unwrap_parens(other);
    let (Some(divisor_text), Some(other_text)) =
        (node_text(divisor, source), node_text(other, source))
    else {
        return true;
    };
    divisor_text.trim() != other_text.trim()
}

/// Strips any number of redundant parentheses (`((a / b))` → `a / b`) so the
/// division-shape/divisor-text checks above see the real expression
/// underneath, not a `parenthesized_expression` wrapper — needed the moment
/// a division is the multiplication's right operand, since Tolk's grammar
/// (confirmed against a real parse) requires parens there for the div to
/// bind inside the mul at all (`c * (a / b)`, never bare `c * a / b`, which
/// left-associative `*`/`/` at the same precedence would instead parse as
/// `(c * a) / b` — the safe order, not this pattern).
fn unwrap_parens(mut node: Node) -> Node {
    while node.kind() == "parenthesized_expression"
        && let Some(inner) = node.child_by_field_name("inner")
    {
        node = inner;
    }
    node
}

/// A `binary_operator` node's operator symbol. Confirmed against a real
/// parse during this detector's implementation: `binary_operator` has 3
/// children in source order, `[left, operator, right]`, but only `left`/
/// `right` are *named* (`named_child(0)`/`named_child(1)`) — the operator
/// itself is an **anonymous** token whose `kind()` *is* its own text (the
/// same shape ordinary punctuation like `(`/`;` takes elsewhere in this
/// grammar), reachable only via the raw, not-just-named, `child(1)`. No
/// `operator_name` node exists — an earlier version of this detector assumed
/// one and it silently matched nothing at all (`named_child(1)` is the
/// *right operand*, not the operator, once anonymous children are excluded
/// from named indexing) until real tests caught the miss.
fn operator_kind(node: Node) -> Option<&'static str> {
    match node.child(1)?.kind() {
        "*" => Some("*"),
        "/" => Some("/"),
        _ => None,
    }
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

    use super::PlDetector;
    use crate::{Detector, DetectorContext};

    fn run(source: &str) -> Vec<sting_core::Finding> {
        let tree = sting_ast::parse(source).expect("grammar loads");
        let store = AstStore::build(&tree, source);
        let ctx = DetectorContext::new(PathBuf::from("contract.tolk"), tree, source.to_string());
        PlDetector.check(&store, &ctx)
    }

    #[test]
    fn flags_division_before_multiplication() {
        let source = "fun f(a: int, b: int, c: int) {\n    val share = a / b * c;\n}\n";
        let findings = run(source);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].detector_id, "PL");
        assert!(findings[0].message.contains("a / b * c"));
    }

    #[test]
    fn flags_division_on_the_right_operand() {
        let source = "fun f(a: int, b: int, c: int) {\n    val share = c * (a / b);\n}\n";
        let findings = run(source);
        assert_eq!(findings.len(), 1);
    }

    #[test]
    fn flags_a_double_division_multiplication() {
        let source =
            "fun f(a: int, b: int, c: int, d: int) {\n    val share = (a / b) * (c / d);\n}\n";
        let findings = run(source);
        assert_eq!(findings.len(), 1);
    }

    #[test]
    fn does_not_flag_multiplication_before_division() {
        let source = "fun f(a: int, b: int, c: int) {\n    val share = a * c / b;\n}\n";
        assert!(run(source).is_empty());
    }

    #[test]
    fn does_not_flag_rounding_down_to_a_multiple_of_the_same_divisor() {
        // `(a / b) * b` — intentional floor-rounding idiom, not this bug.
        let source = "fun f(a: int, b: int) {\n    val rounded = a / b * b;\n}\n";
        assert!(run(source).is_empty());
    }

    #[test]
    fn does_not_flag_rounding_down_with_the_multiplier_first() {
        let source = "fun f(a: int, b: int) {\n    val rounded = b * (a / b);\n}\n";
        assert!(run(source).is_empty());
    }

    #[test]
    fn does_not_flag_plain_multiplication_with_no_division() {
        let source = "fun f(a: int, b: int) {\n    val product = a * b;\n}\n";
        assert!(run(source).is_empty());
    }

    #[test]
    fn does_not_flag_a_lone_division_with_no_multiplication() {
        let source = "fun f(a: int, b: int) {\n    val quotient = a / b;\n}\n";
        assert!(run(source).is_empty());
    }

    #[test]
    fn kind_is_ast_and_severity_is_medium() {
        assert_eq!(PlDetector.kind(), crate::DetectorKind::Ast);
        assert_eq!(PlDetector.default_severity(), sting_core::Severity::Medium);
    }
}
