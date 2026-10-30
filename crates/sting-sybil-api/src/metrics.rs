//! What the service says about itself: counters and approximate latencies, in the format a
//! scraper reads.
//!
//! Written by hand rather than through a metrics library, because what is exported is a handful
//! of counters and two percentiles rather than real histograms — a client library would be a
//! dependency tree for a page of text.
//!
//! The latency ring is atomic and unlocked. A mutex taken on every request would put contention
//! exactly on the path whose latency is the thing being measured. Two writers can overlap and
//! overwrite each other's slot; the percentiles are approximate by construction and say so, and
//! the ring holds only the last few thousand requests between two scrapes.

use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

const RING_SIZE: usize = 2048;

/// The counters and the latency ring.
pub struct Metrics {
    requests_total: AtomicU64,
    errors_total: AtomicU64,
    cache_hits_total: AtomicU64,
    cache_misses_total: AtomicU64,
    cache_hits_missing_cluster_signal_total: AtomicU64,
    latencies_ms: Vec<AtomicU64>,
    next_slot: AtomicUsize,
}

impl Default for Metrics {
    fn default() -> Self {
        Self::new()
    }
}

impl Metrics {
    /// Counters at zero and an empty ring.
    #[must_use]
    pub fn new() -> Self {
        Self {
            requests_total: AtomicU64::new(0),
            errors_total: AtomicU64::new(0),
            cache_hits_total: AtomicU64::new(0),
            cache_misses_total: AtomicU64::new(0),
            cache_hits_missing_cluster_signal_total: AtomicU64::new(0),
            latencies_ms: (0..RING_SIZE).map(|_| AtomicU64::new(0)).collect(),
            next_slot: AtomicUsize::new(0),
        }
    }

    /// Records one request.
    ///
    /// `cache_hit_missing_cluster_signal` is what makes a documented gap measurable: a hit whose
    /// features carry no cluster signal is scored on less evidence than the model was trained on,
    /// and counting those is the difference between a known limitation and an unknown one.
    pub fn record(
        &self,
        latency_ms: u64,
        cache_hit: bool,
        is_error: bool,
        cache_hit_missing_cluster_signal: bool,
    ) {
        self.requests_total.fetch_add(1, Ordering::Relaxed);
        if is_error {
            self.errors_total.fetch_add(1, Ordering::Relaxed);
        }
        if cache_hit {
            self.cache_hits_total.fetch_add(1, Ordering::Relaxed);
            if cache_hit_missing_cluster_signal {
                self.cache_hits_missing_cluster_signal_total
                    .fetch_add(1, Ordering::Relaxed);
            }
        } else {
            self.cache_misses_total.fetch_add(1, Ordering::Relaxed);
        }

        let idx = self.next_slot.fetch_add(1, Ordering::Relaxed) % RING_SIZE;
        self.latencies_ms[idx].store(latency_ms, Ordering::Relaxed);
    }

    #[allow(
        clippy::cast_precision_loss,
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "the ring holds a few thousand samples and the fraction is in [0,1], so the index is exact and non-negative"
    )]
    fn percentile(&self, p: f64) -> u64 {
        let filled = (self.next_slot.load(Ordering::Relaxed)).min(RING_SIZE);
        if filled == 0 {
            return 0;
        }
        let mut samples: Vec<u64> = self.latencies_ms[..filled]
            .iter()
            .map(|slot| slot.load(Ordering::Relaxed))
            .collect();
        samples.sort_unstable();
        let idx = ((samples.len() as f64 - 1.0) * p).round() as usize;
        samples[idx.min(samples.len() - 1)]
    }

    /// Every counter and both percentiles, in the exposition format.
    #[must_use]
    pub fn render_prometheus_text(&self) -> String {
        let requests = self.requests_total.load(Ordering::Relaxed);
        let errors = self.errors_total.load(Ordering::Relaxed);
        let hits = self.cache_hits_total.load(Ordering::Relaxed);
        let misses = self.cache_misses_total.load(Ordering::Relaxed);
        let missing_cluster = self
            .cache_hits_missing_cluster_signal_total
            .load(Ordering::Relaxed);
        let p50 = self.percentile(0.50);
        let p99 = self.percentile(0.99);

        format!(
            "# HELP sybil_http_requests_total Total number of /score requests handled.\n\
             # TYPE sybil_http_requests_total counter\n\
             sybil_http_requests_total {requests}\n\
             # HELP sybil_http_errors_total Total number of /score requests that errored.\n\
             # TYPE sybil_http_errors_total counter\n\
             sybil_http_errors_total {errors}\n\
             # HELP sybil_cache_hits_total Total number of /score requests served from the warm feature cache.\n\
             # TYPE sybil_cache_hits_total counter\n\
             sybil_cache_hits_total {hits}\n\
             # HELP sybil_cache_misses_total Total number of /score requests with a cold feature cache (partial=true).\n\
             # TYPE sybil_cache_misses_total counter\n\
             sybil_cache_misses_total {misses}\n\
             # HELP sybil_cache_hits_missing_cluster_signal_total Cache hits where cluster_agreement/wcc_size were absent (MVP gap, see feature_cache.rs).\n\
             # TYPE sybil_cache_hits_missing_cluster_signal_total counter\n\
             sybil_cache_hits_missing_cluster_signal_total {missing_cluster}\n\
             # HELP sybil_http_request_duration_ms_p50 Approximate P50 latency over the last {RING_SIZE} requests.\n\
             # TYPE sybil_http_request_duration_ms_p50 gauge\n\
             sybil_http_request_duration_ms_p50 {p50}\n\
             # HELP sybil_http_request_duration_ms_p99 Approximate P99 latency over the last {RING_SIZE} requests.\n\
             # TYPE sybil_http_request_duration_ms_p99 gauge\n\
             sybil_http_request_duration_ms_p99 {p99}\n"
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_counters_report_what_was_recorded() {
        let metrics = Metrics::new();
        metrics.record(10, true, false, false);
        metrics.record(20, false, false, false);
        metrics.record(30, true, true, true);

        let text = metrics.render_prometheus_text();
        assert!(text.contains("sybil_http_requests_total 3"));
        assert!(text.contains("sybil_http_errors_total 1"));
        assert!(text.contains("sybil_cache_hits_total 2"));
        assert!(text.contains("sybil_cache_misses_total 1"));
        assert!(text.contains("sybil_cache_hits_missing_cluster_signal_total 1"));
    }

    #[test]
    fn the_percentiles_come_from_the_latencies_recorded() {
        let metrics = Metrics::new();
        for ms in 1..=100u64 {
            metrics.record(ms, true, false, false);
        }
        let text = metrics.render_prometheus_text();
        assert!(
            text.contains("sybil_http_request_duration_ms_p50 50")
                || text.contains("sybil_http_request_duration_ms_p50 51")
        );
        assert!(
            text.contains("sybil_http_request_duration_ms_p99 99")
                || text.contains("sybil_http_request_duration_ms_p99 100")
        );
    }

    #[test]
    fn a_service_that_has_served_nothing_still_reports() {
        let metrics = Metrics::new();
        let text = metrics.render_prometheus_text();
        assert!(text.contains("sybil_http_requests_total 0"));
        assert!(text.contains("sybil_http_request_duration_ms_p50 0"));
    }
}
