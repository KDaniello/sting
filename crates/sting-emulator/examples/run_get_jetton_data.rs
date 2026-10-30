//! A diagnostic (docs/ARCHITECTURE.md): loads a
//! real `libemulator` build, runs `get_jetton_data()` against a real,
//! currently-deployed mainnet contract's code+data (vendored in
//! `corpus/fixtures/emulator/`), and compares the result against ground
//! truth independently obtained from toncenter's own `runGetMethod` (see
//! `corpus/fixtures/emulator/SOURCE.md`).
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "throwaway diagnostic binary, not shipped library code (docs/ARCHITECTURE.md §3.2)"
)]

use std::path::Path;

use sting_emulator::TvmEmulator;
use sting_emulator_sys::LibEmulator;

fn main() {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let repo_root = manifest_dir.join("../..");
    let lib_path = repo_root
        .join("vendor/libemulator")
        .join(sting_emulator_sys::platform_library_filename());
    let fixtures_dir = repo_root.join("corpus/fixtures/emulator");

    println!("loading {}", lib_path.display());
    // SAFETY: the library at `lib_path` was downloaded directly from the
    // ton-blockchain/ton v2026.06 GitHub release by this project, not from
    // an untrusted source.
    let lib = unsafe { LibEmulator::load(&lib_path) }.unwrap_or_else(|e| {
        panic!(
            "failed to load libemulator from {}: {e}",
            lib_path.display()
        )
    });

    println!("emulator_version() = {:?}", lib.version());

    let code_boc =
        std::fs::read_to_string(fixtures_dir.join("lp-jetton-master.code.boc.base64")).unwrap();
    let data_boc =
        std::fs::read_to_string(fixtures_dir.join("lp-jetton-master.data.boc.base64")).unwrap();

    let emulator = TvmEmulator::new(&lib, code_boc.trim(), data_boc.trim())
        .expect("tvm_emulator_create failed");

    // Ground truth fixture (see corpus/fixtures/emulator/SOURCE.md): the
    // contract's own raw address and balance at the block toncenter's
    // runGetMethod ran against. get_jetton_data() on this specific contract
    // needs its own address (MYADDR) to build a content URL, discovered by
    // first running this example *without* set_c7 and seeing VM exit code 9.
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

    println!("success       = {}", response.success);
    println!("vm_exit_code  = {:?}", response.vm_exit_code);
    println!("gas_used      = {:?}", response.gas_used);
    println!("error         = {:?}", response.error);
    match response.stack_depth() {
        Ok(depth) => println!("stack depth   = {depth} (ground truth: 5)"),
        Err(e) => println!("stack depth   = <failed to decode: {e}>"),
    }

    println!();
    let exit_code_ok = response.success && response.vm_exit_code == Some(0);
    let depth_ok = response.stack_depth().ok() == Some(5);
    if exit_code_ok && depth_ok {
        println!(
            "RESULT: MATCH — exit_code=0 and stack depth=5, consistent with toncenter's own runGetMethod"
        );
    } else {
        println!(
            "RESULT: MISMATCH — see corpus/fixtures/emulator/SOURCE.md for the expected ground truth"
        );
        std::process::exit(1);
    }
}
