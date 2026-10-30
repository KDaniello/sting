//! `AC` — Access Control (sting's own
//! addition past the `TONScanner` taxonomy, alongside `TLB`). Flags a path
//! from `onInternalMessage`'s entry to a storage-mutating `.save()` call
//! (directly, or through a same-file free-function/method call) with no
//! recognized sender/payment/signature check anywhere on that path.
//!
//! **Scope is deliberately narrower than every other detector in this
//! taxonomy** — a real-corpus research pass (`corpus/clean/acton-contracts/`,
//! 16 real contracts with an `onInternalMessage`) found two structural facts
//! that rule out a high-precision v1 the way `PL`/`BR`/`UR` had:
//!
//! 1. **The sender-check idiom is not one shape, but a family of at least a
//!    dozen** — direct `assert (in.senderAddress == storage.field) throw
//!    ...;` (`jetton-v2.1/JettonWallet.tolk:102`), a reusable helper
//!    (`assertSenderIsAdmin`, `jetton-v2.1/JettonMinter.tolk:38-47`),
//!    map/set-membership (`multisig-v2.1/Multisig.tolk:87-95`),
//!    signature-validity in place of a sender check entirely
//!    (`w5.2/WalletV5.tolk:106-108`), and a payment-threshold check standing
//!    in for authorization on intentionally-public paths
//!    (`elector/complaints.tolk:34`). No single AST pattern covers all of
//!    these — this module recognizes the family via a small, real-corpus-
//!    derived vocabulary of marker substrings ([`AUTH_MARKERS`]), not a
//!    single structural match, and documents that as a heuristic, not a
//!    soundness argument.
//! 2. **A field named `owner`/`admin` is neither necessary nor sufficient**
//!    for a contract to have privileged operations —
//!    `multisig-v2.1/storage.tolk`, `w5.2/storage.tolk`, and
//!    `highload-v3.1/storage.tolk` all gate real admin-shaped operations
//!    with no such field (signature/set-membership instead), while
//!    `counter/types.tolk` has none and is correctly, intentionally public.
//!    No field-name heuristic is used here at all.
//!
//! **What this module deliberately does NOT attempt** (documented
//! boundaries, not silent gaps):
//! - Anything outside `onInternalMessage` — `onExternalMessage`'s
//!   authorization model is signature-based from the start, a different
//!   shape not unified with this module's sender/payment vocabulary.
//! - **Cross-file *method* calls, still fully unresolved** — the narrower
//!   *free-function* half of this gap is closed, see [`classify_function`]'s
//!   own doc comment: `check_statement` only ever follows a delegated call
//!   through [`classify_function`] for a bare-identifier callee; a method
//!   call (`<obj>.<method>(...)`) is checked only against the fixed
//!   [`DATA_MUTATION_METHODS`]/[`CODE_MUTATION_METHOD`] vocabulary and never
//!   delegated to, same-file or cross-file. `multisig-v2.1/Order.tolk`'s own
//!   `storage.approve(...)` call into `order-helpers.tolk`'s
//!   `OrderStorageInitialized.approve` is a real, confirmed instance —
//!   verified directly against the real file this same round, not a guess.
//!   Extending delegation to methods needs its own real-corpus calibration
//!   pass (a concrete false-positive risk was already found by hand in this
//!   exact file: `assert (foundSigner) throw ...;`'s `foundSigner` comes
//!   from tuple-destructuring, a shape [`bool_alias_assignment`] does not
//!   recognize today), so it is deliberately left out of cross-file
//!   resolution.
//! - Distinguishing "intentionally public, no check needed" for a function
//!   with **no** check of any kind (`counter/Counter.tolk`) from a genuinely
//!   forgotten check — research found no static signal that tells these
//!   apart; this shape will read as a finding either way, calibrated against
//!   the real corpus like every other detector in this taxonomy, not
//!   silently suppressed.
//!
//! **Gap found via real external validation, not this project's own
//! corpus** (`TONresistor/zk-resistor-contracts` — a real, independent,
//! live-mainnet codebase, not
//! vendored anywhere in `corpus/`): [`AUTH_MARKERS`]'s `"senderAddress"`
//! literal-substring check misses a real, sound check when the caller
//! extracts `in.senderAddress` into a plain, differently-named parameter
//! before calling a same-file helper — e.g. `handleTakeWalletAddress(sender:
//! address, msg: TakeWalletAddress) { assert (sender == identity.jettonMaster)
//! throw ...; ... }`, called as `handleTakeWalletAddress(in.senderAddress,
//! msg)`. The check is real and correct; `"sender"` alone is not in
//! [`AUTH_MARKERS`] because it is far more likely to collide with unrelated
//! identifiers (`senderNonce`, a comment, ...) than the more specific
//! `"senderAddress"` — confirmed twice independently in this one codebase
//! (`Pool.handleTakeWalletAddress`, `Factory.handlePoolDeployConfirmed`).
//! This specific *cross-call, parameter-renaming* shape is still open, not
//! yet confirmed anywhere outside this one codebase — fixing it needs
//! tracking which parameter position a delegated call's `in.senderAddress`
//! argument lands on, a materially bigger interprocedural change than the
//! next paragraph's fix, and one real source alone does not clear this
//! project's own `docs/domains/detectors.md` bar.
//!
//! **Fixed this round — a related but distinct, now three-source-confirmed
//! gap**: a *local* variable (not a delegated call's parameter) bound from a
//! recognized sender-address source and later compared bare, with no
//! `"senderAddress"` substring anywhere near the comparison itself. Two
//! independent shapes, both real: `val sender = in.senderAddress;` (found in
//! `zk-resistor-contracts` alongside the delegated-call gap above — the
//! *local-variable* half of that codebase's pattern, distinct from the
//! cross-call half that is still open) and, confirmed in a third, unrelated
//! codebase (`tonkeeper/ton-subscriptions`'s `nativeSub.tolk`), the classic
//! low-level `onInternalMessage(myBalance: int, msgValue: int, msgFull:
//! cell, msgBody: slice)` signature's own idiom for reading the source
//! address — `var cs = msgFull.beginParse(); cs.loadMessageFlags(); val
//! sender = cs.loadAddress();` — which has no `InMessage`/`senderAddress` in
//! scope at all. [`sender_alias_assignment`] recognizes both shapes (a
//! `"senderAddress"`-containing right-hand side, or a bare
//! `<slice>.loadAddress()` call) and records the bound name; unlike
//! [`AUTH_MARKERS`]'s raw substring match, [`is_authorizing_condition`]
//! checks a sender alias by exact identifier equality against the
//! condition's own identifiers, not substring containment — so this does
//! not reopen the `"sender"`-collides-with-`"senderNonce"` risk the module
//! doc above already rejected for [`AUTH_MARKERS`] itself.
//!
//! **New gap found this same batch audit, still open** — one real instance,
//! not yet independently confirmed elsewhere:
//! `tonkeeper/ton-subscriptions`'s `nativeSub.tolk`'s `Message_Destruct` arm
//! guards its trailing `storage.save()` with an *exhaustive* `if (sender ==
//! storage.beneficiary) { ... } else if (sender == storage.wallet) { ... }
//! else { throw ...; }` chain, then falls through to the mutation
//! afterwards — every path through the chain either matches one of the two
//! disjuncts or throws, so reaching the mutation does prove `sender` was
//! checked, but this module's positive-guard handling in
//! [`handle_if_statement`] only propagates `authorized` into a single `if`'s
//! own `then`, not past a whole `if`/`else if`/`else`-throw chain the way
//! idiom #5 (a single `if` with a nested `assert`, no `else`) already does.
//! Generalizing idiom #5's "reaching past means authorized" handling to an
//! `else if` chain whose final `else` unconditionally exits would fix this,
//! but one source does not clear this project's own methodology bar either
//! — left open, tracked here rather than fixed speculatively.
//!
//! **Recall gap found this round, not a precision gap** — everything above
//! is about *false positives* (over-flagging). This one is the opposite and
//! more serious: this module recognized exactly one spelling of "this
//! mutates persistent storage", the method call `.save()` — but `.save()`
//! is itself just a thin, conventional wrapper around the real primitive,
//! `contract.setData(cell)` (confirmed present, directly or via a `.save()`
//! wrapper, in **every** `storage.tolk`/equivalent file in this whole
//! project's corpus — not a guess). A contract written in the lower-level
//! `global`-variable + free-function `loadData()`/`saveData()` style (no
//! `struct`, no `lazy`, no `.save()` at all — an older but still-live real
//! idiom, confirmed on `hacktheton.com`'s CTF levels) was **completely invisible** to this detector: `sting lint`
//! found zero findings across all seven of that site's real, intentionally
//! vulnerable Tolk levels, including one (`tolk_level`) whose entire bug
//! *is* an unguarded `saveData()` call one line long. [`DATA_MUTATION_METHODS`]
//! now also recognizes `.setData()` directly (not just `.save()`), so a
//! same-file free function that calls it (`saveData()`) is caught exactly
//! the same way an unguarded helper already was via [`classify_function`] —
//! no special-casing of the name `saveData` needed, the existing delegated-
//! call machinery already generalizes. [`CODE_MUTATION_METHOD`]
//! (`.setCodePostponed()`, the *other* primitive `contract` exposes,
//! replacing the contract's own code) gets its own, more severe finding
//! ([`code_upgrade_finding`], `Critical` not `Medium`) — confirmed as its
//! own real, distinct bug on the same site's `upgrade_level`, an
//! unauthenticated full code takeover, not a storage-field write.
//! [`LEGACY_DATA_MUTATION_FUNCTIONS`]/[`LEGACY_CODE_MUTATION_FUNCTIONS`]
//! additionally recognize the same two primitives' pre-1.0 Tolk names
//! (`setContractData`/`setContractCodePostponed`, plain free functions, no
//! `contract.` receiver) — confirmed still the literal spelling used by
//! `hacktheton.com`'s actual, currently-live level source (predating the
//! `contract.` namespace's introduction), so this fix closes the finding on
//! that real site's contracts as actually written today, not just on a
//! hypothetical modern equivalent.
//!
//! **New gap found this same batch audit, still open, needs new
//! infrastructure not just a wider vocabulary**: `hacktheton.com`'s
//! `proxy_level` relays an arbitrary message — attacker-controlled
//! destination address *and* body, read straight from `inMsgBody` — with
//! only a state check (`assert(ctxEnabled, 501)`), no sender check at all.
//! This is not a `.save()`/`.setData()`/`.setCodePostponed()` shape at
//! all — the vulnerable path never mutates persistent storage or code, it
//! only calls `sendRawMessage(...)` with a destination this module has no
//! concept of "attacker-controlled" for. Recognizing it would need genuine
//! taint-tracking (does the address argument to a send trace back to
//! `inMsgBody.loadAddress()`), not an extension of the method/function-name
//! vocabulary [`DATA_MUTATION_METHODS`]/[`CODE_MUTATION_METHOD`] already
//! are — a materially bigger, structurally different piece of
//! infrastructure this module does not have. One real source; left open,
//! not built speculatively (has the
//! full writeup, including three more real, real patterns from the same
//! batch — predictable public-storage-seeded randomness, a public-getter
//! time-equality gate, and untrusted-continuation execution — that are
//! thematically closer to `BR`/a wholly new detector than to `AC`, and are
//! tracked there instead of here).
//!
//! `DetectorKind::Ast` "with elements of local CFG" (`ur.rs`'s own
//! precedent): recursive descent over block/branch/match structure within
//! one function, tracking a single `authorized: bool` fact (not a per-name
//! set like `UR` — `AC` asks one whole-path question, "was *any* recognized
//! check seen on the way here", not "which of several lookups is confirmed
//! found"), plus a one-hop-or-more same-file call-following step
//! ([`classify_function`], memoized, cycle-guarded) for the
//! dispatch-delegates-to-a-helper shape confirmed real in
//! `elector/Elector.tolk`.
//!
//! **Two precision fixes, direct follow-up to the batch audit above, both
//! confirmed on the real motivating target (`Skiba111/orbit-ton`), not
//! built speculatively**:
//!
//! 1. **[`AUTH_MARKERS`] now matches case/underscore-insensitively**
//!    ([`normalize_for_marker_match`]) — `orbit-ton` is written entirely in
//!    FunC-inherited `snake_case` (`msg_value`, `sender`, not `msgValue`,
//!    `sender`), so a literal `"msgValue"` substring check structurally
//!    could not see `assert(msg_value > 5000000, ...)`-style payment-threshold
//!    guards no matter how real they were. Closed exactly 2 real findings on
//!    `orbit-ton` (`factory.tolk:438`, `registry.tolk:261`, both genuine
//!    `msg_value`-threshold guards), zero new false positives, and this
//!    workspace's own corpus improved too (33→32 FP) — not a regression
//!    risk, a normalization bug fix, not new vocabulary.
//! 2. **A condition can now resolve a same-file/cross-file
//!    *boolean-returning* helper call**
//!    ([`condition_calls_a_boolean_auth_helper`],
//!    [`FnProfile::is_boolean_auth_check`]) — `if (isOwner(sender)) {...}`,
//!    a different, common shape from the bare-statement void-delegate
//!    `check_statement` already resolved (`isOwner(sender);` as its own
//!    statement, ignoring the return value, still correctly does **not**
//!    count — see `is_boolean_auth_check`'s own doc comment for why
//!    conflating the two would be a real bug, not just a missed case).
//!    **Honestly, this did not close any additional finding on
//!    `orbit-ton`** — that codebase's actual `require_owner(sender,
//!    service_addr)` pattern goes through *two more* levels of indirection
//!    (`require_owner` calls `assert(addr_equal(sender, owner), ...)`,
//!    `addr_equal` itself calls `extract_account_id` twice and compares the
//!    results — neither `addr_equal` nor `extract_account_id` ever mentions
//!    any recognizable marker text, at any depth), which is not closable by
//!    a textual-marker heuristic at any reasonable recursion depth without
//!    real type-aware semantic analysis this module does not have and is
//!    not attempting to grow into. This fix is real and confirmed correct
//!    by its own unit tests (a genuine, common idiom — a boolean auth-check
//!    helper used inside a condition), just not yet demonstrated as a
//!    corpus-precision win the way fix 1 is — an honest distinction, not
//!    glossed over.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use sting_ast::{AstStore, FunctionDef, Language};
use sting_core::{DetectorId, Finding, Severity};
use tree_sitter::Node;

use crate::support::{
    CrossFileCache, as_method_call, block_children, bound_names, function_body, identifier_text,
    is_simple_exit_block, location_of, match_arm_body, unwrap_parens,
};
use crate::{Detector, DetectorContext, DetectorKind};

/// Substrings whose presence in a condition's raw text marks it as
/// authorizing — either an identity check (`senderAddress`, a signature
/// validity call) or a payment-threshold gate standing in for one on a
/// path this corpus confirms is intentionally public
/// (`elector/complaints.tolk:34`, `dns/DnsCollection.tolk:35`). A textual
/// vocabulary, not a structural pattern, because this module's own doc
/// comment's point 1 found no single AST shape covers the real idiom family
/// — every marker here is a literal identifier/field name confirmed present
/// in this workspace's real corpus, not a guess.
const AUTH_MARKERS: [&str; 4] = [
    "senderAddress",
    "isSignatureValid",
    "valueCoins",
    "msgValue",
];

/// Strips everything but ASCII letters/digits and lowercases — used so
/// [`AUTH_MARKERS`] recognizes a marker regardless of whether the source
/// spells it in Tolk's own camelCase convention or FunC-inherited
/// `snake_case` (`msgValue` vs `msg_value`). A real, confirmed gap, not a
/// speculative fix:
/// found `Skiba111/orbit-ton` — an entirely `snake_case`-styled real
/// contract — using `assert(msg_value >= REGISTRY_MIN_VALUE, ...)` as a
/// genuine payment-threshold guard, structurally invisible to a literal
/// `"msgValue"` substring check. Does not change the underlying
/// substring-containment precision trade-off [`AUTH_MARKERS`]'s own doc
/// comment already accepts (a longer identifier that happens to contain a
/// marker as a sub-string was already possible before this normalization;
/// removing case/underscores from both sides does not introduce a new kind
/// of false-positive risk).
fn normalize_for_marker_match(text: &str) -> String {
    text.chars()
        .filter(char::is_ascii_alphanumeric)
        .map(|c| c.to_ascii_lowercase())
        .collect()
}

/// Method names recognized as an immediate, universal *data*-persistence
/// mutation: the conventional `.save()` wrapper, and the real primitive it
/// wraps, `contract.setData(cell)` — recognized directly so a contract that
/// never defines a `.save()`-named method at all (the `global`-variable +
/// free-function idiom, see this module's doc comment) is not invisible to
/// this detector.
const DATA_MUTATION_METHODS: [&str; 2] = ["save", "setData"];

/// The other real primitive `contract` exposes: replacing the contract's
/// own code. Kept separate from [`DATA_MUTATION_METHODS`] because an
/// unguarded call is categorically more severe (full code takeover, not a
/// storage-field write) and gets its own, `Critical`-severity finding
/// ([`code_upgrade_finding`]).
const CODE_MUTATION_METHOD: &str = "setCodePostponed";

/// Pre-1.0 Tolk API names for the same two primitives above, as plain free
/// functions with no `contract.` receiver — not in current stdlib docs, but
/// confirmed as the literal, still-live spelling `hacktheton.com`'s CTF
/// levels use (see this module's doc comment). Cheap to also recognize
/// since real, currently-live vulnerable code uses them.
const LEGACY_DATA_MUTATION_FUNCTIONS: [&str; 1] = ["setContractData"];
const LEGACY_CODE_MUTATION_FUNCTIONS: [&str; 1] = ["setContractCodePostponed"];

pub(crate) struct AcDetector;

impl Detector for AcDetector {
    fn id(&self) -> DetectorId {
        "AC"
    }

    fn kind(&self) -> DetectorKind {
        DetectorKind::Ast
    }

    fn default_severity(&self) -> Severity {
        Severity::Medium
    }

    /// Off unless `Sting.toml` names `"AC"` in `detectors.enable`.
    ///
    /// The measurement behind it, not an impression: 409 storage-mutation
    /// sites in 89 real contracts were labeled by hand ("must be guarded" vs
    /// "deliberately public"), and this detector's firings were crossed
    /// against that labeling. 52% land on sites that are open to anyone by
    /// design — a public bid, a permissionless finalization after expiry, a
    /// counter with no owner concept at all, a mint whose authorization is
    /// economic rather than by identity. None of the four separating
    /// hypotheses named in the plan before the work started survived the
    /// held-out half: the best reached 33% precision against a threshold of
    /// 80%.
    ///
    /// The other 48% are recognition gaps, and those keep getting fixed —
    /// the `try`-block gap ([`handle_unconditional_wrapper`]) was one of
    /// them and removed 22 real-world false positives. But
    /// closing every last one still leaves the half that no static signal
    /// separates, so the honest default is off.
    fn is_default_enabled(&self) -> bool {
        false
    }

    /// Analyzes **every** free function named `onInternalMessage`, not just
    /// the first one the store happens to hold.
    ///
    /// A single Tolk file declares one entrypoint, so on the real input shape
    /// — `sting lint` over a directory, one file at a time — this is exactly
    /// the previous behaviour. It differs on the *benchmark's* input, which
    /// concatenates a project's files into one text: 11 of the corpus's
    /// projects define two or three entrypoints between them (`jetton-v2.1`
    /// is a minter plus a wallet, `dns` is three contracts), and
    /// [`AstStore::function_by_name`] returns the first, so this detector was
    /// measured against one contract per project and was structurally blind
    /// to the rest.
    ///
    /// That is a measurement bug, not a detector one — it made `AC` look
    /// quieter on the corpus than it is on the same code linted normally.
    /// Found by an experiment that expected the
    /// opposite: reverting the `try` fix left every benchmark number
    /// identical, which it could not have done if the newly vendored
    /// `nominator-pool` (three entrypoints) were being analyzed past the
    /// first.
    fn check(&self, store: &AstStore, ctx: &DetectorContext) -> Vec<Finding> {
        let mut findings = Vec::new();
        let entrypoints = store
            .functions()
            .filter(|f| f.name == "onInternalMessage" && f.receiver_type.is_none());

        for function in entrypoints {
            let Some(body) = function_body(function, ctx) else {
                continue;
            };
            let env = Env {
                source: ctx.source(),
                ctx,
                store,
                function_name: &function.name,
            };
            let mut cache = HashMap::new();
            let mut visiting = HashSet::new();
            let mut cross_file = CrossFileCache::default();
            walk_block(
                &block_children(body),
                false,
                &Aliases::default(),
                &env,
                &mut cache,
                &mut visiting,
                &mut cross_file,
                &mut findings,
            );
        }
        findings
    }
}

/// Identifies one function for [`FnProfile`] memoization/cycle-detection —
/// the file it is defined in, plus its name. Same-file calls all share one
/// file (the file being linted), so this behaves exactly like the old
/// bare-`String`-name key did for every pre-existing test; a cross-file call
/// across files resolves into a *different* file, which needs its own key
/// space so a same-named function in two different files (a common shape:
/// every `storage.tolk` in this corpus defines its own `.save()`) is never
/// conflated with, or mistaken for a cycle back to, an unrelated same-named
/// function elsewhere.
#[derive(Clone, PartialEq, Eq, Hash)]
struct CallKey {
    file: PathBuf,
    name: String,
}

/// The two kinds of name aliases this module tracks while walking a
/// function body, bundled so every recursive-descent function threads one
/// parameter instead of two. `bool_aliases` (plain booleans meaning "an
/// authorizing condition held", the `Config.tolk:24`/`:74` shape) and
/// `sender_aliases` (identifiers holding a sender-address value, this
/// module's newest fix — see the module doc comment's "Fixed this round"
/// section) use different matching rules in [`is_authorizing_condition`]
/// (bare-identifier equality vs. any-identifier-in-subtree), so they are
/// kept as separate sets rather than merged into one.
#[derive(Clone, Default)]
struct Aliases {
    bool_aliases: HashSet<String>,
    sender_aliases: HashSet<String>,
}

/// Bundles the parts of a walk that never change across one function's
/// recursive descent — same shape as `ur.rs`'s `Env`.
struct Env<'a> {
    source: &'a str,
    ctx: &'a DetectorContext,
    store: &'a AstStore,
    function_name: &'a str,
}

/// A same-file function's call-graph-relevant profile, memoized by name in
/// [`AcDetector::check`]'s `cache`.
#[derive(Clone, Copy, Debug, Default)]
struct FnProfile {
    /// A path exists from this function's entry to a `.save()` call (direct
    /// or via a same-file callee with its own `has_unguarded_mutation`) with
    /// no recognized check first.
    has_unguarded_mutation: bool,
    /// This function's whole body never mutates storage at all, and does
    /// contain at least one recognized check — calling it and returning
    /// normally (rather than throwing) implies that check passed, matching
    /// the real `assertSenderIsAdmin`-style helper idiom
    /// (`jetton-v2.1/JettonMinter.tolk:38-47`). Only ever safe to use for a
    /// **bare-statement** call — see `is_boolean_auth_check`'s own doc
    /// comment for why a call *inside a condition* needs a different
    /// property entirely, not this one.
    is_check_only: bool,
    /// This function's whole body is exactly one `return <expr>;` where
    /// `<expr>` is itself an authorizing condition — a boolean-returning
    /// helper idiom (`fun isOwner(sender: address): bool { return
    /// senderAddress == storage.ownerAddress; }`), used only when such a
    /// function is called *inside* a condition
    /// ([`condition_calls_a_boolean_auth_helper`]). **Deliberately not
    /// merged with `is_check_only`**: that field means "calling this and
    /// not having it throw proves the check held", true only for a void
    /// assert-style guard — a bare *statement* call to a boolean-returning
    /// function whose result is discarded proves nothing at all (it never
    /// throws), so conflating the two would be a real correctness bug in
    /// `check_statement`'s bare-statement delegation, not just a missed
    /// case.
    is_boolean_auth_check: bool,
}

/// Walks `stmts` in source order, threading `authorized` (has a recognized
/// check been seen on the current path) and `bool_aliases` (plain boolean
/// variable names known to mean "an authorizing condition held" — the
/// `Config.tolk:24`/`:74` shape, a flag computed before the dispatch `match`
/// and consumed inside one specific arm's guard, several statements later).
/// Returns whether *any* path through `stmts` reached an unguarded mutation
/// — used both to emit findings (top-level call from `check`) and, via
/// [`classify_function`], to answer "does calling this function risk an
/// unguarded mutation" for a same-file caller.
#[allow(clippy::too_many_arguments)]
fn walk_block(
    stmts: &[Node],
    authorized_in: bool,
    aliases_in: &Aliases,
    env: &Env,
    cache: &mut HashMap<CallKey, FnProfile>,
    visiting: &mut HashSet<CallKey>,
    cross_file: &mut CrossFileCache,
    findings: &mut Vec<Finding>,
) -> bool {
    let mut authorized = authorized_in;
    let mut aliases = aliases_in.clone();
    let mut found = false;
    for &stmt in stmts {
        // Sender-alias checked first: `is_sender_source`'s own text check
        // (`"senderAddress"` substring, or a `.loadAddress()` call) would
        // otherwise also satisfy `bool_alias_assignment`'s looser
        // `is_authorizing_condition` text check (which does not require the
        // right-hand side to actually be boolean-typed), incorrectly
        // filing an address-valued binding like `val sender =
        // in.senderAddress;` as a bare-boolean alias instead.
        if let Some(alias) = sender_alias_assignment(stmt, env.source) {
            aliases.sender_aliases.insert(alias);
            continue;
        }
        if let Some(alias) = bool_alias_assignment(stmt, env, cache, visiting, cross_file) {
            aliases.bool_aliases.insert(alias);
            continue;
        }
        if let Some(alias) = tuple_found_alias_assignment(stmt, env.source) {
            aliases.bool_aliases.insert(alias);
            continue;
        }
        match stmt.kind() {
            "match_statement" => {
                found |= walk_match_arms(
                    stmt, authorized, &aliases, env, cache, visiting, cross_file, findings,
                );
            }
            "assert_statement" => {
                if let Some(condition) = stmt.child_by_field_name("condition")
                    && is_authorizing_condition(
                        condition, &aliases, env, cache, visiting, cross_file,
                    )
                {
                    authorized = true;
                }
            }
            "while_statement" => {
                if let (Some(condition), Some(body)) = (
                    stmt.child_by_field_name("condition"),
                    stmt.child_by_field_name("body"),
                ) {
                    let loop_authorized = authorized
                        || is_authorizing_condition(
                            condition, &aliases, env, cache, visiting, cross_file,
                        );
                    found |= walk_block(
                        &block_children(body),
                        loop_authorized,
                        &aliases,
                        env,
                        cache,
                        visiting,
                        cross_file,
                        findings,
                    );
                }
            }
            "do_while_statement" => {
                // Same reasoning as `ur.rs`'s do-while handling: the
                // condition is checked *after* the body on every iteration,
                // so it cannot gate entry — recurse with the current state
                // unchanged.
                if let Some(body) = stmt.child_by_field_name("body") {
                    found |= walk_block(
                        &block_children(body),
                        authorized,
                        &aliases,
                        env,
                        cache,
                        visiting,
                        cross_file,
                        findings,
                    );
                }
            }
            "if_statement" => {
                let (stmt_found, authorized_after) = handle_if_statement(
                    stmt, authorized, &aliases, env, cache, visiting, cross_file, findings,
                );
                found |= stmt_found;
                authorized = authorized_after;
            }
            "block_statement" | "repeat_statement" | "try_catch_statement" => {
                let (stmt_found, authorized_after) = handle_unconditional_wrapper(
                    stmt, authorized, &aliases, env, cache, visiting, cross_file, findings,
                );
                found |= stmt_found;
                authorized = authorized_after;
            }
            _ => {
                let (stmt_found, grants) =
                    check_statement(stmt, authorized, env, cache, visiting, cross_file, findings);
                found |= stmt_found;
                if grants {
                    authorized = true;
                }
            }
        }
    }
    found
}

/// Handles a statement that wraps a block without gating entry to it — a
/// bare nested scope, `repeat`, or `try`/`catch`. Before this existed,
/// `walk_block` had no arm for these and they fell through to
/// [`check_statement`], which finds the mutations inside but never sees the
/// `assert`s inside as `assert_statement`s. The consequence was not a missed
/// corner case: a handler whose whole body is wrapped in `try { ... } catch`
/// had **every** check inside it ignored and **every** mutation inside it
/// reported. Measured on the labelled real-world set, this one
/// gap produced 11 of `AC`'s 52 false positives on already-authorized code,
/// all of them the same shape (`webdom-contracts`' offer/sale handlers wrap
/// their body in `try` and assert the sender inside it).
///
/// `ur.rs` received exactly this fix a round earlier; `AC` never did.
///
/// Returns `(found_unguarded_mutation, authorized_after)`. Authorization
/// propagates back out **only** for a bare `block_statement`: `repeat`'s body
/// may run zero times, and an `assert` inside a `try` that fails transfers
/// control to `catch` rather than past the whole statement — so in neither
/// case does reaching the code *after* the wrapper prove the check held.
#[allow(clippy::too_many_arguments)]
fn handle_unconditional_wrapper(
    stmt: Node,
    authorized: bool,
    aliases: &Aliases,
    env: &Env,
    cache: &mut HashMap<CallKey, FnProfile>,
    visiting: &mut HashSet<CallKey>,
    cross_file: &mut CrossFileCache,
    findings: &mut Vec<Finding>,
) -> (bool, bool) {
    let blocks: Vec<Node> = if stmt.kind() == "block_statement" {
        vec![stmt]
    } else {
        stmt.named_children(&mut stmt.walk())
            .filter(|c| c.kind() == "block_statement")
            .collect()
    };

    let mut found = false;
    let mut authorized_after = authorized;
    for block in blocks {
        let children = block_children(block);
        found |= walk_block(
            &children, authorized, aliases, env, cache, visiting, cross_file, findings,
        );
        if stmt.kind() == "block_statement" {
            authorized_after |=
                block_grants_authorization(&children, aliases, env, cache, visiting, cross_file);
        }
    }
    (found, authorized_after)
}

/// Whether a bare nested scope contains a top-level authorizing `assert` —
/// the only reason [`handle_unconditional_wrapper`] propagates authorization
/// outward. Deliberately shallow: a check nested one level deeper is inside
/// some other statement whose own arm already decides what it proves.
fn block_grants_authorization(
    stmts: &[Node],
    aliases: &Aliases,
    env: &Env,
    cache: &mut HashMap<CallKey, FnProfile>,
    visiting: &mut HashSet<CallKey>,
    cross_file: &mut CrossFileCache,
) -> bool {
    stmts.iter().any(|stmt| {
        stmt.kind() == "assert_statement"
            && stmt.child_by_field_name("condition").is_some_and(|c| {
                is_authorizing_condition(c, aliases, env, cache, visiting, cross_file)
            })
    })
}

/// Handles one `if_statement`, split out of `walk_block` to keep that
/// function under clippy's line-count limit. Returns `(found_unguarded_
/// mutation, authorized_after)` — the second is what `walk_block` threads
/// into the rest of its own statement loop.
#[allow(clippy::too_many_arguments)]
fn handle_if_statement(
    stmt: Node,
    authorized: bool,
    aliases: &Aliases,
    env: &Env,
    cache: &mut HashMap<CallKey, FnProfile>,
    visiting: &mut HashSet<CallKey>,
    cross_file: &mut CrossFileCache,
    findings: &mut Vec<Finding>,
) -> (bool, bool) {
    let Some(condition) = stmt.child_by_field_name("condition") else {
        return (false, authorized);
    };
    let then_body = stmt.child_by_field_name("body");
    let alternative = stmt.child_by_field_name("alternative");
    let is_auth = is_authorizing_condition(condition, aliases, env, cache, visiting, cross_file);
    let then_is_exit = then_body.is_some_and(is_simple_exit_block);
    let then_is_check_only = match then_body {
        Some(b) => {
            !body_contains_mutation(b, env.source)
                && body_contains_authorizing_check(b, env, cache, visiting, cross_file)
        }
        None => false,
    };
    let mut found = false;

    if is_auth && (then_is_exit || then_is_check_only) {
        // Either a bail-out guard (`if (<auth marker>) { return/throw; }` —
        // the overwhelmingly common real shape,
        // `highload-v3.1/HighloadWalletV3.tolk:22-24`) or idiom #5 from
        // `ac.rs`'s own doc comment (`if (sender != trustedA) { assert
        // (sender == trustedB) throw ...; }` with no `else`, `then` purely a
        // nested check — `jetton-v2.1/JettonWallet.tolk:41-50`,
        // `jetton-v1.3/NotcoinWallet.tolk:40-49`). Both converge on the same
        // shape: reaching past the `if` means the authorizing condition
        // held one way or another, so this propagates `authorized` past the
        // `if`, not just inside `then`. The marker vocabulary carries no
        // polarity information, so this assumes (as every real instance
        // found does) that reaching past means authorization.
        if let Some(b) = then_body {
            found |= walk_block(
                &block_children(b),
                authorized,
                aliases,
                env,
                cache,
                visiting,
                cross_file,
                findings,
            );
        }
        let alt_authorized = authorized;
        if let Some(alt) = alternative {
            found |= walk_alternative(
                alt,
                alt_authorized,
                aliases,
                env,
                cache,
                visiting,
                cross_file,
                findings,
            );
        }
        return (found, true);
    }

    if is_auth {
        // Positive guard: `if (<auth marker>) { <privileged> }` — the
        // `then` branch only runs once the marker condition held.
        if let Some(b) = then_body {
            found |= walk_block(
                &block_children(b),
                true,
                aliases,
                env,
                cache,
                visiting,
                cross_file,
                findings,
            );
        }
        if let Some(alt) = alternative {
            found |= walk_alternative(
                alt, authorized, aliases, env, cache, visiting, cross_file, findings,
            );
        }
        return (found, authorized);
    }

    if let Some(b) = then_body {
        found |= walk_block(
            &block_children(b),
            authorized,
            aliases,
            env,
            cache,
            visiting,
            cross_file,
            findings,
        );
    }
    if let Some(alt) = alternative {
        found |= walk_alternative(
            alt, authorized, aliases, env, cache, visiting, cross_file, findings,
        );
    }
    (found, authorized)
}

#[allow(clippy::too_many_arguments)]
fn walk_alternative(
    alt: Node,
    authorized: bool,
    aliases: &Aliases,
    env: &Env,
    cache: &mut HashMap<CallKey, FnProfile>,
    visiting: &mut HashSet<CallKey>,
    cross_file: &mut CrossFileCache,
    findings: &mut Vec<Finding>,
) -> bool {
    if alt.kind() == "if_statement" {
        walk_block(
            std::slice::from_ref(&alt),
            authorized,
            aliases,
            env,
            cache,
            visiting,
            cross_file,
            findings,
        )
    } else {
        walk_block(
            &block_children(alt),
            authorized,
            aliases,
            env,
            cache,
            visiting,
            cross_file,
            findings,
        )
    }
}

/// Walks every arm of `match_stmt` independently, each starting from the
/// state as of entering the `match` (mirrors `ur.rs`'s `walk_match_arms`) —
/// the universal `onInternalMessage` op-code-dispatch shape means a guard
/// inside one arm must not leak into a sibling arm.
#[allow(clippy::too_many_arguments)]
fn walk_match_arms(
    match_stmt: Node,
    authorized: bool,
    aliases: &Aliases,
    env: &Env,
    cache: &mut HashMap<CallKey, FnProfile>,
    visiting: &mut HashSet<CallKey>,
    cross_file: &mut CrossFileCache,
    findings: &mut Vec<Finding>,
) -> bool {
    let Some(arms_body) = match_stmt
        .named_child(0)
        .and_then(|match_expr| match_expr.child_by_field_name("body"))
    else {
        return false;
    };
    let mut cursor = arms_body.walk();
    let arms: Vec<Node> = arms_body
        .named_children(&mut cursor)
        .filter(|c| c.kind() == "match_arm")
        .collect();
    let mut found = false;
    for arm in arms {
        let Some(body) = match_arm_body(arm) else {
            continue;
        };
        found |= if body.kind() == "block_statement" {
            walk_block(
                &block_children(body),
                authorized,
                aliases,
                env,
                cache,
                visiting,
                cross_file,
                findings,
            )
        } else {
            walk_block(
                std::slice::from_ref(&body),
                authorized,
                aliases,
                env,
                cache,
                visiting,
                cross_file,
                findings,
            )
        };
    }
    found
}

/// Scans one non-control-flow statement's whole subtree for two things: a
/// `.save()` call (the recognized universal persistence-mutation idiom,
/// `fun <Name>.save(self) { contract.setData(self.toCell()); }`), flagged
/// when `!authorized`; and a call to a same-file free function/method
/// (resolved via [`sting_ast::AstStore::function_by_name`]), whose own
/// [`FnProfile`] is looked up via [`classify_function`] — a delegated call
/// to a function with `has_unguarded_mutation` is flagged the same way a
/// direct `.save()` would be, and a call to an `is_check_only` helper grants
/// `authorized` to the caller for the rest of its own block (the
/// `assertSenderIsAdmin(...)` idiom). Returns `(found_unguarded_mutation,
/// grants_authorization)`.
#[allow(clippy::too_many_arguments)]
fn check_statement(
    stmt: Node,
    authorized: bool,
    env: &Env,
    cache: &mut HashMap<CallKey, FnProfile>,
    visiting: &mut HashSet<CallKey>,
    cross_file: &mut CrossFileCache,
    findings: &mut Vec<Finding>,
) -> (bool, bool) {
    let mut found = false;
    let mut grants = false;
    for visited in sting_ast::Preorder::from_node(stmt) {
        let node = visited.node;
        if let Some(call) = as_method_call(node, env.source, Language::Tolk) {
            if DATA_MUTATION_METHODS.contains(&call.method.as_str()) && !authorized {
                found = true;
                findings.push(mutation_finding(node, &format!(".{}()", call.method), env));
            } else if call.method == CODE_MUTATION_METHOD && !authorized {
                found = true;
                findings.push(code_upgrade_finding(
                    node,
                    &format!(".{}()", call.method),
                    env,
                ));
            } else {
                // Method delegation: `<obj>.<method>` where `<method>` is
                // neither of the two fixed vocabulary entries above — try
                // resolving it as a receiver-typed function's own body, the
                // same way a bare free-function call already does via
                // `classify_function`. See `classify_method`'s own doc
                // comment for why this cannot know the receiver's actual
                // type and what that means for precision.
                let profile = classify_method(&call.method, env, cache, visiting, cross_file);
                if profile.has_unguarded_mutation && !authorized {
                    found = true;
                    findings.push(delegated_finding(node, &call.method, env));
                }
                if profile.is_check_only {
                    grants = true;
                }
            }
            continue;
        }
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
        if LEGACY_DATA_MUTATION_FUNCTIONS.contains(&name.as_str()) {
            if !authorized {
                found = true;
                findings.push(mutation_finding(node, &format!("{name}()"), env));
            }
            continue;
        }
        if LEGACY_CODE_MUTATION_FUNCTIONS.contains(&name.as_str()) {
            if !authorized {
                found = true;
                findings.push(code_upgrade_finding(node, &format!("{name}()"), env));
            }
            continue;
        }
        // No same-file-only gate here: a name not
        // found in `env.store` may still resolve cross-file via a compiled
        // `ProjectStore` — `classify_function` itself now decides that, and
        // degrades to today's exact `FnProfile::default()` when no project
        // is attached, or the name resolves nowhere at all either way.
        let profile = classify_function(&name, env, cache, visiting, cross_file);
        if profile.has_unguarded_mutation && !authorized {
            found = true;
            findings.push(delegated_finding(node, &name, env));
        }
        if profile.is_check_only {
            grants = true;
        }
    }
    (found, grants)
}

/// Resolves `name`'s [`FnProfile`], memoized in `cache` and guarded against
/// call cycles via `visiting` — same-file first (unchanged v1 behavior), and
/// then cross-file via [`classify_cross_file_function`] on a
/// same-file miss. Either an unresolvable name (nowhere same-file, no
/// project attached, or resolves nowhere in one either) or a cycle defaults
/// to `FnProfile::default()` — "safe, not check-only", the same
/// under-report-not-over-report choice this module's doc comment states for
/// cross-file calls generally.
fn classify_function(
    name: &str,
    env: &Env,
    cache: &mut HashMap<CallKey, FnProfile>,
    visiting: &mut HashSet<CallKey>,
    cross_file: &mut CrossFileCache,
) -> FnProfile {
    let key = CallKey {
        file: env.ctx.file().to_path_buf(),
        name: name.to_string(),
    };
    if let Some(profile) = cache.get(&key) {
        return *profile;
    }
    // A same-file cycle is caught precisely, by the exact key repeating in
    // `visiting`. A *cross-file* cycle through a re-entrant import chain
    // could in principle compute a `CallKey` that narrowly misses matching
    // an already-visiting one (two different-looking `PathBuf`s for what is
    // really the same file — `tolk`'s own reported path vs. whatever form
    // `sting-pipeline` passed in), which would defeat that precise guard;
    // this depth cap is the unconditional backstop that keeps recursion
    // termination from depending on path-string equality holding perfectly
    // (`docs/ARCHITECTURE.md` §9: detectors must not hang on unusual input).
    // 64 is far past any real call chain this module's whole corpus has
    // ever shown — this module's own doc comment: real delegation is 1-2
    // hops deep.
    if visiting.contains(&key) || visiting.len() > 64 {
        return FnProfile::default();
    }
    visiting.insert(key.clone());

    let profile = match env.store.function_by_name(name) {
        Some(function) => classify_same_file_hit(function, env, cache, visiting, cross_file),
        None => {
            classify_cross_file_function(name, env, cache, visiting, cross_file).unwrap_or_default()
        }
    };

    visiting.remove(&key);
    cache.insert(key, profile);
    profile
}

/// The same-file half of [`classify_function`]: `function` is `name` as
/// found in `env`'s own [`sting_ast::AstStore`] — unchanged v1 walk, just
/// now also threading `cross_file` through in case *this* function's own
/// body itself calls something that only resolves cross-file.
fn classify_same_file_hit(
    function: &sting_ast::FunctionDef,
    env: &Env,
    cache: &mut HashMap<CallKey, FnProfile>,
    visiting: &mut HashSet<CallKey>,
    cross_file: &mut CrossFileCache,
) -> FnProfile {
    let Some(body) = function_body(function, env.ctx) else {
        return FnProfile::default();
    };
    let mut scratch = Vec::new();
    let has_unguarded_mutation = walk_block(
        &block_children(body),
        false,
        &Aliases::default(),
        env,
        cache,
        visiting,
        cross_file,
        &mut scratch,
    );
    let is_check_only = !body_contains_mutation(body, env.source)
        && body_contains_authorizing_check(body, env, cache, visiting, cross_file);
    let is_boolean_auth_check =
        body_returns_an_authorizing_comparison(body, env, cache, visiting, cross_file);
    FnProfile {
        has_unguarded_mutation,
        is_check_only,
        is_boolean_auth_check,
    }
}

/// The cross-file half of [`classify_function`]: `name` was not found in
/// `env`'s
/// own same-file `AstStore`, so this tries a compiled
/// [`sting_tolkc::ProjectStore`] instead — absent unless
/// `sting-pipeline::LintController::lint_paths` successfully compiled this
/// project via a real, vendored `tolk` binary (see
/// [`crate::DetectorContext::resolve_call`]'s own doc comment).
///
/// `None` — treated by [`classify_function`] exactly like
/// `FnProfile::default()` — for no project attached, zero or several
/// (ambiguous) matches, a resolved file this crate cannot read or parse, or
/// a resolved file whose own fresh tree-sitter parse does not actually
/// contain a same-named function. That last case is not expected (`tolk`'s
/// and `sting-ast`'s two independent views of "what functions exist"
/// disagreeing on a file `tolk` itself just compiled would be surprising),
/// but must degrade safely, not panic, if it ever happens either way.
fn classify_cross_file_function(
    name: &str,
    env: &Env,
    cache: &mut HashMap<CallKey, FnProfile>,
    visiting: &mut HashSet<CallKey>,
    cross_file: &mut CrossFileCache,
) -> Option<FnProfile> {
    let matches = env.ctx.resolve_call(name)?;
    let [only] = matches.as_slice() else {
        return None;
    };
    let project = env.ctx.project_store()?;
    let path = project.path_for_file_id(only.ident_loc.file_id)?;
    let loaded = cross_file.get_or_load(&path, project)?;
    let cross_env = Env {
        source: loaded.1.source(),
        ctx: &loaded.1,
        store: &loaded.0,
        function_name: name,
    };
    Some(classify_function(
        name, &cross_env, cache, visiting, cross_file,
    ))
}

/// Same-file functions named `method_name` that are genuinely methods
/// (`receiver_type.is_some()`) — the method-call counterpart of
/// `env.store.function_by_name`, which does not discriminate a method from
/// a free function sharing the same bare name (`sting_ast::FunctionDef`'s
/// own `name` field is the bare name either way, `receiver_type` is a
/// separate field) — see [`classify_method`]'s own doc comment for why that
/// distinction matters here specifically.
fn same_file_methods_named<'a>(store: &'a AstStore, method_name: &str) -> Vec<&'a FunctionDef> {
    store
        .functions()
        .filter(|f| f.name == method_name && f.receiver_type.is_some())
        .collect()
}

/// Resolves `method_name`'s [`FnProfile`] the same way [`classify_function`]
/// resolves a free function's — same-file first, then cross-file, same
/// cache/cycle-guard discipline — but restricted to genuine methods
/// (`receiver_type.is_some()`), and requiring *exactly one* match at
/// whichever tier resolves it. Closes the gap this module's doc comment
/// names: `check_statement` used to only ever follow a
/// delegated call through a bare-identifier callee, never a method call
/// (`multisig-v2.1/Order.tolk`'s own `storage.approve(...)`, confirmed real
/// — see the module doc comment's own "Cross-file *method* calls" section).
///
/// **Cannot know a call site's own receiver type — a heuristic, not a
/// soundness proof.** Neither tree-sitter nor `tolk`'s own `symbolTypesJson`
/// carry per-variable inferred types (`sting-tolkc`'s own doc comment: a
/// flat signature table, not an AST) — `storage.approve(...)`'s `storage`
/// could in principle be any type with an `approve` method, and this module
/// has no way to check which one a specific local variable actually is.
/// Two different receiver types both defining a method with the same bare
/// name cannot be told apart here at all; ambiguous (0 or 2+ matches at a
/// tier) means "give up at that tier, try the next" or "give up entirely" —
/// the same fail-safe default as everywhere else in this module's whole
/// cross-file story, never guessed at.
///
/// A separate `CallKey` namespace from [`classify_function`]'s own (the
/// `name` stored in the key is prefixed): `approve` the free function and
/// `approve` the method are two different questions this module can ask
/// about the same file, and must not share one cache/cycle-guard entry.
fn classify_method(
    method_name: &str,
    env: &Env,
    cache: &mut HashMap<CallKey, FnProfile>,
    visiting: &mut HashSet<CallKey>,
    cross_file: &mut CrossFileCache,
) -> FnProfile {
    let key = CallKey {
        file: env.ctx.file().to_path_buf(),
        name: format!("method::{method_name}"),
    };
    if let Some(profile) = cache.get(&key) {
        return *profile;
    }
    if visiting.contains(&key) || visiting.len() > 64 {
        return FnProfile::default();
    }
    visiting.insert(key.clone());

    let profile = match same_file_methods_named(env.store, method_name).as_slice() {
        [only] => classify_same_file_hit(only, env, cache, visiting, cross_file),
        [] => classify_cross_file_method(method_name, env, cache, visiting, cross_file)
            .unwrap_or_default(),
        _ambiguous => FnProfile::default(),
    };

    visiting.remove(&key);
    cache.insert(key, profile);
    profile
}

/// The cross-file half of [`classify_method`] — resolves `method_name`
/// against every function in the whole compiled project whose own qualified
/// name (`tolk`'s own `"<ReceiverType>.<name>"` convention for a method,
/// confirmed against real compiler output) ends in
/// `.method_name`, exactly one match required. Deliberately scans the
/// *whole* project (`ProjectStore::symbol_types().functions`), not
/// `resolve_call`/`reachable_files` from this file specifically —
/// `resolve_call` needs an exact name (the qualified form this call site
/// cannot construct, not knowing the receiver's real type), and every file
/// in one compiled project is, by construction, already reachable from
/// whichever entrypoint `tolk` itself compiled (the same reasoning
/// `uta.rs`'s own cross-file fix already established).
fn classify_cross_file_method(
    method_name: &str,
    env: &Env,
    cache: &mut HashMap<CallKey, FnProfile>,
    visiting: &mut HashSet<CallKey>,
    cross_file: &mut CrossFileCache,
) -> Option<FnProfile> {
    let project = env.ctx.project_store()?;
    let matches: Vec<_> = project
        .symbol_types()
        .functions
        .iter()
        .filter(|f| f.name.contains('.') && f.name.rsplit('.').next() == Some(method_name))
        .collect();
    let [only] = matches.as_slice() else {
        return None;
    };
    let path = project.path_for_file_id(only.ident_loc.file_id)?;
    let loaded = cross_file.get_or_load(&path, project)?;
    let cross_env = Env {
        source: loaded.1.source(),
        ctx: &loaded.1,
        store: &loaded.0,
        function_name: method_name,
    };
    Some(classify_method(
        method_name,
        &cross_env,
        cache,
        visiting,
        cross_file,
    ))
}

/// Whether `body` contains a recognized data/code mutation call anywhere
/// (any of [`DATA_MUTATION_METHODS`]/[`CODE_MUTATION_METHOD`] as a method
/// call, or [`LEGACY_DATA_MUTATION_FUNCTIONS`]/[`LEGACY_CODE_MUTATION_FUNCTIONS`]
/// as a free-function call), ignoring control flow — used only to decide
/// [`FnProfile::is_check_only`] ("never mutates at all"), not to decide
/// whether a specific path is guarded (that is `walk_block`'s job).
fn body_contains_mutation(body: Node, source: &str) -> bool {
    sting_ast::Preorder::from_node(body).any(|visited| {
        let node = visited.node;
        if let Some(call) = as_method_call(node, source, Language::Tolk) {
            return DATA_MUTATION_METHODS.contains(&call.method.as_str())
                || call.method == CODE_MUTATION_METHOD;
        }
        if node.kind() != "function_call" {
            return false;
        }
        let Some(name) = node
            .child_by_field_name("callee")
            .filter(|c| c.kind() == "identifier")
            .and_then(|c| identifier_text(c, source))
        else {
            return false;
        };
        LEGACY_DATA_MUTATION_FUNCTIONS.contains(&name.as_str())
            || LEGACY_CODE_MUTATION_FUNCTIONS.contains(&name.as_str())
    })
}

/// Whether `body` contains at least one recognized check (an
/// `assert_statement` or bail-out `if_statement` whose condition is
/// [`is_authorizing_condition`]) anywhere — the other half of
/// [`FnProfile::is_check_only`]'s "this function's only job is asserting a
/// check" heuristic.
fn body_contains_authorizing_check(
    body: Node,
    env: &Env,
    cache: &mut HashMap<CallKey, FnProfile>,
    visiting: &mut HashSet<CallKey>,
    cross_file: &mut CrossFileCache,
) -> bool {
    sting_ast::Preorder::from_node(body).any(|visited| {
        let node = visited.node;
        match node.kind() {
            "assert_statement" => node.child_by_field_name("condition").is_some_and(|c| {
                is_authorizing_condition(c, &Aliases::default(), env, cache, visiting, cross_file)
            }),
            "if_statement" => node.child_by_field_name("condition").is_some_and(|c| {
                is_authorizing_condition(c, &Aliases::default(), env, cache, visiting, cross_file)
            }),
            _ => false,
        }
    })
}

/// Whether `body` is exactly one `return <expr>;` statement whose `<expr>`
/// is itself [`is_authorizing_condition`] — see [`FnProfile::
/// is_boolean_auth_check`]'s own doc comment for the real bug this stays
/// deliberately separate from [`body_contains_authorizing_check`]/
/// `is_check_only` to avoid. Deliberately narrow (a *single*-statement
/// body, not "contains a `return` anywhere") — the same "start narrow"
/// choice this module makes everywhere else in its cross-function/
/// cross-file story; a multi-statement boolean helper with several return
/// paths is a real, plausible future extension, not built speculatively
/// without a confirmed real shape to calibrate against.
fn body_returns_an_authorizing_comparison(
    body: Node,
    env: &Env,
    cache: &mut HashMap<CallKey, FnProfile>,
    visiting: &mut HashSet<CallKey>,
    cross_file: &mut CrossFileCache,
) -> bool {
    match block_children(body).as_slice() {
        [stmt] if stmt.kind() == "return_statement" => {
            stmt.child_by_field_name("body").is_some_and(|expr| {
                is_authorizing_condition(
                    expr,
                    &Aliases::default(),
                    env,
                    cache,
                    visiting,
                    cross_file,
                )
            })
        }
        _ => false,
    }
}

/// Whether `condition`'s text names a recognized authorizing marker
/// ([`AUTH_MARKERS`], matched case/underscore-insensitively — see
/// [`normalize_for_marker_match`]), `condition` is a bare identifier
/// resolving through `aliases.bool_aliases` to a previously-recognized
/// authorizing boolean (the `Config.tolk:24`/`:74` flag-before-the-match
/// shape), `condition` references a known `aliases.sender_aliases`
/// identifier anywhere in its own subtree (this module's `sender_alias`
/// fix, e.g. `sender == storage.beneficiary`), or `condition` calls a
/// same-file/cross-file boolean-returning helper whose own body is itself
/// an authorizing comparison ([`condition_calls_a_boolean_auth_helper`],
/// [`FnProfile::is_boolean_auth_check`]): `if (isOwner(sender)) {...}` is
/// a different, common real-world shape from the bare-statement
/// void-delegate `check_statement` already resolved — this condition never
/// used to look inside a function call at all). Deliberately two different
/// matching rules for the two alias kinds: a bool alias must be checked
/// bare (`if (isAdmin)`, not `isAdmin && x`, unchanged v1 scope) while a
/// sender alias is checked anywhere in the condition's subtree, since it is
/// always one side of a comparison (`sender == ...`), never the whole
/// condition by itself.
#[allow(clippy::too_many_arguments)]
fn is_authorizing_condition(
    condition: Node,
    aliases: &Aliases,
    env: &Env,
    cache: &mut HashMap<CallKey, FnProfile>,
    visiting: &mut HashSet<CallKey>,
    cross_file: &mut CrossFileCache,
) -> bool {
    let source = env.source;
    let condition = unwrap_parens(condition);
    if condition.kind() == "identifier" {
        return identifier_text(condition, source)
            .is_some_and(|name| aliases.bool_aliases.contains(&name));
    }
    if !aliases.sender_aliases.is_empty()
        && sting_ast::Preorder::from_node(condition)
            .filter_map(|v| identifier_text(v.node, source))
            .any(|name| aliases.sender_aliases.contains(&name))
    {
        return true;
    }
    if let Ok(text) = condition.utf8_text(source.as_bytes()) {
        let normalized_text = normalize_for_marker_match(text);
        if AUTH_MARKERS
            .iter()
            .any(|marker| normalized_text.contains(&normalize_for_marker_match(marker)))
        {
            return true;
        }
    }
    condition_calls_a_boolean_auth_helper(condition, env, cache, visiting, cross_file)
}

/// Whether `condition` contains, anywhere in its own subtree, a call to a
/// same-file or cross-file function/method whose own [`FnProfile::
/// is_boolean_auth_check`] is `true` — **not** `is_check_only`, which means
/// something different and unsafe to use here (see that field's own doc
/// comment). Reuses the exact same resolution/cache/cycle-guard machinery
/// [`classify_function`]/[`classify_method`] already provide for a
/// bare-statement delegate, so this composes safely with the existing
/// recursion protection (a condition inside a function this is itself in
/// the middle of classifying cannot cause unbounded recursion: `visiting`
/// already catches the cycle and degrades to `FnProfile::default()`, the
/// same fail-safe every other cross-file/cross-function path in this
/// module already uses).
fn condition_calls_a_boolean_auth_helper(
    condition: Node,
    env: &Env,
    cache: &mut HashMap<CallKey, FnProfile>,
    visiting: &mut HashSet<CallKey>,
    cross_file: &mut CrossFileCache,
) -> bool {
    for visited in sting_ast::Preorder::from_node(condition) {
        let node = visited.node;
        if let Some(call) = as_method_call(node, env.source, Language::Tolk) {
            if classify_method(&call.method, env, cache, visiting, cross_file).is_boolean_auth_check
            {
                return true;
            }
            continue;
        }
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
        if classify_function(&name, env, cache, visiting, cross_file).is_boolean_auth_check {
            return true;
        }
    }
    false
}

/// Whether `expr` is *itself*, structurally, a recognized sender-address
/// source — a bare `<obj>.senderAddress` field read (`dot_access`, checked
/// by node kind, not just substring text: `in.senderAddress ==
/// storage.adminAddress` must NOT match here, only the field read on its
/// own — a comparison built *from* `senderAddress` is a boolean, not an
/// address, and must stay eligible for [`bool_alias_assignment`] instead),
/// or a bare `<slice>.loadAddress()` call — the classic low-level
/// `onInternalMessage(myBalance: int, msgValue: int, msgFull: cell,
/// msgBody: slice)` signature's own idiom for reading the source address
/// out of the message envelope (`var cs = msgFull.beginParse(); ... val
/// sender = cs.loadAddress();`), confirmed real in
/// `tonkeeper/ton-subscriptions`'s `nativeSub.tolk` — see this module's doc
/// comment.
fn is_sender_source(expr: Node, source: &str) -> bool {
    if expr.kind() == "dot_access"
        && let Ok(text) = expr.utf8_text(source.as_bytes())
        && text.contains("senderAddress")
    {
        return true;
    }
    as_method_call(expr, source, Language::Tolk).is_some_and(|call| call.method == "loadAddress")
}

/// If `stmt` is `NAME = <expr>;` (reassignment) or `var/val NAME = <expr>;`
/// (fresh declaration) where `<expr>` is [`is_sender_source`], the bound
/// name — mirrors [`bool_alias_assignment`]'s exact shape for a different
/// alias kind.
fn sender_alias_assignment(stmt: Node, source: &str) -> Option<String> {
    if stmt.kind() != "expression_statement" {
        return None;
    }
    let expr = stmt.named_child(0)?;
    if expr.kind() != "assignment" {
        return None;
    }
    let left = expr.child_by_field_name("left")?;
    let right = expr.child_by_field_name("right")?;
    if !is_sender_source(right, source) {
        return None;
    }
    if left.kind() == "var_declaration_lhs" {
        match bound_names(left, source).as_slice() {
            [(_, name)] => Some(name.clone()),
            _ => None,
        }
    } else {
        identifier_text(left, source)
    }
}

/// If `stmt` is `NAME = <cond>;` (reassignment) or `var/val NAME = <cond>;`
/// (fresh declaration) where `<cond>` is itself
/// [`is_authorizing_condition`] (checked with an empty alias set — chaining
/// an alias of an alias is out of v1's scope, mirroring `ur.rs`'s identical
/// choice for guard form 7), the bound name.
fn bool_alias_assignment(
    stmt: Node,
    env: &Env,
    cache: &mut HashMap<CallKey, FnProfile>,
    visiting: &mut HashSet<CallKey>,
    cross_file: &mut CrossFileCache,
) -> Option<String> {
    let source = env.source;
    if stmt.kind() != "expression_statement" {
        return None;
    }
    let expr = stmt.named_child(0)?;
    if expr.kind() != "assignment" {
        return None;
    }
    let left = expr.child_by_field_name("left")?;
    let right = expr.child_by_field_name("right")?;
    if !is_authorizing_condition(right, &Aliases::default(), env, cache, visiting, cross_file) {
        return None;
    }
    if left.kind() == "var_declaration_lhs" {
        match bound_names(left, source).as_slice() {
            [(_, name)] => Some(name.clone()),
            _ => None,
        }
    } else {
        identifier_text(left, source)
    }
}

/// If `stmt` is `val (_, name) = <expr>;` — a tuple-destructuring
/// declaration with exactly two bindings — where `name`'s own text
/// contains `"found"`, the bound name. The real
/// `(signerIndex, foundSigner) = <lookup>` idiom
/// (`multisig-v2.1/order-helpers.tolk`'s `DirectIndexedAddressMap.
/// findSignerByAddress`, called from `Order.tolk` as `val (signerIndex,
/// foundSigner) = storage.remaining.signers.findSignerByAddress(...)`,
/// later checked bare via `assert (foundSigner) throw ...;`) — a *third*
/// shape of "boolean meaning a check succeeded", distinct from both
/// existing alias kinds: [`bool_alias_assignment`]'s single-binding form
/// requires the right-hand side to itself already be
/// [`is_authorizing_condition`] (a direct comparison), which a lookup call
/// returning a tuple never is; [`sender_alias_assignment`] is address-typed,
/// not boolean. No type information is available here (tree-sitter carries
/// none, and `tolk`'s own `symbolTypesJson` is a flat signature table, not
/// per-statement types either), so this is a narrow, name-based vocabulary
/// match in the same spirit as [`AUTH_MARKERS`] itself — not a structural
/// proof the second position really is a found-flag, only the real,
/// confirmed spelling this was found from. Found by hand while auditing
/// `Order.tolk` while extending delegation to methods: without this, that work
/// would have introduced a real false positive on this exact file, flagging
/// `storage.approve(...)` as unguarded despite the `assert (foundSigner)`
/// immediately before it.
fn tuple_found_alias_assignment(stmt: Node, source: &str) -> Option<String> {
    if stmt.kind() != "expression_statement" {
        return None;
    }
    let expr = stmt.named_child(0)?;
    if expr.kind() != "assignment" {
        return None;
    }
    let left = expr.child_by_field_name("left")?;
    if left.kind() != "var_declaration_lhs" {
        return None;
    }
    match bound_names(left, source).as_slice() {
        [(_, _first), (_, second)] if second.to_lowercase().contains("found") => {
            Some(second.clone())
        }
        _ => None,
    }
}

fn mutation_finding(node: Node, mutation_text: &str, env: &Env) -> Finding {
    Finding {
        detector_id: "AC",
        severity: Severity::Medium,
        message: format!(
            "function `{}` calls `{mutation_text}` with no recognized sender/payment/signature check on this path — an unauthenticated caller may reach this storage mutation",
            env.function_name
        ),
        remediation: "add a check before this mutation — `assert (in.senderAddress == storage.ownerAddress) throw ...;`, a payment-threshold check if this path is meant to be publicly callable, or a signature check for a signature-based auth model — or, if the mutation is genuinely meant to be open to anyone, leave a comment saying so, since this detector cannot tell that apart from a forgotten check".to_string(),
        location: location_of(node, env.ctx),
        confidence: None,
        dynamic_verification: None,
        suggestion: None,
    }
}

/// Same shape as [`mutation_finding`], but for an unguarded call to
/// [`CODE_MUTATION_METHOD`]/[`LEGACY_CODE_MUTATION_FUNCTIONS`] — replacing
/// the contract's own code, not writing a storage field. `Critical`, not
/// `Medium`: an unauthenticated caller here does not just corrupt state,
/// they take over the contract entirely — confirmed as a real, standalone
/// bug (`hacktheton.com`'s `upgrade_level`, see this module's doc comment),
/// not a hypothetical severity bump.
fn code_upgrade_finding(node: Node, mutation_text: &str, env: &Env) -> Finding {
    Finding {
        detector_id: "AC",
        severity: Severity::Critical,
        message: format!(
            "function `{}` calls `{mutation_text}` — replacing the contract's own code — with no recognized sender/payment/signature check on this path — an unauthenticated caller could take over the whole contract",
            env.function_name
        ),
        remediation: "add a check before this call — `assert (in.senderAddress == storage.ownerAddress) throw ...;` or a signature check for a signature-based auth model — an unguarded code upgrade lets anyone replace this contract's logic entirely, not just corrupt one field".to_string(),
        location: location_of(node, env.ctx),
        confidence: None,
        dynamic_verification: None,
        suggestion: None,
    }
}

fn delegated_finding(node: Node, callee_name: &str, env: &Env) -> Finding {
    Finding {
        detector_id: "AC",
        severity: Severity::Medium,
        message: format!(
            "function `{}` calls `{callee_name}()` with no recognized check first; `{callee_name}` mutates storage on some path with no guard of its own — an unauthenticated caller may reach it",
            env.function_name
        ),
        remediation: format!(
            "either add a sender/payment/signature check in `{}` before calling `{callee_name}()`, or move the check inside `{callee_name}` itself so every caller is protected, not just this one",
            env.function_name
        ),
        location: location_of(node, env.ctx),
        confidence: None,
        dynamic_verification: None,
        suggestion: None,
    }
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

    use super::AcDetector;
    use crate::{Detector, DetectorContext};

    fn run(source: &str) -> Vec<sting_core::Finding> {
        let tree = sting_ast::parse(source).expect("grammar loads");
        let store = AstStore::build(&tree, source);
        let ctx = DetectorContext::new(PathBuf::from("contract.tolk"), tree, source.to_string());
        AcDetector.check(&store, &ctx)
    }

    #[test]
    fn flags_a_save_with_no_check_at_all() {
        let source = "fun onInternalMessage(in: InMessage) {\n    var storage = lazy Storage.load();\n    storage.value += 1;\n    storage.save();\n}\n";
        let findings = run(source);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].detector_id, "AC");
    }

    #[test]
    fn does_not_flag_a_direct_sender_address_assert_guard() {
        let source = "fun onInternalMessage(in: InMessage) {\n    var storage = lazy Storage.load();\n    assert (in.senderAddress == storage.ownerAddress) throw 100;\n    storage.value += 1;\n    storage.save();\n}\n";
        assert!(run(source).is_empty());
    }

    #[test]
    fn does_not_flag_a_bail_out_if_guard() {
        let source = "fun onInternalMessage(in: InMessage) {\n    var storage = lazy Storage.load();\n    if (in.senderAddress != storage.ownerAddress) {\n        return;\n    }\n    storage.value += 1;\n    storage.save();\n}\n";
        assert!(run(source).is_empty());
    }

    #[test]
    fn does_not_flag_a_bail_out_guard_preceded_by_a_comment() {
        let source = "fun onInternalMessage(in: InMessage) {\n    var storage = lazy Storage.load();\n    if (in.senderAddress != storage.ownerAddress) {\n        // not the owner, bail\n        return;\n    }\n    storage.value += 1;\n    storage.save();\n}\n";
        assert!(run(source).is_empty());
    }

    #[test]
    fn does_not_flag_a_payment_threshold_guard() {
        let source = "fun onInternalMessage(in: InMessage) {\n    var storage = lazy Storage.load();\n    if (in.valueCoins < MIN_PRICE) {\n        return;\n    }\n    storage.value += 1;\n    storage.save();\n}\n";
        assert!(run(source).is_empty());
    }

    #[test]
    fn does_not_flag_a_signature_valid_guard() {
        let source = "fun onInternalMessage(in: InMessage) {\n    var storage = lazy Storage.load();\n    assert (isSignatureValid(in.body.hash(), in.signature, storage.publicKey)) throw 101;\n    storage.value += 1;\n    storage.save();\n}\n";
        assert!(run(source).is_empty());
    }

    #[test]
    fn does_not_flag_a_call_to_a_check_only_helper() {
        // `jetton-v2.1/JettonMinter.tolk:38-47`'s real `assertSenderIsAdmin` shape.
        let source = "fun assertSenderIsAdmin(senderAddress: address, adminAddress: address) {\n    assert (senderAddress == adminAddress) throw 100;\n}\nfun onInternalMessage(in: InMessage) {\n    var storage = lazy Storage.load();\n    assertSenderIsAdmin(in.senderAddress, storage.adminAddress);\n    storage.value += 1;\n    storage.save();\n}\n";
        assert!(run(source).is_empty());
    }

    #[test]
    fn does_not_flag_a_disjunctive_if_assert_guard_with_no_else() {
        // `jetton-v2.1/JettonWallet.tolk:41-50`/`jetton-v1.3/NotcoinWallet.tolk:40-49`'s
        // real shape: `if (sender != minter) { assert (sender == computed)
        // throw ...; }` with no `else` — a disjunction, not a bail-out.
        let source = "fun onInternalMessage(in: InMessage) {\n    var storage = lazy Storage.load();\n    if (in.senderAddress != storage.minterAddress) {\n        assert (in.senderAddress == calcAddressOfWallet(storage.minterAddress)) throw 100;\n    }\n    storage.value += 1;\n    storage.save();\n}\n";
        assert!(run(source).is_empty());
    }

    #[test]
    fn flags_a_delegated_call_to_an_unguarded_same_file_helper() {
        let source = "fun processNewStake(storage: Storage, in: InMessage) {\n    storage.value += 1;\n    storage.save();\n}\nfun onInternalMessage(in: InMessage) {\n    var storage = lazy Storage.load();\n    processNewStake(storage, in);\n}\n";
        let findings = run(source);
        assert_eq!(findings.len(), 1);
        assert!(findings[0].message.contains("processNewStake"));
    }

    #[test]
    fn does_not_flag_a_delegated_call_when_the_callee_checks_internally() {
        let source = "fun processNewStake(storage: Storage, in: InMessage) {\n    assert (in.senderAddress == storage.ownerAddress) throw 100;\n    storage.value += 1;\n    storage.save();\n}\nfun onInternalMessage(in: InMessage) {\n    var storage = lazy Storage.load();\n    processNewStake(storage, in);\n}\n";
        assert!(run(source).is_empty());
    }

    #[test]
    fn does_not_flag_a_call_to_an_unresolved_cross_file_function() {
        let source = "fun onInternalMessage(in: InMessage) {\n    var storage = lazy Storage.load();\n    someExternalHelper(storage, in);\n}\n";
        assert!(run(source).is_empty());
    }

    #[test]
    fn does_not_flag_a_boolean_alias_of_an_authorizing_check() {
        // `config/Config.tolk:24`/`:74`'s real shape: a flag computed before
        // the dispatch `match`, consumed inside one specific arm's guard.
        let source = "fun onInternalMessage(in: InMessage) {\n    var storage = lazy Storage.load();\n    val isAdmin = in.senderAddress == storage.adminAddress;\n    match (in.op) {\n        1 => {\n            if (isAdmin) {\n                storage.value += 1;\n                storage.save();\n            }\n        }\n        else => {\n            return;\n        }\n    }\n}\n";
        assert!(run(source).is_empty());
    }

    #[test]
    fn flags_a_guard_missing_inside_one_match_arm_but_present_in_another() {
        let source = "fun onInternalMessage(in: InMessage) {\n    var storage = lazy Storage.load();\n    match (in.op) {\n        1 => {\n            assert (in.senderAddress == storage.ownerAddress) throw 100;\n            storage.value += 1;\n            storage.save();\n        }\n        else => {\n            storage.value += 2;\n            storage.save();\n        }\n    }\n}\n";
        let findings = run(source);
        assert_eq!(
            findings.len(),
            1,
            "the else arm's unguarded save must still be flagged even though the 1 arm guards its own"
        );
    }

    #[test]
    fn analyzes_every_entrypoint_not_only_the_first() {
        // The benchmark concatenates a project's files into one text, and 11
        // corpus projects define two or three entrypoints between them. Only
        // the first used to be analyzed, so the detector was measured against
        // one contract per project.
        let source = "fun onInternalMessage(in: InMessage) {\n    var storage = lazy Storage.load();\n    assert (in.senderAddress == storage.ownerAddress) throw 100;\n    storage.value += 1;\n    storage.save();\n}\n\nfun onInternalMessage(in: InMessage) {\n    var other = lazy Storage.load();\n    other.value += 2;\n    other.save();\n}\n";
        assert_eq!(
            run(source).len(),
            1,
            "the second entrypoint's unguarded save must be reported even though the first is guarded"
        );
    }

    #[test]
    fn a_method_of_the_same_name_does_not_shadow_the_real_entrypoint() {
        // Real external finding (`chainlink-ton/contracts/contracts/mcms/
        // mcms.tolk`): the file declares both `fun MCMS<T>.onInternalMessage`
        // — a method — and the actual `fun onInternalMessage(in: InMessage)`
        // entrypoint. `AstStore::function_by_name` matches methods by their
        // bare name, so it returned the method, and the detector analyzed a
        // body that is not an entrypoint at all while never looking at the one
        // that is.
        let source = "fun Storage.onInternalMessage(mutate self, sender: address) {\n    self.value += 1;\n}\n\nfun onInternalMessage(in: InMessage) {\n    var storage = lazy Storage.load();\n    storage.value += 1;\n    storage.save();\n}\n";
        assert_eq!(
            run(source).len(),
            1,
            "the free entrypoint must be the one analyzed, not the same-named method"
        );
    }

    #[test]
    fn does_not_flag_a_sender_check_asserted_inside_a_try_block() {
        // A real external finding: every `webdom-contracts`
        // offer/sale handler wraps its whole body in `try { ... } catch (e, _)
        // { returnNftToSender(...) }` and asserts the sender *inside* the try
        // (`purchase_offers/ton_simple_offer/contract.tolk:113`). `walk_block`
        // had no arm for `try_catch_statement`, so the `assert` was never seen
        // as one and the `save()` two statements later was reported. 11 of the
        // 52 false positives measured on the labeled real-world set.
        let source = "fun onInternalMessage(in: InMessage) {\n    var storage = lazy Storage.load();\n    try {\n        assert (in.senderAddress == storage.domainAddress) throw 100;\n        storage.value += 1;\n        storage.save();\n    } catch (e, _) {\n        return;\n    }\n}\n";
        assert!(run(source).is_empty());
    }

    #[test]
    fn still_flags_an_unguarded_mutation_inside_a_try_block() {
        // The inverse of the fix above: wrapping a handler in `try` must not
        // become a way to silence the detector.
        let source = "fun onInternalMessage(in: InMessage) {\n    var storage = lazy Storage.load();\n    try {\n        storage.value += 1;\n        storage.save();\n    } catch (e, _) {\n        return;\n    }\n}\n";
        assert_eq!(run(source).len(), 1);
    }

    #[test]
    fn does_not_carry_a_check_asserted_inside_a_try_past_the_whole_statement() {
        // An `assert` inside `try` that fails transfers control to `catch`,
        // not past the `try`/`catch` — so reaching the code *after* it does
        // not prove the check held, and a mutation there is still unguarded.
        let source = "fun onInternalMessage(in: InMessage) {\n    var storage = lazy Storage.load();\n    try {\n        assert (in.senderAddress == storage.ownerAddress) throw 100;\n    } catch (e, _) {\n    }\n    storage.value += 1;\n    storage.save();\n}\n";
        assert_eq!(run(source).len(), 1);
    }

    #[test]
    fn does_not_flag_a_sender_check_asserted_inside_a_bare_nested_scope() {
        // A bare `{ ... }` scope runs unconditionally, so an `assert` inside
        // it does authorize the statements that follow the scope.
        let source = "fun onInternalMessage(in: InMessage) {\n    var storage = lazy Storage.load();\n    {\n        assert (in.senderAddress == storage.ownerAddress) throw 100;\n    }\n    storage.value += 1;\n    storage.save();\n}\n";
        assert!(run(source).is_empty());
    }

    #[test]
    fn does_not_carry_a_check_asserted_inside_repeat_past_the_loop() {
        // `repeat` may run zero times, so its body proves nothing about the
        // code after it.
        let source = "fun onInternalMessage(in: InMessage) {\n    var storage = lazy Storage.load();\n    repeat (in.counter) {\n        assert (in.senderAddress == storage.ownerAddress) throw 100;\n    }\n    storage.value += 1;\n    storage.save();\n}\n";
        assert_eq!(run(source).len(), 1);
    }

    #[test]
    fn does_not_flag_a_local_sender_alias_extracted_from_sender_address() {
        // Real external finding (direction 5, zk-resistor-contracts'
        // *local-variable* half of its pattern): `val sender =
        // in.senderAddress;` followed by a bare `sender == storage.field`
        // comparison, with no `"senderAddress"` substring anywhere near the
        // comparison itself.
        let source = "fun onInternalMessage(in: InMessage) {\n    var storage = lazy Storage.load();\n    val sender = in.senderAddress;\n    assert (sender == storage.ownerAddress) throw 100;\n    storage.value += 1;\n    storage.save();\n}\n";
        assert!(run(source).is_empty());
    }

    #[test]
    fn does_not_flag_a_sender_read_via_raw_slice_load_address() {
        // Real external finding: `tonkeeper/ton-subscriptions`'s
        // `nativeSub.tolk` uses the classic low-level `onInternalMessage`
        // signature (no `InMessage`/`senderAddress` in scope at all) and
        // reads the source address by hand: `var cs = msgFull.beginParse();
        // ...; val sender = cs.loadAddress();`.
        let source = "fun onInternalMessage(myBalance: int, msgValue: int, msgFull: cell, msgBody: slice) {\n    var storage = lazy Storage.load();\n    var cs = msgFull.beginParse();\n    val sender = cs.loadAddress();\n    if (sender != storage.beneficiary) {\n        return;\n    }\n    storage.value += 1;\n    storage.save();\n}\n";
        assert!(run(source).is_empty());
    }

    #[test]
    fn still_flags_a_save_when_a_sender_alias_exists_but_is_never_checked() {
        // The sender-alias exemption must not become a blanket exemption
        // for any function that merely reads the sender somewhere — it
        // still has to be checked, exactly like a direct `in.senderAddress`
        // read that is never compared against anything.
        let source = "fun onInternalMessage(in: InMessage) {\n    var storage = lazy Storage.load();\n    val sender = in.senderAddress;\n    storage.lastSender = sender;\n    storage.save();\n}\n";
        let findings = run(source);
        assert_eq!(findings.len(), 1);
    }

    #[test]
    fn does_not_flag_a_function_with_no_on_internal_message() {
        let source =
            "fun helper(storage: Storage) {\n    storage.value += 1;\n    storage.save();\n}\n";
        assert!(run(source).is_empty());
    }

    #[test]
    fn does_not_flag_a_function_with_no_save_call() {
        let source = "fun onInternalMessage(in: InMessage): int {\n    var storage = lazy Storage.load();\n    return storage.value;\n}\n";
        assert!(run(source).is_empty());
    }

    #[test]
    fn flags_a_direct_set_data_call_with_no_check() {
        // Real external finding (direction 5, `hacktheton.com`'s
        // `tolk_level`, the low-level `global` + free-function idiom's
        // modern-Tolk equivalent): `.save()` is not the only spelling of
        // "persist storage" — `contract.setData(...)` is the real primitive
        // underneath it, and is completely invisible to this detector when
        // called directly, not through a `.save()`-named wrapper.
        let source = "fun onInternalMessage(in: InMessage) {\n    var storage = lazy Storage.load();\n    storage.value += 1;\n    contract.setData(storage.toCell());\n}\n";
        let findings = run(source);
        assert_eq!(findings.len(), 1);
        assert!(findings[0].message.contains(".setData()"));
    }

    #[test]
    fn flags_a_delegated_call_to_a_free_function_wrapping_set_data() {
        // The `saveData()` idiom itself: a same-file free function (not a
        // `.save()`-named method) whose only job is calling
        // `contract.setData(...)`. No special-casing of the name
        // `saveData` needed — the existing delegated-call machinery
        // ([`classify_function`]) already generalizes once `setData` itself
        // is a recognized mutation.
        let source = "fun saveData() {\n    contract.setData(beginCell().endCell());\n}\nfun onInternalMessage(in: InMessage) {\n    saveData();\n}\n";
        let findings = run(source);
        assert_eq!(findings.len(), 1);
        assert!(findings[0].message.contains("saveData"));
    }

    #[test]
    fn flags_a_legacy_free_function_set_contract_data_call() {
        // Real external finding: `hacktheton.com`'s actual, currently-live
        // level source uses the pre-1.0 Tolk API's free-function name
        // (`setContractData`, no `contract.` receiver) directly, not the
        // current `contract.setData(...)` — this closes the finding on
        // that real site's contracts as literally written today.
        let source = "fun onInternalMessage(myBalance: int, msgValue: int, inMsgFull: cell, inMsgBody: slice) {\n    setContractData(beginCell().endCell());\n}\n";
        let findings = run(source);
        assert_eq!(findings.len(), 1);
        assert!(findings[0].message.contains("setContractData()"));
    }

    #[test]
    fn flags_an_unguarded_code_upgrade_as_critical() {
        // Real external finding: `hacktheton.com`'s `upgrade_level` — a
        // full, unauthenticated code takeover, not a storage-field write.
        // Distinct, more severe finding than the data-mutation case above.
        let source = "fun onInternalMessage(in: InMessage) {\n    val code: cell = in.body.loadRef();\n    contract.setCodePostponed(code);\n}\n";
        let findings = run(source);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].severity, sting_core::Severity::Critical);
        assert!(
            findings[0]
                .message
                .contains("replacing the contract's own code")
        );
    }

    #[test]
    fn flags_a_legacy_free_function_code_upgrade_call() {
        let source = "fun onInternalMessage(myBalance: int, msgValue: int, inMsgFull: cell, inMsgBody: slice) {\n    val code: cell = inMsgBody.loadRef();\n    setContractCodePostponed(code);\n}\n";
        let findings = run(source);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].severity, sting_core::Severity::Critical);
    }

    #[test]
    fn does_not_flag_a_guarded_set_data_call() {
        let source = "fun onInternalMessage(in: InMessage) {\n    var storage = lazy Storage.load();\n    assert (in.senderAddress == storage.ownerAddress) throw 401;\n    contract.setData(storage.toCell());\n}\n";
        assert!(run(source).is_empty());
    }

    #[test]
    fn does_not_flag_a_guarded_code_upgrade_call() {
        let source = "fun onInternalMessage(in: InMessage) {\n    var storage = lazy Storage.load();\n    assert (in.senderAddress == storage.ownerAddress) throw 401;\n    val code: cell = in.body.loadRef();\n    contract.setCodePostponed(code);\n}\n";
        assert!(run(source).is_empty());
    }

    #[test]
    fn kind_is_ast_and_severity_is_medium() {
        assert_eq!(AcDetector.kind(), crate::DetectorKind::Ast);
        assert_eq!(AcDetector.default_severity(), sting_core::Severity::Medium);
    }

    /// Builds a one-function [`sting_tolkc::ProjectStore`] resolving
    /// `function_name` to a real file written to `dir` (so
    /// `support::CrossFileCache::get_or_load`'s own real
    /// `std::fs::read_to_string` succeeds), importable from a hypothetical
    /// `main.tolk` at `file_id` `0`. `helper_source` is written verbatim —
    /// callers control whether the helper mutates unguarded, checks only, or
    /// neither.
    fn cross_file_project(
        dir: &std::path::Path,
        helper_source: &str,
    ) -> (
        std::path::PathBuf,
        std::sync::Arc<sting_tolkc::ProjectStore>,
    ) {
        let helper_path = dir.join("helper.tolk");
        std::fs::write(&helper_path, helper_source).expect("write helper.tolk");
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
                    name: "processNewStake".to_string(),
                    return_ty_idx: 0,
                    num_params: 2,
                    ident_loc: sting_tolkc::SourceSpan::from([1, 1, 1, 1, 10]),
                    end_loc: sting_tolkc::SourceSpan::from([1, 3, 1, 3, 2]),
                    params: Vec::new(),
                }],
            }));
        (helper_path, project)
    }

    #[test]
    fn flags_a_cross_file_delegated_call_to_an_unguarded_helper() {
        // The same shape as `flags_a_delegated_call_to_an_unguarded_
        // same_file_helper` above, except `processNewStake` now lives in a
        // real *different* file, resolved via a compiled `ProjectStore`
        // instead of `env.store.function_by_name`.
        let dir = tempfile::tempdir().expect("tempdir");
        let (_helper_path, project) = cross_file_project(
            dir.path(),
            "fun processNewStake(storage: Storage, in: InMessage) {\n    storage.value += 1;\n    storage.save();\n}\n",
        );
        let source = "fun onInternalMessage(in: InMessage) {\n    var storage = lazy Storage.load();\n    processNewStake(storage, in);\n}\n";
        let tree = sting_ast::parse(source).expect("grammar loads");
        let store = AstStore::build(&tree, source);
        let ctx = DetectorContext::new(PathBuf::from("main.tolk"), tree, source.to_string())
            .with_project(project, 0);

        let findings = AcDetector.check(&store, &ctx);
        assert_eq!(
            findings.len(),
            1,
            "processNewStake is unguarded and cross-file-resolvable, must be flagged"
        );
        assert!(findings[0].message.contains("processNewStake"));
    }

    #[test]
    fn does_not_flag_a_cross_file_delegated_call_when_the_callee_checks_internally() {
        let dir = tempfile::tempdir().expect("tempdir");
        let (_helper_path, project) = cross_file_project(
            dir.path(),
            "fun processNewStake(storage: Storage, in: InMessage) {\n    assert (in.senderAddress == storage.ownerAddress) throw 100;\n    storage.value += 1;\n    storage.save();\n}\n",
        );
        let source = "fun onInternalMessage(in: InMessage) {\n    var storage = lazy Storage.load();\n    processNewStake(storage, in);\n}\n";
        let tree = sting_ast::parse(source).expect("grammar loads");
        let store = AstStore::build(&tree, source);
        let ctx = DetectorContext::new(PathBuf::from("main.tolk"), tree, source.to_string())
            .with_project(project, 0);

        assert!(AcDetector.check(&store, &ctx).is_empty());
    }

    #[test]
    fn does_not_flag_an_ambiguous_cross_file_resolution() {
        // Two same-named functions, both reachable — treated as
        // unresolvable, the same conservative default as a genuinely
        // unresolved name (`does_not_flag_a_call_to_an_unresolved_cross_
        // file_function` above).
        let dir = tempfile::tempdir().expect("tempdir");
        let (_helper_path, project) = cross_file_project(
            dir.path(),
            "fun processNewStake(storage: Storage, in: InMessage) {\n    storage.value += 1;\n    storage.save();\n}\n",
        );
        // Rebuild with a second, equally-reachable definition at a third file.
        let other_path = dir.path().join("other.tolk");
        std::fs::write(
            &other_path,
            "fun processNewStake(storage: Storage, in: InMessage) {\n    storage.value += 1;\n    storage.save();\n}\n",
        )
        .expect("write other.tolk");
        let symbol_types = sting_tolkc::SymbolTypes {
            files: vec![
                sting_tolkc::FileEntry {
                    file_id: 0,
                    file_name: "main.tolk".to_string(),
                    size_chars: 0,
                    imports: vec![1, 2],
                },
                sting_tolkc::FileEntry {
                    file_id: 1,
                    file_name: project
                        .path_for_file_id(1)
                        .expect("helper registered")
                        .display()
                        .to_string(),
                    size_chars: 0,
                    imports: vec![],
                },
                sting_tolkc::FileEntry {
                    file_id: 2,
                    file_name: other_path.display().to_string(),
                    size_chars: 0,
                    imports: vec![],
                },
            ],
            unique_types: Vec::new(),
            struct_instantiations: Vec::new(),
            alias_instantiations: Vec::new(),
            declarations: Vec::new(),
            functions: vec![
                sting_tolkc::FunctionEntry {
                    f_idx: 0,
                    name: "processNewStake".to_string(),
                    return_ty_idx: 0,
                    num_params: 2,
                    ident_loc: sting_tolkc::SourceSpan::from([1, 1, 1, 1, 10]),
                    end_loc: sting_tolkc::SourceSpan::from([1, 3, 1, 3, 2]),
                    params: Vec::new(),
                },
                sting_tolkc::FunctionEntry {
                    f_idx: 1,
                    name: "processNewStake".to_string(),
                    return_ty_idx: 0,
                    num_params: 2,
                    ident_loc: sting_tolkc::SourceSpan::from([2, 1, 1, 1, 10]),
                    end_loc: sting_tolkc::SourceSpan::from([2, 3, 1, 3, 2]),
                    params: Vec::new(),
                },
            ],
        };
        let ambiguous_project = std::sync::Arc::new(sting_tolkc::ProjectStore::new(symbol_types));

        let source = "fun onInternalMessage(in: InMessage) {\n    var storage = lazy Storage.load();\n    processNewStake(storage, in);\n}\n";
        let tree = sting_ast::parse(source).expect("grammar loads");
        let store = AstStore::build(&tree, source);
        let ctx = DetectorContext::new(PathBuf::from("main.tolk"), tree, source.to_string())
            .with_project(ambiguous_project, 0);

        assert!(AcDetector.check(&store, &ctx).is_empty());
    }

    #[test]
    fn does_not_flag_a_tuple_destructured_found_flag_checked_bare() {
        // The real multisig-v2.1/Order.tolk shape: a
        // tuple-returning lookup binds a "found" flag as the *second*
        // position, checked bare afterwards — must be recognized as a
        // guard the same way a direct boolean alias already is.
        let source = "fun onInternalMessage(in: InMessage) {\n    var storage = lazy Storage.load();\n    val (signerIndex, foundSigner) = storage.remaining.signers.findSignerByAddress(in.senderAddress);\n    assert (foundSigner) throw 100;\n    storage.value += 1;\n    storage.save();\n}\n";
        assert!(run(source).is_empty());
    }

    #[test]
    fn still_flags_a_save_when_the_found_flag_is_bound_but_never_checked() {
        // The same binding must not become a blanket exemption — it still
        // has to be checked, exactly like every other alias kind's own
        // "bound but unchecked" test.
        let source = "fun onInternalMessage(in: InMessage) {\n    var storage = lazy Storage.load();\n    val (signerIndex, foundSigner) = storage.remaining.signers.findSignerByAddress(in.senderAddress);\n    storage.value += 1;\n    storage.save();\n}\n";
        let findings = run(source);
        assert_eq!(findings.len(), 1);
    }

    #[test]
    fn does_not_treat_an_unrelated_tuple_destructure_as_a_found_alias() {
        // The second bound name must actually contain "found" — an
        // ordinary two-value tuple destructure (neither name suggesting a
        // lookup result) must not be treated as an authorizing alias.
        let source = "fun onInternalMessage(in: InMessage) {\n    var storage = lazy Storage.load();\n    val (a, b) = storage.remaining.signers.findSignerByAddress(in.senderAddress);\n    if (b) {\n        storage.value += 1;\n        storage.save();\n    }\n}\n";
        let findings = run(source);
        assert_eq!(findings.len(), 1);
    }

    #[test]
    fn flags_a_method_delegated_call_to_an_unguarded_same_file_method() {
        // The real multisig-v2.1/Order.tolk shape:
        // `storage.approve(...)` is a METHOD call, not a free-function
        // call — `check_statement`'s delegation logic used to only ever
        // follow a bare-identifier callee.
        let source = "fun OrderStorage.approve(mutate self, signerIndex: uint8) {\n    self.remaining.approvalsNum += 1;\n    self.save();\n}\nfun onInternalMessage(in: InMessage) {\n    var storage = lazy OrderStorage.load();\n    storage.approve(1);\n}\n";
        let findings = run(source);
        assert_eq!(findings.len(), 1);
        assert!(findings[0].message.contains("approve"));
    }

    #[test]
    fn does_not_flag_a_method_delegated_call_when_the_callee_checks_internally() {
        let source = "fun OrderStorage.approve(mutate self, senderAddress: address, signerIndex: uint8) {\n    assert (senderAddress == self.ownerAddress) throw 100;\n    self.remaining.approvalsNum += 1;\n    self.save();\n}\nfun onInternalMessage(in: InMessage) {\n    var storage = lazy OrderStorage.load();\n    storage.approve(in.senderAddress, 1);\n}\n";
        assert!(run(source).is_empty());
    }

    #[test]
    fn does_not_treat_two_same_named_methods_on_different_types_as_resolved() {
        // Ambiguous same-file resolution (two different receiver types both
        // defining `approve`) — this module cannot tell a call site's
        // actual receiver type apart (see `classify_method`'s own doc
        // comment), must not guess which one `storage.approve(1)` means.
        let source = "fun OrderStorage.approve(mutate self, signerIndex: uint8) {\n    self.remaining.approvalsNum += 1;\n    self.save();\n}\nfun OtherStorage.approve(mutate self, signerIndex: uint8) {\n    self.value += 1;\n    self.save();\n}\nfun onInternalMessage(in: InMessage) {\n    var storage = lazy OrderStorage.load();\n    storage.approve(1);\n}\n";
        assert!(run(source).is_empty());
    }

    #[test]
    fn flags_a_cross_file_method_delegation_to_an_unguarded_method() {
        let dir = tempfile::tempdir().expect("tempdir");
        let helper_path = dir.path().join("helper.tolk");
        std::fs::write(
            &helper_path,
            "fun OrderStorage.approve(mutate self, signerIndex: uint8) {\n    self.remaining.approvalsNum += 1;\n    self.save();\n}\n",
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
                    name: "OrderStorage.approve".to_string(),
                    return_ty_idx: 0,
                    num_params: 2,
                    ident_loc: sting_tolkc::SourceSpan::from([1, 1, 1, 1, 10]),
                    end_loc: sting_tolkc::SourceSpan::from([1, 3, 1, 3, 2]),
                    params: Vec::new(),
                }],
            }));

        let source = "fun onInternalMessage(in: InMessage) {\n    var storage = lazy OrderStorage.load();\n    storage.approve(1);\n}\n";
        let tree = sting_ast::parse(source).expect("grammar loads");
        let store = AstStore::build(&tree, source);
        let ctx = DetectorContext::new(PathBuf::from("main.tolk"), tree, source.to_string())
            .with_project(project, 0);

        let findings = AcDetector.check(&store, &ctx);
        assert_eq!(findings.len(), 1);
        assert!(findings[0].message.contains("approve"));
    }

    #[test]
    fn does_not_flag_a_cross_file_method_delegation_when_the_callee_checks_internally() {
        let dir = tempfile::tempdir().expect("tempdir");
        let helper_path = dir.path().join("helper.tolk");
        std::fs::write(
            &helper_path,
            "fun OrderStorage.approve(mutate self, senderAddress: address, signerIndex: uint8) {\n    assert (senderAddress == self.ownerAddress) throw 100;\n    self.remaining.approvalsNum += 1;\n    self.save();\n}\n",
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
                    name: "OrderStorage.approve".to_string(),
                    return_ty_idx: 0,
                    num_params: 2,
                    ident_loc: sting_tolkc::SourceSpan::from([1, 1, 1, 1, 10]),
                    end_loc: sting_tolkc::SourceSpan::from([1, 4, 1, 4, 2]),
                    params: Vec::new(),
                }],
            }));

        let source = "fun onInternalMessage(in: InMessage) {\n    var storage = lazy OrderStorage.load();\n    storage.approve(in.senderAddress, 1);\n}\n";
        let tree = sting_ast::parse(source).expect("grammar loads");
        let store = AstStore::build(&tree, source);
        let ctx = DetectorContext::new(PathBuf::from("main.tolk"), tree, source.to_string())
            .with_project(project, 0);

        assert!(AcDetector.check(&store, &ctx).is_empty());
    }

    #[test]
    fn recognizes_a_snake_case_payment_threshold_marker() {
        // Real gap found via external validation (Skiba111/orbit-ton — an entirely
        // snake_case-styled real contract): AUTH_MARKERS's "msgValue" is a
        // literal camelCase substring, so `msg_value >= threshold` was
        // structurally invisible before normalization.
        let source = "fun onInternalMessage(myBalance: int, msg_value: int, inMsgFull: cell, inMsgBody: slice) {\n    var storage = lazy Storage.load();\n    assert (msg_value >= 100) throw 100;\n    storage.value += 1;\n    storage.save();\n}\n";
        assert!(run(source).is_empty());
    }

    #[test]
    fn recognizes_a_snake_case_sender_address_marker() {
        let source = "fun onInternalMessage(in: InMessage) {\n    var storage = lazy Storage.load();\n    val sender_address = in.senderAddress;\n    assert (sender_address == storage.owner_address) throw 100;\n    storage.value += 1;\n    storage.save();\n}\n";
        assert!(run(source).is_empty());
    }

    #[test]
    fn still_flags_when_no_marker_matches_regardless_of_casing() {
        // Casing normalization must not turn into an unconditional pass —
        // a condition with no recognized marker at all, snake_case or not,
        // must still be flagged.
        let source = "fun onInternalMessage(myBalance: int, msg_value: int, inMsgFull: cell, inMsgBody: slice) {\n    var storage = lazy Storage.load();\n    assert (myBalance > 0) throw 100;\n    storage.value += 1;\n    storage.save();\n}\n";
        let findings = run(source);
        assert_eq!(findings.len(), 1);
    }

    #[test]
    fn does_not_flag_when_a_boolean_helper_used_inside_an_if_condition_checks_internally() {
        // The new, distinct gap from the bare-statement delegate case
        // (already covered by does_not_flag_a_call_to_a_check_only_helper
        // above): a boolean-*returning* helper called *inside* a
        // condition, `if (isOwner(...)) {...}` — not a void guard called as
        // its own statement.
        let source = "fun isOwner(sender: address): bool {\n    return senderAddress == storage.ownerAddress;\n}\nfun onInternalMessage(in: InMessage) {\n    var storage = lazy Storage.load();\n    if (isOwner(in.senderAddress)) {\n        storage.value += 1;\n        storage.save();\n    }\n}\n";
        assert!(run(source).is_empty());
    }

    #[test]
    fn does_not_flag_when_a_boolean_helper_used_inside_an_assert_condition_checks_internally() {
        let source = "fun isOwner(sender: address): bool {\n    return senderAddress == storage.ownerAddress;\n}\nfun onInternalMessage(in: InMessage) {\n    var storage = lazy Storage.load();\n    assert (isOwner(in.senderAddress)) throw 100;\n    storage.value += 1;\n    storage.save();\n}\n";
        assert!(run(source).is_empty());
    }

    #[test]
    fn does_not_treat_a_bare_statement_call_to_a_boolean_helper_as_authorizing() {
        // The correctness boundary `FnProfile::is_boolean_auth_check`'s own
        // doc comment states explicitly: calling a boolean-returning
        // function as a bare statement and discarding its result proves
        // nothing (it never throws) — must stay flagged, unlike a genuine
        // void assert-style guard (does_not_flag_a_call_to_a_check_only_helper).
        let source = "fun isOwner(sender: address): bool {\n    return senderAddress == storage.ownerAddress;\n}\nfun onInternalMessage(in: InMessage) {\n    var storage = lazy Storage.load();\n    isOwner(in.senderAddress);\n    storage.value += 1;\n    storage.save();\n}\n";
        let findings = run(source);
        assert_eq!(
            findings.len(),
            1,
            "a discarded boolean result must not grant authorization"
        );
    }

    #[test]
    fn still_flags_when_the_boolean_helper_condition_is_false_shaped_but_unrelated() {
        // A boolean helper whose body does not itself resolve to an
        // authorizing comparison must not be treated as one just because
        // it is called inside a condition.
        let source = "fun isPositive(x: int): bool {\n    return x > 0;\n}\nfun onInternalMessage(in: InMessage) {\n    var storage = lazy Storage.load();\n    if (isPositive(storage.counter)) {\n        storage.value += 1;\n        storage.save();\n    }\n}\n";
        let findings = run(source);
        assert_eq!(findings.len(), 1);
    }

    #[test]
    fn resolves_a_boolean_auth_helper_across_files() {
        // Not `cross_file_project` — that helper's own `SymbolTypes.functions`
        // entry is hardcoded to the name `processNewStake` for its existing
        // callers, so it cannot stand in for a differently-named function
        // like `isOwner` here.
        let dir = tempfile::tempdir().expect("tempdir");
        let helper_path = dir.path().join("helper.tolk");
        std::fs::write(
            &helper_path,
            "fun isOwner(sender: address): bool {\n    return senderAddress == storage.ownerAddress;\n}\n",
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
                    name: "isOwner".to_string(),
                    return_ty_idx: 0,
                    num_params: 1,
                    ident_loc: sting_tolkc::SourceSpan::from([1, 1, 1, 1, 10]),
                    end_loc: sting_tolkc::SourceSpan::from([1, 3, 1, 3, 2]),
                    params: Vec::new(),
                }],
            }));

        let source = "fun onInternalMessage(in: InMessage) {\n    var storage = lazy Storage.load();\n    if (isOwner(in.senderAddress)) {\n        storage.value += 1;\n        storage.save();\n    }\n}\n";
        let tree = sting_ast::parse(source).expect("grammar loads");
        let store = AstStore::build(&tree, source);
        let ctx = DetectorContext::new(PathBuf::from("main.tolk"), tree, source.to_string())
            .with_project(project, 0);
        assert!(AcDetector.check(&store, &ctx).is_empty());
    }
}
