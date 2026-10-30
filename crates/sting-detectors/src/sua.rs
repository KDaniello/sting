//! `SUA` — Suspicious Unary Assignment. The classic C-family `x =+ 1`/
//! `x =- 1` lexical typo: parses as `x = (+1)`/`x = (-1)` (a plain
//! assignment of a unary-prefixed expression), almost certainly meant as
//! `x += 1`/`x -= 1`. In a balance/quantity-tracking context this *replaces*
//! the tracked value instead of *adjusting* it — a silent correctness bug,
//! not a style nit.
//!
//! **Sting's own addition, ported from Misti's `SuspiciousUnary`**
//! (from the Misti teardown) — not a
//! `TONScanner` code, not a `sting_core::DefectClass` member (same
//! reasoning as `UTA`: a real, narrow, well-defined vulnerability class
//! found via prior-art research, not this project's own corpus audit).
//! Misti scores its own version `Low` severity; matched here rather than
//! guessed independently.
//!
//! **Detected by exact byte adjacency, not by re-deriving whitespace
//! semantics** — confirmed via a real parse (not assumed): `x =+ 1`'s `=`
//! token ends at the exact byte the following `unary_operator` node starts
//! at (zero gap), while `x = +1` (deliberate unary use) has the space
//! between them, so the two byte offsets differ. `DetectorKind::Ast`, no
//! per-function/per-contract scoping needed — the same shape means the same
//! thing regardless of where it appears.
//!
//! **Scoped to a plain `assignment` (`=`) whose `right` is a `unary_operator`
//! with `+`/`-`** — deliberately not `set_assignment` (`+=`/`-=`/...),
//! which is the *correct* spelling this bug is mistaken for, and deliberately
//! not `!`/other unary operators, which have no compound-assignment
//! equivalent to be confused with in the first place.

use sting_ast::AstStore;
use sting_core::{DetectorId, Finding, Severity, Suggestion};
use tree_sitter::Node;

use crate::support::location_of;
use crate::{Detector, DetectorContext, DetectorKind};

pub(crate) struct SuaDetector;

impl Detector for SuaDetector {
    fn id(&self) -> DetectorId {
        "SUA"
    }

    fn kind(&self) -> DetectorKind {
        DetectorKind::Ast
    }

    fn default_severity(&self) -> Severity {
        Severity::Low
    }

    fn check(&self, _store: &AstStore, ctx: &DetectorContext) -> Vec<Finding> {
        let source = ctx.source();
        let mut findings = Vec::new();
        for visited in sting_ast::Preorder::from_node(ctx.tree().root_node()) {
            let node = visited.node;
            if let Some(typo) = suspicious_unary_operator(node) {
                let intended = if typo.operator == "+" { "+=" } else { "-=" };
                findings.push(Finding {
                    detector_id: self.id(),
                    severity: self.default_severity(),
                    message: format!(
                        "`{}` — no space between `=` and the following unary `{}` reads like a mistyped `{intended}`, not a deliberate unary expression",
                        node_text(node, source).unwrap_or("<expr>"),
                        typo.operator
                    ),
                    remediation: format!(
                        "if this was meant to adjust the value, write `{intended}`; if the unary `{}` is deliberate, add a space after `=` to make that unambiguous",
                        typo.operator
                    ),
                    location: location_of(node, ctx),
                    confidence: None,
                    dynamic_verification: None,
                    // A deterministic, single-answer fix — `=+`/`=-` has
                    // exactly one plausible intended spelling, `+=`/`-=`
                    // (see this module's own doc comment: the *other*
                    // reading, a deliberate unary expression, is exactly
                    // what a human adding a space would signal — a real
                    // ambiguity resolved by the fix itself, not guessed
                    // away).
                    suggestion: Some(Suggestion {
                        start_byte: typo.equals_start,
                        end_byte: typo.operator_end,
                        replacement: intended.to_string(),
                    }),
                });
            }
        }
        findings
    }
}

/// The parts of a confirmed `x =+ 1`/`x =- 1` typo [`SuaDetector::check`]
/// needs: the recognized operator's own text, plus the exact byte span
/// covering `=` immediately followed by `+`/`-` (`equals_start..
/// operator_end`) — the 2-byte replacement target for `Suggestion`
/// (`"=+"`/`"=-"` → `"+="`/`"-="`, swapping the two characters' order is
/// the whole fix).
struct SuaTypo {
    operator: &'static str,
    equals_start: usize,
    operator_end: usize,
}

/// If `node` is an `assignment` (`=`) whose `right` is a `unary_operator`
/// with operator `+`/`-`, immediately adjacent to the `=` token in the raw
/// source (zero bytes between them), the confirmed typo's details.
fn suspicious_unary_operator(node: Node) -> Option<SuaTypo> {
    if node.kind() != "assignment" {
        return None;
    }
    let equals = node.child(1)?;
    if equals.kind() != "=" {
        return None;
    }
    let right = node.child_by_field_name("right")?;
    if right.kind() != "unary_operator" || right.start_byte() != equals.end_byte() {
        return None;
    }
    // `operator_name` — the same named field `ur.rs`'s own
    // `negated_is_found_check` already reads on this node kind, for `!`
    // rather than `+`/`-`; reused here rather than a positional `child(0)`
    // access for the same robustness reason.
    let operator_name = right.child_by_field_name("operator_name")?;
    let operator = match operator_name.kind() {
        "+" => "+",
        "-" => "-",
        _ => return None,
    };
    Some(SuaTypo {
        operator,
        equals_start: equals.start_byte(),
        operator_end: operator_name.end_byte(),
    })
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

    use super::SuaDetector;
    use crate::{Detector, DetectorContext};

    fn run(source: &str) -> Vec<sting_core::Finding> {
        let tree = sting_ast::parse(source).expect("grammar loads");
        let store = AstStore::build(&tree, source);
        let ctx = DetectorContext::new(PathBuf::from("contract.tolk"), tree, source.to_string());
        SuaDetector.check(&store, &ctx)
    }

    /// Applies `finding.suggestion` to `source` — the same byte-splice
    /// `sting-cli`'s real `--fix` will perform, reproduced here so this
    /// module's own tests can verify the *exact resulting text*, not just
    /// that a `Suggestion` is present.
    fn apply_suggestion(source: &str, finding: &sting_core::Finding) -> String {
        let s = finding
            .suggestion
            .as_ref()
            .expect("this test's finding must carry a suggestion");
        format!(
            "{}{}{}",
            &source[..s.start_byte],
            s.replacement,
            &source[s.end_byte..]
        )
    }

    #[test]
    fn flags_the_plus_typo() {
        let source = "fun f() {\n    var x = 1;\n    x =+ 1;\n}\n";
        let findings = run(source);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].detector_id, "SUA");
        assert!(findings[0].message.contains('+'));
        assert!(findings[0].remediation.contains("+="));
        assert_eq!(
            apply_suggestion(source, &findings[0]),
            "fun f() {\n    var x = 1;\n    x += 1;\n}\n"
        );
    }

    #[test]
    fn flags_the_minus_typo() {
        let source = "fun f() {\n    var x = 1;\n    x =- 1;\n}\n";
        let findings = run(source);
        assert_eq!(findings.len(), 1);
        assert!(findings[0].remediation.contains("-="));
        assert_eq!(
            apply_suggestion(source, &findings[0]),
            "fun f() {\n    var x = 1;\n    x -= 1;\n}\n"
        );
    }

    #[test]
    fn applying_the_suggestion_produces_source_with_no_remaining_findings() {
        // The fix isn't just textually correct (checked above) — applying
        // it must also make the finding go away, not merely relocate it,
        // and the result must still be valid Tolk `sting_ast::parse` can
        // read (a byte-splice with an off-by-one would likely produce
        // something that fails to parse at all).
        for source in [
            "fun f() {\n    var x = 1;\n    x =+ 1;\n}\n",
            "fun f() {\n    var x = 1;\n    x =- 1;\n}\n",
        ] {
            let findings = run(source);
            let fixed = apply_suggestion(source, &findings[0]);
            sting_ast::parse(&fixed).expect("the fixed source must still parse");
            assert!(
                run(&fixed).is_empty(),
                "expected no SUA findings left after applying the suggestion to {source:?}, got fixed source {fixed:?}"
            );
        }
    }

    #[test]
    fn does_not_flag_a_deliberate_unary_plus_with_a_space() {
        let source = "fun f() {\n    var x = 1;\n    x = +1;\n}\n";
        assert!(run(source).is_empty());
    }

    #[test]
    fn does_not_flag_a_deliberate_unary_minus_with_a_space() {
        let source = "fun f() {\n    var x = 1;\n    x = -1;\n}\n";
        assert!(run(source).is_empty());
    }

    #[test]
    fn does_not_flag_a_plain_assignment_with_no_unary_operand() {
        let source = "fun f() {\n    var x = 1;\n    x = 2;\n}\n";
        assert!(run(source).is_empty());
    }

    #[test]
    fn does_not_flag_a_real_compound_assignment() {
        let source = "fun f() {\n    var x = 1;\n    x += 1;\n}\n";
        assert!(run(source).is_empty());
    }

    #[test]
    fn does_not_flag_negation_of_a_boolean() {
        let source = "fun f(flag: bool) {\n    var y = false;\n    y =!flag;\n}\n";
        assert!(run(source).is_empty());
    }

    #[test]
    fn kind_is_ast_and_severity_is_low() {
        assert_eq!(SuaDetector.kind(), crate::DetectorKind::Ast);
        assert_eq!(SuaDetector.default_severity(), sting_core::Severity::Low);
    }
}
