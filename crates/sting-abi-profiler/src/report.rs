//! Timing measurement primitives and the table this crate's whole reason
//! for existing renders: "which part of the code takes how much time",
//! answered with real numbers, not sampled call stacks — see this crate's
//! own `src/main.rs` module doc comment for why a coarse, hand-instrumented
//! table was chosen over a sampling profiler (`samply`) on this platform.

use std::fmt::Write as _;
use std::time::{Duration, Instant};

/// One measured stage: a named unit of work, timed by running it
/// repeatedly for at least [`MIN_STAGE_DURATION`] wall-clock time so the
/// reported average isn't noise from a single fast call.
#[derive(Debug, Clone)]
pub struct StageResult {
    /// Which pipeline this stage belongs to (e.g. `"Tolk"`, `"Tact"`,
    /// `"FunC"`, `"Tier 3 probe"`).
    pub component: &'static str,
    /// What this stage measures (e.g. `"compile (tolk subprocess)"`).
    pub stage: &'static str,
    /// How many times the stage body ran.
    pub calls: u64,
    /// Total wall-clock time across every call.
    pub total: Duration,
    /// A free-form note — e.g. "derived: full − parse", or a corpus size.
    pub note: String,
}

impl StageResult {
    /// Average wall-clock time per call — `Duration::ZERO` if never called
    /// (a skipped stage still gets a placeholder row, never a divide panic).
    #[must_use]
    pub fn avg(&self) -> Duration {
        self.total
            .checked_div(u32::try_from(self.calls.max(1)).unwrap_or(u32::MAX))
            .unwrap_or(Duration::ZERO)
    }
}

/// Minimum wall-clock budget [`time_stage`] runs a stage's body for before
/// stopping — long enough that OS scheduling jitter/timer resolution
/// doesn't dominate a single call's reported average, short enough that the
/// whole suite finishes in seconds, not minutes. `500ms` balances two real
/// stages at opposite extremes: `Tolk` mapping (single-digit
/// microseconds/call, tens of thousands of samples at this budget) and
/// `Tolk`'s own subprocess compile (tens of milliseconds/call — 500ms
/// yields a handful of real samples, enough to smooth out one-off process-
/// scheduling noise without a multi-second run).
pub const MIN_STAGE_DURATION: Duration = Duration::from_millis(500);

/// A safety upper bound on calls, independent of elapsed time — exists only
/// to bound genuinely pathological cases (a stage whose body is somehow
/// both slow *and* never lets [`MIN_STAGE_DURATION`] elapse due to timer
/// quirks), not to cut a fast stage's loop short. Deliberately far above
/// what any real stage in this crate needs: even the fastest measured stage
/// (`Tolk` mapping, single-digit microseconds/call) needs only tens of
/// thousands of calls to fill [`MIN_STAGE_DURATION`], comfortably under
/// this cap.
pub const MAX_STAGE_CALLS: u64 = 500_000;

/// Runs `body` repeatedly until [`MIN_STAGE_DURATION`] has elapsed or
/// [`MAX_STAGE_CALLS`] calls have run, whichever comes first, and returns
/// the aggregate [`StageResult`] — the one timing primitive every
/// `profile_*` module in this crate builds on, so every stage in the final
/// table was measured the same way.
pub fn time_stage<F: FnMut()>(
    component: &'static str,
    stage: &'static str,
    note: impl Into<String>,
    mut body: F,
) -> StageResult {
    let start = Instant::now();
    let mut calls = 0u64;
    loop {
        body();
        calls += 1;
        if start.elapsed() >= MIN_STAGE_DURATION || calls >= MAX_STAGE_CALLS {
            break;
        }
    }
    StageResult {
        component,
        stage,
        calls,
        total: start.elapsed(),
        note: note.into(),
    }
}

/// A stage that could not run at all (missing vendored compiler/binary) —
/// still gets a row in the final table, `calls: 0`, so a reader sees
/// exactly what was skipped and why, never a silently absent component.
#[must_use]
pub fn skipped_stage(
    component: &'static str,
    stage: &'static str,
    reason: impl Into<String>,
) -> StageResult {
    StageResult {
        component,
        stage,
        calls: 0,
        total: Duration::ZERO,
        note: format!("SKIPPED: {}", reason.into()),
    }
}

#[allow(
    clippy::cast_precision_loss,
    reason = "nanos is a report-formatting value bounded by realistic stage durations (this crate's own stages run for at most low-single-digit seconds) — always far under f64's exact-integer range (2^53ns ≈ 104 days), never a real precision concern"
)]
fn format_duration(d: Duration) -> String {
    let nanos = d.as_nanos();
    if nanos == 0 {
        return "-".to_string();
    }
    if nanos < 1_000 {
        format!("{nanos}ns")
    } else if nanos < 1_000_000 {
        format!("{:.2}us", nanos as f64 / 1_000.0)
    } else if nanos < 1_000_000_000 {
        format!("{:.3}ms", nanos as f64 / 1_000_000.0)
    } else {
        format!("{:.3}s", nanos as f64 / 1_000_000_000.0)
    }
}

/// Renders every [`StageResult`] as one markdown table, columns sorted by
/// component (input order preserved) then descending by average time
/// within each component — the slowest, most interesting stage of each
/// component leads.
#[must_use]
pub fn render_table(results: &[StageResult]) -> String {
    let mut rows = results.to_vec();
    // Stable sort by component (preserves caller's component ordering),
    // then descending average within each component — `sort_by_key` alone
    // would re-sort components alphabetically, losing the caller's
    // intentional Tolk/Tact/FunC/Tier-3 ordering.
    let component_order: Vec<&str> = {
        let mut seen = Vec::new();
        for r in &rows {
            if !seen.contains(&r.component) {
                seen.push(r.component);
            }
        }
        seen
    };
    rows.sort_by_key(|r| {
        let comp_rank = component_order
            .iter()
            .position(|c| *c == r.component)
            .unwrap_or(usize::MAX);
        (comp_rank, std::cmp::Reverse(r.avg()))
    });

    let mut out = String::new();
    out.push_str("| Component | Stage | Calls | Total | Avg/call | Note |\n");
    out.push_str("|---|---|---|---|---|---|\n");
    for r in &rows {
        let _ = writeln!(
            out,
            "| {} | {} | {} | {} | {} | {} |",
            r.component,
            r.stage,
            r.calls,
            format_duration(r.total),
            format_duration(r.avg()),
            if r.note.is_empty() { "-" } else { &r.note },
        );
    }
    out
}
