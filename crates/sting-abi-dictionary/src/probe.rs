//! Bytecode-only get-method probing — given real on-chain `code`+`data`
//! (no source, ever) and a [`crate::MethodDictionary`], calls every
//! **unambiguous, zero-argument** dictionary entry against the real
//! contract via `sting-emulator` and classifies each by its real result.
//!
//! # Why only zero-argument entries
//!
//! Confirmed empirically while designing this module (real vendored
//! `libemulator`, real mainnet jetton-master fixture,
//! `corpus/fixtures/emulator/lp-jetton-master.*`): calling a real,
//! genuinely-present get-method that *does* exist but takes a parameter
//! (`get_wallet_address(owner: address)`) with an **empty** argument stack
//! does not fail the way a missing method does — it throws a *different*,
//! unrelated error (stack underflow, VM exit code 2), which this module has
//! no principled way to distinguish from "this method doesn't exist and
//! failed for some other coincidental reason." Only Level 1 (zero-argument)
//! probing has an unambiguous signal (below) — supplying real, plausible
//! argument values for parameterized methods is a distinct, harder problem
//! not attempted here.
//!
//! # The "does this method exist" signal, confirmed empirically
//!
//! Also confirmed against the same real fixture: calling a genuinely-absent
//! method id (8 different ones tried — plausible-looking real names the
//! fixture doesn't have, and arbitrary raw ids) reliably produced **VM exit
//! code 11 with a resulting stack depth of exactly 1** (the pushed method
//! id, echoed back untouched) — the real, compiled dispatch table's
//! `DICTIGETJMPZ ... THROWARG 11` fallback for "no matching key," visible
//! directly in the emulator's own VM log. A genuinely-present, real
//! zero-argument method returned VM exit code `0` with a real, multi-value
//! stack. This module treats exactly that pattern
//! (`exit_code == 11 && depth == 1`) as "confirmed absent," `exit_code` `0`
//! or `1` (TVM's own normal-termination codes) as "confirmed present," and
//! everything else as inconclusive — logged, never asserted either way.
//! **Caveat, not hidden**: a contract could in principle `throw(11)` from
//! its own real logic for an unrelated reason; the `depth == 1` check
//! (exactly the untouched pushed id, nothing else) makes an accidental
//! collision with that pattern unlikely but not theoretically impossible —
//! this is an empirically-grounded heuristic, not a TVM-spec guarantee.

use sting_emulator::{TvmEmulator, empty_stack_boc};
use sting_emulator_sys::LibEmulator;

use crate::dictionary::{ContractKind, MethodDictionary};

/// One dictionary entry that really does exist on the probed contract.
#[derive(Debug, Clone, serde::Serialize)]
pub struct RecognizedMethod {
    /// The get-method's real name (from the dictionary).
    pub name: String,
    /// The real TVM method id that was called.
    pub tvm_method_id: i64,
    /// How many values the real call returned — the only thing this module
    /// claims to know about the *value* side; it does not decode individual
    /// stack entries (see this crate's own top-level doc comment on why
    /// that's a separate, harder problem).
    pub stack_depth: u32,
    /// Which real contracts this name/id pair was originally learned from
    /// (dictionary provenance, carried through for transparency).
    pub seen_in: Vec<String>,
    /// The dictionary entry's heuristic contract-kind classification,
    /// carried through so a caller can see *why* [`ProbeReport::kind_conflicts`]
    /// flagged a pair, not just that it did.
    pub contract_kind: ContractKind,
}

/// The result of probing one address against a dictionary.
#[derive(Debug, Clone, serde::Serialize)]
pub struct ProbeReport {
    /// The address that was probed.
    pub address: String,
    /// Every dictionary entry confirmed present on this contract.
    pub recognized: Vec<RecognizedMethod>,
    /// How many unambiguous, zero-argument dictionary entries were actually
    /// probed (matches [`MethodDictionary::probeable_entries`]'s count at
    /// probe time).
    pub probed_count: usize,
    /// How many probes returned neither the "present" nor the "absent"
    /// signature — genuinely unclear, not silently folded into either
    /// bucket.
    pub inconclusive_count: usize,
    /// Total distinct ids known to the dictionary used for this probe
    /// (including ambiguous/parameterized ones that were skipped, for
    /// context on how much of the dictionary was actually usable).
    pub dictionary_size: usize,
    /// Pairs of method names among [`Self::recognized`] whose dictionary
    /// entries carry **different, specific** [`ContractKind`]s (e.g. one
    /// `Wallet`, one `Jetton`) — a real, empirically observed risk
    /// (`docs/domains/abi-ir.md`, case `hikkB9r4zttC`:
    /// the same opcode meant `InternalTransfer` on a highload wallet in the
    /// dictionary, but the probed target may not actually be one). This
    /// module never resolves a conflict by guessing which entry is
    /// "really" right — a caller sees the conflict and decides, the same
    /// "report ambiguity, don't guess" principle [`MethodDictionary`]
    /// already applies to CRC16 id collisions. Empty when every recognized
    /// entry agrees, or when fewer than two carry a non-[`ContractKind::Other`]
    /// kind at all.
    pub kind_conflicts: Vec<(String, String)>,
}

/// Errors constructing the emulator handle for a probe.
#[derive(Debug, thiserror::Error)]
pub enum ProbeError {
    /// `TvmEmulator::new` failed — malformed `code`/`data` `BoC`, most likely.
    #[error("failed to construct the TVM emulator: {0}")]
    EmulatorInit(String),
    /// `set_c7` failed.
    #[error("failed to set c7: {0}")]
    SetC7(String),
    /// Building the empty argument stack failed (should not happen in
    /// practice — surfaced rather than unwrapped anyway).
    #[error("failed to build an empty argument stack: {0}")]
    EmptyStack(String),
}

/// VM exit code TON's real compiled dispatch tables (FunC/Tolk/Tact,
/// confirmed for the `FunC` case empirically — see this module's own doc
/// comment) fall through to for an id with no matching dispatch entry.
const UNKNOWN_METHOD_EXIT_CODE: i32 = 11;

/// Everything [`probe`] needs about the contract being probed — bundled
/// into one struct rather than five loose parameters (`clippy::
/// too_many_arguments`), and because these five really are one cohesive
/// "which real on-chain contract, at what real chain context" concept, not
/// five independent knobs.
pub struct ProbeTarget<'a> {
    /// The contract's real code, as a base64 `BoC`.
    pub code_boc_base64: &'a str,
    /// The contract's real data, as a base64 `BoC`.
    pub data_boc_base64: &'a str,
    /// The contract's own address (raw or friendly form) — required for
    /// `set_c7`; some real get-methods read `MYADDR` (see this module's own
    /// doc comment on why `set_c7` is always called).
    pub address: &'a str,
    /// Unix timestamp to expose as `NOW` — best-effort, does not affect the
    /// presence/absence classification itself, only values a recognized
    /// method's own logic might compute from it.
    pub unixtime: u32,
    /// Balance (nanotons) to expose as `BALANCE` — same best-effort caveat
    /// as `unixtime`.
    pub balance: u64,
}

/// Probes every unambiguous, zero-argument entry in `dictionary` against
/// `target`'s real `code`/`data` — no source is read or required anywhere
/// in this call.
///
/// # Errors
/// Returns [`ProbeError`] if the emulator handle itself can't be
/// constructed (malformed input) — individual per-method probe failures are
/// never propagated as an overall `Err`, they land in `inconclusive_count`.
pub fn probe(
    lib: &LibEmulator,
    target: &ProbeTarget<'_>,
    dictionary: &MethodDictionary,
) -> Result<ProbeReport, ProbeError> {
    let emulator = TvmEmulator::new(lib, target.code_boc_base64, target.data_boc_base64)
        .map_err(|e| ProbeError::EmulatorInit(e.to_string()))?;
    // Best-effort — a wrong balance/unixtime can only affect a contract's
    // own *returned values* for logic that reads them (BALANCE/NOW), never
    // the presence/absence classification this module makes, which depends
    // only on VM exit code + stack depth. Omitting c7 entirely was
    // empirically confirmed (this crate's design phase, same fixture) to
    // cause a spurious exit code 9 on methods that read MYADDR — always
    // set it.
    emulator
        .set_c7(target.address, target.unixtime, target.balance, None)
        .map_err(|e| ProbeError::SetC7(e.to_string()))?;
    let empty_stack = empty_stack_boc().map_err(|e| ProbeError::EmptyStack(e.to_string()))?;

    let mut recognized = Vec::new();
    let mut probed_count = 0usize;
    let mut inconclusive_count = 0usize;

    for (id, entry) in dictionary.probeable_entries() {
        probed_count += 1;
        let Ok(id_as_i32) = i32::try_from(id) else {
            // Real TON method ids are 19-bit, always fit i32 — a value that
            // doesn't is not a real id this dictionary should ever contain,
            // but skip rather than panic if it somehow does.
            inconclusive_count += 1;
            continue;
        };
        let Ok(response) = emulator.run_get_method(id_as_i32, &empty_stack) else {
            inconclusive_count += 1;
            continue;
        };
        if !response.success {
            inconclusive_count += 1;
            continue;
        }
        let depth = response.stack_depth().ok();
        match classify(response.vm_exit_code, depth) {
            Classification::Recognized(depth) => recognized.push(RecognizedMethod {
                name: entry.name.clone(),
                tvm_method_id: id,
                stack_depth: depth,
                seen_in: entry.seen_in.clone(),
                contract_kind: entry.contract_kind,
            }),
            // Confirmed absent — not an error, not counted anywhere; this is
            // the expected, common case for the vast majority of dictionary
            // entries a given contract doesn't implement.
            Classification::ConfirmedAbsent => {}
            Classification::Inconclusive => inconclusive_count += 1,
        }
    }

    let kind_conflicts = find_kind_conflicts(&recognized);

    Ok(ProbeReport {
        address: target.address.to_string(),
        recognized,
        probed_count,
        inconclusive_count,
        dictionary_size: dictionary.len(),
        kind_conflicts,
    })
}

/// Every pair of [`RecognizedMethod`]s whose [`ContractKind`]s disagree —
/// both specific (not [`ContractKind::Other`]), and not equal. Quadratic in
/// `recognized.len()`, deliberately not optimized: real dictionaries
/// recognize a handful of methods per probe, never thousands (see this
/// crate's own real validation numbers, `docs/domains/method-dictionary.md`).
fn find_kind_conflicts(recognized: &[RecognizedMethod]) -> Vec<(String, String)> {
    let mut conflicts = Vec::new();
    for (i, a) in recognized.iter().enumerate() {
        for b in &recognized[i + 1..] {
            if a.contract_kind != b.contract_kind
                && a.contract_kind != ContractKind::Other
                && b.contract_kind != ContractKind::Other
            {
                conflicts.push((a.name.clone(), b.name.clone()));
            }
        }
    }
    conflicts
}

/// The three, and only three, outcomes a single probe call can land in —
/// pulled out of [`probe`]'s loop as a pure function so the classification
/// rule itself (this module's central safety argument: "never resolved by
/// guessing") is unit-testable without a real emulator handle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Classification {
    /// Really present — carries the real stack depth the call returned.
    Recognized(u32),
    /// Confirmed absent (the dispatch-table-miss signature).
    ConfirmedAbsent,
    /// Neither signature matched — genuinely unclear, must not be folded
    /// into either bucket above.
    Inconclusive,
}

pub(crate) fn classify(vm_exit_code: Option<i32>, stack_depth: Option<u32>) -> Classification {
    match (vm_exit_code, stack_depth) {
        (Some(0 | 1), Some(depth)) => Classification::Recognized(depth),
        (Some(UNKNOWN_METHOD_EXIT_CODE), Some(1)) => Classification::ConfirmedAbsent,
        _ => Classification::Inconclusive,
    }
}

#[cfg(test)]
mod tests {
    use super::{
        Classification, RecognizedMethod, UNKNOWN_METHOD_EXIT_CODE, classify, find_kind_conflicts,
    };
    use crate::dictionary::ContractKind;

    fn recognized(name: &str, kind: ContractKind) -> RecognizedMethod {
        RecognizedMethod {
            name: name.to_string(),
            tvm_method_id: 0,
            stack_depth: 0,
            seen_in: Vec::new(),
            contract_kind: kind,
        }
    }

    #[test]
    fn two_specific_disagreeing_kinds_are_flagged_as_a_conflict() {
        // The real risk this guards against, seen on a real target: a highload
        // wallet's `InternalTransfer` opcode coincidentally matching what
        // would otherwise look like a jetton method on some other target.
        let recognized_methods = vec![
            recognized("get_jetton_data", ContractKind::Jetton),
            recognized("internal_transfer", ContractKind::Wallet),
        ];
        let conflicts = find_kind_conflicts(&recognized_methods);
        assert_eq!(
            conflicts,
            vec![(
                "get_jetton_data".to_string(),
                "internal_transfer".to_string()
            )]
        );
    }

    #[test]
    fn matching_kinds_are_not_a_conflict() {
        let recognized_methods = vec![
            recognized("get_jetton_data", ContractKind::Jetton),
            recognized("get_wallet_address", ContractKind::Jetton),
        ];
        assert!(find_kind_conflicts(&recognized_methods).is_empty());
    }

    #[test]
    fn other_kind_never_conflicts_with_anything() {
        // `Other` means "genuinely unclassified," not "a third real kind" —
        // two `Other` entries, or an `Other` next to a specific kind, must
        // never be flagged.
        let recognized_methods = vec![
            recognized("get_proposal", ContractKind::Other),
            recognized("get_jetton_data", ContractKind::Jetton),
            recognized("list_proposals", ContractKind::Other),
        ];
        assert!(find_kind_conflicts(&recognized_methods).is_empty());
    }

    #[test]
    fn exit_code_zero_or_one_with_a_real_depth_is_recognized() {
        assert_eq!(classify(Some(0), Some(5)), Classification::Recognized(5));
        assert_eq!(classify(Some(1), Some(1)), Classification::Recognized(1));
    }

    #[test]
    fn the_dispatch_miss_signature_is_confirmed_absent() {
        assert_eq!(
            classify(Some(UNKNOWN_METHOD_EXIT_CODE), Some(1)),
            Classification::ConfirmedAbsent
        );
    }

    #[test]
    fn exit_code_eleven_at_a_depth_other_than_one_is_inconclusive_not_absent() {
        // A real contract's own logic could plausibly throw(11) for an
        // unrelated reason with a different stack shape — must not be
        // folded into "confirmed absent" just because the exit code matches.
        assert_eq!(
            classify(Some(UNKNOWN_METHOD_EXIT_CODE), Some(2)),
            Classification::Inconclusive
        );
        assert_eq!(
            classify(Some(UNKNOWN_METHOD_EXIT_CODE), None),
            Classification::Inconclusive
        );
    }

    #[test]
    fn a_parameterized_methods_stack_underflow_is_inconclusive_not_absent() {
        // Exit code 2 (stack underflow) is what a real, present but
        // parameterized method produces when called with an empty stack —
        // must never be read as "confirmed absent."
        assert_eq!(classify(Some(2), Some(1)), Classification::Inconclusive);
    }

    #[test]
    fn an_undecodable_stack_depth_is_inconclusive_even_with_a_normal_exit_code() {
        assert_eq!(classify(Some(0), None), Classification::Inconclusive);
    }

    #[test]
    fn no_exit_code_at_all_is_inconclusive() {
        assert_eq!(classify(None, Some(1)), Classification::Inconclusive);
        assert_eq!(classify(None, None), Classification::Inconclusive);
    }
}
