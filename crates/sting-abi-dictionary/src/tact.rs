//! Which unique on-chain codes a Tact compiler emitted, decided from bytecode
//! alone.
//!
//! A language perimeter declared without a measurement is a preference. This
//! module supplies the measurement for one side of it: how much of the chain
//! is Tact, counted over unique codes rather than over addresses, because one
//! popular deployment repeated ten thousand times is one contract, not ten
//! thousand.
//!
//! # The signature
//!
//! Tact's compiler emits get methods no hand-written contract carries by
//! accident: `supported_interfaces`, `get_abi_ipfs` and
//! `lazy_deployment_completed`. Two of the three together are a compiler's
//! fingerprint. One alone is not: measured against this tree's own corpus, 2
//! of the 46 files carrying `supported_interfaces` are hand-written `FunC` —
//! `whales-nominator-pool-get.fc` really does declare it — so a single marker
//! is reported as its own verdict rather than counted as Tact.
//!
//! # What the count is a floor of, and not a ceiling
//!
//! **This counts contracts that declare themselves, not contracts Tact wrote.**
//! Tact 1.4.1 (2024-07-26) turned `supported_interfaces` and `get_abi_ipfs`
//! into options a project switches on, and 1.6.0 (2025-02-28) stopped emitting
//! `lazy_deployment_completed` by default. A contract compiled since then with
//! the defaults carries no marker and is counted as not Tact. Bytecode offers
//! no other fingerprint, so a real upper bound needs source, not a probe.
//!
//! # When a marker counts as present
//!
//! A marker is present when the call returns cleanly, exit code 0 or 1, and
//! absent on any other exit code. This is stricter than [`mod@crate::probe`],
//! which calls everything but the dispatch-table miss inconclusive, and the
//! three markers are why: each returns constants and reads no storage, so on a
//! contract Tact emitted, with that contract's own real data, none of them can
//! fail. A marker that threw is a marker the contract does not have.
//!
//! The rule was written after the measurement said the looser one was not
//! usable: of 389 unique mainnet codes, 324 answer the dispatch-table miss
//! (exit 11, the echoed id alone on the stack) and 56 answer exit 32 from a
//! hand-rolled dispatcher — a wallet comparing the id against `seqno` and
//! `get_public_key` and throwing when neither matches — with one code each at
//! exit 2 and exit 7. Counting those 58 as inconclusive turned a 3.3% upper
//! bound into 18.3%, which decides nothing.
//!
//! Silence is kept for the one case that really is silence: the emulator did
//! not answer at all. That lands in [`TactSignature::inconclusive`].
//!
//! # What this does not do
//!
//! It does not name a Tact version: the markers bound it loosely, and the
//! compiler does not write its version into the code cell. It does not read
//! what a marker returned — presence is the whole signal. It says nothing
//! about `FunC` against Tolk, which share the same absence of a fingerprint.

use sting_emulator::{TvmEmulator, empty_stack_boc};
use sting_emulator_sys::LibEmulator;

use crate::probe::{ProbeError, ProbeTarget};

/// One get method the Tact compiler emits, by name and by the id that name
/// resolves to under `(crc16(name) & 0xffff) | 0x10000`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Marker {
    /// The method's name, as Tact emits it.
    pub name: &'static str,
    /// The TVM method id that name resolves to.
    pub tvm_method_id: i32,
}

/// `supported_interfaces` — every Tact contract, every version.
pub const INTROSPECTION: Marker = Marker {
    name: "supported_interfaces",
    tvm_method_id: 113_617,
};
/// `lazy_deployment_completed` — Tact through 1.5.
pub const LAZY_DEPLOYMENT: Marker = Marker {
    name: "lazy_deployment_completed",
    tvm_method_id: 115_390,
};
/// `get_abi_ipfs` — Tact from 1.6, when the ABI getter is on.
pub const ABI_IPFS: Marker = Marker {
    name: "get_abi_ipfs",
    tvm_method_id: 121_275,
};

/// Which markers one code carries, and how many probes said nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize)]
pub struct TactSignature {
    /// `supported_interfaces` is present.
    pub introspection: bool,
    /// `lazy_deployment_completed` is present.
    pub lazy_deployment: bool,
    /// `get_abi_ipfs` is present.
    pub abi_ipfs: bool,
    /// How many of the three probes landed in neither "present" nor "absent".
    /// Carried rather than folded into either, so a verdict resting on two
    /// answers out of three is visible as such.
    pub inconclusive: u8,
}

impl TactSignature {
    /// How many of the three markers are present.
    #[must_use]
    pub const fn present(&self) -> u8 {
        self.introspection as u8 + self.lazy_deployment as u8 + self.abi_ipfs as u8
    }

    /// The verdict this signature supports — the rule itself, pure, so it is
    /// read and tested without an emulator.
    #[must_use]
    pub const fn verdict(&self) -> TactVerdict {
        match (self.present(), self.inconclusive) {
            (0, 0) => TactVerdict::Absent,
            // No marker, and a probe that said nothing. A check that did not
            // run must not read as a check that came back clean.
            (0, _) => TactVerdict::Undecided,
            (1, _) => TactVerdict::OneMarker,
            _ => TactVerdict::Emitted,
        }
    }
}

/// What bytecode alone can say about the compiler that emitted a code.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, serde::Serialize)]
pub enum TactVerdict {
    /// Two markers or more — a compiler's fingerprint, not a coincidence.
    Emitted,
    /// Exactly one. Tact 1.6 without the ABI getter, or a hand-written
    /// contract that declares the same method; this module refuses to guess
    /// which.
    OneMarker,
    /// No marker, and every probe answered.
    Absent,
    /// No marker, and at least one probe answered neither present nor absent.
    /// Counted apart from [`Self::Absent`]: an unanswered check is not a
    /// negative result.
    Undecided,
}

/// Probes `target` for the three markers.
///
/// # Errors
/// Returns [`ProbeError`] when the emulator handle itself cannot be built —
/// a failing individual probe lands in [`TactSignature::inconclusive`], never
/// in an `Err`.
pub fn signature_of(
    lib: &LibEmulator,
    target: &ProbeTarget<'_>,
) -> Result<TactSignature, ProbeError> {
    let emulator = TvmEmulator::new(lib, target.code_boc_base64, target.data_boc_base64)
        .map_err(|e| ProbeError::EmulatorInit(e.to_string()))?;
    emulator
        .set_c7(target.address, target.unixtime, target.balance, None)
        .map_err(|e| ProbeError::SetC7(e.to_string()))?;
    let empty_stack = empty_stack_boc().map_err(|e| ProbeError::EmptyStack(e.to_string()))?;

    let mut signature = TactSignature::default();
    let markers = [
        (INTROSPECTION, &mut signature.introspection),
        (LAZY_DEPLOYMENT, &mut signature.lazy_deployment),
        (ABI_IPFS, &mut signature.abi_ipfs),
    ];
    let mut inconclusive = 0u8;
    for (marker, present) in markers {
        match emulator.run_get_method(marker.tvm_method_id, &empty_stack) {
            // The VM answered. Exit code 0 or 1 is TVM's own normal
            // termination; anything else is a marker this contract does not
            // have (see this module's own rule).
            Ok(response) if response.success => {
                *present = matches!(response.vm_exit_code, Some(0 | 1));
            }
            // The emulator did not answer — the one case that is silence.
            Ok(_) | Err(_) => inconclusive += 1,
        }
    }
    signature.inconclusive = inconclusive;
    Ok(signature)
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "docs/ARCHITECTURE.md §3.2 scopes the no-unwrap/expect policy to library code outside tests"
)]
mod tests {
    use super::{ABI_IPFS, INTROSPECTION, LAZY_DEPLOYMENT, TactSignature, TactVerdict};

    fn signature(introspection: bool, lazy: bool, ipfs: bool) -> TactSignature {
        TactSignature {
            introspection,
            lazy_deployment: lazy,
            abi_ipfs: ipfs,
            inconclusive: 0,
        }
    }

    #[test]
    fn two_markers_are_a_compiler_fingerprint() {
        assert_eq!(signature(true, true, false).verdict(), TactVerdict::Emitted);
        assert_eq!(signature(true, false, true).verdict(), TactVerdict::Emitted);
    }

    #[test]
    fn one_marker_is_not_resolved_by_guessing() {
        assert_eq!(
            signature(true, false, false).verdict(),
            TactVerdict::OneMarker
        );
        assert_eq!(
            signature(false, true, false).verdict(),
            TactVerdict::OneMarker
        );
    }

    #[test]
    fn a_probe_that_said_nothing_is_not_a_negative_result() {
        let silent = TactSignature {
            introspection: false,
            lazy_deployment: false,
            abi_ipfs: false,
            inconclusive: 1,
        };
        assert_eq!(silent.verdict(), TactVerdict::Undecided);
    }

    #[test]
    fn no_marker_is_no_verdict_beyond_absence() {
        assert_eq!(
            signature(false, false, false).verdict(),
            TactVerdict::Absent
        );
    }

    /// The ids are literals here and were measured independently in
    /// `corpus-abi/method-dictionary.json`, which was built from real sources.
    /// Two sources agreeing is what makes a literal id checkable.
    #[test]
    fn every_marker_id_matches_the_dictionary_built_from_real_sources() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../corpus-abi/method-dictionary.json");
        let dictionary = crate::MethodDictionary::load(&path).expect("the checked-in dictionary");
        for marker in [INTROSPECTION, LAZY_DEPLOYMENT, ABI_IPFS] {
            let found = dictionary
                .probeable_entries()
                .find(|(_, entry)| entry.name == marker.name)
                .map(|(id, _)| id);
            assert_eq!(
                found,
                Some(i64::from(marker.tvm_method_id)),
                "{}",
                marker.name
            );
        }
    }
}
