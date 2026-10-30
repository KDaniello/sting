//! A memory-leak check (docs/ARCHITECTURE.md): calls
//! `run_get_method` 10,000 times in a tight loop and compares process RSS
//! before/after. This is the direct, empirical test of the finding in
//! `docs/domains/emulator.md` that every prior third-party binding
//! (pytvm/tongo/ton4j) leaks a native allocation per call because none of
//! them call `string_destroy()` — this crate's [`sting_emulator_sys::LibEmulator::run_get_method`]
//! does, and this is where that claim gets checked against reality instead
//! of taken on faith.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "throwaway diagnostic binary, not shipped library code (docs/ARCHITECTURE.md §3.2)"
)]

use std::path::Path;

use sting_emulator::TvmEmulator;
use sting_emulator_sys::LibEmulator;
use sysinfo::{Pid, System};

const ITERATIONS: u32 = 10_000;

// Generous threshold: a real leak here is one full strdup'd JSON response
// string (order of magnitude: hundreds of bytes) retained *every single
// call* — that would show up as multiple MiB of growth, not noise. A few
// hundred KiB of total drift over 10,000 iterations is consistent with
// allocator fragmentation/bookkeeping, not a per-call leak.
const LEAK_THRESHOLD_KIB: u64 = 2048;

fn current_rss_bytes(sys: &mut System, pid: Pid) -> u64 {
    sys.refresh_processes(sysinfo::ProcessesToUpdate::Some(&[pid]), true);
    sys.process(pid)
        .map(sysinfo::Process::memory)
        .unwrap_or_default()
}

fn main() {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let repo_root = manifest_dir.join("../..");
    let lib_path = repo_root
        .join("vendor/libemulator")
        .join(sting_emulator_sys::platform_library_filename());
    let fixtures_dir = repo_root.join("corpus/fixtures/emulator");

    // SAFETY: the same vendored, project-downloaded library every other probe
    // here loads.
    let lib = unsafe { LibEmulator::load(&lib_path) }.unwrap_or_else(|e| {
        panic!(
            "failed to load libemulator from {}: {e}",
            lib_path.display()
        )
    });

    let code_boc =
        std::fs::read_to_string(fixtures_dir.join("lp-jetton-master.code.boc.base64")).unwrap();
    let data_boc =
        std::fs::read_to_string(fixtures_dir.join("lp-jetton-master.data.boc.base64")).unwrap();
    let code_boc = code_boc.trim();
    let data_boc = data_boc.trim();

    let pid = sysinfo::get_current_pid().expect("failed to get current pid");
    let mut sys = System::new();

    // Warm up: let allocator/OS-level bookkeeping (heap growth, page-in of
    // this binary's own code) settle before taking the "before" measurement,
    // so we are measuring the loop's steady-state behavior, not startup.
    for _ in 0..200 {
        run_once(&lib, code_boc, data_boc);
    }

    let rss_before = current_rss_bytes(&mut sys, pid);
    println!(
        "RSS before {ITERATIONS} iterations: {} KiB",
        rss_before / 1024
    );

    for i in 0..ITERATIONS {
        run_once(&lib, code_boc, data_boc);
        if i % 2000 == 0 {
            let rss = current_rss_bytes(&mut sys, pid);
            println!("  iteration {i}: RSS = {} KiB", rss / 1024);
        }
    }

    let rss_after = current_rss_bytes(&mut sys, pid);
    println!(
        "RSS after  {ITERATIONS} iterations: {} KiB",
        rss_after / 1024
    );

    let delta_kib = rss_after.saturating_sub(rss_before) / 1024;
    let per_call_bytes = if ITERATIONS > 0 {
        f64::from(u32::try_from(rss_after.saturating_sub(rss_before)).unwrap_or(u32::MAX))
            / f64::from(ITERATIONS)
    } else {
        0.0
    };
    println!("\ndelta: {delta_kib} KiB over {ITERATIONS} calls ({per_call_bytes:.2} bytes/call)");

    if delta_kib > LEAK_THRESHOLD_KIB {
        println!(
            "RESULT: SUSPECTED LEAK — RSS grew by {delta_kib} KiB, above the {LEAK_THRESHOLD_KIB} KiB noise threshold"
        );
        std::process::exit(1);
    }
    println!("RESULT: NO LEAK DETECTED — RSS growth within noise threshold");
}

fn run_once(lib: &LibEmulator, code_boc: &str, data_boc: &str) {
    let emulator = TvmEmulator::new(lib, code_boc, data_boc).expect("tvm_emulator_create failed");
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
    // `emulator` (and the JSON response string already freed inside
    // run_get_method) drops here — Drop calls tvm_emulator_destroy.
}
