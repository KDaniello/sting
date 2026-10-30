//! Shared dataflow machinery for detectors that need a whole-function,
//! branch-merged sequence of `storeXxx`/`loadXxx` events — extracted from
//! `id.rs` once `tlb.rs` needed the identical event-collection/
//! classification/attribution shape, differing only in what each detector
//! does with the collected events (`ID` compares field *order*; `TLB`
//! compares each matched field's *TL-B shape*). Matches this crate's own
//! established `support.rs`-extraction precedent (see that module's doc
//! comment): moved here once a second real consumer needed the identical
//! shape, not written speculatively ahead of one.
//!
//! [`KeyedEvent`] carries more than `ID` alone ever reads (`method`/`args`
//! alongside the `key` used for its comparison) — deliberately: `ID`'s own
//! comparison key is just `key` (see `id.rs`'s module doc comment for why
//! method name and arguments must **not** factor into its order comparison),
//! but `TLB` needs `method`/`args` to inspect each matched field's declared
//! width/signedness. Carrying both from one shared collection pass avoids
//! two separate tree-walks over the same function body.
//!
//! Same v1 scope as `id.rs` originally documented (still applies to every
//! consumer of this module, not just `ID`):
//! - Only whole-function bodies whose **every exit block** agrees on the
//!   same event sequence are classified at all; anything ambiguous is
//!   skipped, not guessed at.
//! - A function is "store-like" only if *every* dataflow event in it is a
//!   `storeXxx` call (≥ 2 of them); "load-like" only if every event is a
//!   `loadXxx` call whose result is bound to a name (≥ 2).
//! - `method` is now a real (non-ignored) part of dataflow-fact equality,
//!   not just `key` — a change from `id.rs`'s original, `key`-only equality,
//!   made when this was extracted: two branches writing the same field via
//!   two *different* methods (e.g. `storeUint`/`storeInt` on the same key
//!   text) are now correctly treated as a genuine per-branch disagreement
//!   (`Fact::Conflict`, not silently merged into one arbitrary choice) —
//!   strictly more conservative than before (turns a `Some(Classification)`
//!   into `None` at worst, never the reverse), so it cannot introduce a new
//!   false positive in `ID`, only make an already-rare edge case correctly
//!   unclassified instead of silently arbitrary.

use tree_sitter::Node;

use sting_ast::{ContractDef, Language};
use sting_dataflow::{Semilattice, Transfer, WorklistSolver};

use crate::cfg;
use crate::support::{as_method_call, bound_names, identifier_text, nth_call_argument_text};

/// One `storeXxx`/`loadXxx` call, reduced to what a consumer of this module
/// needs: `key` is the field-identity comparison key (the first argument's
/// text for a store — the value being written — or the bound variable's name
/// for a load); `method`/`args` are the raw call shape, for a consumer that
/// needs to inspect the declared width/type instead of (or in addition to)
/// `key`.
#[derive(Clone, Debug)]
pub(crate) struct KeyedEvent<'tree> {
    pub key: String,
    pub method: String,
    pub args: Node<'tree>,
    pub node: Node<'tree>,
}

/// A whole function's classified event sequence — `None` (not this enum) if
/// the function is neither store-like nor load-like, or ambiguous across
/// branches; see this module's doc comment.
#[derive(Debug)]
pub(crate) enum Classification<'tree> {
    Store(Vec<KeyedEvent<'tree>>),
    Load(Vec<KeyedEvent<'tree>>),
}

#[derive(Clone, Debug, PartialEq)]
enum Event<'tree> {
    Store {
        key: String,
        method: String,
        args: EqNode<'tree>,
        node: EqNode<'tree>,
    },
    Load {
        key: String,
        method: String,
        args: EqNode<'tree>,
        node: EqNode<'tree>,
    },
}

/// Wraps a `Node` so `Event`/`Fact` can derive `PartialEq`: `tree_sitter::Node`
/// does not implement it, but a store/load call's *node identity* never
/// factors into whether two dataflow facts are "the same event" — only
/// `key`/`method` do (see this module's doc comment on why `method` was
/// added to real equality and `args`/`node` were not) — the node is carried
/// only to attach a precise `Finding` location later.
#[derive(Clone, Copy, Debug)]
struct EqNode<'tree>(Node<'tree>);
impl PartialEq for EqNode<'_> {
    fn eq(&self, _other: &Self) -> bool {
        true
    }
}

#[derive(Clone, Debug, PartialEq)]
enum Fact<'tree> {
    Undef,
    Seq(Vec<Event<'tree>>),
    Conflict,
}

struct FlatSeqLattice;
impl<'tree> Semilattice<Fact<'tree>> for FlatSeqLattice {
    fn bottom(&self) -> Fact<'tree> {
        Fact::Undef
    }

    fn join(&self, a: &Fact<'tree>, b: &Fact<'tree>) -> Fact<'tree> {
        match (a, b) {
            (Fact::Undef, x) | (x, Fact::Undef) => x.clone(),
            (Fact::Conflict, _) | (_, Fact::Conflict) => Fact::Conflict,
            (Fact::Seq(x), Fact::Seq(y)) => {
                if x == y {
                    Fact::Seq(x.clone())
                } else {
                    Fact::Conflict
                }
            }
        }
    }
}

struct EventTransfer<'a> {
    source: &'a str,
}
impl<'tree> Transfer<Fact<'tree>> for EventTransfer<'_> {
    type Block = Vec<Node<'tree>>;

    fn transfer(&self, state_in: &Fact<'tree>, block: &Self::Block) -> Fact<'tree> {
        let new_events = block_events(block, self.source);
        match state_in {
            Fact::Conflict => Fact::Conflict,
            Fact::Undef if new_events.is_empty() => Fact::Undef,
            Fact::Undef => Fact::Seq(new_events),
            Fact::Seq(prefix) => {
                let mut combined = prefix.clone();
                combined.extend(new_events);
                Fact::Seq(combined)
            }
        }
    }
}

/// Every store/load event contributed by `block`'s statements, in order.
fn block_events<'tree>(block: &[Node<'tree>], source: &str) -> Vec<Event<'tree>> {
    block
        .iter()
        .filter_map(|&stmt| statement_chain(stmt, source))
        .flat_map(|chain| chain_events(chain, source))
        .collect()
}

/// A statement's call chain (`unwind_chain`'d), plus the name it was bound
/// to if the statement is `var/val NAME = <chain>;` or a plain reassignment
/// `NAME = <chain>;` — needed to key a `load` event by the variable a field
/// was read into.
struct StatementChain<'tree> {
    calls: Vec<crate::support::MethodCall<'tree>>,
    bound_name: Option<String>,
}

fn statement_chain<'tree>(stmt: Node<'tree>, source: &str) -> Option<StatementChain<'tree>> {
    if stmt.kind() != "expression_statement" {
        return None;
    }
    let expr = stmt.named_child(0)?;
    match expr.kind() {
        "function_call" => Some(StatementChain {
            calls: unwind_chain(expr, source),
            bound_name: None,
        }),
        "assignment" => {
            let right = expr.child_by_field_name("right")?;
            if right.kind() != "function_call" {
                return None;
            }
            let left = expr.child_by_field_name("left")?;
            let bound_name = if left.kind() == "var_declaration_lhs" {
                let names = bound_names(left, source);
                (names.len() == 1).then(|| names[0].1.clone())
            } else {
                identifier_text(left, source)
            };
            Some(StatementChain {
                calls: unwind_chain(right, source),
                bound_name,
            })
        }
        _ => None,
    }
}

/// Unwinds a right-nested method-call chain (`beginCell().storeUint(..).storeSlice(..)`)
/// into its individual calls, in **execution** order (first call first) —
/// the parse tree nests them the other way around (outermost node = last
/// call in the chain), confirmed via
/// an inspection of the real trees these detectors run over.
fn unwind_chain<'tree>(node: Node<'tree>, source: &str) -> Vec<crate::support::MethodCall<'tree>> {
    let mut calls = Vec::new();
    let mut current = node;
    while let Some(call) = as_method_call(current, source, Language::Tolk) {
        current = call.obj;
        calls.push(call);
    }
    calls.reverse();
    calls
}

fn chain_events<'tree>(chain: StatementChain<'tree>, source: &str) -> Vec<Event<'tree>> {
    let all_store =
        !chain.calls.is_empty() && chain.calls.iter().all(|c| c.method.starts_with("store"));
    if all_store {
        return chain
            .calls
            .iter()
            .map(|c| Event::Store {
                key: nth_call_argument_text(c.args, 0, source).unwrap_or_default(),
                method: c.method.clone(),
                args: EqNode(c.args),
                node: EqNode(c.method_node),
            })
            .collect();
    }
    if let [call] = chain.calls.as_slice()
        && call.method.starts_with("load")
        && let Some(bound) = chain.bound_name
    {
        return vec![Event::Load {
            key: bound,
            method: call.method.clone(),
            args: EqNode(call.args),
            node: EqNode(call.method_node),
        }];
    }
    Vec::new()
}

/// Runs the dataflow solve over `body`'s CFG and returns the function's
/// overall event sequence — only if every exit block agrees on it (`None`
/// otherwise: ambiguous across paths, out of v1's scope, not guessed at).
/// Also `None` if [`cfg::build`] itself failed (structurally unreachable
/// through the current builder, see that function's own doc comment) —
/// treated the same as "this function's body doesn't support dataflow
/// analysis" rather than propagated as a hard error.
fn whole_function_sequence<'tree>(body: Node<'tree>, source: &str) -> Option<Vec<Event<'tree>>> {
    let built = cfg::build(body).ok()?;
    let facts: std::collections::HashMap<usize, Fact<'tree>> =
        WorklistSolver::solve(&FlatSeqLattice, &EventTransfer { source }, &built);
    let exits: Vec<usize> = built.exit_blocks().collect();
    let first_key = *exits.first()?;
    let first = as_sequence(facts.get(&first_key)?)?;
    let agrees = exits
        .iter()
        .all(|id| as_sequence(facts.get(id).unwrap_or(&Fact::Undef)) == Some(first));
    agrees.then(|| first.to_vec())
}

fn as_sequence<'a, 'tree>(fact: &'a Fact<'tree>) -> Option<&'a [Event<'tree>]> {
    match fact {
        Fact::Undef => Some(&[]),
        Fact::Seq(v) => Some(v.as_slice()),
        Fact::Conflict => None,
    }
}

fn classify<'tree>(events: &[Event<'tree>]) -> Option<Classification<'tree>> {
    if events.len() < 2 {
        return None;
    }
    if events.iter().all(|e| matches!(e, Event::Store { .. })) {
        return Some(Classification::Store(
            events
                .iter()
                .map(|e| match e {
                    Event::Store {
                        key,
                        method,
                        args,
                        node,
                    } => KeyedEvent {
                        key: key.clone(),
                        method: method.clone(),
                        args: args.0,
                        node: node.0,
                    },
                    Event::Load { .. } => unreachable!("filtered to Store above"),
                })
                .collect(),
        ));
    }
    if events.iter().all(|e| matches!(e, Event::Load { .. })) {
        return Some(Classification::Load(
            events
                .iter()
                .map(|e| match e {
                    Event::Load {
                        key,
                        method,
                        args,
                        node,
                    } => KeyedEvent {
                        key: key.clone(),
                        method: method.clone(),
                        args: args.0,
                        node: node.0,
                    },
                    Event::Store { .. } => unreachable!("filtered to Load above"),
                })
                .collect(),
        ));
    }
    None
}

/// Classifies one function body as store-like, load-like, or neither/
/// ambiguous — the single entry point this module exposes.
pub(crate) fn classify_function<'tree>(
    body: Node<'tree>,
    source: &str,
) -> Option<Classification<'tree>> {
    classify(&whole_function_sequence(body, source)?)
}

/// The same-contract test both `ID` and `TLB` need to avoid pairing a
/// store-like function in one contract with a load-like function in another
/// (`id.rs`'s module doc comment explains the original bug this guards
/// against — found independently in `GVR`/`ID`/`UBM` during a
/// post-Block-09 corpus review). Callers still get the two owners
/// themselves via `support::contracts_in_source_order`/
/// `support::nearest_contract_for` directly — only this small comparison
/// (attributed-to-the-same-contract vs. not) is shared here.
pub(crate) fn same_contract(a: Option<&ContractDef>, b: Option<&ContractDef>) -> bool {
    match (a, b) {
        (Some(a), Some(b)) => std::ptr::eq(a, b),
        (None, None) => true,
        _ => false,
    }
}

/// Isolated tests for [`classify_function`] itself, independent of `id.rs`/
/// `tlb.rs` — this module previously had zero tests of its own, exercised
/// only transitively through those two detectors .
#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "docs/ARCHITECTURE.md §3.2 scopes the no-unwrap/expect/panic policy to library code outside tests"
)]
mod tests {
    use sting_ast::AstStore;
    use tree_sitter::{Node, Tree};

    use super::{Classification, classify_function};

    fn body_for<'tree>(tree: &'tree Tree, source: &str, name: &str) -> Node<'tree> {
        let store = AstStore::build(tree, source);
        let function = store.function_by_name(name).expect("function is declared");
        function
            .range
            .locate(tree)
            .expect("range resolves")
            .child_by_field_name("body")
            .expect("function has a body")
    }

    #[test]
    fn classifies_a_pure_store_function_as_store() {
        let source = "fun save(balance: int, owner: int) {\n    var b = beginCell().storeUint(balance, 64).storeUint(owner, 256);\n    contract.setData(b.endCell());\n}\n";
        let tree = sting_ast::parse(source).expect("grammar loads");
        let body = body_for(&tree, source, "save");

        match classify_function(body, source) {
            Some(Classification::Store(events)) => {
                assert_eq!(events.len(), 2);
                assert_eq!(events[0].key, "balance");
                assert_eq!(events[0].method, "storeUint");
                assert_eq!(events[1].key, "owner");
                assert_eq!(events[1].method, "storeUint");
            }
            other => panic!("expected a Store classification with 2 events, got {other:?}"),
        }
    }

    #[test]
    fn classifies_a_pure_load_function_as_load() {
        let source = "fun load() {\n    var s = contract.getData().beginParse();\n    val balance = s.loadUint(64);\n    val owner = s.loadUint(256);\n}\n";
        let tree = sting_ast::parse(source).expect("grammar loads");
        let body = body_for(&tree, source, "load");

        match classify_function(body, source) {
            Some(Classification::Load(events)) => {
                assert_eq!(events.len(), 2);
                assert_eq!(events[0].key, "balance");
                assert_eq!(events[0].method, "loadUint");
                assert_eq!(events[1].key, "owner");
                assert_eq!(events[1].method, "loadUint");
            }
            other => panic!("expected a Load classification with 2 events, got {other:?}"),
        }
    }

    /// Two branches disagreeing on the *method* used for the same key
    /// (`storeInt` vs `storeUint` on the same `balance`) must merge to
    /// `Fact::Conflict` at the join point — this module's own doc comment
    /// explains why `method` is real equality, not just `key` — and
    /// `classify_function` must surface that as `None`, not guess which
    /// branch is "the real one".
    #[test]
    fn returns_none_when_branches_disagree_on_the_store_method_for_the_same_key() {
        let source = "fun save(useSigned: bool, balance: int) {\n    var b = beginCell();\n    if (useSigned) {\n        b = b.storeInt(balance, 64);\n    } else {\n        b = b.storeUint(balance, 64);\n    }\n    contract.setData(b.endCell());\n}\n";
        let tree = sting_ast::parse(source).expect("grammar loads");
        let body = body_for(&tree, source, "save");

        assert!(
            classify_function(body, source).is_none(),
            "conflicting store methods across branches for the same key must not classify"
        );
    }

    #[test]
    fn returns_none_for_fewer_than_two_events() {
        // A single storeXxx call is not enough to call this a "manual save
        // chain" — `classify`'s own `events.len() < 2` guard.
        let source = "fun save(balance: int) {\n    var b = beginCell().storeUint(balance, 64);\n    contract.setData(b.endCell());\n}\n";
        let tree = sting_ast::parse(source).expect("grammar loads");
        let body = body_for(&tree, source, "save");

        assert!(classify_function(body, source).is_none());
    }
}
