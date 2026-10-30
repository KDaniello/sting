//! `LEP` — Lack End Parse (`docs/domains/detectors.md`).
//!
//! A function receives a `slice` parameter, reads from it, and never calls
//! `.assertEnd()` on it before returning — so a caller can attach unexpected
//! extra data the handler silently ignores instead of rejecting.
//!
//! Reading means advancing the read cursor, and not merely naming the
//! parameter ([`NON_ADVANCING_SLICE_METHODS`]). The distinction was not
//! anticipated: it came back from a run against a production stablecoin,
//! where 22 of 24 findings were guard helpers shaped like
//! `fun requireAdmin(self: slice, admin: slice)` whose only contact with the
//! parameter is a bits comparison. An address is not a parse stream, nothing
//! is consumed, and there is no tail to assert the end of — demanding one is
//! meaningless rather than merely unmet.
//! `DetectorKind::Ast` "with elements of a local CFG" per the doc: this
//! walks `if`/`match` nesting directly (matching every branch's outcome with
//! a plain `&&`) rather than building a `sting_dataflow::Cfg` and running
//! `WorklistSolver` — Tolk has no `goto`/labeled jumps, so structured
//! recursion is a complete substitute for a general fixpoint solver here,
//! and is simpler. Contrast `id`/`ubm`, which *do* need the generic solver
//! (`cfg.rs`) because their fact is an ordered event sequence /
//! may-reach-on-some-path question, not "does a boolean hold on every
//! path".
//!
//! v1 scope, deliberately narrow and verified against real code (see this
//! module's tests and the measurements in
//! `docs/domains/detectors.md`/`docs/domains/precision-sources.md`):
//! only `slice`-typed *parameters* are checked (not `.beginParse()` locals).
//! "Used"/"forwarded"/"`assertEnd`-called" tracking follows a small **alias
//! set**, not just the literal parameter name — `var cs = inMsg;` (a plain
//! identifier-to-identifier rebind, no method call involved) is common
//! enough in `FunC`-derived Tolk (`docs/domains/precision-sources.md`'s
//! `corpus/clean/converted/wallet3` is the canonical example: the classic
//! wallet-v3 reference contract aliases its whole external-message slice
//! into `cs` before parsing it) that tracking only the original name
//! produced a real, common, measured false positive across most of that
//! corpus once it existed to measure against. Extended to close exactly
//! that gap ([`collect_aliases`]) — a value derived via an actual *method
//! call* (`s.removeLastBits(n)` bound to a new variable, `WalletV5.tolk`'s
//! real `onExternalMessage`) is still **not** tracked back to `s`; that
//! remains a known, observed false positive, and a concrete target for a
//! real taint-tracking pass rather than guessed at here.
//!
//! `mutate`-declared `slice` parameters are skipped entirely (found during a
//! post-Block-09 manual corpus review, not anticipated in v1's original
//! scope): `fun T.unpackFromSlice(mutate s: slice): T` is pass-by-reference
//! specifically so the *caller* keeps reading `s` after the call returns —
//! every real instance of this shape in `corpus/clean/acton-contracts/`
//! consumes only its own struct field's worth of `s` by design, so demanding
//! `.assertEnd()` inside the callee would be wrong, not just unmet.
//!
//! A plain reassignment (`s = other;`, no `var`/`val`) to a name outside
//! `tracked` breaks that name's alias to the original slice from that
//! statement onward, in the same straight-line sequence
//! (`all_paths_call_assert_end`'s own `tracked` clone is mutated as it walks
//! statements in source order — see [`reassignment_breaking_alias`]). Found
//! during a post-Block-09 corpus review: `val x = s.loadUint(32); s = other;
//! s.assertEnd();` used to be treated as fully safe — the read on the
//! original slice was real, but the `.assertEnd()` moments later actually
//! closes `other`, not `s`. `s = anotherTrackedAlias;` (reassigning to a
//! name *already* known to alias the same slice) is deliberately **not**
//! treated as breaking, matching `collect_aliases`'s own "alias of a tracked
//! name is itself tracked" rule. **Known remaining gap, not covered by this
//! fix**: a reassignment inside one arm of an `if`/`match` does not
//! propagate its invalidation past that branch's own merge point (each
//! branch walks its own independent `tracked` clone) — see
//! `flags_a_reassignment_inside_a_branch_before_the_first_assert_end_it_should_only_partially_cover`'s
//! test for the concrete shape this does not yet close, kept alongside the
//! already-documented method-call-derived-alias gap above rather than
//! silently assumed fixed.
//!
//! **New gap found via real external validation** (`TONresistor/zk-resistor-contracts` — real, independent,
//! live-mainnet code, not vendored in `corpus/`): a function that validates a
//! tracked slice's *exact* remaining shape via `.remainingBitsCount()`/
//! `.remainingRefsCount()` comparisons (either as an upfront "reject if the
//! total isn't exactly N bits / M refs" guard before parsing, or as the
//! entire body of a pure shape-validator returning `bool`) never calls
//! `.assertEnd()` literally, but proves the same "no unexpected trailing
//! data" property this detector exists to check — confirmed twice
//! independently in this one codebase
//! (`Pool.isWellFormedDepositPayload`, `sparse-set.sparseSetIsCanonicalSiblingChunk`).
//! Not fixed here: recognizing this soundly needs to relate the checked
//! total back to what the rest of the function actually consumes (a real
//! width-tracking question, not a simple "was some `remainingBitsCount()`
//! call seen anywhere" heuristic, which would risk new false *negatives* on
//! this already historically weakest-precision detector — `docs/domains/detectors.md`).
//! Documented as an open, real, twice-confirmed gap rather than guessed at.

use std::collections::HashSet;

use sting_ast::{AstStore, Language};
use sting_core::{DetectorId, Finding, Severity};
use tree_sitter::Node;

use crate::support::{
    MethodCall, as_method_call, block_children, bound_names, function_body, identifier_text,
    match_arm_body,
};
use crate::{Detector, DetectorContext, DetectorKind};

pub(crate) struct LepDetector;

impl Detector for LepDetector {
    fn id(&self) -> DetectorId {
        "LEP"
    }

    fn kind(&self) -> DetectorKind {
        DetectorKind::Ast
    }

    /// `Info`, alongside `DEPR`:
    /// a fact about the code, not a claim of a defect.
    ///
    /// This was `Medium` and measured 34.9% precision, the worst number in
    /// the project. The manual review behind that number found something
    /// worse than noise: about 16 of the 28 false positives are
    /// **syntactically true** — `.assertEnd()` really is not called. Among
    /// them `wallet-v4`, one of the most widely deployed and most audited
    /// contracts on the network. The detector was right and the industry
    /// disagreed with it.
    ///
    /// So the question stopped being "how do we sharpen the heuristic" and
    /// became "is an unchecked slice tail a defect at all" — recorded as
    /// [Q3](../../../docs/OPEN-QUESTIONS.md) and answered **no**, on three
    /// independent grounds: `docs.ton.org`'s own list of 18 vulnerability
    /// classes has no such class; a study of 34 public professional audit
    /// reports covering 233 real vulnerabilities (arXiv 2509.10823) attributes
    /// 6 of them to parsing/serialization as a whole and never names this
    /// form — the strings `end_parse`, `assertEnd` and `slice` do not occur
    /// in that paper at all; and a search for a public TON incident rooted
    /// in an unchecked tail returned none.
    ///
    /// Reporting stays exactly as informative as it was. What changed is the
    /// claim attached to it. One public incident reopens Q3 and this line.
    fn default_severity(&self) -> Severity {
        Severity::Info
    }

    fn languages(&self) -> &'static [Language] {
        &[Language::Tolk, Language::Func]
    }

    fn check(&self, store: &AstStore, ctx: &DetectorContext) -> Vec<Finding> {
        let mut findings = Vec::new();
        let source = ctx.source();
        let dialect = Dialect::of(ctx.language());

        for function in store.functions() {
            let Some(body) = function_body(function, ctx) else {
                continue;
            };
            for param in &function.parameters {
                if param.type_text.as_deref() != Some("slice") {
                    continue;
                }
                if param.is_mutate {
                    // `mutate s: slice` is pass-by-reference specifically so
                    // the caller can keep reading `s` after this call
                    // returns — a cooperating sub-parser consuming *part* of
                    // a shared slice, not the sole/final consumer this
                    // detector's whole pattern assumes. Confirmed against
                    // real code, not a hypothetical: every
                    // `unpackFromSlice(mutate s: slice)`-shaped helper in
                    // `corpus/clean/acton-contracts/` (7 of them, across
                    // `config`/`dns`/`multisig-v2.1`/`tolk-bench/07_telemint`)
                    // was flagged before this exclusion existed — demanding
                    // `.assertEnd()` inside any of them would be actively
                    // wrong, since the whole point of `mutate` here is that
                    // there is deliberately more data left for the caller.
                    continue;
                }
                let tracked = collect_aliases(body, &param.name, source, &dialect);
                if returns_a_tracked_slice(body, &tracked, source, &dialect) {
                    // The caller gets the slice back and keeps reading it, so this function is a
                    // cooperating sub-parser rather than the last consumer — the same shape Tolk
                    // spells `mutate s: slice` and this detector already skips there.
                    continue;
                }
                let (used, forwarded) = usage(body, &tracked, source, &dialect);
                if !used || forwarded {
                    // Unused: nothing was read, so nothing was left
                    // unaccounted for. Forwarded: responsibility for ending
                    // the parse moved to whatever this slice (or a tracked
                    // alias of it) was passed to
                    // (`docs/domains/detectors.md`).
                    continue;
                }
                if all_paths_call_assert_end(&block_children(body), &tracked, source, &dialect) {
                    continue;
                }
                findings.push(Finding {
                    detector_id: self.id(),
                    severity: self.default_severity(),
                    message: format!(
                        "function `{}` reads `slice` parameter `{}` but does not call `.{}()` on every path — trailing data the caller attached would be silently ignored",
                        function.name, param.name, dialect.end_parse
                    ),
                    remediation: format!(
                        "call `{}.{}()` after the last read on every return path, including inside every branch",
                        param.name, dialect.end_parse
                    ),
                    location: ctx.location(function.range),
                    confidence: None,
                    dynamic_verification: None,
                    suggestion: None,
                });
            }
        }

        findings
    }
}

/// Grows `{param_name}` into the set of names known to alias it — see this
/// module's doc comment. A single forward pass over `body`: an alias is
/// only recognized once its own `var X = Y;`/`val X = Y;` declaration has
/// been visited, so it must be declared textually before use — true for
/// every real shape observed (`docs/domains/precision-sources.md`'s
/// corpus), and a deliberate simplification otherwise (matches this
/// detector's existing single-forward-pass approach to `usage`).
fn collect_aliases(
    body: Node,
    param_name: &str,
    source: &str,
    dialect: &Dialect,
) -> HashSet<String> {
    let mut tracked = HashSet::new();
    tracked.insert(param_name.to_string());
    if dialect.language == Language::Func {
        // A `FunC` alias is a known gap rather than an oversight: `slice cs = in_msg_body;` binds
        // through a `variable_declaration` whose initialiser is a sibling, not a child, and no file
        // in the measured scope aliases a parameter that way. Tracking a shape nothing uses would
        // be guessing; the day one appears it arrives as a false positive with a file behind it.
        return tracked;
    }
    for visited in sting_ast::Preorder::from_node(body) {
        let node = visited.node;
        if node.kind() != "var_declaration_lhs" {
            continue;
        }
        let Some(parent) = node.parent() else {
            continue;
        };
        if parent.kind() != "assignment"
            || parent.child_by_field_name("left").map(|l| l.id()) != Some(node.id())
        {
            continue;
        }
        let Some(right) = parent.child_by_field_name("right") else {
            continue;
        };
        let Some(alias_of) = identifier_text(right, source) else {
            continue;
        };
        if !tracked.contains(&alias_of) {
            continue;
        }
        if let [(_, name)] = bound_names(node, source).as_slice() {
            tracked.insert(name.clone());
        }
    }
    tracked
}

/// Whether any name in `tracked` is referenced anywhere in `body` (`used`),
/// and whether one is ever passed by identifier as a call argument to
/// another call (`forwarded`).
fn usage(body: Node, tracked: &HashSet<String>, source: &str, dialect: &Dialect) -> (bool, bool) {
    let mut used = false;
    let mut forwarded = false;
    for visited in sting_ast::Preorder::from_node(body) {
        let node = visited.node;
        if let Some(name) = identifier_text(node, source)
            && tracked.contains(&name)
            && dialect.is_an_argument(node)
            && !dialect.is_a_receiver(node)
        {
            forwarded = true;
            used = true;
            continue;
        }
        if let Some(call) = as_method_call(node, source, dialect.language)
            && let Some(name) = identifier_text(call.obj, source)
            && tracked.contains(&name)
            && dialect.advances(&call)
        {
            used = true;
        }
    }
    (used, forwarded)
}

/// What one grammar calls the things this detector reasons about.
///
/// The pattern does not change between languages: a slice parameter is read, and nobody says the
/// read consumed all of it. The words do. Keeping them in one value rather than branching at every
/// site is what stops the two languages from drifting into two detectors that agree by accident.
struct Dialect {
    /// Which grammar the tree came from, for unwinding a call.
    language: Language,
    /// The call that asserts a slice is spent.
    end_parse: &'static str,
    /// The field holding the taken branch of a conditional.
    consequent: &'static str,
}

impl Dialect {
    /// The dialect for `language`.
    const fn of(language: Language) -> Self {
        match language {
            Language::Tolk => Self {
                language,
                end_parse: "assertEnd",
                consequent: "body",
            },
            Language::Func => Self {
                language,
                end_parse: "end_parse",
                consequent: "consequent",
            },
        }
    }

    /// Whether `node` sits where a call's arguments go.
    ///
    /// Being there is necessary and not sufficient: in `FunC` a receiver and its call are spliced
    /// into whatever encloses them, so the receiver of `body~load_uint(32)` inside
    /// `throw_unless(701, body~load_uint(32) == 5)` sits in the argument group of `throw_unless`
    /// without being handed to it. The caller pairs this with [`Self::is_a_receiver`], which is the
    /// same distinction that already had to be drawn inside a `return`.
    ///
    /// Passing a slice on passes the duty of ending it, so a function that forwards one is not
    /// failing to close a parse it never owned. Each grammar spells the position differently and
    /// neither spelling reads the other: Tolk wraps every argument in a `call_argument` node, while
    /// `FunC` has none at all — its arguments are the named children of the parenthesised group the
    /// call carries. Asking for Tolk's node in a `FunC` tree finds nothing, and the whole class of
    /// forwarding entrypoints — which is how `elector-code.fc` and `config-code.fc` are written,
    /// end to end — reported as a defect.
    fn is_an_argument(&self, node: Node) -> bool {
        let Some(parent) = node.parent() else {
            return false;
        };
        match self.language {
            Language::Tolk => parent.kind() == "call_argument",
            Language::Func => parent
                .parent()
                .and_then(|call| call.child_by_field_name("arguments"))
                .is_some_and(|arguments| arguments.id() == parent.id()),
        }
    }

    /// Whether `node` is the thing a call is invoked on.
    ///
    /// A receiver inside a `return` is the difference between handing the slice back and handing
    /// back a number read out of it. `fun extractBouncedBody(body: slice): uint256` returns
    /// `body.loadUint(n)`: the caller gets an integer and the slice dies here, so the parse is this
    /// function's to end after all.
    fn is_a_receiver(&self, node: Node) -> bool {
        let Some(parent) = node.parent() else {
            return false;
        };
        match self.language {
            Language::Tolk => {
                parent.kind() == "dot_access"
                    && parent
                        .child_by_field_name("obj")
                        .is_some_and(|obj| obj.id() == node.id())
            }
            Language::Func => node
                .next_named_sibling()
                .is_some_and(|next| next.kind() == "method_call"),
        }
    }

    /// Whether a call leaves a tracked slice further along than it found it.
    ///
    /// The two languages put the answer in different places, and each is the cheapest place its own
    /// language offers. Tolk decides in the callee's declaration — `mutate self` — which a call site
    /// cannot see, so the answer is transcribed from the standard library: every `fun slice.*`
    /// declared without it is listed, and a method absent from the list counts as advancing.
    ///
    /// `FunC` decides at the call site, in one character: `~f(…)` assigns the call's first result
    /// back into the receiver, and the compiler refuses the form unless that result has the
    /// receiver's own type. `.f(…)` does not assign, and that is the exact and only claim made here
    /// — **the advance is not lost, it is returned**. `load_uint` is `(slice, int) -> (slice, int)`,
    /// so `s.load_uint(32)` evaluates to the advanced slice paired with the value, and a caller that
    /// writes `(slice rest, int x) = s.load_uint(32)` keeps it. What matters for this detector is
    /// `s` itself, which the dot form leaves exactly where it was: nothing was consumed out of the
    /// parameter, so nothing is missing from its tail.
    ///
    /// The sigil is not quite the whole story either — it also picks which function runs, `~f`
    /// before `f`, and the standard library ships both spellings of some names with different
    /// return types. That changes which declaration is called, not whether the receiver was
    /// advanced, which is all this asks.
    fn advances(&self, call: &MethodCall) -> bool {
        match self.language {
            Language::Func => call.receiver_written == Some(true),
            Language::Tolk => !NON_ADVANCING_SLICE_METHODS.contains(&call.method.as_str()),
        }
    }
}

/// Slice operations that provably cannot leave a tail behind, so a function whose only contact with
/// its parameter is one of these has nothing to assert the end of.
///
/// The list is not a guess about what sounds harmless. In Tolk a method advances the caller's read
/// cursor only when it takes `mutate self`, so this is every `fun slice.*` in the standard library
/// that does not — transcribed from `ton-blockchain/ton`'s `crypto/smartcont/tolk-stdlib/`, checked
/// 2026-08-23. `preloadUint` and its siblings are here for that reason and not by intuition: they
/// read data without consuming it, and `assertEnd` after a preload would fail on any slice that
/// carried anything at all.
///
/// Two entries are older spellings that no longer occur in the standard library: `isSliceBitsEqual`
/// and `isEndOfSlice`, the pre-rename names of `bitsEqual` and `isEmpty`. They are kept because
/// deployed contracts are written against the toolchain they were deployed with, and the code this
/// list was derived from uses both.
///
/// Anything absent from this list counts as advancing the cursor, including a method this analyser
/// has never seen. That direction is deliberate: a project's own `mutate self` parser — `ctxBody
/// .parseProvideCurrentQuote()` in real code — is unknown here, and treating an unknown method as
/// harmless would turn a true finding into silence.
const NON_ADVANCING_SLICE_METHODS: &[&str] = &[
    "assertEnd",
    "bitsEqual",
    "bitsHash",
    "calculateSize",
    "calculateSizeStrict",
    "depth",
    "getFirstBits",
    "getLastBits",
    "getMiddleBits",
    "hash",
    "isEmpty",
    "isEndOfBits",
    "isEndOfRefs",
    "isEndOfSlice",
    "isSliceBitsEqual",
    "preloadBits",
    "preloadDict",
    "preloadInt",
    "preloadMaybeRef",
    "preloadRef",
    "preloadUint",
    "remainingBitsAndRefsCount",
    "remainingBitsCount",
    "remainingRefsCount",
];

/// If `stmt` is a plain (non-`var`/`val`) reassignment `NAME = <rhs>;` whose
/// target `NAME` is currently in `tracked`, and `<rhs>` is **not** itself a
/// name already in `tracked` (which would just re-establish the same alias,
/// e.g. `s = anotherTrackedAlias;`), returns `Some(NAME)` — the name to stop
/// treating as an alias of the original slice from this point on. `None`
/// for anything else: not a plain reassignment (a `var`/`val` declaration
/// has `left.kind() == "var_declaration_lhs"`, handled by
/// [`collect_aliases`], not this function), the target isn't tracked to
/// begin with, or the alias survives the reassignment.
fn reassignment_breaking_alias(
    stmt: Node,
    tracked: &HashSet<String>,
    source: &str,
    dialect: &Dialect,
) -> Option<String> {
    if stmt.kind() != "expression_statement" {
        return None;
    }
    if dialect.language == Language::Func {
        return func_reassignment_breaking_alias(stmt, tracked, source);
    }
    let expr = stmt.named_child(0)?;
    if expr.kind() != "assignment" {
        return None;
    }
    let left = expr.child_by_field_name("left")?;
    if left.kind() == "var_declaration_lhs" {
        return None;
    }
    let name = identifier_text(left, source)?;
    if !tracked.contains(&name) {
        return None;
    }
    let right = expr.child_by_field_name("right")?;
    let rhs_still_aliased =
        identifier_text(right, source).is_some_and(|rhs_name| tracked.contains(&rhs_name));
    if rhs_still_aliased { None } else { Some(name) }
}

/// Whether `.assertEnd()` is called on a name in `tracked` on *every* path
/// through `stmts`, given the caller's already-accumulated `seen` state
/// going in. Walks `stmts` in source order against its own owned clone of
/// `tracked`, dropping a name the moment a plain reassignment breaks its
/// alias to the original slice (see [`reassignment_breaking_alias`] and this
/// module's doc comment) — a later `.assertEnd()` on that same name, further
/// down this same straight-line sequence, no longer counts.
fn all_paths_call_assert_end(
    stmts: &[Node],
    tracked: &HashSet<String>,
    source: &str,
    dialect: &Dialect,
) -> bool {
    let mut seen = false;
    let mut tracked = tracked.clone();
    for &stmt in stmts {
        if let Some(name) = reassignment_breaking_alias(stmt, &tracked, source, dialect) {
            tracked.remove(&name);
            continue;
        }
        match stmt.kind() {
            "if_statement" => {
                seen = every_branch_asserts(stmt, &tracked, source, dialect, seen);
            }
            "match_statement" => {
                seen = all_arms_call_assert_end(stmt, &tracked, source, seen, dialect);
            }
            "return_statement" | "throw_statement" => {
                return seen || statement_calls_assert_end(stmt, &tracked, source, dialect);
            }
            _ => {
                seen = seen || statement_calls_assert_end(stmt, &tracked, source, dialect);
            }
        }
    }
    seen
}

fn all_arms_call_assert_end(
    match_stmt: Node,
    tracked: &HashSet<String>,
    source: &str,
    seen_before: bool,
    dialect: &Dialect,
) -> bool {
    let Some(arms_body) = match_stmt
        .named_child(0)
        .and_then(|match_expr| match_expr.child_by_field_name("body"))
    else {
        return seen_before;
    };
    let mut cursor = arms_body.walk();
    let arms: Vec<Node> = arms_body
        .named_children(&mut cursor)
        .filter(|c| c.kind() == "match_arm")
        .collect();
    if arms.is_empty() {
        return seen_before;
    }

    let mut has_else = false;
    let mut all_arms_seen = true;
    for arm in arms {
        has_else |= arm.child_by_field_name("pattern_else").is_some();
        let arm_seen = match match_arm_body(arm) {
            Some(b) if b.kind() == "block_statement" => {
                seen_before
                    || all_paths_call_assert_end(&block_children(b), tracked, source, dialect)
            }
            Some(b) if b.kind() == "return_statement" || b.kind() == "throw_statement" => {
                seen_before || statement_calls_assert_end(b, tracked, source, dialect)
            }
            Some(b) => seen_before || statement_calls_assert_end(b, tracked, source, dialect),
            None => seen_before,
        };
        all_arms_seen &= arm_seen;
    }
    // No `else` arm: conservatively assume a "no arm matched" path may exist
    // and it carries forward whatever was true before the match, same as an
    // `if` with no `else` (see cfg.rs's identical reasoning for `id`/`ubm`).
    all_arms_seen && (has_else || seen_before)
}

/// The `FunC` spelling of the same statement.
///
/// There is no `assignment` node in this grammar: `_expr10` is hidden, so `s = other;` is an
/// `expression_statement` holding a name, an unnamed `=` and whatever is on the right. Reading it
/// through Tolk's node kinds found nothing at all, which meant a slice reassigned away from the
/// parameter kept its alias and an `end_parse` on the new binding read as closing the old one — a
/// missed finding rather than a spurious one, which is the worse of the two.
fn func_reassignment_breaking_alias(
    stmt: Node,
    tracked: &HashSet<String>,
    source: &str,
) -> Option<String> {
    let target = stmt.named_child(0)?;
    let name = identifier_text(target, source)?;
    if !tracked.contains(&name) {
        return None;
    }
    let mut cursor = stmt.walk();
    let assigns = stmt
        .children(&mut cursor)
        .any(|child| !child.is_named() && child.utf8_text(source.as_bytes()) == Ok("="));
    if !assigns {
        return None;
    }
    // Re-pointing one tracked alias at another leaves the slice tracked, exactly as in Tolk.
    let rhs = stmt.named_child(1)?;
    if identifier_text(rhs, source).is_some_and(|other| tracked.contains(&other)) {
        return None;
    }
    Some(name)
}

/// Whether every way out of a conditional asserts the end of a tracked slice.
///
/// One node can hold a whole chain. `FunC` splices `elseif` into the statement it belongs to rather
/// than nesting it, so `if … elseif … else` is a single `if_statement` carrying two `consequent`
/// fields — the grammar says so, `consequent` is declared `multiple` — and reading only the first
/// walked past the middle branch without looking into it. A branch nobody looked into is a branch
/// that asserts nothing, and reporting it as covered is the failure this detector exists to avoid,
/// in the direction that looks safe.
///
/// A conditional with no `else` at all always leaves a way through that took no branch, so it can
/// never establish the claim on its own.
fn every_branch_asserts(
    stmt: Node,
    tracked: &HashSet<String>,
    source: &str,
    dialect: &Dialect,
    seen_before: bool,
) -> bool {
    if seen_before {
        // The claim was already established before the conditional was reached, and no branch of it
        // can take that back.
        return true;
    }
    let mut cursor = stmt.walk();
    let mut branches: Vec<Node> = stmt
        .children_by_field_name(dialect.consequent, &mut cursor)
        .filter(|node| node.kind() == "block_statement")
        .collect();
    let mut alternatives = stmt.walk();
    let otherwise: Vec<Node> = stmt
        .children_by_field_name("alternative", &mut alternatives)
        .filter(|node| matches!(node.kind(), "block_statement" | "if_statement"))
        .collect();
    if otherwise.is_empty() {
        return false;
    }
    branches.extend(otherwise);
    branches.iter().all(|branch| {
        if branch.kind() == "if_statement" {
            all_paths_call_assert_end(std::slice::from_ref(branch), tracked, source, dialect)
        } else {
            all_paths_call_assert_end(&block_children(*branch), tracked, source, dialect)
        }
    })
}

/// Whether the function hands a tracked slice back to its caller.
///
/// `FunC` has no `mutate` keyword. Its cooperating sub-parser is a function that takes a slice,
/// consumes part of it, and returns it alongside what it read — `(slice, (int, int))
/// ~load_op_and_query_id(slice s)` is the shape, and the standard library beside every wallet in
/// the ecosystem is written in it. Demanding that such a function end the parse would be demanding
/// that it eat the data its caller is waiting for.
///
/// Being handed on as an argument is a different thing and is already `forwarded`, and a receiver
/// is a third: `fun extractBouncedBody(body: slice): uint256` returns a number read out of the
/// slice, so the parse was this function's to end after all.
fn returns_a_tracked_slice(
    body: Node,
    tracked: &HashSet<String>,
    source: &str,
    dialect: &Dialect,
) -> bool {
    sting_ast::Preorder::from_node(body)
        .map(|visited| visited.node)
        .filter(|node| node.kind() == "return_statement")
        .any(|statement| {
            sting_ast::Preorder::from_node(statement)
                .map(|visited| visited.node)
                .any(|node| {
                    identifier_text(node, source).is_some_and(|name| tracked.contains(&name))
                        && !dialect.is_an_argument(node)
                        && !dialect.is_a_receiver(node)
                })
        })
}

fn statement_calls_assert_end(
    stmt: Node,
    tracked: &HashSet<String>,
    source: &str,
    dialect: &Dialect,
) -> bool {
    sting_ast::Preorder::from_node(stmt)
        .map(|v| v.node)
        .filter_map(|n| as_method_call(n, source, dialect.language))
        .any(|call| {
            call.method == dialect.end_parse
                && identifier_text(call.obj, source).is_some_and(|name| tracked.contains(&name))
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

    use sting_ast::{AstStore, Language};

    use super::LepDetector;
    use crate::{Detector, DetectorContext};

    fn run(source: &str) -> Vec<sting_core::Finding> {
        let tree = sting_ast::parse(source).expect("grammar loads");
        let store = AstStore::build(&tree, source);
        let ctx = DetectorContext::new(PathBuf::from("contract.tolk"), tree, source.to_string());
        LepDetector.check(&store, &ctx)
    }

    /// The same detector against a `FunC` tree.
    fn run_func(source: &str) -> Vec<sting_core::Finding> {
        let tree = sting_ast::parse_as(Language::Func, source).expect("grammar loads");
        let store = AstStore::build_as(Language::Func, &tree, source);
        let ctx = DetectorContext::new(PathBuf::from("contract.fc"), tree, source.to_string())
            .in_language(Language::Func);
        LepDetector.check(&store, &ctx)
    }

    /// The pattern in its second spelling: a body is read and its end is never asserted.
    #[test]
    fn flags_a_func_slice_parameter_read_but_never_ended() {
        let source =
            "() recv_internal(slice in_msg_body) {\n  int op = in_msg_body~load_uint(32);\n}\n";
        let findings = run_func(source);
        assert_eq!(findings.len(), 1);
        assert!(
            findings[0].message.contains("end_parse"),
            "{:?}",
            findings[0]
        );
    }

    /// `end_parse()` is what `assertEnd()` is, and saying so closes the finding.
    #[test]
    fn does_not_flag_a_func_slice_parameter_that_is_ended() {
        let source = "() recv_internal(slice in_msg_body) {\n  int op = in_msg_body~load_uint(32);\n  in_msg_body.end_parse();\n}\n";
        assert!(run_func(source).is_empty());
    }

    /// `FunC` answers "does this advance the cursor" at the call site, in one character: `~` writes
    /// the result back into the receiver and `.` does not. A preload consumes nothing, so there is
    /// no tail to assert the end of.
    #[test]
    fn does_not_flag_a_func_slice_parameter_that_is_only_preloaded() {
        let source = "() f(slice s) {\n  int x = s.preload_uint(32);\n}\n";
        assert!(run_func(source).is_empty());
    }

    /// A guard that only compares a slice consumes nothing, exactly as in Tolk — the rule is the
    /// same rule, read off the grammar instead of off a list.
    #[test]
    fn does_not_flag_a_func_slice_parameter_that_is_only_compared() {
        let source = "() require_owner(slice sender, slice owner) {\n  throw_unless(101, equal_slices(sender, owner));\n}\n";
        assert!(run_func(source).is_empty());
    }

    /// One branch ending the parse is not every branch ending it.
    #[test]
    fn flags_a_func_slice_ended_on_only_one_branch() {
        let source = "() f(slice s, int cond) {\n  int x = s~load_uint(32);\n  if (cond) {\n    s.end_parse();\n  }\n}\n";
        assert_eq!(run_func(source).len(), 1);
    }

    /// Both branches ending it is every branch ending it, and the `consequent` field is where
    /// `FunC` keeps the taken one.
    #[test]
    fn does_not_flag_a_func_slice_ended_on_both_branches() {
        let source = "() f(slice s, int cond) {\n  int x = s~load_uint(32);\n  if (cond) {\n    s.end_parse();\n  } else {\n    s.end_parse();\n  }\n}\n";
        assert!(run_func(source).is_empty());
    }

    /// `FunC` has no `mutate` keyword. Its cooperating sub-parser takes a slice, consumes part of it
    /// and hands it back, and the standard library beside every wallet in the ecosystem is written
    /// that way. Demanding it end the parse would demand it eat what its caller is waiting for.
    #[test]
    fn does_not_flag_a_func_slice_the_function_hands_back() {
        let source = "(slice, (int, int)) ~load_op_and_query_id(slice s) inline {
  int op = s~load_uint(32);
  int query_id = s~load_uint(64);
  return (s, (op, query_id));
}
";
        assert!(run_func(source).is_empty());
    }

    /// Returning a number read out of a slice is not returning the slice: the caller gets an
    /// integer, the slice dies here, and ending the parse was this function's job after all. The
    /// shape is real — `extractBouncedBody` in two vendored production contracts — and a rule that
    /// silenced it would have been a rule about receivers rather than about return values.
    #[test]
    fn flags_a_slice_whose_read_value_is_returned_rather_than_the_slice() {
        let source = "fun extractBouncedBody(body: slice): int {
    return body.loadUint(32);
}";
        assert_eq!(run(source).len(), 1);
        let func = "int extract(slice body) {
  return body~load_uint(32);
}
";
        assert_eq!(run_func(func).len(), 1);
    }

    /// A receiver sits inside the argument group of whatever encloses it, because `FunC` splices a
    /// call and its receiver into the surrounding expression. Counting that as handing the slice
    /// away silenced the most common opcode-dispatch line the language has.
    #[test]
    fn flags_a_func_slice_read_inside_another_calls_arguments() {
        let source = "() recv_internal(slice in_msg_body) {
  throw_unless(701, in_msg_body~load_uint(32) == 5);
}
";
        assert_eq!(run_func(source).len(), 1);
    }

    /// `elseif` is spliced into the statement it belongs to rather than nested, so one node carries
    /// several `consequent` fields. Reading only the first walked past the middle branch and called
    /// it covered.
    #[test]
    fn flags_a_func_elseif_branch_that_does_not_end_the_parse() {
        let source = "() f(slice s, int a, int b) {
  int op = s~load_uint(32);
  if (a) { s.end_parse(); }
  elseif (b) { }
  else { s.end_parse(); }
}
";
        assert_eq!(run_func(source).len(), 1);
    }

    /// Re-pointing the name at another slice ends the alias: the `end_parse` below closes the new
    /// binding, not the parameter that was read. Tolk has asserted this for a while; `FunC` spells
    /// the assignment without a node of its own, so the rule had to be written twice.
    #[test]
    fn flags_a_func_slice_reassigned_away_before_the_end_parse() {
        let source = "() f(slice s, cell c) {
  int op = s~load_uint(32);
  s = c.begin_parse();
  s.end_parse();
}
";
        assert_eq!(run_func(source).len(), 1);
    }

    /// A detector says which languages it reads, so a run can hold back the ones that have no rule
    /// rather than let their silence read as a clean file.
    #[test]
    fn the_detector_declares_both_languages() {
        assert_eq!(
            LepDetector.languages(),
            &[Language::Tolk, Language::Func][..]
        );
    }

    #[test]
    fn flags_a_slice_param_read_but_never_assert_ended() {
        let source = "fun f(s: slice) {\n    val x = s.loadUint(32);\n    return;\n}";
        let findings = run(source);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].detector_id, "LEP");
        assert!(findings[0].message.contains("function `f`"));
    }

    #[test]
    fn does_not_flag_a_mutate_slice_parameter() {
        // Real corpus finding, not a hypothetical: `mutate s: slice` is a
        // cooperating sub-parser's by-reference parameter — the caller reads
        // more of `s` after this call returns, so this function is never
        // meant to be the sole/final consumer `.assertEnd()` would require.
        // `corpus/clean/acton-contracts/multisig-v2.1/types.tolk`'s
        // `unpackFromSlice` is the real shape this mirrors.
        let source = "fun T.unpackFromSlice(mutate s: slice): T {\n    return s.loadRef();\n}";
        assert!(run(source).is_empty());
    }

    #[test]
    fn does_not_flag_when_assert_end_is_called_before_returning() {
        let source = "fun f(s: slice) {\n    val x = s.loadUint(32);\n    s.assertEnd();\n}";
        assert!(run(source).is_empty());
    }

    /// A guard helper compares an address and consumes nothing, so there is no tail to assert the
    /// end of. The shape that this rule came from: 22 of 24 findings against a production
    /// stablecoin were this and nothing else.
    #[test]
    fn does_not_flag_a_slice_parameter_that_is_only_compared() {
        let source = "fun requireAdmin(self: slice, admin: slice) {\n    assert(self.isSliceBitsEqual(admin), 1);\n}";
        assert!(run(source).is_empty());
    }

    /// `preload` reads without consuming, and `assertEnd` after one would fail on any slice that
    /// carried anything — so it cannot be what the detector asks for.
    #[test]
    fn does_not_flag_a_slice_parameter_that_is_only_preloaded() {
        let source = "fun f(s: slice) {\n    val x = s.preloadUint(32);\n}";
        assert!(run(source).is_empty());
    }

    /// A method this analyser has never seen counts as advancing the cursor. A project's own
    /// `mutate self` parser is exactly that, and treating it as harmless would turn a true finding
    /// into silence.
    #[test]
    fn flags_a_slice_parameter_read_by_a_method_it_does_not_know() {
        let source = "fun f(s: slice) {\n    val x = s.parseSomethingCustom();\n}";
        assert_eq!(run(source).len(), 1);
    }

    #[test]
    fn does_not_flag_an_unused_slice_parameter() {
        let source = "fun f(s: slice) {\n    return;\n}";
        assert!(run(source).is_empty());
    }

    #[test]
    fn does_not_flag_when_the_slice_is_forwarded_to_another_call() {
        let source = "fun f(s: slice) {\n    return g(s);\n}";
        assert!(run(source).is_empty());
    }

    #[test]
    fn flags_when_one_branch_of_an_if_forgets_assert_end() {
        let source = "fun f(s: slice, cond: bool) {\n    val x = s.loadUint(32);\n    if (cond) {\n        s.assertEnd();\n    }\n}";
        let findings = run(source);
        assert_eq!(
            findings.len(),
            1,
            "only one of the two paths out of the if calls assertEnd"
        );
    }

    #[test]
    fn does_not_flag_when_both_branches_of_an_if_call_assert_end() {
        let source = "fun f(s: slice, cond: bool) {\n    val x = s.loadUint(32);\n    if (cond) {\n        s.assertEnd();\n    } else {\n        s.assertEnd();\n    }\n}";
        assert!(run(source).is_empty());
    }

    #[test]
    fn does_not_flag_when_assert_end_is_called_before_the_if_and_both_branches_are_plain() {
        let source = "fun f(s: slice, cond: bool) {\n    val x = s.loadUint(32);\n    s.assertEnd();\n    if (cond) {\n        val y = 1;\n    }\n}";
        assert!(run(source).is_empty());
    }

    #[test]
    fn flags_when_a_match_arm_is_missing_assert_end() {
        let source = "fun f(s: slice, x: int) {\n    val a = s.loadUint(32);\n    match (x) {\n        1 => { s.assertEnd(); }\n        else => { return; }\n    }\n}";
        let findings = run(source);
        assert_eq!(findings.len(), 1);
    }

    #[test]
    fn does_not_flag_when_every_match_arm_calls_assert_end() {
        let source = "fun f(s: slice, x: int) {\n    val a = s.loadUint(32);\n    match (x) {\n        1 => { s.assertEnd(); }\n        else => { s.assertEnd(); }\n    }\n}";
        assert!(run(source).is_empty());
    }

    #[test]
    fn does_not_flag_when_assert_end_is_called_via_a_plain_alias() {
        // corpus/clean/converted/wallet3's real onExternalMessage shape
        // (docs/domains/precision-sources.md):
        // `var cs = inMsg;` is a plain identifier rebind, not a method-call
        // derivation — tracked as an alias of the original parameter.
        let source = "fun f(inMsg: slice) {\n    var cs = inMsg;\n    val x = cs.loadUint(32);\n    cs.assertEnd();\n}";
        assert!(run(source).is_empty());
    }

    #[test]
    fn flags_when_assert_end_is_missing_even_via_an_alias() {
        let source = "fun f(inMsg: slice) {\n    var cs = inMsg;\n    val x = cs.loadUint(32);\n}";
        let findings = run(source);
        assert_eq!(findings.len(), 1);
    }

    #[test]
    fn does_not_flag_when_an_alias_is_forwarded_to_another_call() {
        let source = "fun f(inMsg: slice) {\n    var cs = inMsg;\n    return g(cs);\n}";
        assert!(run(source).is_empty());
    }

    #[test]
    fn flags_when_assert_end_is_called_on_a_slice_reassigned_away_from_the_original() {
        // Real bug this fix closes: `s.loadUint(32)` genuinely reads the
        // original parameter slice, but `s = other;` then rebinds `s` to a
        // completely unrelated value before `.assertEnd()` is called — the
        // assertEnd closes `other`, not the slice that was actually read
        // from, so this must still be flagged. `other` is a local (not a
        // second `slice` parameter of its own) specifically so this test
        // isolates the reassignment fix from LEP's separate, unrelated
        // per-parameter check.
        let source = "fun f(s: slice) {\n    val other = someOtherSlice();\n    val x = s.loadUint(32);\n    s = other;\n    s.assertEnd();\n}";
        let findings = run(source);
        assert_eq!(
            findings.len(),
            1,
            "assertEnd on a slice reassigned away from the original must not count"
        );
        assert!(findings[0].message.contains("`s`"));
    }

    #[test]
    fn does_not_flag_when_reassigned_to_another_tracked_alias_of_the_same_slice() {
        // `s = other;` only breaks the alias when `other` is *not* already
        // known to refer to the same underlying slice — reassigning to an
        // existing tracked alias (e.g. one `collect_aliases` already added
        // via `var other = s;`) is not a break, `.assertEnd()` afterwards
        // still genuinely closes the original slice.
        let source = "fun f(s: slice) {\n    var other = s;\n    val x = s.loadUint(32);\n    s = other;\n    s.assertEnd();\n}";
        assert!(run(source).is_empty());
    }

    #[test]
    fn flags_a_reassignment_inside_a_branch_before_the_first_assert_end_it_should_only_partially_cover()
     {
        // Known remaining gap, documented in this module's doc comment, not
        // silently assumed fixed: a reassignment inside one arm of an `if`
        // does not propagate its invalidation past that branch's own merge
        // point (each branch walks its own independent `tracked` clone). On
        // the `cond == true` path here, `.assertEnd()` after the `if`
        // actually closes `other`, not `s` — this *should* still be
        // flagged, but currently is not, because the reassignment inside
        // the `if`'s body is invisible to the code after it. Locked in as a
        // known limitation, the same way `does_not_track_an_alias_derived_via_a_method_call`
        // above locks in the method-call-derived-alias gap.
        let source = "fun f(s: slice, cond: bool) {\n    val other = someOtherSlice();\n    val x = s.loadUint(32);\n    if (cond) {\n        s = other;\n    }\n    s.assertEnd();\n}";
        assert!(
            run(source).is_empty(),
            "known gap: cross-branch reassignment invalidation is not yet tracked — if this now fails, the gap has been closed, update this test and the module doc comment instead of the assertion"
        );
    }

    #[test]
    fn does_not_track_an_alias_derived_via_a_method_call() {
        // The still-open gap this module's doc comment names explicitly:
        // `s.removeLastBits(n)` is a *derivation*, not a plain rebind, and
        // stays untracked — WalletV5.tolk's real onExternalMessage
        // (docs/domains/detectors.md) is exactly
        // this shape.
        let source = "fun f(s: slice) {\n    var rest = s.removeLastBits(64);\n    val x = rest.loadUint(32);\n    rest.assertEnd();\n}";
        let findings = run(source);
        assert_eq!(
            findings.len(),
            1,
            "assertEnd on the method-derived alias is not tracked back to s"
        );
    }

    #[test]
    fn ignores_a_non_slice_parameter() {
        let source = "fun f(x: int) {\n    return;\n}";
        assert!(run(source).is_empty());
    }

    #[test]
    fn kind_is_ast_and_severity_is_info() {
        assert_eq!(LepDetector.kind(), crate::DetectorKind::Ast);
        // `Info`, not `Medium` — see `default_severity`'s own
        // doc comment for the three grounds behind the downgrade.
        assert_eq!(LepDetector.default_severity(), sting_core::Severity::Info);
        assert!(
            LepDetector.is_default_enabled(),
            "C moves LEP to the reference category next to DEPR, which is on by default —              it is B that turns a detector off, and LEP did not take B"
        );
    }
}
