//! Single-message fuzz target (`docs/domains/emulator.md`
//! item 1): one synthetic internal message through
//! `TvmEmulator::send_internal_message` against
//! `corpus/fixtures/fuzz/BalanceLedger.tolk`. Invariants — "no panic/crash
//! on the Rust side" (libFuzzer's own job to detect) and "the balance
//! invariant holds" (`sting_fuzz::run_message_sequence`'s job) — are
//! checked by panicking on a detected [`sting_fuzz::InvariantViolation`],
//! which libFuzzer reports as a crash with the violation's own message.
#![no_main]

use std::path::Path;
use std::sync::OnceLock;

use libfuzzer_sys::fuzz_target;
use sting_emulator_sys::LibEmulator;
use sting_fuzz::{MsgSeq, SyntheticMessage, run_message_sequence};

fn lib() -> &'static LibEmulator {
    static LIB: OnceLock<LibEmulator> = OnceLock::new();
    LIB.get_or_init(|| {
        let repo_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
        let path = repo_root
            .join("vendor/libemulator")
            .join(sting_emulator_sys::platform_library_filename());
        sting_emulator::load_library(&path)
            .unwrap_or_else(|e| panic!("failed to load libemulator from {}: {e}", path.display()))
    })
}

fuzz_target!(|msg: SyntheticMessage| {
    let outcome = run_message_sequence(
        lib(),
        &MsgSeq {
            messages: vec![msg],
        },
    );
    if let Some(violation) = outcome.violation {
        panic!("{violation}");
    }
});
