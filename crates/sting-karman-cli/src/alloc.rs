//! A counting global allocator: memory observability without an OS-specific RSS reader.
//!
//! Counting at the allocator gives numbers RSS cannot: total bytes ever allocated (allocation
//! pressure), live bytes, and the live peak — per process, exact, on every platform. The
//! allocator's behaviour is `System`'s unchanged, so a binary built with these counters
//! allocates exactly like one without them.
//!
//! The counters are process-global relaxed atomics, which orders nothing but does move a cache
//! line between cores on every allocation. A workload allocating in an inner loop therefore
//! measures its own instrumentation rather than itself — the reason the engine reports events
//! into a buffer it reuses instead of a fresh collection per action.

use std::{
    alloc::{GlobalAlloc, Layout, System},
    sync::atomic::{AtomicU64, Ordering},
};

static TOTAL_ALLOCATED: AtomicU64 = AtomicU64::new(0);
static ALLOCATION_COUNT: AtomicU64 = AtomicU64::new(0);
static LIVE: AtomicU64 = AtomicU64::new(0);
static LIVE_PEAK: AtomicU64 = AtomicU64::new(0);

/// One reading of the allocation counters.
#[derive(Debug, Clone, Copy)]
pub(crate) struct AllocSnapshot {
    pub(crate) total_allocated_bytes: u64,
    pub(crate) allocation_count: u64,
    pub(crate) live_bytes: u64,
    pub(crate) live_peak_bytes: u64,
}

#[derive(Debug)]
pub(crate) struct CountingAlloc;

impl CountingAlloc {
    #[must_use]
    pub(crate) fn snapshot() -> AllocSnapshot {
        AllocSnapshot {
            total_allocated_bytes: TOTAL_ALLOCATED.load(Ordering::Relaxed),
            allocation_count: ALLOCATION_COUNT.load(Ordering::Relaxed),
            live_bytes: LIVE.load(Ordering::Relaxed),
            live_peak_bytes: LIVE_PEAK.load(Ordering::Relaxed),
        }
    }

    /// Restarts peak tracking from the current live level, so a measurement window observes
    /// its own peak rather than the process's history.
    pub(crate) fn reset_peak() {
        LIVE_PEAK.store(LIVE.load(Ordering::Relaxed), Ordering::Relaxed);
    }
}

fn record_alloc(size: u64) {
    TOTAL_ALLOCATED.fetch_add(size, Ordering::Relaxed);
    ALLOCATION_COUNT.fetch_add(1, Ordering::Relaxed);
    let live = LIVE.fetch_add(size, Ordering::Relaxed) + size;
    LIVE_PEAK.fetch_max(live, Ordering::Relaxed);
}

// SAFETY: every method delegates to `System` unchanged and only adds relaxed counter updates,
// so the GlobalAlloc contract (layout fidelity, uniqueness of returned pointers) is exactly
// `System`'s.
unsafe impl GlobalAlloc for CountingAlloc {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        // SAFETY: forwarded verbatim; the caller upholds GlobalAlloc's contract for `layout`.
        let ptr = unsafe { System.alloc(layout) };
        if !ptr.is_null() {
            record_alloc(layout.size() as u64);
        }
        ptr
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        // SAFETY: forwarded verbatim; `ptr` and `layout` come from a matching `alloc`.
        unsafe { System.dealloc(ptr, layout) };
        LIVE.fetch_sub(layout.size() as u64, Ordering::Relaxed);
    }
}
