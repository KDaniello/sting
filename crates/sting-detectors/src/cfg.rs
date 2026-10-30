//! Builds a minimal intra-procedural CFG (`sting_dataflow::Cfg<Vec<Node>>`)
//! from a Tolk function body, for the two `DetectorKind::Dataflow`
//! detectors (`id`, `ubm`) that need "does X hold on some/every path"
//! reasoning across `if`/`match` branches (`docs/domains/detectors.md`).
//!
//! `LEP` deliberately does **not** use this: the doc classifies it as
//! `DetectorKind::Ast` "with elements of a local CFG" specifically because a
//! full block-graph + `WorklistSolver` fixpoint is more machinery than its
//! single boolean "was `assertEnd` reached" question needs — direct
//! recursion over the same `if`/`match` nesting (see `lep.rs`) is a complete
//! substitute given Tolk has no `goto`/labeled jumps, and is simpler to
//! read.
//!
//! Scope, honestly bounded rather than silently wrong:
//! - Only `if`/`match` branching becomes real edges. `while`/`repeat`/
//!   `do_while`/`try` bodies are **not** recursed into — their statements
//!   are treated as one opaque straight-line unit wherever they sit (no loop
//!   back-edge, no branch into the loop body). Real message handlers (the
//!   corpus these detectors target) do not loop;
//!   `WalletV5.tolk`'s `processExtraActions` is the one real counterexample
//!   in the current corpus, and is intentionally out of `ID`/`UBM`'s v1
//!   scope.
//! - `if`/`match` condition and scrutinee **expressions** are not scanned
//!   for events — only the statements inside branch bodies are, so a
//!   condition is never double-counted against its own branch bodies.
//! - If every branch of an `if`/`match` is terminal (`return`/`throw`), the
//!   synthetic join block created after it has no real predecessor and is
//!   technically unreachable dead code, but the builder still lowers any
//!   statements that textually follow into it (see
//!   `join_after_all_terminal_branches_is_unreachable_but_still_built`
//!   below). No real fixture and no golden fixture structures code this way; a general reachability pass would be needed
//!   to close this gap and is not worth it at this corpus size.

use sting_dataflow::{Cfg, CfgError};
use tree_sitter::Node;

use crate::support::{block_children, match_arm_body};

/// Builds a CFG for `body` (a `block_statement` node): one block per
/// straight-line run of statements, with `if`/`match` branches becoming real
/// edges that converge at a synthetic join block.
///
/// # Errors
/// Returns [`CfgError`] if [`Builder`]'s own bookkeeping ever produced a
/// malformed graph — `Builder` maintains `Cfg::new`'s invariants by
/// construction today (every block/successor-list pair is pushed together
/// in [`Builder::new_block`], every successor id comes from that same
/// method), so this is not reachable through the current builder, only
/// through a future bug in it; every caller here treats it the same way as
/// "this function's body doesn't support dataflow analysis," not a reason
/// to crash the whole lint run .
pub(crate) fn build(body: Node) -> Result<Cfg<Vec<Node>>, CfgError> {
    let mut builder = Builder {
        blocks: vec![Vec::new()],
        successors: vec![Vec::new()],
    };
    builder.lower(&block_children(body), 0);
    Cfg::new(0, builder.blocks, builder.successors)
}

struct Builder<'tree> {
    blocks: Vec<Vec<Node<'tree>>>,
    successors: Vec<Vec<usize>>,
}

impl<'tree> Builder<'tree> {
    fn new_block(&mut self) -> usize {
        self.blocks.push(Vec::new());
        self.successors.push(Vec::new());
        self.blocks.len() - 1
    }

    fn link(&mut self, from: usize, to: usize) {
        self.successors[from].push(to);
    }

    /// Creates a fresh block, links `from` to it, and lowers `body`'s
    /// statements into it (`body` is a branch's `block_statement`).
    fn lower_branch(&mut self, body: Node<'tree>, from: usize) -> Vec<usize> {
        let entry = self.new_block();
        self.link(from, entry);
        if body.kind() == "block_statement" {
            self.lower(&block_children(body), entry)
        } else {
            // Defensive only: `if_statement`'s `body`/`alternative` block
            // and `match_arm`'s `block` alternative are always
            // `block_statement` per grammar.js — this never actually
            // triggers against the real grammar, kept so a future grammar
            // change surfaces as a wrong-shaped CFG (caught by this crate's
            // tests) rather than a panic.
            self.blocks[entry].push(body);
            vec![entry]
        }
    }

    /// Lowers `stmts` starting in block `current`. Returns the blocks left
    /// "open" at the end — reached without a `return`/`throw` — for the
    /// caller to link to whatever comes next (or, at the top level, to
    /// leave as real CFG exits).
    fn lower(&mut self, stmts: &[Node<'tree>], current: usize) -> Vec<usize> {
        let mut current = current;
        for &stmt in stmts {
            match stmt.kind() {
                "if_statement" => {
                    let cond_block = current;
                    let then_open = stmt
                        .child_by_field_name("body")
                        .map_or_else(|| vec![cond_block], |b| self.lower_branch(b, cond_block));
                    let else_open = match stmt.child_by_field_name("alternative") {
                        Some(alt) if alt.kind() == "if_statement" => {
                            let else_entry = self.new_block();
                            self.link(cond_block, else_entry);
                            self.lower(std::slice::from_ref(&alt), else_entry)
                        }
                        Some(alt) => self.lower_branch(alt, cond_block),
                        None => vec![cond_block],
                    };
                    let join = self.new_block();
                    for open in then_open.into_iter().chain(else_open) {
                        self.link(open, join);
                    }
                    current = join;
                }
                "match_statement" => {
                    current = self.lower_match(stmt, current);
                }
                "return_statement" | "throw_statement" => {
                    self.blocks[current].push(stmt);
                    return vec![];
                }
                _ => {
                    self.blocks[current].push(stmt);
                }
            }
        }
        vec![current]
    }

    fn lower_match(&mut self, match_stmt: Node<'tree>, current: usize) -> usize {
        let match_block = current;
        let arms = match_stmt
            .named_child(0)
            .and_then(|match_expr| match_expr.child_by_field_name("body"))
            .map(|match_body| {
                let mut cursor = match_body.walk();
                match_body
                    .named_children(&mut cursor)
                    .filter(|c| c.kind() == "match_arm")
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();

        let mut open = Vec::new();
        let mut has_else_arm = false;
        for arm in arms {
            has_else_arm |= arm.child_by_field_name("pattern_else").is_some();
            let arm_entry = self.new_block();
            self.link(match_block, arm_entry);
            match match_arm_body(arm) {
                Some(b) if b.kind() == "block_statement" => {
                    // Lower directly into `arm_entry` (already a fresh block
                    // linked from `match_block`) rather than via
                    // `lower_branch`, which would add a redundant
                    // pass-through hop.
                    open.extend(self.lower(&block_children(b), arm_entry));
                }
                Some(b) if b.kind() == "return_statement" || b.kind() == "throw_statement" => {
                    self.blocks[arm_entry].push(b);
                }
                Some(b) => {
                    self.blocks[arm_entry].push(b);
                    open.push(arm_entry);
                }
                None => open.push(arm_entry),
            }
        }
        if !has_else_arm {
            // Conservative: a `match` without an `else` arm might still not
            // match anything at runtime for a non-exhaustive scrutinee type
            // (`docs/domains/detectors.md` gives no guarantee Tolk's
            // compiler rejects that) — model a direct fallthrough edge so a
            // "must hold on every path" analysis (LEP-style; not used by
            // this builder itself, but the shape must stay sound for future
            // reuse) does not wrongly assume every path enters some arm.
            open.push(match_block);
        }
        let join = self.new_block();
        for o in open {
            self.link(o, join);
        }
        join
    }
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "docs/ARCHITECTURE.md §3.2 scopes the no-unwrap/expect policy to library code outside tests"
)]
mod tests {
    use sting_dataflow::Cfg;
    use tree_sitter::Node;

    use super::build;

    fn cfg_for(source: &str) -> Cfg<Vec<Node<'_>>> {
        // Leaked so the returned Cfg (borrowing Nodes from the tree) can
        // outlive this helper without threading a `Tree` local through every
        // test — acceptable in test-only code, never in library code
        // (`docs/ARCHITECTURE.md` §3.2's unwrap/expect carve-out is the same
        // kind of test-only leniency).
        let tree: &'static tree_sitter::Tree =
            Box::leak(Box::new(sting_ast::parse(source).expect("grammar loads")));
        let store = sting_ast::AstStore::build(tree, source);
        let function = store.function_by_name("f").expect("f is declared");
        let body = function
            .range
            .locate(tree)
            .expect("range resolves")
            .child_by_field_name("body")
            .expect("f has a body");
        build(body).expect("well-formed CFG from a real parsed function body")
    }

    #[test]
    fn straight_line_body_is_a_single_block() {
        let cfg = cfg_for("fun f() {\n    val a = 1;\n    val b = 2;\n}");
        assert_eq!(cfg.exit_blocks().collect::<Vec<_>>(), vec![0]);
    }

    #[test]
    fn if_with_one_terminal_branch_and_no_else_has_two_reachable_exits() {
        // then returns (its own exit); no else, so the implicit "condition
        // false" path falls through directly to `val a = 1;` at the join
        // (a second, separate exit) — two genuinely different reachable
        // exit points, not one.
        let cfg = cfg_for(
            "fun f(cond: bool) {\n    if (cond) {\n        return;\n    }\n    val a = 1;\n}",
        );
        // blocks: 0=cond, 1=then(return, terminal), 2=join(has `val a = 1`)
        let exits: Vec<_> = cfg.exit_blocks().collect();
        assert_eq!(
            exits.len(),
            2,
            "the early return and the join are both reachable exits"
        );
    }

    #[test]
    fn if_else_both_falling_through_converge_on_one_join() {
        let cfg = cfg_for(
            "fun f(cond: bool) {\n    if (cond) {\n        val a = 1;\n    } else {\n        val b = 2;\n    }\n    val c = 3;\n}",
        );
        assert_eq!(
            cfg.exit_blocks().collect::<Vec<_>>().len(),
            1,
            "both branches fall through to the same join, which is the only exit"
        );
    }

    #[test]
    fn join_after_all_terminal_branches_is_unreachable_but_still_built() {
        // Documented limitation (see module doc comment): both branches
        // `return`, so the join created after the `if` has no real
        // predecessor. This test locks in the current (imperfect) behavior
        // so a future fix is a deliberate change, not a silent regression.
        let cfg = cfg_for(
            "fun f(cond: bool) {\n    if (cond) {\n        return;\n    } else {\n        return;\n    }\n}",
        );
        let exits: Vec<_> = cfg.exit_blocks().collect();
        assert_eq!(
            exits.len(),
            3,
            "the two real (terminal) exits plus the unreachable join, per the documented limitation"
        );
    }

    #[test]
    fn match_without_else_arm_gets_a_conservative_fallthrough_edge() {
        let cfg = cfg_for(
            "fun f(x: int) {\n    match (x) {\n        1 => { return; }\n    }\n    val a = 1;\n}",
        );
        // Two genuinely separate reachable exits: the arm's own `return`,
        // and the join — now reachable too, via the conservative direct
        // "no arm matched" edge from match_block straight to it, then
        // falling through `val a = 1;` to an implicit return. Without the
        // conservative edge the join would be unreachable (same as the
        // if/match-all-terminal-branches cases above); with it, it is a
        // real second path.
        let exits: Vec<_> = cfg.exit_blocks().collect();
        assert_eq!(exits.len(), 2);
    }

    #[test]
    fn match_with_else_arm_and_all_arms_terminal_has_no_conservative_edge() {
        let cfg = cfg_for(
            "fun f(x: int) {\n    match (x) {\n        1 => { return; }\n        else => { return; }\n    }\n}",
        );
        // No fallthrough edge is added (an else arm is present), so the
        // join is unreachable (same documented limitation as the if/else
        // all-terminal case) alongside the two real terminal exits.
        let exits: Vec<_> = cfg.exit_blocks().collect();
        assert_eq!(exits.len(), 3);
    }
}
