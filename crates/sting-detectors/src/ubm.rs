//! `UBM` — Unchecked Bounced Message (`docs/domains/detectors.md`).
//!
//! A function commits a storage mutation (`.save()`) and then sends a
//! message that can bounce, but the file has no `onBouncedMessage` handler
//! (or one that does not itself touch storage) to undo the mutation if the
//! send bounces — `CertiK`'s Jetton-wallet fund-loss pattern: balance already
//! debited, downstream transfer fails and bounces, debit never reverted.
//!
//! `DetectorKind::Dataflow`: "has `.save()` already happened by the time we
//! reach this `.send()`" is a **may-reach-on-some-path** question (existential:
//! does *some* execution reach a risky send having already saved), the
//! logical dual of `LEP`'s **must-hold-on-every-path** question — modeled
//! with the same `sting_dataflow::{Cfg, WorklistSolver}` machinery `id.rs`
//! uses, but a boolean OR-lattice instead of `id`'s ordered-sequence flat
//! lattice: `bottom = false`, `join = ||`. A security detector should err
//! toward more findings a human dismisses over silently missing a real one,
//! so "may" (OR) rather than "must" (AND) is the deliberately conservative
//! choice for *this* direction — contrast `LEP`, where the conservative
//! choice for *its* question runs the other way (AND across branches, since
//! there recall means requiring assertEnd on every path, not just some).
//!
//! The scope here is narrower than the interprocedural one the document
//! aspires to: save-then-send is checked **within one function** (matches both real
//! CertiK-documented incidents and this corpus's own `JettonMinter`/
//! `JettonWallet` shape: the mutation and the risky send are always in the
//! same `match` arm of the same `onInternalMessage`). The bounce-handler
//! lookup itself *is* file-level (`onBouncedMessage` is its own top-level
//! function, found via `AstStore`, not nested in the sender) — that much
//! cross-function reasoning the doc's pattern requires unconditionally.
//! Bounce-eligibility is read from the nearest preceding `createMessage({
//! bounce: ... })` call's `bounce` field in program order, not resolved
//! through the exact variable identity sent to `.send()` — a documented,
//! deliberate simplification (see `create_message_argument`).
//!
//! The `onBouncedMessage` lookup is scoped to the risky-send function's own
//! contract (nearest-preceding `contract{}` header, `support::nearest_contract_for`
//! — the same fix as `GvrDetector`/`IdDetector`), not a single
//! `AstStore::function_by_name` lookup across the whole analyzed unit. The
//! original implementation found *a* handler named `onBouncedMessage`
//! anywhere in the file and reused it for every risky-send function
//! regardless of which contract each belonged to: in a multi-contract
//! analysis unit (`sting-bench`'s multi-file merge) where contract A has no
//! bounce handler but contract B does, A's genuinely unguarded risky send
//! was wrongly suppressed by B's unrelated handler — a false *negative* in a
//! security detector, the more dangerous direction of error this crate's
//! detectors are supposed to err away from (see this module's `join = ||`
//! rationale above). A risky-send function with no preceding contract header
//! at all is matched only against another equally unattributed
//! `onBouncedMessage`, preserving this detector's original whole-file
//! behavior for the common single-contract-file case.
//!
//! `last_message_is_safe` — "is the nearest preceding `createMessage({
//! bounce: ... })` non-bounceable" — is carried across CFG block boundaries
//! via its own dataflow pass (`MessageSafetyLattice`/`MessageSafetyTransfer`),
//! not a plain local re-initialized per block. Found missing during a
//! post-Block-09 corpus review: a `createMessage` in one block followed by
//! `.send()` in a later block (split by any intervening `if`/`match`) used
//! to lose the safety flag and wrongly flag a genuinely safe send — see
//! `MessageSafety`'s own doc comment for why this needs a four-value lattice
//! (`Unknown`/`Safe`/`Unsafe`/`Conflict`), not a second boolean.
//!
//! **Recall gap fixed this round, the same root cause `ac.rs` independently
//! found and fixed** (
//! `hacktheton.com`): `.save()` is only the conventional, struct-based
//! spelling of "persist storage" — the real primitive underneath it is
//! `contract.setData(cell)`, confirmed used directly (not just inside a
//! `.save()`-named wrapper) in several real files in this project's own
//! corpus (`config/config-helpers.tolk`, `jetton-v1.3/NotcoinMinter.tolk`,
//! `jetton-v2.1/JettonMinter.tolk`, among others). [`is_save_call`] now
//! recognizes both `.save()` and `.setData()` as method calls, plus the
//! pre-1.0 Tolk free-function name `setContractData(...)` (no `contract.`
//! receiver — confirmed still the literal spelling used by
//! `hacktheton.com`'s actual, currently-live level source). Deliberately
//! **not** extended to `contract.setCodePostponed(...)` (code upgrade,
//! `ac.rs`'s separate `CODE_MUTATION_METHOD`): per Tolk's own docs a
//! postponed code change takes effect at the end of the *current*
//! transaction's action phase, before any bounce (a separate, later
//! transaction) could even arrive — there is no "revert this via
//! `onBouncedMessage`" story for a code upgrade the way there is for a data
//! save, so folding it into this detector's save-then-send question would
//! not be sound, not just untested.

use sting_ast::{AstStore, ContractDef, FunctionDef, Language};
use sting_core::{DetectorId, Finding, Severity};
use sting_dataflow::{Semilattice, Transfer, WorklistSolver};
use tree_sitter::Node;

use crate::cfg;
use crate::support::{
    as_method_call, contracts_in_source_order, create_message_argument, function_body,
    identifier_text, location_of, nearest_contract_for,
};
use crate::{Detector, DetectorContext, DetectorKind};

pub(crate) struct UbmDetector;

impl Detector for UbmDetector {
    fn id(&self) -> DetectorId {
        "UBM"
    }

    fn kind(&self) -> DetectorKind {
        DetectorKind::Dataflow
    }

    fn default_severity(&self) -> Severity {
        Severity::Critical
    }

    fn check(&self, store: &AstStore, ctx: &DetectorContext) -> Vec<Finding> {
        let source = ctx.source();
        let contracts_in_order = contracts_in_source_order(store);

        let mut findings = Vec::new();
        for function in store.functions() {
            if function.name == "onBouncedMessage" {
                continue;
            }
            let Some(body) = function_body(function, ctx) else {
                continue;
            };
            let Some(risky_send) = first_risky_send_after_save(body, source) else {
                continue;
            };

            let owner = nearest_contract_for(function.range.start_byte, &contracts_in_order);
            let handler = bounce_handler_for(store, owner, &contracts_in_order);
            let handler_restores_state = handler
                .and_then(|f| function_body(f, ctx))
                .is_some_and(|body| body_contains_save(body, source));

            let (message, remediation) = if handler.is_none() {
                let scope = owner.map_or_else(
                    || "this file".to_string(),
                    |c| format!("contract `{}`", c.name),
                );
                (
                    format!(
                        "function `{}` saves storage and then sends a message that can bounce, but {scope} defines no `onBouncedMessage` handler — a bounce would leave the saved state unreverted",
                        function.name
                    ),
                    "add a `fun onBouncedMessage(in: InMessageBounced)` in this contract that reverts (or otherwise reconciles) the same storage change this function made, so a bounce doesn't leave state permanently out of sync with what was actually sent".to_string(),
                )
            } else if handler_restores_state {
                continue;
            } else {
                (
                    format!(
                        "function `{}` saves storage and then sends a message that can bounce; `onBouncedMessage` exists but does not appear to touch storage itself, so a bounce would leave the saved state unreverted",
                        function.name
                    ),
                    "have `onBouncedMessage` load storage and revert (or otherwise reconcile) the change this function made before the bounceable send, not just acknowledge the bounce".to_string(),
                )
            };
            findings.push(Finding {
                detector_id: self.id(),
                severity: self.default_severity(),
                message,
                remediation,
                location: location_of(risky_send, ctx),
                confidence: None,
                dynamic_verification: None,
                suggestion: None,
            });
        }
        findings
    }
}

/// The `onBouncedMessage` handler attributed to the same contract as `owner`
/// (`None` meaning "no contract header precedes this function at all" is
/// matched only against another equally-unattributed handler — see this
/// module's doc comment). A file can legitimately define more than one
/// `onBouncedMessage`, one per contract, in a multi-contract analysis unit;
/// `AstStore::function_by_name`'s first-match-wins would silently borrow an
/// unrelated contract's handler (or its absence).
fn bounce_handler_for<'a>(
    store: &'a AstStore,
    owner: Option<&ContractDef>,
    contracts_in_order: &[&ContractDef],
) -> Option<&'a FunctionDef> {
    store
        .functions()
        .filter(|f| f.name == "onBouncedMessage")
        .find(|f| {
            let handler_owner = nearest_contract_for(f.range.start_byte, contracts_in_order);
            match (handler_owner, owner) {
                (Some(a), Some(b)) => std::ptr::eq(a, b),
                (None, None) => true,
                _ => false,
            }
        })
}

/// Boolean "may already have called `.save()`" lattice: existential across
/// branches (`join = ||`), matching this detector's "does *some* path reach
/// a risky send having already saved" question — see this module's doc
/// comment for why that is the appropriately conservative direction here.
struct MaySavedLattice;
impl Semilattice<bool> for MaySavedLattice {
    fn bottom(&self) -> bool {
        false
    }
    fn join(&self, a: &bool, b: &bool) -> bool {
        *a || *b
    }
}

struct SavedTransfer<'tree> {
    source: &'tree str,
}
impl<'tree> Transfer<bool> for SavedTransfer<'tree> {
    type Block = Vec<Node<'tree>>;
    fn transfer(&self, state_in: &bool, block: &Self::Block) -> bool {
        *state_in || any_statement_saves(block, self.source)
    }
}

fn any_statement_saves(stmts: &[Node], source: &str) -> bool {
    stmts.iter().any(|&stmt| {
        sting_ast::Preorder::from_node(stmt)
            .map(|v| v.node)
            .any(|n| is_save_call(n, source))
    })
}

/// Whether a save-equivalent call (see this module's doc comment's "Recall
/// gap" section) is called anywhere in `body`, ignoring control flow — used
/// only for the file-level "does the bounce handler touch storage at all"
/// check, which does not need path sensitivity: any branch saving is enough
/// to call the handler "restores state".
fn body_contains_save(body: Node, source: &str) -> bool {
    sting_ast::Preorder::from_node(body)
        .map(|v| v.node)
        .any(|n| is_save_call(n, source))
}

/// Whether `node` is a call that persists storage: the conventional
/// `.save()`/`.setData()` method call, or the pre-1.0 Tolk free-function
/// name `setContractData(...)` — see this module's doc comment's "Recall
/// gap" section.
fn is_save_call(node: Node, source: &str) -> bool {
    if let Some(call) = as_method_call(node, source, Language::Tolk) {
        return matches!(call.method.as_str(), "save" | "setData");
    }
    if node.kind() != "function_call" {
        return false;
    }
    let Some(callee) = node.child_by_field_name("callee") else {
        return false;
    };
    identifier_text(callee, source).as_deref() == Some("setContractData")
}

/// Runs both the may-saved and message-safety dataflow solves over `body`'s
/// CFG, then re-scans each block with its derived `IN` facts to find the
/// first `.send(...)` call that (a) is reachable having already saved, and
/// (b) is not provably non-bounceable via the nearest preceding
/// `createMessage({ bounce: ... })` literal *on that same path* — not just
/// "somewhere earlier in this one block", see [`MessageSafetyLattice`]'s doc
/// comment for why that needs its own dataflow pass, not a plain local.
/// Returns that call's method-name node for the `Finding` location.
fn first_risky_send_after_save<'tree>(
    body: Node<'tree>,
    source: &'tree str,
) -> Option<Node<'tree>> {
    let built = cfg::build(body).ok()?;
    let saved_out = WorklistSolver::solve(&MaySavedLattice, &SavedTransfer { source }, &built);
    let safety_out = WorklistSolver::solve(
        &MessageSafetyLattice,
        &MessageSafetyTransfer { source },
        &built,
    );

    for block_id in 0..built.block_count() {
        let saved_in = built
            .predecessors(block_id)
            .iter()
            .any(|p| *saved_out.get(p).unwrap_or(&false));
        let safety_in =
            built
                .predecessors(block_id)
                .iter()
                .fold(MessageSafety::Unknown, |acc, p| {
                    MessageSafetyLattice
                        .join(&acc, safety_out.get(p).unwrap_or(&MessageSafety::Unknown))
                });
        if let Some(node) = risky_send_in_block(built.block(block_id), saved_in, safety_in, source)
        {
            return Some(node);
        }
    }
    None
}

/// Scans one block's statements in order, tracking `saved`/the most
/// recently seen message's bounce-safety, starting from `saved_in`/`safety_in`
/// (both derived from the block's CFG predecessors — see
/// [`first_risky_send_after_save`]).
fn risky_send_in_block<'tree>(
    block: &[Node<'tree>],
    saved_in: bool,
    safety_in: MessageSafety,
    source: &str,
) -> Option<Node<'tree>> {
    let mut saved = saved_in;
    let mut last_message_is_safe = safety_in;
    for &stmt in block {
        for visited in sting_ast::Preorder::from_node(stmt) {
            let node = visited.node;
            if let Some(literal) = create_message_argument(node, source) {
                last_message_is_safe = if bounce_mode_is_no_bounce(literal, source) {
                    MessageSafety::Safe
                } else {
                    MessageSafety::Unsafe
                };
            } else if is_external_log_message_call(node, source) {
                last_message_is_safe = MessageSafety::Safe;
            }
            if is_save_call(node, source) {
                saved = true;
            } else if let Some(call) = as_method_call(node, source, Language::Tolk)
                && call.method == "send"
                && saved
                && chained_safety(call.obj, source).unwrap_or(last_message_is_safe)
                    != MessageSafety::Safe
            {
                return Some(call.method_node);
            }
        }
    }
    None
}

/// Whether a message built **directly in the receiver** of `.send()` is safe:
/// `createExternalLogMessage({ ... }).send(...)` or
/// `createMessage({ bounce: ... }).send(...)`.
///
/// Needed because of the traversal order. [`risky_send_in_block`] walks in
/// preorder, so the outer `function_call` — the `.send` itself — is reached
/// **before** the nested `createMessage` or `createExternalLogMessage`. While
/// the message goes into a variable in its own statement the order is reversed
/// and everything works; written as a chain, `.send()` is judged by the "last
/// message seen", which on this path has not happened yet.
///
/// Found on `xssnick/payment-channel-contract`: a **false Critical** on a live
/// payment channel that sends `createExternalLogMessage(...).send(...)` after
/// saving. The earlier fix to the recognizer (`is_external_log_message_call`)
/// closed the through-a-variable form and did not fire on this one — exactly
/// the class of incompleteness `UR` had shown a round earlier. Confirmed by a
/// minimal reproduction: the same message gives 0 findings through a variable
/// and 1 as a chain.
///
/// `None` means no message was built in the receiver, and the decision then
/// falls to the accumulated path state as before.
fn chained_safety(receiver: Node, source: &str) -> Option<MessageSafety> {
    for visited in sting_ast::Preorder::from_node(receiver) {
        let node = visited.node;
        if is_external_log_message_call(node, source) {
            return Some(MessageSafety::Safe);
        }
        if let Some(literal) = create_message_argument(node, source) {
            return Some(if bounce_mode_is_no_bounce(literal, source) {
                MessageSafety::Safe
            } else {
                MessageSafety::Unsafe
            });
        }
    }
    None
}

/// Tri-state "is the nearest preceding `createMessage({ bounce: ... })` on
/// *this path* non-bounceable" lattice — the counterpart to
/// [`MaySavedLattice`] for message safety instead of storage mutation, used
/// to carry `last_message_is_safe` across CFG block boundaries.
///
/// Found missing during a post-Block-09 corpus review: `risky_send_in_block`
/// used to re-initialize `last_message_is_safe` to `false` at the top of
/// *every* block, never threading it through `WorklistSolver` the way
/// `saved` already was. A `createMessage({ bounce: BounceMode.NoBounce })`
/// in one block followed by `.send()` in a later block — split by any
/// intervening `if`/`match`, even one whose branches are unrelated to the
/// message at all — lost the safety flag and the send was wrongly flagged,
/// a false positive not covered by any of this module's 11 original tests
/// (all of them keep `createMessage` and `.send()` in the same block).
///
/// This genuinely is not a boolean OR/AND question like `MaySavedLattice`'s
/// — it is "what was the last concrete value on this specific path", which
/// is why `Unknown` (bottom, no message seen yet on this path) sits below
/// `Safe`/`Unsafe` (both concrete, mutually incomparable) below `Conflict`
/// (top: two predecessor paths disagree on the last message's safety, and
/// picking either side would be a guess). Same `Undef`/concrete/`Conflict`
/// lattice shape as `id.rs`'s `Fact`, for the same underlying reason: a
/// "last value seen" fact cannot be soundly merged with plain union/OR.
/// `Conflict` is treated as `Unsafe` by [`risky_send_in_block`] (`!= Safe`),
/// matching this whole module's "err toward more findings" stance (see the
/// module doc comment's `join = ||` rationale) — an ambiguous last message
/// is not proof of safety.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum MessageSafety {
    Unknown,
    Safe,
    Unsafe,
    Conflict,
}

struct MessageSafetyLattice;
impl Semilattice<MessageSafety> for MessageSafetyLattice {
    fn bottom(&self) -> MessageSafety {
        MessageSafety::Unknown
    }
    fn join(&self, a: &MessageSafety, b: &MessageSafety) -> MessageSafety {
        match (a, b) {
            (MessageSafety::Unknown, x) | (x, MessageSafety::Unknown) => *x,
            (MessageSafety::Conflict, _)
            | (_, MessageSafety::Conflict)
            | (MessageSafety::Safe, MessageSafety::Unsafe)
            | (MessageSafety::Unsafe, MessageSafety::Safe) => MessageSafety::Conflict,
            (MessageSafety::Safe, MessageSafety::Safe) => MessageSafety::Safe,
            (MessageSafety::Unsafe, MessageSafety::Unsafe) => MessageSafety::Unsafe,
        }
    }
}

struct MessageSafetyTransfer<'tree> {
    source: &'tree str,
}
impl<'tree> Transfer<MessageSafety> for MessageSafetyTransfer<'tree> {
    type Block = Vec<Node<'tree>>;
    fn transfer(&self, state_in: &MessageSafety, block: &Self::Block) -> MessageSafety {
        let mut current = *state_in;
        for &stmt in block {
            for visited in sting_ast::Preorder::from_node(stmt) {
                if let Some(literal) = create_message_argument(visited.node, self.source) {
                    current = if bounce_mode_is_no_bounce(literal, self.source) {
                        MessageSafety::Safe
                    } else {
                        MessageSafety::Unsafe
                    };
                }
            }
        }
        current
    }
}

/// Whether `node` is a call to `createExternalLogMessage(...)` — a
/// **fundamentally different** message kind from `createMessage`, not a
/// second spelling of it: `createMessage` builds an `int_msg_info` message
/// (contract-to-contract, genuinely bounceable back into `onBouncedMessage`
/// if `bounce` is set and the receiver's compute phase throws — the whole
/// pattern this detector exists to catch); `createExternalLogMessage` builds
/// an `ext_out_msg_info` message (`dest:MsgAddressExt` per
/// `ton-blockchain/ton`'s `crypto/block/block.tlb`) — an outbound event log
/// with no on-chain receiving contract at all, structurally incapable of
/// ever bouncing back, regardless of any `bounce`-shaped field or send mode
/// on the call. Missing this recognizer entirely made `.send()` on an
/// external log message read as `MessageSafety::Unknown` (never proven
/// safe), so every real contract emitting an event via
/// `createExternalLogMessage` after a `.save()` — a normal, common pattern,
/// not a corner case — was wrongly flagged. Found via real external
/// validation, not this project's own corpus : none of `corpus/clean/`'s vendored contracts use
/// `createExternalLogMessage` even once, so this gap had no way to surface
/// during calibration until a real, independent codebase that does was
/// audited.
fn is_external_log_message_call(node: Node, source: &str) -> bool {
    if node.kind() != "function_call" {
        return false;
    }
    let Some(callee) = node.child_by_field_name("callee") else {
        return false;
    };
    identifier_text(callee, source).as_deref() == Some("createExternalLogMessage")
}

/// Whether a `createMessage({ ... })` argument literal (see
/// [`create_message_argument`]) declares a non-bounceable `bounce` field.
///
/// Tolk's `createMessage` accepts **two** equally valid spellings for "this
/// message cannot bounce": the `BounceMode.NoBounce` enum value, and a plain
/// `bounce: false` boolean literal — both compile and both mean the same
/// thing at the protocol level (a single bit in the message header). Found
/// missing during a manual corpus review (`docs/TESTING.md`'s
/// follow-up bug hunt): the original implementation only recognized the enum
/// spelling, so real, genuinely safe `bounce: false` sends (16 call sites
/// across 6 files in `corpus/clean/acton-contracts/` alone) were flagged as
/// bounceable — e.g. `corpus/clean/acton-contracts/dns/DnsItem.tolk`'s
/// `GovernanceRelease` self-destruct send, which explicitly sets `bounce:
/// false` right next to the `.send()` call this detector flagged.
fn bounce_mode_is_no_bounce(object_literal: Node, source: &str) -> bool {
    let Some(args) = object_literal.child_by_field_name("arguments") else {
        return false;
    };
    let mut cursor = args.walk();
    args.named_children(&mut cursor)
        .filter(|c| c.kind() == "instance_argument")
        .any(|arg| {
            let is_bounce_field = arg
                .child_by_field_name("name")
                .and_then(|n| n.utf8_text(source.as_bytes()).ok())
                == Some("bounce");
            let value_text = arg
                .child_by_field_name("value")
                .and_then(|v| v.utf8_text(source.as_bytes()).ok());
            is_bounce_field && matches!(value_text, Some("BounceMode.NoBounce" | "false"))
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

    use super::UbmDetector;
    use crate::{Detector, DetectorContext};

    fn run(source: &str) -> Vec<sting_core::Finding> {
        let tree = sting_ast::parse(source).expect("grammar loads");
        let store = AstStore::build(&tree, source);
        let ctx = DetectorContext::new(PathBuf::from("contract.tolk"), tree, source.to_string());
        UbmDetector.check(&store, &ctx)
    }

    const SEND_AFTER_SAVE: &str = "fun onInternalMessage(in: InMessage) {\n    var storage = lazy S.load();\n    storage.balance -= 1;\n    storage.save();\n    val out = createMessage({ bounce: BounceMode.Only256BitsOfBody, dest: x, value: 0, body: y });\n    out.send(SEND_MODE_PAY_FEES_SEPARATELY);\n}\n";

    #[test]
    fn flags_a_bounceable_send_after_save_with_no_handler_at_all() {
        let findings = run(SEND_AFTER_SAVE);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].detector_id, "UBM");
        assert!(findings[0].message.contains("no `onBouncedMessage`"));
    }

    #[test]
    fn flags_when_the_handler_exists_but_does_not_touch_storage() {
        let source = format!(
            "{SEND_AFTER_SAVE}\nfun onBouncedMessage(in: InMessageBounced) {{\n    return;\n}}\n"
        );
        let findings = run(&source);
        assert_eq!(findings.len(), 1);
        assert!(
            findings[0]
                .message
                .contains("does not appear to touch storage")
        );
    }

    #[test]
    fn does_not_flag_a_send_of_an_external_log_message_after_save() {
        // Real corpus finding, not invented (direction
        // 5): `TONresistor/zk-resistor-contracts`'s real `Pool.handleDeposit`/
        // `handleWithdraw`/`handleTakeWalletAddress` all save storage, then
        // send an event built via `createExternalLogMessage` — an
        // `ext_out_msg_info` message, structurally incapable of ever bouncing
        // back into `onBouncedMessage` (see `is_external_log_message_call`'s
        // doc comment). None of this workspace's own corpus uses
        // `createExternalLogMessage`, so this false positive had no way to
        // surface until a real, independent codebase that does was audited.
        let source = "fun onInternalMessage(in: InMessage) {\n    var storage = lazy S.load();\n    storage.balance -= 1;\n    storage.save();\n    val event = createExternalLogMessage({ dest: ExtOutLogBucket { topic: 1 }, body: y });\n    event.send(SEND_MODE_REGULAR);\n}\n";
        assert!(run(source).is_empty());
    }

    #[test]
    fn does_not_flag_when_the_handler_restores_state() {
        let source = format!(
            "{SEND_AFTER_SAVE}\nfun onBouncedMessage(in: InMessageBounced) {{\n    var storage = lazy S.load();\n    storage.balance += 1;\n    storage.save();\n}}\n"
        );
        assert!(run(&source).is_empty());
    }

    #[test]
    fn does_not_suppress_a_risky_send_using_an_unrelated_contracts_handler() {
        // Real bug this fix closes: contract `A` has no bounce handler of
        // its own; contract `B` (later in the same analysis unit) happens
        // to define one that does restore state. Before this fix,
        // `AstStore::function_by_name` found *a* `onBouncedMessage`
        // anywhere in the file and reused it for every risky-send function,
        // wrongly suppressing `A`'s genuinely unguarded risky send — a
        // false negative in a security detector, the direction of error
        // this crate's detectors are supposed to err away from.
        let source = r#"
contract A {
    author: "test"
}

fun aSend(in: InMessage) {
    var storage = lazy S.load();
    storage.balance -= 1;
    storage.save();
    val out = createMessage({ bounce: BounceMode.Only256BitsOfBody, dest: x, value: 0, body: y });
    out.send(SEND_MODE_PAY_FEES_SEPARATELY);
}

contract B {
    author: "test"
}

fun onBouncedMessage(in: InMessageBounced) {
    var storage = lazy S.load();
    storage.balance += 1;
    storage.save();
}
"#;
        let findings = run(source);
        assert_eq!(
            findings.len(),
            1,
            "A's risky send must still be flagged despite B's unrelated handler"
        );
        assert!(findings[0].message.contains("no `onBouncedMessage`"));
        assert!(findings[0].message.contains("contract `A`"));
    }

    #[test]
    fn matches_each_risky_send_against_its_own_contracts_handler_only() {
        // Same two-contract shape, but both `A` and `B` define their own
        // `onBouncedMessage` — `A`'s does not touch storage, `B`'s does.
        // Confirms the fix doesn't just suppress everything in a
        // multi-contract file, it correctly pairs each risky send with its
        // real owner's handler, not the first `onBouncedMessage` found in
        // the whole file.
        let source = r#"
contract A {
    author: "test"
}

fun aSend(in: InMessage) {
    var storage = lazy S.load();
    storage.balance -= 1;
    storage.save();
    val out = createMessage({ bounce: BounceMode.Only256BitsOfBody, dest: x, value: 0, body: y });
    out.send(SEND_MODE_PAY_FEES_SEPARATELY);
}

fun onBouncedMessage(in: InMessageBounced) {
    return;
}

contract B {
    author: "test"
}

fun bSend(in: InMessage) {
    var storage = lazy S.load();
    storage.balance -= 1;
    storage.save();
    val out = createMessage({ bounce: BounceMode.Only256BitsOfBody, dest: x, value: 0, body: y });
    out.send(SEND_MODE_PAY_FEES_SEPARATELY);
}

fun onBouncedMessage(in: InMessageBounced) {
    var storage = lazy S.load();
    storage.balance += 1;
    storage.save();
}
"#;
        let findings = run(source);
        assert_eq!(
            findings.len(),
            1,
            "only A's risky send should be flagged — its own handler doesn't restore state, B's own handler does"
        );
        assert!(findings[0].message.contains("aSend"));
        assert!(
            findings[0]
                .message
                .contains("does not appear to touch storage")
        );
    }

    #[test]
    fn does_not_flag_a_send_with_no_bounce() {
        let source = "fun onInternalMessage(in: InMessage) {\n    var storage = lazy S.load();\n    storage.balance -= 1;\n    storage.save();\n    val out = createMessage({ bounce: BounceMode.NoBounce, dest: x, value: 0, body: y });\n    out.send(SEND_MODE_PAY_FEES_SEPARATELY);\n}\n";
        assert!(run(source).is_empty());
    }

    #[test]
    fn does_not_flag_a_safe_message_split_from_its_send_by_an_unrelated_branch() {
        // Real bug this fix closes: `createMessage` and `.send()` end up in
        // different CFG blocks whenever anything at all branches between
        // them — even an `if` whose body has nothing to do with the
        // message. Before this fix, `last_message_is_safe` reset to `false`
        // at the top of every block, so this genuinely safe send was
        // wrongly flagged.
        let source = "fun onInternalMessage(in: InMessage) {\n    var storage = lazy S.load();\n    storage.balance -= 1;\n    storage.save();\n    val out = createMessage({ bounce: BounceMode.NoBounce, dest: x, value: 0, body: y });\n    if (in.someFlag) {\n        doSomethingUnrelated();\n    }\n    out.send(SEND_MODE_PAY_FEES_SEPARATELY);\n}\n";
        assert!(run(source).is_empty());
    }

    #[test]
    fn flags_an_unsafe_message_split_from_its_send_by_an_unrelated_branch() {
        // Same shape as the test above, but the message is genuinely
        // bounceable — confirms the fix doesn't just suppress everything
        // across a block boundary, it correctly carries the *actual* safety
        // value through.
        let source = "fun onInternalMessage(in: InMessage) {\n    var storage = lazy S.load();\n    storage.balance -= 1;\n    storage.save();\n    val out = createMessage({ bounce: BounceMode.Only256BitsOfBody, dest: x, value: 0, body: y });\n    if (in.someFlag) {\n        doSomethingUnrelated();\n    }\n    out.send(SEND_MODE_PAY_FEES_SEPARATELY);\n}\n";
        let findings = run(source);
        assert_eq!(findings.len(), 1);
    }

    #[test]
    fn does_not_flag_a_send_with_bounce_set_to_the_plain_boolean_false() {
        // Real corpus finding, not a hypothetical: `bounce: false` is an
        // equally valid, equally safe spelling to `BounceMode.NoBounce` —
        // `corpus/clean/acton-contracts/dns/DnsItem.tolk`'s
        // `GovernanceRelease` arm uses exactly this form and was wrongly
        // flagged before `bounce_mode_is_no_bounce` recognized it
        // (`docs/TESTING.md`'s follow-up bug hunt).
        let source = "fun onInternalMessage(in: InMessage) {\n    var storage = lazy S.load();\n    storage.balance -= 1;\n    storage.save();\n    val out = createMessage({ bounce: false, dest: x, value: 0, body: y });\n    out.send(SEND_MODE_PAY_FEES_SEPARATELY);\n}\n";
        assert!(run(source).is_empty());
    }

    #[test]
    fn does_not_flag_a_send_with_no_bounce_and_a_nested_object_literal_body() {
        // Regression test: a message body constructed as its own
        // `object_literal` (`body: SomeMessage { queryId: 1 }`, the real
        // shape used throughout corpus/fixtures/tolk-bench, not the bare
        // identifier `body: y` the other tests use) sits nested inside
        // createMessage's own argument literal. A `Preorder` walk visits
        // that inner literal right after the outer one; matching on "any
        // object_literal" instead of specifically createMessage's own
        // argument wrongly let the inner literal (no `bounce` field) reset
        // `last_message_is_safe` back to unsafe. Caught by
        // corpus/fixtures/detectors/UBM/negative/02-no-bounce-flag-set.tolk
        // wrongly firing during this block's implementation.
        let source = "fun onInternalMessage(in: InMessage) {\n    var storage = lazy S.load();\n    storage.balance -= 1;\n    storage.save();\n    val out = createMessage({ bounce: BounceMode.NoBounce, dest: x, value: 0, body: SomeMessage { queryId: 1 } });\n    out.send(SEND_MODE_PAY_FEES_SEPARATELY);\n}\n";
        assert!(run(source).is_empty());
    }

    #[test]
    fn does_not_flag_a_send_with_no_preceding_save() {
        let source = "fun onInternalMessage(in: InMessage) {\n    val out = createMessage({ bounce: BounceMode.Only256BitsOfBody, dest: x, value: 0, body: y });\n    out.send(SEND_MODE_PAY_FEES_SEPARATELY);\n}\n";
        assert!(run(source).is_empty());
    }

    #[test]
    fn does_not_flag_a_save_with_no_send_at_all() {
        let source = "fun onInternalMessage(in: InMessage) {\n    var storage = lazy S.load();\n    storage.balance -= 1;\n    storage.save();\n}\n";
        assert!(run(source).is_empty());
    }

    #[test]
    fn flags_only_the_branch_that_actually_saves_before_sending() {
        // Same shape as real JettonMinter.tolk: a match with independent
        // arms, only one of which saves-then-sends. Confirms per-path
        // dataflow, not a single flat whole-function scan (which would
        // wrongly let one arm's `save()` leak into another arm's `send()`).
        let source = "fun onInternalMessage(in: InMessage) {\n    match (in.op) {\n        1 => {\n            var storage = lazy S.load();\n            storage.balance -= 1;\n            storage.save();\n            val out = createMessage({ bounce: BounceMode.Only256BitsOfBody, dest: x, value: 0, body: y });\n            out.send(SEND_MODE_PAY_FEES_SEPARATELY);\n        }\n        else => {\n            val out2 = createMessage({ bounce: BounceMode.Only256BitsOfBody, dest: x, value: 0, body: y });\n            out2.send(SEND_MODE_PAY_FEES_SEPARATELY);\n        }\n    }\n}\n";
        let findings = run(source);
        assert_eq!(
            findings.len(),
            1,
            "only the arm that saved before sending should be flagged"
        );
    }

    #[test]
    fn flags_a_bounceable_send_after_a_direct_set_data_call() {
        // Same root cause `ac.rs` independently found (direction 5,
        // hacktheton.com): `.save()` is not the only spelling of "persist
        // storage" — `contract.setData(...)` is the real primitive, and was
        // completely invisible to this detector when called directly.
        let source = "fun onInternalMessage(in: InMessage) {\n    var storage = lazy S.load();\n    storage.balance -= 1;\n    contract.setData(storage.toCell());\n    val out = createMessage({ bounce: BounceMode.Only256BitsOfBody, dest: x, value: 0, body: y });\n    out.send(SEND_MODE_PAY_FEES_SEPARATELY);\n}\n";
        let findings = run(source);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].detector_id, "UBM");
    }

    #[test]
    fn flags_a_bounceable_send_after_the_legacy_free_function_set_contract_data() {
        // Real external finding: `hacktheton.com`'s actual, currently-live
        // level source uses the pre-1.0 Tolk API's free-function name
        // (`setContractData`, no `contract.` receiver).
        let source = "fun onInternalMessage(myBalance: int, msgValue: int, inMsgFull: cell, inMsgBody: slice) {\n    setContractData(beginCell().endCell());\n    val out = createMessage({ bounce: BounceMode.Only256BitsOfBody, dest: x, value: 0, body: y });\n    out.send(SEND_MODE_PAY_FEES_SEPARATELY);\n}\n";
        let findings = run(source);
        assert_eq!(findings.len(), 1);
    }

    #[test]
    fn a_handler_restoring_state_via_set_data_still_counts_as_restoring() {
        let source = "fun onInternalMessage(in: InMessage) {\n    var storage = lazy S.load();\n    storage.balance -= 1;\n    contract.setData(storage.toCell());\n    val out = createMessage({ bounce: BounceMode.Only256BitsOfBody, dest: x, value: 0, body: y });\n    out.send(SEND_MODE_PAY_FEES_SEPARATELY);\n}\nfun onBouncedMessage(in: InMessageBounced) {\n    var storage = lazy S.load();\n    storage.balance += 1;\n    contract.setData(storage.toCell());\n}\n";
        assert!(run(source).is_empty());
    }

    #[test]
    fn kind_is_dataflow_and_severity_is_critical() {
        assert_eq!(UbmDetector.kind(), crate::DetectorKind::Dataflow);
        assert_eq!(
            UbmDetector.default_severity(),
            sting_core::Severity::Critical
        );
    }

    /// A log event sent as a **chain**, with no intermediate variable.
    ///
    /// The walk is preorder, so the outer `.send(...)` is visited before the
    /// nested `createExternalLogMessage(...)`, and the recognizer that closed
    /// the through-a-variable form did not fire on this one. Found on
    /// `xssnick/payment-channel-contract` — a false **Critical** on a live
    /// payment channel. Confirmed by a minimal reproduction: the same message
    /// gives 0 findings through a variable and 1 as a chain.
    #[test]
    fn chained_external_log_message_send_is_not_risky() {
        let source = "fun onInternalMessage(in: InMessage) {
    var storage = lazy S.load();
    storage.balance -= 1;
    storage.save();
    createExternalLogMessage({ dest: createAddressNone(), body: y }).send(SEND_MODE_REGULAR);
}
";
        assert!(
            run(source).is_empty(),
            "an external log cannot bounce, through a variable or as a chain"
        );
    }

    /// The other side: a chain carrying a genuinely bounceable message must
    /// still be found. A test that only checks for silence would pass on a
    /// detector that is silent always.
    #[test]
    fn chained_bounceable_message_send_is_still_risky() {
        let source = "fun onInternalMessage(in: InMessage) {
    var storage = lazy S.load();
    storage.balance -= 1;
    storage.save();
    createMessage({ bounce: true, dest: x, value: 0, body: y }).send(SEND_MODE_REGULAR);
}
";
        assert_eq!(
            run(source).len(),
            1,
            "a chain must not hide a real bounceable send"
        );
    }

    /// And a chain with an explicit `BounceMode.NoBounce` is safe.
    #[test]
    fn chained_no_bounce_message_send_is_not_risky() {
        let source = "fun onInternalMessage(in: InMessage) {
    var storage = lazy S.load();
    storage.balance -= 1;
    storage.save();
    createMessage({ bounce: BounceMode.NoBounce, dest: x, value: 0, body: y }).send(SEND_MODE_REGULAR);
}
";
        assert!(
            run(source).is_empty(),
            "an explicit NoBounce in a chain must read the same as through a variable"
        );
    }
}
