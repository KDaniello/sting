//! Message-sequence fuzz target (`docs/domains/emulator.md` §9): a sequence
//! of synthetic internal messages against a single
//! persistent `corpus/fixtures/fuzz/BalanceLedger.tolk` instance, checking
//! that the balance never goes negative across the whole sequence — this is
//! the target that actually finds the seeded bug, confirmed by a real
//! `cargo fuzz run` session.
#![no_main]

use std::path::Path;
use std::sync::OnceLock;

use libfuzzer_sys::fuzz_target;
use sting_emulator_sys::LibEmulator;
use sting_fuzz::{MsgSeq, run_message_sequence};

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

fuzz_target!(|seq: MsgSeq| {
    let outcome = run_message_sequence(lib(), &seq);
    if let Some(violation) = outcome.violation {
        panic!("{violation}");
    }
});
