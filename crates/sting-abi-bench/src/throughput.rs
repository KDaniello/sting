//! Throughput/memory measurement —
//! `docs/domains/abi-ir.md`'s targets (≥4
//! contracts/sec for Tolk/Tact batch on 8 cores, ≥50 contracts/sec for
//! `FunC`, a documented memory upper bound), measured in the same run as
//! accuracy rather than in a separate one.
//!
//! Uses `rayon`'s global thread pool for parallel batch extraction (same
//! mechanism `sting-abi-service`'s own `/v1/extract/batch` already uses,
//! `crates/sting-abi-service/src/routes/extract.rs`) and `sysinfo` to
//! sample this process's own RSS before/after — a coarse, real measurement
//! (not a profiler), sufficient to answer "what's the memory budget for N
//! parallel extractions," the actual question §5.2 asks.

use std::time::Instant;

use rayon::prelude::*;

/// One batch throughput measurement.
#[derive(Debug, Clone, Copy, serde::Serialize)]
pub struct ThroughputResult {
    /// How many contracts were extracted.
    pub contract_count: usize,
    /// How many succeeded (a failure still counts toward wall-clock time —
    /// throughput is measured against the attempted batch, not just the
    /// successful subset, since a real batch endpoint pays that cost too).
    pub success_count: usize,
    /// Wall-clock seconds for the whole batch.
    pub elapsed_secs: f64,
    /// `contract_count / elapsed_secs`.
    pub contracts_per_sec: f64,
    /// This process's resident set size delta (bytes) across the batch —
    /// `None` if `sysinfo` could not read process memory on this platform.
    pub rss_delta_bytes: Option<u64>,
}

/// Runs `extract_one` over every item in `items` in parallel (via `rayon`'s
/// global pool, matching `sting-abi-service`'s own batch endpoint), timing
/// the whole batch and sampling RSS before/after.
pub fn measure_batch<T: Sync, F: Fn(&T) -> bool + Sync>(
    items: &[T],
    extract_one: F,
) -> ThroughputResult {
    let mut system = sysinfo::System::new();
    let pid = sysinfo::get_current_pid().ok();
    let before_rss = pid.and_then(|pid| {
        system.refresh_processes(sysinfo::ProcessesToUpdate::Some(&[pid]), true);
        system.process(pid).map(sysinfo::Process::memory)
    });

    let start = Instant::now();
    let success_count = items.par_iter().filter(|item| extract_one(item)).count();
    let elapsed = start.elapsed();

    let after_rss = pid.and_then(|pid| {
        system.refresh_processes(sysinfo::ProcessesToUpdate::Some(&[pid]), true);
        system.process(pid).map(sysinfo::Process::memory)
    });

    let elapsed_secs = elapsed.as_secs_f64();
    // `items.len()` is never remotely close to 2^52 (a real corpus is at
    // most thousands of contracts) — `as f64` losing precision above that
    // threshold is not a reachable concern here, just a lint that can't see
    // the practical bound.
    #[allow(clippy::cast_precision_loss)]
    let contracts_per_sec = if elapsed_secs > 0.0 {
        items.len() as f64 / elapsed_secs
    } else {
        0.0
    };

    ThroughputResult {
        contract_count: items.len(),
        success_count,
        elapsed_secs,
        contracts_per_sec,
        rss_delta_bytes: before_rss.zip(after_rss).map(|(b, a)| a.saturating_sub(b)),
    }
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "docs/ARCHITECTURE.md §3.2 scopes the no-unwrap/expect policy to library code outside tests"
)]
mod tests {
    use super::measure_batch;

    #[test]
    fn measures_a_trivial_batch_without_panicking() {
        let items = vec![1, 2, 3, 4, 5];
        let result = measure_batch(&items, |_| true);
        assert_eq!(result.contract_count, 5);
        assert_eq!(result.success_count, 5);
        assert!(result.contracts_per_sec >= 0.0);
    }

    #[test]
    fn failures_still_count_toward_the_batch_but_not_success_count() {
        let items = vec![1, 2, 3];
        let result = measure_batch(&items, |n| *n != 2);
        assert_eq!(result.contract_count, 3);
        assert_eq!(result.success_count, 2);
    }
}
