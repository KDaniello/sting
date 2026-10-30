//! Real-`libemulator` integration tests for [`sting_scamcheck::honeypot`]
//! (`docs/domains/scamcheck.md`).
//!
//! Uses two real, currently-deployed jetton holder wallets, chosen
//! specifically because together they proved the module's own diagnosis:
//! - `ston-fi-lp-holder-wallet.*` (a real STON.fi LP-jetton holder, **not**
//!   a TON "precompiled contract") — proves the real-holder approach
//!   itself is sound: a genuine transfer within balance succeeds, one
//!   exceeding balance reverts for a real, contract-enforced reason.
//! - `usdt-holder-wallet.*` (a real USD₮ holder — USD₮'s jetton wallet
//!   **is** a TON "precompiled contract") — the exact fixture an earlier
//!   version of this module (synthetic storage, guessed layout) produced
//!   a false positive against. Switching to 100% real bytes did **not**
//!   fix that specific false positive — it revealed a *different*, real
//!   cause: probing this jetton's raw on-chain bytecode via emulated TVM
//!   reverts with exit code 9 regardless of transfer amount, because real
//!   validators run this contract natively, never via interpreted TVM.
//!   This module now detects that case explicitly
//!   (`ProbeOutcome::PrecompiledContractUnsupported`) instead of
//!   misreporting either `Executed` or `Reverted`.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "docs/ARCHITECTURE.md §3.2 scopes the no-unwrap/expect/panic policy to library code outside tests"
)]

use std::path::{Path, PathBuf};

use sting_emulator_sys::LibEmulator;
use sting_scamcheck::honeypot::{HoneypotProbe, ProbeOutcome, probe_transfer};
use tonlib_core::TonAddress;
use tonlib_core::cell::{ArcCell, BagOfCells};

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

fn read_cell(fixture_name: &str) -> ArcCell {
    let boc = read_fixture(fixture_name);
    let bag = BagOfCells::parse_base64(boc.trim())
        .unwrap_or_else(|e| panic!("parse {fixture_name}: {e}"));
    bag.single_root()
        .unwrap_or_else(|e| panic!("single root in {fixture_name}: {e}"))
        .clone()
}

/// Ground truth for `ston-fi-lp-holder-wallet.*`
/// (`corpus/fixtures/emulator/SOURCE.md`): a real STON.fi LP-jetton
/// holder, not a precompiled contract.
const LP_HOLDER_OWNER_HEX: &str =
    "0:c0fe7eae7e328d5d19c8dfbd8e7e592fc7dea5b90b982bad72959b571cf861d7";
const LP_HOLDER_WALLET_HEX: &str =
    "0:612b9fca34014506ee76d58c7a8d4e0c765171115412652af4fb8de4f77622c2";
const LP_HOLDER_BALANCE: u64 = 54_332_047_944;
const LP_HOLDER_UNIXTIME: u32 = 1_784_655_644;
const LP_HOLDER_LT: u64 = 91_656_448_000_021;

/// Ground truth for `usdt-holder-wallet.*` — a real USD₮ holder, whose
/// jetton wallet **is** a TON precompiled contract.
const USDT_OWNER_HEX: &str = "0:88f51de0df12207257c6aafd6112e0ca6781e564be8fbeb3914b875a63222249";
const USDT_WALLET_HEX: &str = "0:00000dc46f93a5b60d8f8933ccb567e9064de322e4348da6fd35a7bb2de44a44";
const USDT_BALANCE: u64 = 14_539_900;
const USDT_UNIXTIME: u32 = 1_729_780_530;
const USDT_LT: u64 = 50_234_963_000_001;

fn probe(
    code_fixture: &str,
    data_fixture: &str,
    owner_hex: &str,
    wallet_hex: &str,
    transfer_amount: u64,
) -> HoneypotProbe {
    HoneypotProbe {
        wallet_code: read_cell(code_fixture),
        wallet_data: read_cell(data_fixture),
        wallet_address: wallet_hex.parse().expect("parse real wallet address"),
        owner: owner_hex.parse().expect("parse real owner"),
        transfer_amount,
        destination: TonAddress::new(0, [0x22; 32].into()),
    }
}

#[test]
fn a_real_non_precompiled_holder_can_sell_within_balance() {
    let Some(lib) = load_lib_or_skip() else {
        return;
    };
    let probe = probe(
        "ston-fi-lp-holder-wallet.code.boc.base64",
        "ston-fi-lp-holder-wallet.data.boc.base64",
        LP_HOLDER_OWNER_HEX,
        LP_HOLDER_WALLET_HEX,
        LP_HOLDER_BALANCE / 2,
    );
    let config_boc = read_fixture("mainnet-config.boc.base64");
    let outcome = probe_transfer(
        &lib,
        config_boc.trim(),
        &probe,
        LP_HOLDER_UNIXTIME,
        LP_HOLDER_LT,
    )
    .expect("probe_transfer failed outright");
    assert_eq!(outcome, ProbeOutcome::Executed);
}

#[test]
fn a_real_non_precompiled_holder_reverts_when_exceeding_real_balance() {
    let Some(lib) = load_lib_or_skip() else {
        return;
    };
    let probe = probe(
        "ston-fi-lp-holder-wallet.code.boc.base64",
        "ston-fi-lp-holder-wallet.data.boc.base64",
        LP_HOLDER_OWNER_HEX,
        LP_HOLDER_WALLET_HEX,
        LP_HOLDER_BALANCE + 1,
    );
    let config_boc = read_fixture("mainnet-config.boc.base64");
    let outcome = probe_transfer(
        &lib,
        config_boc.trim(),
        &probe,
        LP_HOLDER_UNIXTIME,
        LP_HOLDER_LT,
    )
    .expect("probe_transfer failed outright");
    assert!(
        matches!(outcome, ProbeOutcome::Reverted { .. }),
        "exceeding the real balance should revert for a real, contract-enforced reason — \
         got {outcome:?}"
    );
}

#[test]
fn a_real_precompiled_usdt_holder_is_reported_as_unsupported_not_misdiagnosed() {
    let Some(lib) = load_lib_or_skip() else {
        return;
    };
    let probe = probe(
        "usdt-holder-wallet.code.boc.base64",
        "usdt-holder-wallet.data.boc.base64",
        USDT_OWNER_HEX,
        USDT_WALLET_HEX,
        USDT_BALANCE / 2,
    );
    let config_boc = read_fixture("mainnet-config.boc.base64");
    let outcome = probe_transfer(&lib, config_boc.trim(), &probe, USDT_UNIXTIME, USDT_LT)
        .expect("probe_transfer failed outright");
    assert_eq!(
        outcome,
        ProbeOutcome::PrecompiledContractUnsupported,
        "USD₮'s jetton wallet is a real TON precompiled contract — this must be reported \
         honestly as unsupported, not misdiagnosed as Executed (the original false positive \
         this fixture exposed) or Reverted (an equally wrong false-suspicious signal)"
    );
}
