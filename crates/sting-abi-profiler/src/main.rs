//! `sting-abi-profiler` — a permanent, standalone profiling harness for the
//! abi-layer extraction pipeline (Tolk/Tact/`FunC`) and the Tier 3
//! (`sting-abi-dictionary`) probe: real corpus in, a table of "which part
//! of the code takes how much time" out.
//!
//! **Why hand-instrumented timing, not a sampling profiler.** The natural
//! first choice was `samply` (cross-platform, pure Rust, no `perf`
//! dependency) — installed and working, but recording on this Windows
//! machine requires `xperf` (Windows Performance Toolkit, part of the
//! Windows ADK — a large download, and installing it typically needs admin
//! rights), neither of which is installed here without being asked for. Coarse, hand-picked stage boundaries (subprocess compile vs.
//! pure-Rust mapping; `tree-sitter` parse vs. heuristic extraction; network
//! fetch vs. `TvmEmulator` setup vs. the per-method-id emulation loop) give
//! real, load-bearing answers to "where does the time go" without needing
//! call-stack sampling — this crate's own module boundaries already line
//! up with the questions worth asking (subprocess cost vs. our own Rust
//! code; parsing vs. our own heuristics).
//!
//! **Why a separate crate, not throwaway `examples/` + reverted
//! instrumentation.** An earlier pass at this used exactly that — one-off
//! example binaries plus temporary `eprintln!` timers inside
//! `sting-abi-func`/`sting-abi-tact` that were reverted right after. Real
//! numbers, but nothing to re-run later without redoing the same manual
//! surgery. This crate is the permanent version: `cargo run --release -p
//! sting-abi-profiler` any time this workspace's performance
//! characteristics are in question again, no library-crate edits, no
//! cleanup step.
//!
//! **Scope, honestly bounded**: covers the abi-layer extractors + Tier 3
//! probe (what this profiling pass was actually asked to cover) — not the
//! security branch's detector pipeline (`sting-detectors`/`sting-dataflow`),
//! which was explicitly not selected for this pass and would be a natural,
//! separate `profile_security` module to add here later, not a reason to
//! block on today.

mod dictionary;
mod func;
mod report;
mod tact;
mod tolk;

use std::path::PathBuf;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn main() {
    let root = repo_root();
    let mut results = Vec::new();

    eprintln!("Profiling Tolk (compile + mapping)...");
    results.extend(tolk::profile(&root));

    eprintln!("Profiling Tact (compile + mapping)...");
    results.extend(tact::profile(&root));

    eprintln!("Profiling FunC (parse + full extract)...");
    results.extend(func::profile(&root));

    eprintln!("Profiling Tier 3 dictionary probe (fetch + setup + emulation loop)...");
    results.extend(dictionary::profile(&root));

    eprintln!();
    println!("{}", report::render_table(&results));
}
