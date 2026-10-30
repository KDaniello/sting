//! Real-`libemulator` integration test for [`sting_scamcheck::ston_fi`]
//! (`docs/domains/scamcheck.md`), against a real,
//! currently active STON.fi v2 USD₮/TON pool
//! (`corpus/fixtures/emulator/ston-fi-pool.{code,data}.boc.base64`, see
//! `corpus/fixtures/emulator/SOURCE.md`).
//!
//! `pool_total_supply` is cross-checked against real ground truth: this
//! pool contract is itself the LP jetton's TEP-74 master, and
//! `toncenter.com/api/v3/jetton/masters` independently reports
//! `total_supply = 55183210687` for this exact address — a wrong stack
//! decode at a longer, previously-untested `get_pool_data()` shape (more
//! fields than `get_jetton_data()`'s) fails loudly here against real data.
//!
//! `candidate_lp_wallet_address` has no independent "who really holds the
//! LP" ground truth available (that would need identifying the actual
//! liquidity provider, which is out of scope — see the module doc comment's
//! scope note) — this test only asserts the call succeeds and
//! resolves to a well-formed, distinct address for two different
//! candidates, proving the argument-encoding path works against real
//! bytecode, not that any particular address is *the* LP holder.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "docs/ARCHITECTURE.md §3.2 scopes the no-unwrap/expect/panic policy to library code outside tests"
)]

use std::path::{Path, PathBuf};

use sting_emulator::TvmEmulator;
use sting_emulator_sys::LibEmulator;
use sting_scamcheck::ston_fi::{candidate_lp_wallet_address, pool_total_supply};
use tonlib_core::TonAddress;

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

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

fn pool_emulator(lib: &LibEmulator) -> TvmEmulator<'_> {
    let code_boc = read_fixture("ston-fi-pool.code.boc.base64");
    let data_boc = read_fixture("ston-fi-pool.data.boc.base64");
    let emulator = TvmEmulator::new(lib, code_boc.trim(), data_boc.trim())
        .expect("tvm_emulator_create failed");
    // Ground truth (corpus/fixtures/emulator/SOURCE.md): this contract's
    // `get_jetton_data()` reads `MYADDR` (fails with exit code 9 otherwise,
    // the same requirement `run_get_jetton_data.rs` documents for the
    // other fixture) — address/balance/unixtime fetched live from
    // toncenter.com/api/v3 for this exact address on 2026-07-23.
    let address: TonAddress = "EQD8TJ8xEWB1SpnRE4d89YO3jl0W0EiBnNS4IBaHaUmdfizE"
        .parse()
        .expect("parse the fixture's own known-good address");
    emulator
        .set_c7(&address.to_hex(), 1_784_766_721, 5_237_739_227_200, None)
        .expect("tvm_emulator_set_c7 failed");
    emulator
}

#[test]
fn pool_total_supply_matches_toncenter_ground_truth() {
    let Some(lib) = load_lib_or_skip() else {
        return;
    };
    let pool = pool_emulator(&lib);
    let total_supply = pool_total_supply(&pool).expect("pool_total_supply failed");
    assert_eq!(
        total_supply, 55_183_210_687,
        "expected toncenter's ground-truth total_supply for this exact pool"
    );
}

#[test]
fn candidate_lp_wallet_address_resolves_distinct_addresses_for_distinct_candidates() {
    let Some(lib) = load_lib_or_skip() else {
        return;
    };
    let pool = pool_emulator(&lib);

    // The pool's own LP-jetton admin address (real, ground truth:
    // corpus/fixtures/emulator/SOURCE.md) and an arbitrary distinct
    // address — neither is asserted to be *the* liquidity provider, only
    // that resolution succeeds and is injective for two different inputs.
    let admin = TonAddress::from_hex_str(
        "0:779dcc815138d9500e449c5291e7f12738c23d575b5310000f6a253bd607384e",
    )
    .expect("parse the fixture's own known-good admin address");
    let other = TonAddress::new(0, [0x11; 32].into());

    let admin_wallet =
        candidate_lp_wallet_address(&pool, &admin).expect("resolve admin's LP wallet address");
    let other_wallet =
        candidate_lp_wallet_address(&pool, &other).expect("resolve other's LP wallet address");

    assert_ne!(
        admin_wallet, other_wallet,
        "distinct owners must resolve to distinct LP wallet addresses"
    );
}
