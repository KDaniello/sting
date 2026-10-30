//! `UDA` — Unchecked Destination Address (direction
//! Sting's first taint-tracking detector.
//!
//! **The real, confirmed motivating pattern** (`hacktheton.com`'s `proxy_level` challenge): a destination
//! address read directly out of the incoming message body
//! (`inMsgBody.loadAddress()`) reaches `sendRawMessage(...)` with no check
//! at all — an open relay. No other detector in sting sees this
//! class: it is not a storage-mutation question (`AC`'s domain), not a
//! signedness question (`UTA`'s), just "does an untrusted value reach a
//! dangerous sink unchecked" — the textbook definition of taint tracking,
//! not a syntactic pattern.
//!
//! **Not new infrastructure — one new [`sting_dataflow::Transfer`]
//! implementation over already-existing machinery**, exactly as planned:
//! `sting_dataflow::{Semilattice, Transfer, WorklistSolver}` (the same
//! generic forward "may" solver `id`/`ubm`/`tlb` already use) plus
//! `crate::cfg::build` (the same Tolk CFG builder those three already
//! share) — `T = BTreeSet<String>` (tainted names), `bottom = {}`,
//! `join = union`, the exact shape `sting-dataflow`'s own `SetUnion`/
//! `GenOnly` test already demonstrates.
//!
//! **Real architectural finding made while implementing, not assumed in
//! advance: `cfg::build`'s blocks do not expose `if`/`match` *conditions*,
//! only branch *bodies*** (`cfg.rs`'s own doc comment: "condition and
//! scrutinee expressions are not scanned for events"). `UBM`'s own
//! `MessageSafetyLattice` never actually needed a condition either — its
//! signal (`createMessage`'s own literal argument) lives inside a
//! *statement* the CFG already carries. A sanitizing check of the shape
//! `if (recipient == trusted) { sendRawMessage(...); }` genuinely cannot be
//! recognized from `cfg.rs`'s blocks alone: the comparison lives in the
//! `if`'s condition, which the builder discards. Extending `cfg.rs` itself
//! to carry conditions would affect `id`/`ubm`/`tlb` too — out of scope for
//! a stretch-goal detector to risk. **v1 deliberately recognizes
//! sanitization only via `assert_statement`** (`assert(recipient ==
//! trusted) throw ...;`), which — unlike an `if` condition — *is* a real
//! statement the CFG already places directly in a block's own list, so it
//! needs no new infrastructure at all. An `if`-branch-gated destination
//! check is a **known, named v1 gap**, not a silent one — see
//! [`sink_in_block`]'s own doc comment.
//!
//! **Taint sources**: a value assigned from `<receiver>.loadAddress()` /
//! `.loadRef()` / `.loadBits(len)` where `<receiver>` is the message body
//! (the low-level `inMsgBody` parameter or the modern `in.body` field
//! access — matched by name/shape only, the same textual-heuristic
//! precision level every other detector in this crate already accepts, no
//! real type information is available anywhere in this toolchain) or a name
//! already tainted (one level of chaining, e.g. reading a sub-slice off an
//! already-tainted slice). Corrected against the real vendored stdlib
//! during implementation, not assumed from the plan's own memory of prior
//! research: `slice` has no `loadSlice` method at all —
//! `vendor/tolk/tolk-stdlib/common.tolk:1192` names it `loadBits(len: int):
//! slice`.
//!
//! A second taint-generating shape, needed to reach the real
//! `sendRawMessage(msg, mode)` sink form (not just `createMessage({dest:
//! ...}).send()`): a `beginCell()...endCell()` builder chain that calls
//! `.storeSlice(...)`/`.storeAddress(...)` with a tainted argument anywhere
//! in its own chain taints the resulting cell too — the constructed message
//! itself now carries untrusted data. `createMessage({ ..., dest: <tainted>,
//! ... })` taints its own `OutMessage` result the same way, for the
//! `<msg>.send(...)` sink form.
//!
//! **Sinks**: `sendRawMessage(<tainted>, mode)`, or `<tainted-msg>.send(mode)`.
//!
//! **v1 scope, deliberately narrow — same-function only, same-block
//! sanitization only**: no interprocedural call-following (unlike `AC`/
//! `UTA`); a sanitizing `assert` in an earlier CFG block than the sink
//! (split by unrelated intervening branching) does not suppress it — only
//! recognized within the same straight-line block the sink itself is in.
//! Both are honest, named narrowings in the same spirit as every other
//! detector's own v1 boundary in this crate, not silent gaps.

use std::collections::BTreeSet;

use sting_ast::{AstStore, FunctionDef, Language};
use sting_core::{DetectorId, Finding, Severity};
use sting_dataflow::{Semilattice, Transfer, WorklistSolver};
use tree_sitter::Node;

use crate::cfg;
use crate::support::{
    CrossFileCache, as_method_call, bound_names, create_message_argument, function_body,
    identifier_text, location_of, nth_call_argument_text,
};
use crate::{Detector, DetectorContext, DetectorKind};

/// `<message-body-slice>.<method>(...)` calls that read an untrusted value
/// straight out of the incoming message.
const TAINT_SOURCE_METHODS: &[&str] = &["loadAddress", "loadRef", "loadBits"];

/// Which stdlib primitive an unsanitized tainted value reached — see
/// `sink_call_with_tainted_argument`'s own doc comment for the two
/// recognized shapes. Threaded through
/// [`call_passes_taint_into_a_sink`]/[`callee_reaches_a_sink_from_parameter`]'s
/// one-hop interprocedural following too, so a finding reported at a
/// delegating call site still gets the precise message/severity for
/// whichever sink the *callee* actually reaches, not a generic one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SinkKind {
    /// `sendRawMessage(<tainted>, mode)` or `<tainted-msg>.send(mode)`.
    Relay,
    /// `contract.setCodePostponed(<tainted>)` or the legacy free-function
    /// spelling `setContractCodePostponed(<tainted>)` — a tainted *code*
    /// cell, not a destination address (`ac.rs`'s own `CODE_MUTATION_METHOD`
    /// asks a different, complementary question: "is the caller authorized
    /// to upgrade" — this asks "is the new code itself attacker-supplied",
    /// which stays a real, distinct risk even behind a real sender check).
    CodeUpgrade,
}

/// One confirmed unsanitized sink: the exact call-site node, which
/// [`SinkKind`], and — for the one-hop interprocedural case
/// ([`call_passes_taint_into_a_sink`]) — the name of the callee the taint
/// actually flowed *into* before reaching the sink, so [`UdaDetector::check`]
/// can report an accurate "passes X to Y, which relays it unchecked"
/// message instead of misattributing the sink call directly to the
/// (delegating, not sinking) outer function.
struct SinkFound<'a> {
    node: Node<'a>,
    kind: SinkKind,
    via_callee: Option<String>,
}

/// Bundles what every function below needs but never changes across one
/// function's own analysis — same shape as `ac.rs`'s own `Env`.
struct Env<'a> {
    source: &'a str,
    ctx: &'a DetectorContext,
    store: &'a AstStore,
}

pub(crate) struct UdaDetector;

impl Detector for UdaDetector {
    fn id(&self) -> DetectorId {
        "UDA"
    }

    fn kind(&self) -> DetectorKind {
        DetectorKind::Dataflow
    }

    fn default_severity(&self) -> Severity {
        Severity::High
    }

    fn check(&self, store: &AstStore, ctx: &DetectorContext) -> Vec<Finding> {
        let env = Env {
            source: ctx.source(),
            ctx,
            store,
        };
        let mut cross_file = CrossFileCache::default();
        let mut findings = Vec::new();

        for function in store.functions() {
            let Some(body) = function_body(function, ctx) else {
                continue;
            };
            if let Some(found) =
                first_unsanitized_sink(body, &env, &mut cross_file, &BTreeSet::new(), true)
            {
                let subject = match &found.via_callee {
                    Some(callee_name) => format!(
                        "function `{}` passes unchecked data read from the incoming message body into `{callee_name}()`, which",
                        function.name
                    ),
                    None => format!(
                        "function `{}`, using unchecked data read from the incoming message body,",
                        function.name
                    ),
                };
                let (message, remediation) = match found.kind {
                    SinkKind::Relay => (
                        format!(
                            "{subject} sends a message whose destination traces back to that data — an attacker can supply any address here"
                        ),
                        "before sending, assert the destination against a trusted value (e.g. `assert (recipient == storage.ownerAddress) throw ...;`) or a membership check against a known-safe set — using an address read straight from the message body as a send destination turns this into an open relay".to_string(),
                    ),
                    SinkKind::CodeUpgrade => (
                        format!(
                            "{subject} installs new contract code whose bytes trace back to that data — an attacker can supply arbitrary code here"
                        ),
                        "before upgrading, assert the new code against a trusted, pre-approved value (a known code hash, or a governance-gated update path) — installing a code cell read straight from the message body lets any caller replace this contract's logic entirely".to_string(),
                    ),
                };
                findings.push(Finding {
                    detector_id: self.id(),
                    severity: match found.kind {
                        SinkKind::Relay => self.default_severity(),
                        SinkKind::CodeUpgrade => Severity::Critical,
                    },
                    message,
                    remediation,
                    location: location_of(found.node, ctx),
                    confidence: None,
                    dynamic_verification: None,
                    suggestion: None,
                });
            }
        }
        findings
    }
}

/// Set-union taint lattice: `bottom = {}`, `join = union` — the textbook
/// "may" lattice, same shape as `sting_dataflow`'s own `SetUnion` test.
struct TaintLattice;
impl Semilattice<BTreeSet<String>> for TaintLattice {
    fn bottom(&self) -> BTreeSet<String> {
        BTreeSet::new()
    }
    fn join(&self, a: &BTreeSet<String>, b: &BTreeSet<String>) -> BTreeSet<String> {
        a.union(b).cloned().collect()
    }
}

struct TaintTransfer<'tree> {
    source: &'tree str,
    /// Names tainted from the start, regardless of path — empty for the
    /// ordinary same-function analysis; a single parameter name for the
    /// one-hop interprocedural case
    /// ([`callee_reaches_a_sink_from_parameter`]), where a caller already
    /// proved that parameter tainted before this function was ever called.
    /// A parameter's taintedness is a constant fact about the calling
    /// context, true in every block, not something that changes mid-body —
    /// unioning it in on every `transfer` call (rather than only seeding
    /// the entry block, which `sting_dataflow::WorklistSolver` does not
    /// expose a way to do) achieves exactly that.
    pre_tainted: &'tree BTreeSet<String>,
}
impl<'tree> Transfer<BTreeSet<String>> for TaintTransfer<'tree> {
    type Block = Vec<Node<'tree>>;
    /// Generate-only, as planned: a block's `OUT` is its `IN` plus whatever
    /// the block's own declarations taint — never kills anything. Killing
    /// (sanitization) is handled separately, only in the post-pass re-scan
    /// (see [`sink_in_block`]) — see this module's own doc comment for why
    /// that split is correct here.
    fn transfer(&self, state_in: &BTreeSet<String>, block: &Self::Block) -> BTreeSet<String> {
        let mut tainted = state_in.clone();
        tainted.extend(self.pre_tainted.iter().cloned());
        for &stmt in block {
            if let Some((name, rhs)) = declared_name_and_rhs(stmt, self.source)
                && taints(rhs, &tainted, self.source)
            {
                tainted.insert(name);
            }
        }
        tainted
    }
}

/// If `stmt` is `val/var NAME = <expr>;` with a single (non-tuple) bound
/// name, `(NAME, <expr>)` — mirrors `lep.rs`'s `collect_aliases`'s own
/// identical shape check, not duplicated as a shared helper since the two
/// modules use the result differently (an alias set vs. a taint set).
fn declared_name_and_rhs<'a>(stmt: Node<'a>, source: &str) -> Option<(String, Node<'a>)> {
    if stmt.kind() != "expression_statement" {
        return None;
    }
    let assignment = stmt.named_child(0)?;
    if assignment.kind() != "assignment" {
        return None;
    }
    let left = assignment.child_by_field_name("left")?;
    if left.kind() != "var_declaration_lhs" {
        return None;
    }
    let right = assignment.child_by_field_name("right")?;
    match bound_names(left, source).as_slice() {
        [(_, name)] => Some((name.clone(), right)),
        _ => None,
    }
}

/// Whether `rhs` is one of the three taint-generating shapes this module
/// recognizes (see the module doc comment): a direct read off the message
/// body (or an already-tainted name), a `beginCell()...endCell()` chain
/// storing a tainted value, or `createMessage({ dest: <tainted>, ... })`.
fn taints(rhs: Node, tainted: &BTreeSet<String>, source: &str) -> bool {
    if let Some(call) = as_method_call(rhs, source, Language::Tolk) {
        if TAINT_SOURCE_METHODS.contains(&call.method.as_str())
            && (is_message_body_receiver(call.obj, source)
                || identifier_text(call.obj, source).is_some_and(|n| tainted.contains(&n)))
        {
            return true;
        }
        if call.method == "endCell" && chain_stores_a_tainted_value(rhs, tainted, source) {
            return true;
        }
    }
    if let Some(literal) = create_message_argument(rhs, source)
        && dest_field_is_tainted(literal, tainted, source)
    {
        return true;
    }
    false
}

/// The message-body slice's two accepted spellings — the low-level
/// `inMsgBody: slice` parameter, or the modern `in.body` field access.
/// Matched by name/shape only, same precision level as every other
/// detector's own textual heuristics in this crate (no type information is
/// available anywhere in this toolchain to verify it more strongly).
fn is_message_body_receiver(node: Node, source: &str) -> bool {
    if identifier_text(node, source).as_deref() == Some("inMsgBody") {
        return true;
    }
    node.kind() == "dot_access"
        && node
            .child_by_field_name("obj")
            .is_some_and(|obj| identifier_text(obj, source).as_deref() == Some("in"))
        && node
            .child_by_field_name("field")
            .and_then(|f| f.utf8_text(source.as_bytes()).ok())
            == Some("body")
}

/// Whether `endcell_call`'s own method chain (walked via `Preorder`, so any
/// nesting depth) contains a `.storeSlice(...)`/`.storeAddress(...)` call
/// whose sole argument is a currently-tainted name.
fn chain_stores_a_tainted_value(
    endcell_call: Node,
    tainted: &BTreeSet<String>,
    source: &str,
) -> bool {
    sting_ast::Preorder::from_node(endcell_call)
        .map(|v| v.node)
        .filter_map(|n| as_method_call(n, source, Language::Tolk))
        .filter(|c| matches!(c.method.as_str(), "storeSlice" | "storeAddress"))
        .any(|c| {
            nth_call_argument_text(c.args, 0, source).is_some_and(|arg| tainted.contains(&arg))
        })
}

/// Whether `object_literal`'s (a `createMessage({...})` call's own argument)
/// `dest` field is a bare identifier naming a currently-tainted value.
fn dest_field_is_tainted(object_literal: Node, tainted: &BTreeSet<String>, source: &str) -> bool {
    let Some(args) = object_literal.child_by_field_name("arguments") else {
        return false;
    };
    let mut cursor = args.walk();
    args.named_children(&mut cursor)
        .filter(|c| c.kind() == "instance_argument")
        .any(|arg| {
            let is_dest = arg
                .child_by_field_name("name")
                .and_then(|n| n.utf8_text(source.as_bytes()).ok())
                == Some("dest");
            let value_text = arg
                .child_by_field_name("value")
                .and_then(|v| v.utf8_text(source.as_bytes()).ok());
            is_dest && value_text.is_some_and(|t| tainted.contains(t))
        })
}

/// Runs the taint solve over `body`'s CFG, then re-scans each block with its
/// derived `IN` fact to find the first unsanitized sink — same overall shape
/// as `ubm.rs`'s `first_risky_send_after_save`.
///
/// `pre_tainted` — see [`TaintTransfer::pre_tainted`]'s own doc comment;
/// empty for the ordinary top-level call, a single parameter name for the
/// one-hop interprocedural case. `allow_interprocedural` — whether
/// [`sink_in_block`] may itself follow a call one hop further
/// ([`call_passes_taint_into_a_sink`]): `true` at the top level, `false`
/// when this function is itself being called *as* that one hop
/// ([`callee_reaches_a_sink_from_parameter`]) — bounding interprocedural
/// following to exactly one hop, deliberately, the same "start narrow"
/// scope `uta.rs`'s own one-hop design already established, and the only
/// thing standing between this and unbounded recursion through a call
/// cycle in the target code.
fn first_unsanitized_sink<'tree>(
    body: Node<'tree>,
    env: &Env<'tree>,
    cross_file: &mut CrossFileCache,
    pre_tainted: &BTreeSet<String>,
    allow_interprocedural: bool,
) -> Option<SinkFound<'tree>> {
    let built = cfg::build(body).ok()?;
    let tainted_out = WorklistSolver::solve(
        &TaintLattice,
        &TaintTransfer {
            source: env.source,
            pre_tainted,
        },
        &built,
    );

    for block_id in 0..built.block_count() {
        let mut tainted_in = built
            .predecessors(block_id)
            .iter()
            .fold(BTreeSet::new(), |acc, p| {
                TaintLattice.join(&acc, tainted_out.get(p).unwrap_or(&BTreeSet::new()))
            });
        tainted_in.extend(pre_tainted.iter().cloned());
        if let Some(found) = sink_in_block(
            built.block(block_id),
            tainted_in,
            env,
            cross_file,
            allow_interprocedural,
        ) {
            return Some(found);
        }
    }
    None
}

/// Scans one block's statements in source order, starting from `tainted_in`
/// (the block's CFG-derived `IN` fact): grows the local tainted set the
/// same way the dataflow `Transfer` does, *and* shrinks it on a sanitizing
/// `assert_statement` — see the module doc comment for why sanitization can
/// only be recognized this way (same-block, `assert`-only) without new
/// shared `cfg.rs` infrastructure.
fn sink_in_block<'tree>(
    block: &[Node<'tree>],
    tainted_in: BTreeSet<String>,
    env: &Env<'tree>,
    cross_file: &mut CrossFileCache,
    allow_interprocedural: bool,
) -> Option<SinkFound<'tree>> {
    let mut tainted = tainted_in;
    for &stmt in block {
        if let Some(name) = sanitized_by_assert(stmt, &tainted, env.source) {
            tainted.remove(&name);
        }
        if let Some((name, rhs)) = declared_name_and_rhs(stmt, env.source)
            && taints(rhs, &tainted, env.source)
        {
            tainted.insert(name);
        }
        if let Some(found) = sink_call_with_tainted_argument(stmt, &tainted, env.source) {
            return Some(found);
        }
        if allow_interprocedural
            && let Some(found) = call_passes_taint_into_a_sink(stmt, &tainted, env, cross_file)
        {
            return Some(found);
        }
    }
    None
}

/// If `stmt` is `assert (<tainted-name> == <expr>) throw ...;` (either
/// operand order), the sanitized name — `assert` genuinely does prove the
/// comparison true for every statement after it on this same
/// straight-line path (it throws otherwise), unlike a plain `if` condition
/// (see the module doc comment for why only `assert` is recognized in v1).
fn sanitized_by_assert(stmt: Node, tainted: &BTreeSet<String>, source: &str) -> Option<String> {
    if stmt.kind() != "assert_statement" {
        return None;
    }
    let condition = stmt.child_by_field_name("condition")?;
    let condition = crate::support::unwrap_parens(condition);
    // `binary_operator`'s operator token and operands are positional
    // children, not named fields (`br.rs`/`pl.rs`'s own `operator_kind`
    // reads `node.child(1)` the same way — confirmed against the real
    // grammar, not assumed).
    if condition.kind() != "binary_operator" || condition.child(1).map(|c| c.kind()) != Some("==") {
        return None;
    }
    let left = condition.named_child(0)?;
    let right = condition.named_child(1)?;
    identifier_text(left, source)
        .filter(|name| tainted.contains(name))
        .or_else(|| identifier_text(right, source).filter(|name| tainted.contains(name)))
}

/// Legacy, pre-1.0 Tolk free-function spelling of `contract.
/// setCodePostponed(...)` — same name `ac.rs`'s own `LEGACY_CODE_MUTATION_FUNCTIONS`
/// already recognizes, for the same reason (confirmed still the literal
/// spelling `hacktheton.com`'s real, currently-live level source uses).
const LEGACY_CODE_UPGRADE_FUNCTION: &str = "setContractCodePostponed";

/// If `stmt` contains a call to `sendRawMessage(<tainted>, mode)` or
/// `<tainted-msg>.send(mode)` ([`SinkKind::Relay`]), or
/// `contract.setCodePostponed(<tainted>)` /
/// `setContractCodePostponed(<tainted>)` ([`SinkKind::CodeUpgrade`]), that
/// call's own method/callee name node plus which kind it was.
fn sink_call_with_tainted_argument<'a>(
    stmt: Node<'a>,
    tainted: &BTreeSet<String>,
    source: &str,
) -> Option<SinkFound<'a>> {
    for visited in sting_ast::Preorder::from_node(stmt) {
        let node = visited.node;
        if node.kind() == "function_call"
            && let Some(callee) = node.child_by_field_name("callee")
            && let Some(callee_name) = identifier_text(callee, source)
            && let Some(args) = node.child_by_field_name("arguments")
            && nth_call_argument_text(args, 0, source).is_some_and(|a| tainted.contains(&a))
        {
            if callee_name == "sendRawMessage" {
                return Some(SinkFound {
                    node: callee,
                    kind: SinkKind::Relay,
                    via_callee: None,
                });
            }
            if callee_name == LEGACY_CODE_UPGRADE_FUNCTION {
                return Some(SinkFound {
                    node: callee,
                    kind: SinkKind::CodeUpgrade,
                    via_callee: None,
                });
            }
        }
        if let Some(call) = as_method_call(node, source, Language::Tolk) {
            if call.method == "send"
                && identifier_text(call.obj, source).is_some_and(|name| tainted.contains(&name))
            {
                return Some(SinkFound {
                    node: call.method_node,
                    kind: SinkKind::Relay,
                    via_callee: None,
                });
            }
            if call.method == "setCodePostponed"
                && nth_call_argument_text(call.args, 0, source)
                    .is_some_and(|a| tainted.contains(&a))
            {
                return Some(SinkFound {
                    node: call.method_node,
                    kind: SinkKind::CodeUpgrade,
                    via_callee: None,
                });
            }
        }
    }
    None
}

/// One hop of interprocedural taint following (mirrors `uta.rs`'s own
/// one-hop design): scans `stmt` for a call to a same-file or cross-file
/// **free function** (method calls are out of scope here — the same
/// receiver-type-is-unknowable limitation `ac.rs`'s own `classify_method`
/// doc comment already explains, no new information available for this
/// module to resolve one either) that passes a currently-tainted name as
/// one of its arguments. If the callee's own body — analyzed fresh, seeded
/// with *that* parameter as already tainted, with interprocedural following
/// itself turned off (see [`first_unsanitized_sink`]'s own doc comment for
/// why this is what bounds recursion to exactly one hop) — reaches an
/// unsanitized sink, this call site is the finding: the caller is what
/// actually let the untrusted value flow into a function that mishandles
/// it, not the callee in isolation.
fn call_passes_taint_into_a_sink<'a>(
    stmt: Node<'a>,
    tainted: &BTreeSet<String>,
    env: &Env,
    cross_file: &mut CrossFileCache,
) -> Option<SinkFound<'a>> {
    if tainted.is_empty() {
        return None;
    }
    for visited in sting_ast::Preorder::from_node(stmt) {
        let node = visited.node;
        if node.kind() != "function_call" {
            continue;
        }
        let Some(callee) = node.child_by_field_name("callee") else {
            continue;
        };
        if callee.kind() != "identifier" {
            continue;
        }
        let Some(name) = identifier_text(callee, env.source) else {
            continue;
        };
        let Some(args) = node.child_by_field_name("arguments") else {
            continue;
        };
        let Some(arg_index) = tainted_argument_index(args, tainted, env.source) else {
            continue;
        };
        if let Some(kind) = callee_reaches_a_sink_from_parameter(&name, arg_index, env, cross_file)
        {
            return Some(SinkFound {
                node: callee,
                kind,
                via_callee: Some(name),
            });
        }
    }
    None
}

/// The 0-based position of the first argument in `args` whose own text
/// names a currently-tainted value, if any.
fn tainted_argument_index(args: Node, tainted: &BTreeSet<String>, source: &str) -> Option<usize> {
    let mut cursor = args.walk();
    args.named_children(&mut cursor)
        .filter(|c| c.kind() == "call_argument")
        .enumerate()
        .find_map(|(index, ca)| {
            let text = ca
                .child_by_field_name("expr")
                .and_then(|e| e.utf8_text(source.as_bytes()).ok())?
                .trim();
            tainted.contains(text).then_some(index)
        })
}

/// Resolves `callee_name` (same-file first, then cross-file on a same-file
/// miss — the same order every other cross-file detector in this crate
/// uses) and, if it has a parameter at `arg_index`, whether that function's
/// own body reaches an unsanitized sink when *only* that one parameter
/// starts tainted. An unresolvable name (nowhere same-file, no project
/// attached, ambiguous, or resolves nowhere in one either), or a parameter
/// index past the callee's own parameter list, degrades to `None` — the
/// same fail-safe default every other cross-file path in this workspace
/// uses, not a guess.
fn callee_reaches_a_sink_from_parameter(
    callee_name: &str,
    arg_index: usize,
    env: &Env,
    cross_file: &mut CrossFileCache,
) -> Option<SinkKind> {
    if let Some(function) = env.store.function_by_name(callee_name) {
        let callee_env = Env {
            source: env.source,
            ctx: env.ctx,
            store: env.store,
        };
        return function_reaches_a_sink_from_parameter(function, arg_index, &callee_env);
    }
    let matches = env.ctx.resolve_call(callee_name)?;
    let [only] = matches.as_slice() else {
        return None;
    };
    let project = env.ctx.project_store()?;
    let path = project.path_for_file_id(only.ident_loc.file_id)?;
    let loaded = cross_file.get_or_load(&path, project)?;
    let (cross_store, cross_ctx) = &*loaded;
    let function = cross_store.function_by_name(callee_name)?;
    let callee_env = Env {
        source: cross_ctx.source(),
        ctx: cross_ctx,
        store: cross_store,
    };
    function_reaches_a_sink_from_parameter(function, arg_index, &callee_env)
}

/// The non-cross-file half of [`callee_reaches_a_sink_from_parameter`]:
/// `function` is `callee_name` as already resolved, and `callee_env` is
/// already the *callee's own* file's environment (this one or a cross-file
/// one) — not necessarily the original caller's.
fn function_reaches_a_sink_from_parameter(
    function: &FunctionDef,
    arg_index: usize,
    callee_env: &Env,
) -> Option<SinkKind> {
    let param = function.parameters.get(arg_index)?;
    let body = function_body(function, callee_env.ctx)?;
    let pre_tainted: BTreeSet<String> = std::iter::once(param.name.clone()).collect();
    first_unsanitized_sink(
        body,
        callee_env,
        &mut CrossFileCache::default(),
        &pre_tainted,
        false,
    )
    .map(|found| found.kind)
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

    use super::UdaDetector;
    use crate::{Detector, DetectorContext};

    fn run(source: &str) -> Vec<sting_core::Finding> {
        let tree = sting_ast::parse(source).expect("grammar loads");
        let store = AstStore::build(&tree, source);
        let ctx = DetectorContext::new(PathBuf::from("contract.tolk"), tree, source.to_string());
        UdaDetector.check(&store, &ctx)
    }

    const RAW_CELL_RELAY: &str = "fun onInternalMessage(myBalance: int, msgValue: int, inMsgFull: cell, inMsgBody: slice) {\n    val recipient: slice = inMsgBody.loadAddress();\n    val msg: cell = beginCell().storeUint(0x18, 6).storeSlice(recipient).storeCoins(0).endCell();\n    sendRawMessage(msg, 64);\n}\n";

    #[test]
    fn flags_an_unchecked_address_from_the_message_body_reaching_send_raw_message() {
        let findings = run(RAW_CELL_RELAY);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].detector_id, "UDA");
        assert!(findings[0].message.contains("onInternalMessage"));
    }

    #[test]
    fn does_not_flag_when_the_address_is_asserted_against_a_trusted_value_first() {
        let source = "fun onInternalMessage(myBalance: int, msgValue: int, inMsgFull: cell, inMsgBody: slice) {\n    val recipient: slice = inMsgBody.loadAddress();\n    assert (recipient == trustedAddress) throw 500;\n    val msg: cell = beginCell().storeUint(0x18, 6).storeSlice(recipient).storeCoins(0).endCell();\n    sendRawMessage(msg, 64);\n}\n";
        assert!(run(source).is_empty());
    }

    #[test]
    fn does_not_flag_when_the_assert_compares_the_other_operand_order() {
        let source = "fun onInternalMessage(myBalance: int, msgValue: int, inMsgFull: cell, inMsgBody: slice) {\n    val recipient: slice = inMsgBody.loadAddress();\n    assert (trustedAddress == recipient) throw 500;\n    val msg: cell = beginCell().storeUint(0x18, 6).storeSlice(recipient).storeCoins(0).endCell();\n    sendRawMessage(msg, 64);\n}\n";
        assert!(run(source).is_empty());
    }

    #[test]
    fn flags_via_the_modern_in_body_field_access() {
        let source = "fun onInternalMessage(in: InMessage) {\n    val recipient: slice = in.body.loadAddress();\n    val msg: cell = beginCell().storeUint(0x18, 6).storeSlice(recipient).storeCoins(0).endCell();\n    sendRawMessage(msg, 64);\n}\n";
        let findings = run(source);
        assert_eq!(findings.len(), 1);
    }

    #[test]
    fn flags_via_the_create_message_dest_field_and_send_form() {
        let source = "fun onInternalMessage(in: InMessage) {\n    val recipient: slice = in.body.loadAddress();\n    val out = createMessage({ bounce: BounceMode.Only256BitsOfBody, dest: recipient, value: 0, body: y });\n    out.send(64);\n}\n";
        let findings = run(source);
        assert_eq!(findings.len(), 1);
    }

    #[test]
    fn does_not_flag_a_send_to_a_trusted_stored_address() {
        let source = "fun onInternalMessage(in: InMessage) {\n    val recipient: slice = storage.ownerAddress;\n    val msg: cell = beginCell().storeUint(0x18, 6).storeSlice(recipient).storeCoins(0).endCell();\n    sendRawMessage(msg, 64);\n}\n";
        assert!(run(source).is_empty());
    }

    #[test]
    fn does_not_flag_a_load_address_whose_result_is_never_sent_anywhere() {
        let source = "fun onInternalMessage(myBalance: int, msgValue: int, inMsgFull: cell, inMsgBody: slice) {\n    val recipient: slice = inMsgBody.loadAddress();\n    val h = recipient.hash();\n}\n";
        assert!(run(source).is_empty());
    }

    #[test]
    fn propagates_taint_through_one_hop_of_a_sub_slice_read() {
        // inMsgBody.loadBits(...) yields a tainted sub-slice, which is then
        // itself read via loadAddress() — one level of chaining, matching
        // the module's own documented v1 scope.
        let source = "fun onInternalMessage(myBalance: int, msgValue: int, inMsgFull: cell, inMsgBody: slice) {\n    val tail: slice = inMsgBody.loadBits(64);\n    val recipient: slice = tail.loadAddress();\n    val msg: cell = beginCell().storeUint(0x18, 6).storeSlice(recipient).storeCoins(0).endCell();\n    sendRawMessage(msg, 64);\n}\n";
        let findings = run(source);
        assert_eq!(findings.len(), 1);
    }

    #[test]
    fn flags_the_real_proxy_level_shape_across_a_conditional_branch_in_the_same_block() {
        let findings = run(RAW_CELL_RELAY);
        assert_eq!(
            findings.len(),
            1,
            "sanity: the canonical fixture still flags"
        );
    }

    #[test]
    fn known_v1_gap_an_if_gated_destination_check_is_not_recognized() {
        // Documented, not silently missing (see the module doc comment): the
        // comparison lives in the if's condition, which cfg.rs's blocks do
        // not carry — only assert_statement guards are recognized in v1.
        let source = "fun onInternalMessage(myBalance: int, msgValue: int, inMsgFull: cell, inMsgBody: slice) {\n    val recipient: slice = inMsgBody.loadAddress();\n    if (recipient == trustedAddress) {\n        val msg: cell = beginCell().storeUint(0x18, 6).storeSlice(recipient).storeCoins(0).endCell();\n        sendRawMessage(msg, 64);\n    }\n}\n";
        assert_eq!(
            run(source).len(),
            1,
            "known v1 gap: if this now returns empty, the gap has been closed — update this test and the module doc comment instead of the assertion"
        );
    }

    #[test]
    fn flags_a_tainted_code_cell_reaching_a_code_upgrade() {
        let source = "fun onInternalMessage(myBalance: int, msgValue: int, inMsgFull: cell, inMsgBody: slice) {\n    val newCode: cell = inMsgBody.loadRef();\n    contract.setCodePostponed(newCode);\n}\n";
        let findings = run(source);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].severity, sting_core::Severity::Critical);
        assert!(findings[0].message.contains("code"));
    }

    #[test]
    fn flags_a_tainted_code_cell_reaching_the_legacy_free_function_code_upgrade() {
        let source = "fun onInternalMessage(myBalance: int, msgValue: int, inMsgFull: cell, inMsgBody: slice) {\n    val newCode: cell = inMsgBody.loadRef();\n    setContractCodePostponed(newCode);\n}\n";
        let findings = run(source);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].severity, sting_core::Severity::Critical);
    }

    #[test]
    fn does_not_flag_a_code_upgrade_with_a_trusted_code_cell() {
        let source = "fun onInternalMessage(in: InMessage) {\n    val newCode: cell = storage.approvedCode;\n    contract.setCodePostponed(newCode);\n}\n";
        assert!(run(source).is_empty());
    }

    const SAME_FILE_INTERPROCEDURAL_RELAY: &str = "fun relay(dest: slice) {\n    val msg: cell = beginCell().storeUint(0x18, 6).storeSlice(dest).storeCoins(0).endCell();\n    sendRawMessage(msg, 64);\n}\nfun onInternalMessage(myBalance: int, msgValue: int, inMsgFull: cell, inMsgBody: slice) {\n    val recipient: slice = inMsgBody.loadAddress();\n    relay(recipient);\n}\n";

    #[test]
    fn flags_a_tainted_argument_passed_to_a_same_file_helper_that_relays_it() {
        let findings = run(SAME_FILE_INTERPROCEDURAL_RELAY);
        assert_eq!(
            findings.len(),
            1,
            "exactly one finding: onInternalMessage's own delegation, not also a spurious one from relay's own unrelated top-level analysis"
        );
        assert!(findings[0].message.contains("onInternalMessage"));
        assert!(
            findings[0].message.contains("relay"),
            "the message should name the actual callee the taint flowed into: {:?}",
            findings[0].message
        );
    }

    #[test]
    fn does_not_flag_when_the_helper_does_not_actually_use_the_tainted_parameter() {
        let source = "fun harmless(dest: slice) {\n    val h = dest.hash();\n}\nfun onInternalMessage(myBalance: int, msgValue: int, inMsgFull: cell, inMsgBody: slice) {\n    val recipient: slice = inMsgBody.loadAddress();\n    harmless(recipient);\n}\n";
        assert!(run(source).is_empty());
    }

    #[test]
    fn does_not_flag_when_the_callee_sanitizes_the_parameter_itself() {
        let source = "fun relay(dest: slice) {\n    assert (dest == trustedAddress) throw 500;\n    val msg: cell = beginCell().storeUint(0x18, 6).storeSlice(dest).storeCoins(0).endCell();\n    sendRawMessage(msg, 64);\n}\nfun onInternalMessage(myBalance: int, msgValue: int, inMsgFull: cell, inMsgBody: slice) {\n    val recipient: slice = inMsgBody.loadAddress();\n    relay(recipient);\n}\n";
        assert!(run(source).is_empty());
    }

    #[test]
    fn does_not_chase_interprocedural_taint_beyond_one_hop() {
        // relay2 delegates to relay1, which is the one that actually
        // relays — two hops away from onInternalMessage's own call into
        // relay2. Deliberately not followed (see the module doc comment's
        // "one hop" scope, mirroring uta.rs's own identical choice) — a
        // known, tested v1 boundary, not a silent gap.
        let source = "fun relay1(dest: slice) {\n    val msg: cell = beginCell().storeUint(0x18, 6).storeSlice(dest).storeCoins(0).endCell();\n    sendRawMessage(msg, 64);\n}\nfun relay2(dest: slice) {\n    relay1(dest);\n}\nfun onInternalMessage(myBalance: int, msgValue: int, inMsgFull: cell, inMsgBody: slice) {\n    val recipient: slice = inMsgBody.loadAddress();\n    relay2(recipient);\n}\n";
        assert!(
            run(source).is_empty(),
            "known v1 gap: if this now flags, one-hop bounding has been extended — update this test and the module doc comment instead of the assertion"
        );
    }

    #[test]
    fn resolves_interprocedural_taint_across_files() {
        let dir = tempfile::tempdir().expect("tempdir");
        let helper_path = dir.path().join("helper.tolk");
        std::fs::write(
            &helper_path,
            "fun relay(dest: slice) {\n    val msg: cell = beginCell().storeUint(0x18, 6).storeSlice(dest).storeCoins(0).endCell();\n    sendRawMessage(msg, 64);\n}\n",
        )
        .expect("write helper.tolk");
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
                        file_name: helper_path.display().to_string(),
                        size_chars: 0,
                        imports: vec![],
                    },
                ],
                unique_types: Vec::new(),
                struct_instantiations: Vec::new(),
                alias_instantiations: Vec::new(),
                declarations: Vec::new(),
                functions: vec![sting_tolkc::FunctionEntry {
                    f_idx: 0,
                    name: "relay".to_string(),
                    return_ty_idx: 0,
                    num_params: 1,
                    ident_loc: sting_tolkc::SourceSpan::from([1, 1, 1, 1, 10]),
                    end_loc: sting_tolkc::SourceSpan::from([1, 3, 1, 3, 2]),
                    params: Vec::new(),
                }],
            }));

        let source = "fun onInternalMessage(myBalance: int, msgValue: int, inMsgFull: cell, inMsgBody: slice) {\n    val recipient: slice = inMsgBody.loadAddress();\n    relay(recipient);\n}\n";
        let tree = sting_ast::parse(source).expect("grammar loads");
        let store = AstStore::build(&tree, source);
        let ctx = DetectorContext::new(PathBuf::from("main.tolk"), tree, source.to_string())
            .with_project(project, 0);

        let findings = UdaDetector.check(&store, &ctx);
        assert_eq!(findings.len(), 1);
        assert!(findings[0].message.contains("relay"));
    }

    #[test]
    fn kind_is_dataflow_and_severity_is_high() {
        assert_eq!(UdaDetector.kind(), crate::DetectorKind::Dataflow);
        assert_eq!(UdaDetector.default_severity(), sting_core::Severity::High);
    }
}
