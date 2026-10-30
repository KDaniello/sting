//! `sting explain <ID>` content: user-facing prose for each built-in
//! detector — what it looks for, its real known false-positive/limitation
//! profile, and why it carries the severity it does.
//!
//! **Deliberately hand-written, not extracted from each detector module's
//! own doc comment.** Those doc comments are written for a contributor to
//! this repository — they cite internal file/line references, prior
//! sessions, and other detectors' internals a first-time external user has
//! no context for. This module answers a different question ("should I
//! trust/act on this finding, and why") for a different reader, sourced
//! from the same ground truth (`docs/domains/detectors.md`'s calibrated
//! numbers and each detector's own `default_severity()`) but written fresh.

use sting_core::DetectorId;

/// One detector's `sting explain` content — see this module's own doc
/// comment for why each field is hand-written prose, not derived text.
pub struct Explanation {
    /// `"GVR — Global Var Redefined"` — the code plus its full taxonomy name.
    pub name: &'static str,
    /// What the detector looks for and why it matters, 2-4 sentences.
    pub summary: &'static str,
    /// Real, calibrated false-positive/limitation profile — not a generic
    /// disclaimer, the actual numbers and shapes found on real code
    /// (`docs/domains/detectors.md`).
    pub false_positives: &'static str,
    /// Why this detector's finding carries the severity it does.
    pub severity_rationale: &'static str,
}

const EXPLANATIONS: &[(DetectorId, Explanation)] = &[
    (
        "GVR",
        Explanation {
            name: "GVR — Global Var Redefined",
            summary: "Flags a local `var` inside a function body that shares its name with a field of the contract's storage struct. Reading the local looks exactly like reading storage; assigning to it silently does not update storage — a classic \"namespace pollution\" bug where a developer means to write to storage but only ever touches a same-named local shadow.",
            false_positives: "The most common false positive is a legitimate read-mutate-write-back idiom: the code reads a storage field into a local, mutates the local, then explicitly writes it back before `.save()` (`storage.field = local;` or `storage.field.set(key, local.toCell())`) — sting recognizes this within the same function and does not flag it. Cross-file storage struct definitions (the contract's `struct` lives in a different file from its handlers) are entirely out of scope — no findings for those files, not a silent gap. On sting's own corpus: 87.5% precision, 3 residual false positives, all a further unrecognized variant of the same write-back idiom.",
            severity_rationale: "High — a shadowed storage assignment is silent data loss: the contract's on-chain state never changes even though the code looks like it does.",
        },
    ),
    (
        "LEP",
        Explanation {
            name: "LEP — Lack End Parse",
            summary: "Flags a function that receives a `slice` parameter, reads from it, and never calls `.assertEnd()` — `.end_parse()` in FunC — before returning — so a caller can attach unexpected extra data to a message and the handler will silently ignore it instead of rejecting the message outright.",
            false_positives: "This is sting's noisiest detector (37.0% precision on our own corpus, and 34.1% on the Tolk half of it alone) — not because of a bug, but because reading a slice without asserting its end is extremely common and mostly harmless in real TON contracts (even `wallet-v4`, one of the most widely deployed and audited wallets on the network, does it routinely). About 16 of its 28 false positives are syntactically true: `.assertEnd()` really is not called. The detector cannot distinguish a genuinely security-relevant unchecked tail in an auth-boundary function from one in an already-gated admin-only path, and it does not follow a slice alias created by a method call (e.g. `.removeLastBits(...)`) — only direct reassignment. Treat a LEP finding as a fact about the code, not a confirmed bug.",
            severity_rationale: "Info, and reference-category alongside DEPR — this reports a fact, it does not claim a defect. It was Medium until the question was asked whether an unchecked slice tail is a defect at all, and answered no on three independent grounds: docs.ton.org's own list of 18 vulnerability classes has no such class; a study of 34 public professional audit reports covering 233 real vulnerabilities (arXiv 2509.10823) attributes 6 to parsing/serialization as a whole and never names this form — `end_parse`, `assertEnd` and `slice` do not appear in that paper at all; and no public TON incident with this root cause could be found. One documented incident reopens the question and this severity.",
        },
    ),
    (
        "ID",
        Explanation {
            name: "ID — Inconsistent Data",
            summary: "Flags a manual storage write (`beginCell().storeUint(a, ..).storeSlice(b, ..)`) and a manual storage read elsewhere that name the same fields but in a different order — the classic swapped-load/save-arguments pattern. Because both fields are usually the same primitive type (often two `int`s), Tolk's type checker cannot catch the swap; only the field order can.",
            false_positives: "100% precision, 0 false positives on sting's own corpus — the most mechanical of sting's checks, an ordered-sequence dataflow fact rather than a heuristic guess. It only compares fields already proven to belong to the same store/load pair (identical field-name set on both sides) attributed to the same contract, including across files reachable from the same compiled entrypoint rather than within one file alone.",
            severity_rationale: "High — a field-order swap silently corrupts every subsequent read of that storage layout, and stays invisible until the two swapped fields happen to hold distinguishable values.",
        },
    ),
    (
        "UBM",
        Explanation {
            name: "UBM — Unchecked Bounced Message",
            summary: "Flags a function that commits a storage mutation (`.save()`/`.setData()`) and then sends a message that can bounce, in a contract with no `onBouncedMessage` handler — or one that exists but never itself touches storage — to undo the mutation if the send bounces. The classic Jetton-wallet fund-loss pattern: balance already debited, the downstream transfer fails and bounces, and the debit is never reverted.",
            false_positives: "74.2% precision on sting's own corpus — every bug found in this detector so far has been a false *negative* (a real risk it missed), not a false positive, because it deliberately errs toward flagging more than a human will need to dismiss rather than silently missing a real one. Known scope boundary: does not extend to `.setCodePostponed()` — a deferred code upgrade applies at the end of the *current* transaction, before any bounce could arrive, so there is nothing to revert. A message built via `createExternalLogMessage` is correctly never treated as bounceable — that message kind cannot bounce by TON protocol construction.",
            severity_rationale: "Critical — this is a direct fund-loss primitive, sting's highest-severity class, reflecting that a successful exploit can silently destroy value with no way to recover it after the fact.",
        },
    ),
    (
        "TLB",
        Explanation {
            name: "TLB — TL-B (De)Serialization Mismatch",
            summary: "Flags a manual storage write and its matching read that name the same field but disagree on its TL-B shape: a different bit-width (`storeUint(x, 32)` read back with `loadUint(64)`), different signedness (`storeInt`/`loadUint` on the same field), or an entirely different primitive (`storeCoins`/`loadUint`). Any of these doesn't just misread that one field — a wrong bit-width shifts the parse position for every field read after it in the same buffer.",
            false_positives: "100% precision, 0 false positives on sting's own corpus. Reuses ID's exact same-contract, same-field-set event-matching machinery, comparing declared shape instead of order — a narrow, mechanical check, not a heuristic.",
            severity_rationale: "High — a shape mismatch can corrupt not just the mismatched field but everything read after it in the same buffer, and stays invisible until values happen to look obviously wrong.",
        },
    ),
    (
        "PL",
        Explanation {
            name: "PL — Precision Loss",
            summary: "Flags division performed before multiplication in the same expression (`a / b * c`) — integer division truncates, so dividing first loses precision that multiplying first would have kept. One deliberate exception: `(a / b) * b` (rounding down to a multiple of `b`) is a common, safe idiom in gas/storage-duration code and is not flagged, since the divisor and the second factor match.",
            false_positives: "100% precision, 0 false positives — a purely syntactic check on operator structure with no dataflow, leaving very little room to be wrong about what the code literally says. The real limitation is scope, not accuracy: only the direct `a / b * c` shape in one expression is checked, not a division result stored in a variable and multiplied later.",
            severity_rationale: "Medium — silent precision loss in financial/quantity math is a real correctness bug, but its practical impact depends on magnitudes sting cannot evaluate statically.",
        },
    ),
    (
        "BR",
        Explanation {
            name: "BR — Bad Randomness",
            summary: "Flags `blockchain.now()` or `blockchain.logicalTime()` used directly as the operand of a modulo (`%`) operation — both are values an attacker can observe in advance or influence, so deriving a \"random\" outcome from them is predictable, not random.",
            false_positives: "100% precision, 0 false positives. Deliberately narrow v1 scope: only a direct call as the modulo operand is checked, not a variable assigned the call's result earlier. Tolk's real `random.*` module (`random.initialize()`/`random.uint256()`) does not match this pattern by receiver or method name at all, so using the real RNG API correctly never trips this detector.",
            severity_rationale: "Medium — exploitability depends entirely on what the \"random\" outcome controls (a lottery payout is far worse than a cosmetic UI choice), which sting cannot judge statically.",
        },
    ),
    (
        "UR",
        Explanation {
            name: "UR — Unchecked Return",
            summary: "Flags `.loadValue()` called on a dictionary lookup result without a preceding check of `.isFound` on that same lookup — an unfound lookup's \"value\" is garbage data, not a safe default, so using it unchecked can treat undefined or attacker-influenced memory as real state.",
            false_positives: "97.9% precision on sting's own corpus — the single residual finding is a real, confirmed gap in the source it was found in (a developer's own comment reading \"assume the value is always found\" in place of an actual check), not a detector bug. Recognizes seven distinct real guard idioms — direct `if (x.isFound)`, negative early-return, ternary, `assert`, guards inside `match` arms, compound `&&` conditions, and a boolean-alias pattern — each added because a real site required it, not designed in the abstract.",
            severity_rationale: "High — using an unfound lookup's garbage value as real data can corrupt logic unpredictably, since the failure only appears when a key is genuinely absent, which is easy to miss in testing.",
        },
    ),
    (
        "AC",
        Explanation {
            name: "AC — Access Control",
            summary: "Follows every path from `onInternalMessage` — including one-hop delegation to same-file or cross-file helper functions and methods — looking for a storage-mutating call (`.save()`, `.setData()`, or a recognized legacy equivalent) with no recognized sender/payment/signature check anywhere on that path. A separate, more serious variant flags a code upgrade (`.setCodePostponed()`) reached the same way.",
            false_positives: "The lowest-precision detector sting has (32.8% on our own corpus), and it is **experimental: off unless `Sting.toml` names \"AC\" in `detectors.enable`**. That is a measurement, not a hedge. 409 storage-mutation sites across 89 real contracts were labeled by hand as \"must be guarded\" or \"deliberately public\", split half-and-half by contract family, and none of the four separating hypotheses named in advance survived the held-out half — the best reached 33% precision against a threshold of 80%. Roughly half of AC's firings land on code that is open to anyone by design: a public bid, a permissionless finalization after expiry, a counter with no owner concept, a mint authorized economically rather than by identity. The other half are recognition gaps — real authorization written in an idiom AC does not know (helper call, a method on a nested storage field, a guard by early return, a guard in the other branch of an if/else, delegation to a library returning a bool, dictionary membership keyed by the sender). Those keep getting fixed; the first half is a ceiling. Treat every AC finding as something to review, never to act on directly.",
            severity_rationale: "Medium for a plain unguarded state mutation, Critical for an unguarded code upgrade — a full code replacement with no check at all is a strictly more severe, harder-to-recover primitive than a single unguarded field write. Severity is unchanged by the move to experimental: when AC does fire on a real miss, the miss is exactly as serious as before; what changed is who sees it by default.",
        },
    ),
    (
        "UTA",
        Explanation {
            name: "UTA — Unchecked Transfer Amount",
            summary: "Flags a function that debits one tracked quantity and credits another by the same amount but validates only one side against going negative — when that amount is read from a *signed* slice load (`.loadInt`, not `.loadUint`), a negative value can flip the transfer's direction and mint value that trivially passes the one-sided check. Found on real, intentionally-vulnerable external code (a hacktheton.com CTF challenge), not sting's own corpus.",
            false_positives: "100% precision, but that figure is measured almost entirely on synthetic mutations and 3 golden fixtures, not on organic findings in diverse real code — a deliberately narrow v1 that requires proof the amount actually traces back to a signed load, either in the same function or one call hop away. Read the 100% figure as \"no false positives observed yet on a small sample\", not \"proven precise across diverse real code\".",
            severity_rationale: "High — when it fires, the underlying bug is a value-minting primitive, one of the more directly exploitable classes sting looks for.",
        },
    ),
    (
        "SUA",
        Explanation {
            name: "SUA — Suspicious Unary Assignment",
            summary: "Flags the classic C-family lexical typo `x =+ 1` / `x =- 1` — Tolk parses this as `x = (+1)` / `x = (-1)`, a plain assignment of a unary-prefixed value, almost certainly meant as `x += 1` / `x -= 1`. In a balance/counter context this silently *replaces* the tracked value instead of *adjusting* it.",
            false_positives: "100% precision, measured almost entirely on synthetic mutations — the pattern is rare enough that sting's own corpus contains no organic example either way. Detected by exact byte adjacency between the `=` token and the following unary operator: `x =+ 1` (no space, the typo) is distinguished from `x = +1` (a space, a deliberate unary expression) by real parse position, not a guess. One of only two detectors (with DEPR) whose finding carries a machine-applicable fix — run `sting lint --fix` to apply it.",
            severity_rationale: "Low — ported from Misti's own `SuspiciousUnary` at the same severity: a real correctness bug, but typically caught quickly once a balance visibly misbehaves, unlike the more silent classes above.",
        },
    ),
    (
        "EPS",
        Explanation {
            name: "EPS — Ensure PRG Seed",
            summary: "Flags a call to `random.uint256()`/`random.range()` with no preceding `random.initialize()` / `initializeBy()` / `setSeed()` on the same execution path. TON's randomness seed for a transaction starts as `SHA256(block_rand_seed . contract_address)` — deliberately excluding the transaction's own logical time — so every message that lands on the same contract within the same block starts from an identical seed unless something changes it first. `random.initialize()` fixes this by mixing in the transaction's logical time.",
            false_positives: "Same-function only in this v1 — a helper function that generates randomness without seeding itself, relying on its caller having already seeded earlier in the same call chain, is a known, undetected false-positive shape (no interprocedural call-following yet, unlike AC/UTA). Not observed in real code so far: the one organic real use in sting's own corpus (`config-helpers.tolk`'s `scanRandomProposal`) calls `random.initialize()` and `random.uint256()` in the same function, so this gap remains theoretical, not measured.",
            severity_rationale: "Medium, deliberately not Critical — even code that seeds correctly on every use remains only as unpredictable as block_rand_seed itself, which a colluding validator ultimately controls (TON's own documentation rates that residual risk Low); this detector closes the same-block-replay hole specifically, and its severity reflects that real ceiling rather than overselling the fix.",
        },
    ),
    (
        "UDA",
        Explanation {
            name: "UDA — Unchecked Destination Address",
            summary: "Sting's first taint-tracking detector: flags a message sent via `sendRawMessage(...)` or `<msg>.send(...)` whose destination address traces back to a value read directly out of the incoming message body (`.loadAddress()`/`.loadRef()`/`.loadBits(...)` on the message body slice) with no check in between. Found on real, intentionally-vulnerable external code (a hacktheton.com CTF challenge, the `proxy_level` pattern) — an unchecked destination turns the contract into an open relay: anyone can make it forward funds anywhere.",
            false_positives: "Same-function only, and — a real, deliberately narrowed v1 boundary found while implementing, not assumed in advance — sanitization is only recognized via an `assert(recipient == trusted) throw ...;` guard, not an `if (recipient == trusted) { ...send... }` branch-gated check: the comparison inside an `if`'s condition isn't visible to the same block-level analysis an `assert` statement is. An `if`-gated destination check is a known, undetected false-positive shape. Precision not yet measured on diverse real code — validated primarily against the original motivating pattern and synthetic cases.",
            severity_rationale: "High — an unchecked destination is a direct fund-redirection primitive; not Critical because, unlike UBM's fund-loss pattern, it requires the contract to already be holding value someone else's message can trigger moving.",
        },
    ),
    (
        "DEPR",
        Explanation {
            name: "DEPR — Deprecated Tolk stdlib usage",
            summary: "Flags calls to a Tolk standard library function or method carrying a real `@deprecated(...)` attribute in Tolk's own stdlib source, plus a stronger tier for names that have been renamed and no longer exist in any current Tolk toolchain at all (e.g. the old `setContractData`, replaced by `contract.setData`). Not a security check — deprecated-but-correct code is not a defect, just an older spelling of the same operation.",
            false_positives: "Not measured for precision and recall the way the other detectors are — there is no \"ground truth\" judgment call here, only a fact about the current Tolk compiler (a name either still exists under that spelling or it doesn't). This list goes stale as Tolk evolves and is periodically re-checked against the real stdlib source. The other detector, alongside SUA, whose findings can carry a machine-applicable `--fix` — but only for entries with one unambiguous replacement (a plain rename or a whole-call literal); most DEPR guidance describes a structural rewrite (e.g. an argument moving to the other side of a method call) that sting deliberately does not try to compute automatically.",
            severity_rationale: "Info for a still-compiling deprecated name, Low for a name that no longer exists in any current Tolk toolchain at all — reflecting \"discouraged\" versus \"will not compile\" as genuinely different severities of problem.",
        },
    ),
];

/// The full explanation for `detector_id`, if it names one of the 12
/// built-in detectors (`DetectorRegistry::builtin()`) — `None` for an
/// unrecognized id, so the caller can print a helpful error rather than a
/// blank page.
#[must_use]
pub fn explain(detector_id: &str) -> Option<&'static Explanation> {
    EXPLANATIONS
        .iter()
        .find(|(id, _)| *id == detector_id)
        .map(|(_, explanation)| explanation)
}

/// Every detector id `explain` recognizes, in the same order as
/// `DetectorRegistry::builtin()` — for an unknown-id error message that
/// lists the valid options instead of just saying "not found".
#[must_use]
pub fn known_ids() -> Vec<DetectorId> {
    EXPLANATIONS.iter().map(|(id, _)| *id).collect()
}

#[cfg(test)]
mod tests {
    use super::{explain, known_ids};
    use crate::DetectorRegistry;

    #[test]
    fn explains_every_builtin_detector_and_nothing_else() {
        let registry = DetectorRegistry::builtin();
        for detector in registry.detectors() {
            let id = detector.id();
            assert!(
                explain(id).is_some(),
                "detector {id} is registered but has no sting explain content"
            );
        }
        assert_eq!(
            known_ids().len(),
            registry.detectors().count(),
            "explain's table and DetectorRegistry::builtin() must name exactly the same detectors"
        );
    }

    #[test]
    fn returns_none_for_an_unknown_id() {
        assert!(explain("NOT-A-REAL-DETECTOR").is_none());
    }

    #[test]
    fn every_explanation_has_non_empty_content_in_every_field() {
        for (id, explanation) in super::EXPLANATIONS {
            assert!(!explanation.name.is_empty(), "{id}: empty name");
            assert!(!explanation.summary.is_empty(), "{id}: empty summary");
            assert!(
                !explanation.false_positives.is_empty(),
                "{id}: empty false_positives"
            );
            assert!(
                !explanation.severity_rationale.is_empty(),
                "{id}: empty severity_rationale"
            );
            assert!(
                explanation.name.starts_with(id),
                "{id}: name {:?} should start with the detector id",
                explanation.name
            );
        }
    }
}
