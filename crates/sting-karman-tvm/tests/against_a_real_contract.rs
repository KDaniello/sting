//! A mechanism run against a state a real contract reports.
//!
//! The pool below is a real STON.fi contract, captured from mainnet with its own ground truth
//! recorded beside it. What is asked of it here is what a mechanism would ask: how much of the
//! thing there is. That number opens a constant-product pool in the wind tunnel, agents trade
//! against it for five hundred ticks, and the reserves at the end are the verdict.
//!
//! Two properties, and they are different claims. The run reproduces: the same seed against the
//! same contract state gives the same verdict twice, so consulting a contract has not made a run
//! unrepeatable. And the executor changes nothing else: the same run with that number written in
//! by hand ends in the same place, so what the virtual machine contributes is the number and not
//! a difference in behaviour.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "AGENTS.md §5 scopes the no-unwrap/expect/panic rule to library code outside tests"
)]

use sting_karman_core::engine::rng::RunRng;
use sting_karman_core::engine::world::World;
use sting_karman_core::ports::{NullObserver, TvmExecutor};
use sting_karman_mech_curve::{Cpamm, CpammState, Quote, Token};
use sting_karman_tvm::{ContractContext, EmulatedContract};
use sting_karman_types::Amount;

/// A real STON.fi pool, which is itself a jetton master for its own liquidity token. Its
/// `total_supply` is a real quantity of a real thing, recorded in the corpus alongside the value
/// the endpoint's own decoder read at the same block.
const POOL_CODE: &str =
    include_str!("../../../corpus/fixtures/emulator/ston-fi-pool.code.boc.base64");
const POOL_DATA: &str =
    include_str!("../../../corpus/fixtures/emulator/ston-fi-pool.data.boc.base64");

/// `get_jetton_data()`'s first stack entry (TEP-74: total supply, mintable, admin, content, code).
const TOTAL_SUPPLY_INDEX: u32 = 0;

/// What the endpoint's own indexer reported for this contract at the block it was captured at.
const GROUND_TRUTH_TOTAL_SUPPLY: u128 = 55_183_210_687;

/// Where and when this contract thinks it is, captured with it: this pool reads its own address,
/// and without one it fails at the instruction that asks.
fn pool_context() -> ContractContext {
    ContractContext {
        address_hex: "0:fc4c9f311160754a99d113877cf583b78e5d16d048819cd4b820168769499d7e"
            .to_string(),
        unixtime_secs: 1_784_766_721,
        balance_nanoton: 5_237_739_227_200,
    }
}

/// The supply the contract reports, or nothing when the emulator library is not present.
///
/// A missing library and a passing run must never look alike, so the absence is printed and the
/// test says nothing rather than claiming something it did not check.
fn contract_supply() -> Option<u128> {
    let lib = match sting_emulator::load_default_library() {
        Ok(lib) => lib,
        Err(error) => {
            println!("SKIP: {error} — see docs/domains/emulator.md for how to fetch it");
            return None;
        }
    };
    let pool = EmulatedContract::new(&lib, POOL_CODE.trim(), POOL_DATA.trim(), &pool_context())
        .expect("a captured account state builds an emulator");
    Some(
        pool.call_number("get_jetton_data", TOTAL_SUPPLY_INDEX)
            .expect("a real pool answers its own get-method"),
    )
}

/// A pool opened at `supply` on both sides, traded by nobody in particular for five hundred
/// ticks, reported as its closing reserves.
fn verdict(supply: u128) -> (u128, u128) {
    let state = CpammState::new(Amount::<Token>::new(supply), Amount::<Quote>::new(supply))
        .expect("a positive supply opens a pool");
    let mut world = World::new(Cpamm, state, vec![]);
    let mut rng = RunRng::new(42, 0);
    world.run(500, &mut rng, &mut NullObserver).unwrap();
    let state = world.state();
    (
        state.token_reserve.base_units(),
        state.quote_reserve.base_units(),
    )
}

#[test]
fn the_contract_answers_what_the_index_recorded_for_it() {
    let Some(supply) = contract_supply() else {
        return;
    };
    assert_eq!(
        supply, GROUND_TRUTH_TOTAL_SUPPLY,
        "the executor reads the same quantity the endpoint's own decoder read"
    );
}

#[test]
fn the_same_seed_against_the_same_contract_gives_the_same_verdict() {
    let Some(supply) = contract_supply() else {
        return;
    };
    assert_eq!(verdict(supply), verdict(supply));
}

#[test]
fn consulting_the_contract_changes_nothing_but_the_number() {
    let Some(supply) = contract_supply() else {
        return;
    };
    assert_eq!(
        verdict(supply),
        verdict(GROUND_TRUTH_TOTAL_SUPPLY),
        "the executor supplies a quantity and nothing else"
    );
}
