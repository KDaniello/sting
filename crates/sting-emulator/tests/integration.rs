//! Integration tests against a real, vendored `libemulator` build
//! (`docs/domains/emulator.md`).
//!
//! `vendor/libemulator/` is `.gitignore`d (a large, platform-specific
//! binary, see that path's comment in `.gitignore`) — a fresh checkout
//! without it present is expected and not a failure. Every test here calls
//! [`load_lib_or_skip`] first and returns early (a trivial pass, Rust's
//! stable test harness has no real "skipped" status) when the library is
//! absent, so `cargo test --workspace` stays green on a machine that never
//! fetched it. `.github/workflows/emulator.yml` fetches the real per-OS
//! binary before running this crate's tests specifically so these checks are
//! not permanently skipped in CI on either platform that matters, Windows and
//! Linux.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "docs/ARCHITECTURE.md §3.2 scopes the no-unwrap/expect/panic policy to library code outside tests"
)]

use std::path::{Path, PathBuf};

use sting_emulator::tlb::{
    ActiveAccountState, InternalMessageParams, build_internal_message, build_shard_account,
};
use sting_emulator::{EmulatorConfig, TransactionEmulator, TvmEmulator, set_verbosity_level};
use sting_emulator_sys::LibEmulator;
use tonlib_core::TonAddress;
use tonlib_core::cell::{BagOfCells, CellBuilder};

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// Loads the real vendored library, or prints a message and returns `None`
/// if it is not present locally (see this module's doc comment).
fn load_lib_or_skip() -> Option<LibEmulator> {
    match sting_emulator::load_default_library() {
        Ok(lib) => Some(lib),
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

/// The direct, empirical check behind the core claim: every prior
/// third-party binding (pytvm/tongo/ton4j) leaks a native allocation per
/// `run_get_method` call because none of them call `string_destroy()`; this
/// crate's [`sting_emulator_sys::LibEmulator::run_get_method`] does. 10,000
/// `create→set_c7→run→destroy` cycles should not grow RSS linearly.
const LEAK_CHECK_ITERATIONS: u32 = 10_000;
// Generous threshold: a real leak here is one full strdup'd JSON response
// string (order of magnitude: hundreds of bytes) retained *every single*
// call — that would show up as multiple MiB of growth, not noise. A few
// hundred KiB of total drift over 10,000 iterations is consistent with
// allocator fragmentation/bookkeeping, not a per-call leak.
const LEAK_THRESHOLD_KIB: u64 = 2048;

#[test]
fn ten_thousand_cycles_of_create_call_destroy_do_not_leak_memory() {
    let Some(lib) = load_lib_or_skip() else {
        return;
    };
    let code_boc = read_fixture("lp-jetton-master.code.boc.base64");
    let data_boc = read_fixture("lp-jetton-master.data.boc.base64");
    let code_boc = code_boc.trim();
    let data_boc = data_boc.trim();

    let pid = sysinfo::get_current_pid().expect("failed to get current pid");
    let mut sys = sysinfo::System::new();
    let rss = |sys: &mut sysinfo::System| -> u64 {
        sys.refresh_processes(sysinfo::ProcessesToUpdate::Some(&[pid]), true);
        sys.process(pid)
            .map(sysinfo::Process::memory)
            .unwrap_or_default()
    };

    let run_once = || {
        let emulator =
            TvmEmulator::new(&lib, code_boc, data_boc).expect("tvm_emulator_create failed");
        emulator
            .set_c7(
                "0:000011a50b2cb855dd55972ea27e8fbb76c3eaf135918723849b0e5bdb69eae6",
                1_784_240_154,
                85_776_672,
                None,
            )
            .expect("tvm_emulator_set_c7 failed");
        let response = emulator
            .run_get_method_no_args("get_jetton_data")
            .expect("run_get_method failed");
        assert!(response.success, "unexpected emulator failure mid-loop");
    };

    // Warm up: let allocator/OS-level bookkeeping settle before the "before"
    // measurement, so steady-state behavior is what gets measured, not startup.
    for _ in 0..200 {
        run_once();
    }
    let rss_before = rss(&mut sys);

    for _ in 0..LEAK_CHECK_ITERATIONS {
        run_once();
    }
    let rss_after = rss(&mut sys);

    let delta_kib = rss_after.saturating_sub(rss_before) / 1024;
    println!(
        "RSS before={} KiB after={} KiB delta={delta_kib} KiB over {LEAK_CHECK_ITERATIONS} calls",
        rss_before / 1024,
        rss_after / 1024
    );
    assert!(
        delta_kib <= LEAK_THRESHOLD_KIB,
        "suspected leak: RSS grew by {delta_kib} KiB over {LEAK_CHECK_ITERATIONS} cycles, above the \
         {LEAK_THRESHOLD_KIB} KiB noise threshold"
    );
}

/// `EmulatorConfig` is meant to be created once and reused across many calls
/// to either emulator type without re-parsing the config `BoC` — this proves
/// that sharing pattern actually works against a real ~109 KiB mainnet
/// config, not just that each type compiles independently.
#[test]
fn emulator_config_object_is_reusable_across_tvm_and_transaction_emulators() {
    let Some(lib) = load_lib_or_skip() else {
        return;
    };
    let config_boc = read_fixture("mainnet-config.boc.base64");
    let config_boc = config_boc.trim();
    let config = EmulatorConfig::new(&lib, config_boc).expect("emulator_config_create failed");

    let code_boc = read_fixture("lp-jetton-master.code.boc.base64");
    let data_boc = read_fixture("lp-jetton-master.data.boc.base64");
    let tvm = TvmEmulator::new(&lib, code_boc.trim(), data_boc.trim())
        .expect("tvm_emulator_create failed");
    tvm.set_config_object(&config)
        .expect("tvm_emulator_set_config_object failed against a real config object");

    let txn =
        TransactionEmulator::new(&lib, config_boc, 0).expect("transaction_emulator_create failed");
    txn.set_config_object(&config)
        .expect("transaction_emulator_set_config_object failed against a real config object");
}

/// `TransactionEmulator`'s RAII and setter surface, exercised against a real
/// mainnet blockchain config (not a hand-rolled or empty one) — proves the
/// bindings work against genuine data. A full `emulate_transaction`
/// end-to-end run (needing a matching real `ShardAccount`+`Message` pair)
/// was deferred for lack of a way to build either — see
/// [`emulate_transaction_runs_against_a_real_contract_with_a_built_shard_account_and_message`]
/// below, added once `sting_emulator::tlb` gave this test a real consumer.
#[test]
fn transaction_emulator_creates_and_configures_against_a_real_mainnet_config() {
    let Some(lib) = load_lib_or_skip() else {
        return;
    };
    let config_boc = read_fixture("mainnet-config.boc.base64");
    let emulator = TransactionEmulator::new(&lib, config_boc.trim(), 0)
        .expect("transaction_emulator_create failed");

    emulator
        .set_unixtime(1_784_240_154)
        .expect("set_unixtime failed");
    emulator.set_lt(50_000_000_000).expect("set_lt failed");
    emulator
        .set_rand_seed(&"0".repeat(64))
        .expect("set_rand_seed failed");
    emulator
        .set_ignore_chksig(true)
        .expect("set_ignore_chksig failed");
    emulator
        .set_debug_enabled(true)
        .expect("set_debug_enabled failed");
    // `emulator` drops here — Drop calls transaction_emulator_destroy. If
    // this test doesn't crash or hang, RAII + every bound setter above works
    // against real data, not just against `null`/garbage inputs.
}

/// The library-wide verbosity toggle against a real loaded library.
#[test]
fn set_verbosity_level_succeeds_against_a_real_library() {
    let Some(lib) = load_lib_or_skip() else {
        return;
    };
    assert!(set_verbosity_level(&lib, 0));
}

/// `tvm_emulator_emulate_run_method_detailed` is fully bound (unlike the
/// leaky non-detailed variant, which is never called at all — see
/// `sting-emulator-sys`'s module doc comment) but has no safe wrapper in
/// this crate (see its module doc comment for why). This still
/// exercises its one always-reachable-without-a-hand-built-c7-tuple path
/// directly through `sting-emulator-sys`: malformed/too-short `params_boc`
/// must come back as `None`, not a crash — the same "fails cleanly, not
/// catastrophically" bar every other binding in this crate is held to.
#[test]
fn emulate_run_method_detailed_handles_malformed_params_without_crashing() {
    let Some(lib) = load_lib_or_skip() else {
        return;
    };
    // SAFETY: `lib` was just loaded from a real, ABI-compatible build above;
    // an empty byte slice is a valid (zero-length) buffer for the call.
    let result = unsafe { lib.emulate_run_method_detailed(&[], 1_000_000) };
    assert!(
        result.is_none(),
        "malformed params_boc should not produce a spurious successful result"
    );
}

/// The real end-to-end path `sting_emulator::tlb` exists for:
/// `build_shard_account` + `build_internal_message` feeding
/// `TransactionEmulator::emulate_transaction` against a real, currently-
/// deployed contract's real code+data (the same LP jetton master fixture the
/// leak-check test above uses) and a real mainnet config — not a hand-rolled
/// or empty account/message pair. There is no independent transaction-level
/// ground truth for this specific contract to assert against (unlike
/// `run_get_jetton_data.rs`'s get-method ground truth, cross-checked against
/// toncenter's own `runGetMethod`), so this test's bar is the one this
/// module's own doc comment sets: a real `ShardAccount`+`Message` pair must
/// make it all the way to genuine TVM opcode execution, not fail at
/// account-state or message-envelope parsing — the exact failure mode a
/// wrong TL-B encoding would produce, and the reason
/// `TvmEmulator::send_internal_message` was not enough for a contract that
/// reads its own forward fee ('s
/// real-target audit work, where this gap was first hit for real).
#[test]
fn emulate_transaction_runs_against_a_real_contract_with_a_built_shard_account_and_message() {
    let Some(lib) = load_lib_or_skip() else {
        return;
    };
    let config_boc = read_fixture("mainnet-config.boc.base64");
    let code_boc = read_fixture("lp-jetton-master.code.boc.base64");
    let data_boc = read_fixture("lp-jetton-master.data.boc.base64");

    let dest = TonAddress::from_hex_str(
        "0:000011a50b2cb855dd55972ea27e8fbb76c3eaf135918723849b0e5bdb69eae6",
    )
    .expect("parse the fixture's own known-good address");

    let shard_account_boc = build_shard_account(&ActiveAccountState {
        address: dest.clone(),
        balance_nanotons: 85_776_672, // same ground-truth balance run_get_jetton_data.rs's set_c7 call uses
        code_boc_base64: code_boc.trim().to_string(),
        data_boc_base64: data_boc.trim().to_string(),
        last_transaction_lt: 50_000_000_000,
        last_transaction_hash: [0; 32],
        last_paid_unixtime: 1_784_240_154,
        storage_used_cells: 10,
        storage_used_bits: 5_000,
    })
    .expect("build_shard_account must succeed against real fixture code/data");

    // An arbitrary, syntactically-opaque body (this fixture's own message
    // vocabulary is not documented anywhere this crate can cite) — the
    // point is not "the jetton master accepts this particular op", only
    // that a real transaction gets emulated at all, reaching genuine TVM
    // execution rather than failing at account/message parsing.
    let mut body_builder = CellBuilder::new();
    body_builder.store_u32(32, 0x0000_0000).expect("build body");
    let body_cell = body_builder.build().expect("build body cell");
    let body_bytes = BagOfCells::from_root(body_cell)
        .serialize(false)
        .expect("serialize body");
    let body_boc_base64 =
        base64::Engine::encode(&base64::engine::general_purpose::STANDARD, body_bytes);

    let src = TonAddress::new(0, [0x42; 32].into());
    let message_boc = build_internal_message(&InternalMessageParams {
        src,
        dest,
        value_nanotons: 50_000_000,
        bounce: true,
        fwd_fee_nanotons: 1_000_000,
        created_lt: 50_000_000_001,
        created_at: 1_784_240_154,
        body_boc_base64,
    })
    .expect("build_internal_message must succeed");

    let emulator = TransactionEmulator::new(&lib, config_boc.trim(), 1)
        .expect("transaction_emulator_create failed");
    emulator
        .set_unixtime(1_784_240_154)
        .expect("set_unixtime failed");
    emulator.set_lt(50_000_000_001).expect("set_lt failed");

    let response = emulator
        .emulate_transaction(&shard_account_boc, &message_boc)
        .expect("emulate_transaction call failed outright");

    let vm_log = response.vm_log.clone().unwrap_or_default();
    println!("success={} error={:?}", response.success, response.error);
    println!("--- vm_log ---\n{vm_log}");

    // The real bar: did TVM actually start executing this account's real
    // code (visible as `execute ...` opcode lines), as opposed to rejecting
    // the input at the ShardAccount/Message envelope level (which would
    // show up as `response.success == false` with a parsing-flavored
    // `error`, and no VM opcode trace at all). Whether the jetton master
    // *likes* opcode `0x00000000` is not the question this test asks.
    assert!(
        vm_log.contains("execute "),
        "expected a real VM opcode trace proving the built ShardAccount/Message parsed \
         correctly and reached genuine execution; got success={} error={:?} vm_log={vm_log:?}",
        response.success,
        response.error
    );
}

/// The refusal has to sit on every path a config takes into the emulator, not
/// on one of them: a version checked in one entry point and not the next is a
/// version nobody checked.
#[test]
fn every_config_entry_point_refuses_a_config_it_cannot_read() {
    let Some(lib) = load_lib_or_skip() else {
        return;
    };
    let config = read_fixture("mainnet-config.boc.base64");
    let emulator =
        TransactionEmulator::new(&lib, config.trim(), 0).expect("the vendored config is the pin");
    let tvm = TvmEmulator::new(
        &lib,
        read_fixture("lp-jetton-master.code.boc.base64").trim(),
        read_fixture("lp-jetton-master.data.boc.base64").trim(),
    )
    .expect("the fixture loads");

    let refusals: Vec<Result<(), sting_emulator::EmulatorError>> = vec![
        TransactionEmulator::new(&lib, "not a config at all", 0).map(|_| ()),
        EmulatorConfig::new(&lib, "not a config at all").map(|_| ()),
        emulator.set_config("not a config at all"),
        tvm.set_c7(
            "0:0000000000000000000000000000000000000000000000000000000000000000",
            0,
            0,
            Some("not a config at all"),
        ),
    ];
    for refusal in refusals {
        let error = refusal.expect_err("a config the tree cannot read must not reach the library");
        assert!(
            matches!(error, sting_emulator::EmulatorError::Config(_)),
            "got {error:?}"
        );
    }
}
