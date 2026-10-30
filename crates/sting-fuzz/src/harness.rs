//! Drives a [`MsgSeq`] through [`crate::fixture`]'s vulnerable contract and
//! checks invariants — the one piece of logic shared by both fuzz targets
//! — both go through `sting-emulator` (`docs/domains/emulator.md`) —
//! and by this crate's own regression tests, so "the fuzzer found it" and
//! "the checker still catches it" are provably the same code path, not two
//! that could silently drift apart.

use sting_emulator::TvmEmulator;
use sting_emulator_sys::LibEmulator;
use tonlib_core::cell::BagOfCells;

use crate::body::build_body_boc;
use crate::fixture::{CODE_BOC_BASE64, DATA_BOC_BASE64};
use crate::types::MsgSeq;

/// Upper bound on how many messages from one [`MsgSeq`] are actually sent —
/// `arbitrary`'s `Vec<T>` generation has no inherent size limit, and an
/// unbounded sequence would make a single fuzz input's wall-clock cost
/// unbounded too (on top of, not instead of, libFuzzer's own `-timeout`
/// flag — see this module's doc comment on why no additional in-process
/// timeout is implemented here).
pub const MAX_MESSAGES_PER_SEQUENCE: usize = 20;

/// Nanotons attached to every synthetic `send_internal_message` call — not
/// itself fuzzed: `BalanceLedger.tolk` never reads the attached value, only
/// the application-level `amount` field inside the body (see
/// `crate::types::SyntheticMessage`'s doc comment), so varying this would
/// add fuzzing surface with no corresponding code path to explore.
pub const ATTACHED_VALUE_NANOTONS: u64 = 1_000_000_000;

/// Gas ceiling applied once per emulator instance — the primary defense
/// against a runaway single call: the gas limit is the defence, and a soft
/// wall-clock timeout only logs a warning. The *process-level*
/// wall-clock guard for a fuzz target specifically is libFuzzer's own
/// `-timeout=<seconds>` flag (`cargo fuzz run -- -timeout=20`), which kills
/// and reports a hanging input — the correct layer for that job in a
/// fuzzing harness, not a second, custom-rolled Rust-side timer racing it.
pub const GAS_LIMIT: i64 = 1_000_000;

/// A detected invariant violation.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum InvariantViolation {
    /// `BalanceLedger.tolk`'s `balance` field went negative — the seeded
    /// bug the harness must demonstrably catch
    /// (`docs/domains/emulator.md`).
    #[error("balance went negative after message {message_index}: {balance}")]
    NegativeBalance {
        /// Index (0-based) of the message in the sequence that caused it.
        message_index: usize,
        /// The negative balance observed.
        balance: i64,
    },
}

/// Outcome of running a full [`MsgSeq`] against a fresh contract instance.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunOutcome {
    /// The balance after the last message that was actually applied (`None`
    /// if no message in the sequence produced a readable result — e.g. an
    /// empty sequence, or every message was skipped as malformed).
    pub final_balance: Option<i64>,
    /// The first invariant violation encountered, if any. Checking stops at
    /// the first violation — a fuzz target only needs one to report a
    /// finding, and continuing past it would just be exploring the same
    /// already-corrupted state further.
    pub violation: Option<InvariantViolation>,
}

/// Runs `seq` against a fresh instance of `crate::fixture`'s vulnerable
/// contract, applying up to [`MAX_MESSAGES_PER_SEQUENCE`] messages in
/// order and checking the balance invariant after each one that actually
/// executes.
///
/// A message whose body fails to build, or whose `send_internal_message`
/// call itself fails or reports `success: false` at the emulator level, is
/// skipped (not applied, not counted as a violation) — those are
/// uninteresting inputs (malformed-by-construction, not a defect in the
/// *contract*), not findings. A response's `new_data` failing to decode as
/// the expected single-`int64` cell is treated the same way — it would mean
/// this harness's own assumption about `BalanceLedger.tolk`'s storage shape
/// broke, not that the contract did something wrong.
#[must_use]
pub fn run_message_sequence(lib: &LibEmulator, seq: &MsgSeq) -> RunOutcome {
    let Ok(emulator) = TvmEmulator::new(lib, CODE_BOC_BASE64.trim(), DATA_BOC_BASE64.trim()) else {
        return RunOutcome {
            final_balance: None,
            violation: None,
        };
    };
    // Best-effort: a gas-limit setter failing does not itself invalidate the
    // run (defense-in-depth, not the thing under test).
    let _ = emulator.set_gas_limit(GAS_LIMIT);

    let mut final_balance = None;
    for (message_index, msg) in seq
        .messages
        .iter()
        .enumerate()
        .take(MAX_MESSAGES_PER_SEQUENCE)
    {
        let Some(body_boc) = build_body_boc(msg) else {
            continue;
        };
        let Ok(response) = emulator.send_internal_message(&body_boc, ATTACHED_VALUE_NANOTONS)
        else {
            continue;
        };
        if !response.success {
            continue;
        }
        let Some(balance) = response.new_data.as_deref().and_then(decode_balance) else {
            continue;
        };
        final_balance = Some(balance);
        if balance < 0 {
            return RunOutcome {
                final_balance,
                violation: Some(InvariantViolation::NegativeBalance {
                    message_index,
                    balance,
                }),
            };
        }
    }

    RunOutcome {
        final_balance,
        violation: None,
    }
}

/// Decodes `BalanceLedger.tolk`'s storage shape — a single cell holding
/// exactly `balance: int` serialized as `storeInt(self.balance, 64)` — from
/// a `new_data` base64 `BoC`. Scoped to this one fixture's known shape
/// deliberately: `sting-emulator` does not (and should not) ship a general
/// storage-layout decoder, see `docs/domains/emulator.md`'s note on
/// not reimplementing a full `VmStack`/cell-schema decoder.
fn decode_balance(new_data_boc_base64: &str) -> Option<i64> {
    let bag = BagOfCells::parse_base64(new_data_boc_base64).ok()?;
    let root = bag.single_root().ok()?;
    root.parser().load_i64(64).ok()
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "docs/ARCHITECTURE.md §3.2 scopes the no-unwrap/expect policy to library code outside tests"
)]
mod tests {
    use sting_emulator_sys::LibEmulator;

    use super::{InvariantViolation, MsgSeq, RunOutcome, run_message_sequence};
    use crate::types::SyntheticMessage;

    const OP_CREDIT: u32 = 1;
    const OP_DEBIT: u32 = 2;

    fn load_lib_or_skip() -> Option<LibEmulator> {
        match sting_emulator::load_default_library() {
            Ok(lib) => Some(lib),
            // A test that cannot load the library says so and passes; a
            // silent skip and a real run must never look alike.
            Err(error) => {
                println!("SKIP: {error} — see docs/domains/emulator.md for how to fetch it");
                None
            }
        }
    }

    fn msg(op: u32, amount: u64) -> SyntheticMessage {
        SyntheticMessage {
            op,
            amount,
            extra: vec![],
        }
    }

    #[test]
    fn an_empty_sequence_produces_no_violation() {
        let Some(lib) = load_lib_or_skip() else {
            return;
        };
        let seq = MsgSeq { messages: vec![] };
        let outcome = run_message_sequence(&lib, &seq);
        assert_eq!(
            outcome,
            RunOutcome {
                final_balance: None,
                violation: None
            }
        );
    }

    #[test]
    fn crediting_then_debiting_within_balance_produces_no_violation() {
        let Some(lib) = load_lib_or_skip() else {
            return;
        };
        let seq = MsgSeq {
            messages: vec![msg(OP_CREDIT, 100), msg(OP_DEBIT, 40)],
        };
        let outcome = run_message_sequence(&lib, &seq);
        assert_eq!(outcome.final_balance, Some(60));
        assert_eq!(outcome.violation, None);
    }

    /// The regression case: a single overdraft-debit message, no preceding
    /// credit at all, silently drives balance negative — exactly the
    /// minimized shape a real fuzz run found, and this reproduces it.
    #[test]
    fn debiting_past_zero_is_caught_as_a_negative_balance_violation() {
        let Some(lib) = load_lib_or_skip() else {
            return;
        };
        let seq = MsgSeq {
            messages: vec![msg(OP_DEBIT, 500)],
        };
        let outcome = run_message_sequence(&lib, &seq);
        assert_eq!(
            outcome.violation,
            Some(InvariantViolation::NegativeBalance {
                message_index: 0,
                balance: -500,
            })
        );
    }

    #[test]
    fn a_malformed_extra_length_does_not_prevent_detecting_the_violation() {
        let Some(lib) = load_lib_or_skip() else {
            return;
        };
        let seq = MsgSeq {
            messages: vec![SyntheticMessage {
                op: OP_DEBIT,
                amount: 10,
                extra: vec![0xFF; 4096],
            }],
        };
        let outcome = run_message_sequence(&lib, &seq);
        assert_eq!(
            outcome.violation,
            Some(InvariantViolation::NegativeBalance {
                message_index: 0,
                balance: -10,
            })
        );
    }

    /// The literal minimized crash a real `cargo fuzz run fuzz_send_message`
    /// session produced — found within the first seconds of fuzzing, then
    /// reduced from over 12 KiB to 12 bytes by `cargo fuzz tmin` — decoded
    /// through `arbitrary` exactly
    /// as `fuzz_target!` itself would, not hand-reconstructed — so this test
    /// is provably replaying the fuzzer's own finding, not a look-alike.
    #[test]
    fn the_minimized_fuzzer_found_crash_is_still_caught() {
        use arbitrary::{Arbitrary as _, Unstructured};

        let Some(lib) = load_lib_or_skip() else {
            return;
        };
        let bytes = include_bytes!("../regressions/negative-balance-op-debit.bin");
        let mut unstructured = Unstructured::new(bytes);
        let msg =
            SyntheticMessage::arbitrary(&mut unstructured).expect("decodes the committed artifact");

        let outcome = run_message_sequence(
            &lib,
            &MsgSeq {
                messages: vec![msg],
            },
        );
        assert!(
            matches!(
                outcome.violation,
                Some(InvariantViolation::NegativeBalance { .. })
            ),
            "expected the committed fuzz artifact to still trip the balance invariant, got {outcome:?}"
        );
    }
}
