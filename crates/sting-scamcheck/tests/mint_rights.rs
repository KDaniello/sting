//! Real-`libemulator` integration test for [`sting_scamcheck::mint_rights`]
//! (`docs/domains/scamcheck.md`), against the same real,
//! currently-deployed contract `sting-emulator`'s own fixtures already use
//! (`corpus/fixtures/emulator/lp-jetton-master.{code,data}.boc.base64` — a
//! `DeDust` LP jetton master, see `corpus/fixtures/emulator/SOURCE.md`) —
//! reusing a real, already-vendored fixture rather than adding a new one.
//!
//! Ground truth for `admin_address` on this specific contract, checked live
//! against `toncenter.com/api/v3/jetton/masters` on 2026-07-23 (not
//! assumed): `"admin_address": null` — mint rights are renounced. This test
//! asserts exactly that outcome, so a wrong stack decode (wrong bit widths,
//! wrong reference order — see `sting_scamcheck::stack`'s module doc
//! comment) fails loudly here against real data, not just against the
//! hand-built synthetic `BoC` in `stack`'s own unit tests.
//!
//! Same "skip gracefully if `vendor/libemulator/` is absent" convention as
//! every other real-`libemulator` test in this workspace
//! (`crates/sting-emulator/tests/integration.rs`).
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "docs/ARCHITECTURE.md §3.2 scopes the no-unwrap/expect/panic policy to library code outside tests"
)]

use std::path::{Path, PathBuf};

use sting_emulator::TvmEmulator;
use sting_emulator_sys::LibEmulator;
use sting_scamcheck::mint_rights::check_mint_rights;

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// Uses `sting_emulator::load_library` (safe) rather than
/// `sting_emulator_sys::LibEmulator::load` (`unsafe fn`) directly — per
/// that function's own doc comment, it exists precisely so downstream
/// crates never need their own `unsafe` block (`docs/ARCHITECTURE.md` §3.1
/// scopes `unsafe` to `sting-emulator`/`sting-emulator-sys` only).
fn load_lib_or_skip() -> Option<LibEmulator> {
    match sting_emulator::load_default_library() {
        Ok(lib) => Some(lib),
        // A test that cannot load the library says so and passes; a silent
        // skip and a real run must never look alike.
        Err(error) => {
            println!("SKIP: {error} — see docs/domains/emulator.md for how to fetch it");
            None
        }
    }
}

fn read_fixture(name: &str) -> String {
    std::fs::read_to_string(repo_root().join("corpus/fixtures/emulator").join(name))
        .unwrap_or_else(|e| panic!("failed to read fixture {name}: {e}"))
}

#[test]
fn mint_rights_are_renounced_on_the_real_dedust_lp_jetton_master() {
    let Some(lib) = load_lib_or_skip() else {
        return;
    };
    let code_boc = read_fixture("lp-jetton-master.code.boc.base64");
    let data_boc = read_fixture("lp-jetton-master.data.boc.base64");

    let emulator = TvmEmulator::new(&lib, code_boc.trim(), data_boc.trim())
        .expect("tvm_emulator_create failed");
    // Same ground-truth c7 setup `sting-emulator`'s own
    // `run_get_jetton_data.rs` example uses for this fixture — this
    // contract's `get_jetton_data()` reads `MYADDR` to build a content URL.
    emulator
        .set_c7(
            "0:000011a50b2cb855dd55972ea27e8fbb76c3eaf135918723849b0e5bdb69eae6",
            1_784_240_154,
            85_776_672,
            None,
        )
        .expect("tvm_emulator_set_c7 failed");

    let signal = check_mint_rights(&emulator).expect("check_mint_rights failed");
    assert!(
        signal.renounced(),
        "expected renounced mint rights (toncenter ground truth: admin_address=null), got {:?}",
        signal.admin_address
    );
}
