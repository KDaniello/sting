//! `surface-load` — what a surface does under sustained load, and where it stops.
//!
//! A latency figure taken from one request is a figure about one request. What a caller needs to
//! know before depending on a service is two things it cannot read off a single call: what the slow
//! end of the distribution looks like when many callers arrive at once, and at what point adding
//! callers stops adding throughput.
//!
//! So this walks concurrency upwards, one step at a time, and reports each step's throughput and
//! percentiles side by side. The step where throughput stops rising is the degradation point, and it
//! is named rather than left for a reader to infer from a table.
//!
//! **What this does not do.** It does not run the service — a load generator that also hosts what it
//! measures measures itself. It sends one request over and over rather than a varied population, so
//! what it reports is the cost of one shape of question and not of a day's traffic. And it is one
//! machine talking to itself over a loopback, which bounds what the numbers mean: they are a floor
//! on latency and a ceiling on throughput, not a production figure.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use anyhow::Context as _;

/// How the walk up the concurrency ladder proceeds.
///
/// Doubling rather than stepping by one: what is being looked for is where the curve bends, and a
/// bend is visible at this resolution while costing eight measurements instead of a hundred. It
/// runs past the core count deliberately — a service whose throughput is still rising at the number
/// of cores it has is a service whose limit has not been found yet.
const LADDER: &[usize] = &[1, 2, 4, 8, 16, 32, 64, 128];

/// How many requests each worker of a step has to make before the step means anything.
///
/// A step where every worker sends two requests measures thread startup: the workers spend most of
/// the step being created, and the throughput that comes out is an artefact of the generator rather
/// than a fact about the service. A step is therefore never smaller than this many per worker,
/// whatever was asked for, and the count it actually sent is reported.
const MIN_PER_WORKER: usize = 8;

/// One step of the ladder, measured.
#[derive(Debug, Clone, serde::Serialize)]
pub struct LoadStep {
    /// How many requests were in flight at once.
    pub concurrency: usize,
    /// How many requests were sent.
    pub requests: usize,
    /// How many came back with a `200`.
    pub answered: usize,
    /// How many did not.
    ///
    /// Reported beside the throughput rather than left to be inferred from the difference: a
    /// request the host refused because it had no socket left is not a slow service, and a step
    /// with refusals in it is a step that measured this machine rather than the one it asked.
    pub refused: usize,
    /// Requests per second, over the whole step.
    pub throughput: f64,
    /// The median request, in milliseconds.
    pub p50_ms: f64,
    /// The ninety-fifth percentile, in milliseconds.
    pub p95_ms: f64,
    /// The slowest request of the step, in milliseconds.
    pub max_ms: f64,
}

/// What one walk up the ladder found.
#[derive(Debug, Clone, serde::Serialize)]
pub struct LoadReport {
    /// Which version of this report's shape it is in.
    pub schema_version: String,
    /// What was measured.
    pub url: String,
    /// Every step, in the order they were run.
    pub steps: Vec<LoadStep>,
    /// The concurrency at which throughput stopped rising, when it did.
    ///
    /// `None` means the ladder ended before the curve bent, which is a fact about the ladder rather
    /// than about the service: the degradation point is above what was tried.
    pub degrades_at: Option<usize>,
}

/// This report's own format version.
pub const LOAD_SCHEMA_VERSION: &str = "1.0";

/// Walks the concurrency ladder against `url`, `requests` per step.
///
/// # Errors
/// When the service cannot be reached at all. A single refused request inside a step is counted
/// rather than raised: what a step measures is how many of its requests were answered.
pub fn run(
    url: &str,
    method: &str,
    headers: &[String],
    requests: usize,
    body: &str,
) -> anyhow::Result<()> {
    anyhow::ensure!(requests > 0, "a load step of no requests measures nothing");
    let request = Request {
        agent: ureq::Agent::new_with_defaults(),
        url: url.to_string(),
        posts: match method.to_ascii_lowercase().as_str() {
            "post" => true,
            "get" => false,
            other => anyhow::bail!("{other} is neither get nor post"),
        },
        headers: headers
            .iter()
            .map(|header| {
                let (name, value) = header
                    .split_once(':')
                    .with_context(|| format!("{header} is not `Name: value`"))?;
                Ok((name.trim().to_string(), value.trim().to_string()))
            })
            .collect::<anyhow::Result<Vec<_>>>()?,
        payload: serde_json::json!({ "body": body }).to_string(),
    };

    // One request before the ladder, so that a service that is not there is reported as that rather
    // than as a throughput of zero.
    send(&request).context("the service did not answer a first request")?;

    let mut steps = Vec::new();
    for concurrency in LADDER {
        steps.push(step(&request, requests, *concurrency));
    }

    let degrades_at = steps
        .windows(2)
        .find(|pair| pair[1].throughput <= pair[0].throughput)
        .map(|pair| pair[1].concurrency);

    let report = LoadReport {
        schema_version: LOAD_SCHEMA_VERSION.to_string(),
        url: request.url.clone(),
        steps,
        degrades_at,
    };
    for step in &report.steps {
        println!(
            "concurrency {:>3}: {:>8.1} req/s  p50 {:>7.1} ms  p95 {:>7.1} ms  max {:>7.1} ms  ({} of {} answered{})",
            step.concurrency,
            step.throughput,
            step.p50_ms,
            step.p95_ms,
            step.max_ms,
            step.answered,
            step.requests,
            if step.refused == 0 {
                String::new()
            } else {
                format!(
                    ", {} refused — this machine ran out before the service did",
                    step.refused
                )
            }
        );
    }
    match report.degrades_at {
        Some(at) => println!("throughput stops rising at concurrency {at}"),
        None => println!(
            "throughput was still rising at concurrency {}, so the degradation point is above this ladder",
            LADDER.last().copied().unwrap_or_default()
        ),
    }
    println!("{}", serde_json::to_string_pretty(&report)?);
    Ok(())
}

/// The one request this ladder repeats.
struct Request {
    /// The connection pool every worker shares.
    ///
    /// One agent rather than a connection per request, and the reason is a measurement rather than
    /// tidiness: a fresh connection per request exhausts a host's ephemeral ports within seconds at
    /// this rate, and what is then measured is the operating system running out of sockets rather
    /// than the service running out of capacity.
    agent: ureq::Agent,
    /// Where it goes.
    url: String,
    /// Whether it is posted with a body or asked for.
    posts: bool,
    /// What it carries besides the body — a key, most often.
    headers: Vec<(String, String)>,
    /// The body, for a surface that takes one.
    payload: String,
}

/// One step: `requests` requests, `concurrency` of them in flight at a time.
fn step(request: &Request, requests: usize, concurrency: usize) -> LoadStep {
    let requests = requests.max(concurrency * MIN_PER_WORKER);
    let remaining = Arc::new(AtomicUsize::new(requests));
    let started = Instant::now();
    let (mut latencies, refused): (Vec<Duration>, usize) = std::thread::scope(|scope| {
        let workers: Vec<_> = (0..concurrency)
            .map(|_| {
                let remaining = Arc::clone(&remaining);
                scope.spawn(move || {
                    let mut mine = Vec::new();
                    let mut refused = 0usize;
                    while remaining.fetch_update(Ordering::SeqCst, Ordering::SeqCst, |left| {
                        left.checked_sub(1)
                    }) != Err(0)
                    {
                        let at = Instant::now();
                        if send(request).is_ok() {
                            mine.push(at.elapsed());
                        } else {
                            refused += 1;
                        }
                    }
                    (mine, refused)
                })
            })
            .collect();
        workers
            .into_iter()
            .filter_map(|worker| worker.join().ok())
            .fold(
                (Vec::new(), 0usize),
                |(mut latencies, refused), (mine, mine_refused)| {
                    latencies.extend(mine);
                    (latencies, refused + mine_refused)
                },
            )
    });
    let elapsed = started.elapsed();
    latencies.sort_unstable();

    LoadStep {
        concurrency,
        requests,
        answered: latencies.len(),
        refused,
        throughput: throughput(latencies.len(), elapsed),
        p50_ms: percentile_ms(&latencies, 0.50),
        p95_ms: percentile_ms(&latencies, 0.95),
        max_ms: percentile_ms(&latencies, 1.0),
    }
}

/// One request, answered or refused.
fn send(request: &Request) -> anyhow::Result<()> {
    let response = if request.posts {
        let mut call = request
            .agent
            .post(&request.url)
            .header("content-type", "application/json");
        for (name, value) in &request.headers {
            call = call.header(name, value);
        }
        call.send(&request.payload)?
    } else {
        let mut call = request.agent.get(&request.url);
        for (name, value) in &request.headers {
            call = call.header(name, value);
        }
        call.call()?
    };
    anyhow::ensure!(
        response.status() == 200,
        "the service answered {}",
        response.status()
    );
    Ok(())
}

/// Requests per second, or zero when no time passed.
///
/// The counts here are request counts — thousands, not quadrillions — so the precision `as f64`
/// loses above 2^52 is a bound this measurement never reaches.
#[allow(clippy::cast_precision_loss)]
fn throughput(answered: usize, elapsed: Duration) -> f64 {
    if elapsed.is_zero() {
        return 0.0;
    }
    (answered as f64 / elapsed.as_secs_f64() * 10.0).round() / 10.0
}

/// The latency at `share` of the distribution, in milliseconds.
///
/// Nearest-rank on a sorted list rather than an interpolation: a percentile of a latency sample is
/// a real request that really took that long, and averaging two neighbours invents one that did not.
#[allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]
fn percentile_ms(sorted: &[Duration], share: f64) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    let rank = (share * sorted.len() as f64).ceil() as usize;
    let index = rank.saturating_sub(1).min(sorted.len() - 1);
    (sorted[index].as_secs_f64() * 10_000.0).round() / 10.0
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "docs/ARCHITECTURE.md §3.2 scopes the no-unwrap/expect policy to library code outside tests"
)]
mod tests {
    use std::time::Duration;

    use super::{percentile_ms, throughput};

    #[test]
    fn a_percentile_is_a_request_that_really_took_that_long() {
        let sorted: Vec<Duration> = (1..=10).map(Duration::from_millis).collect();

        assert!((percentile_ms(&sorted, 0.50) - 5.0).abs() < f64::EPSILON);
        assert!((percentile_ms(&sorted, 0.95) - 10.0).abs() < f64::EPSILON);
        assert!((percentile_ms(&sorted, 1.0) - 10.0).abs() < f64::EPSILON);
    }

    #[test]
    fn an_empty_sample_measures_nothing_rather_than_dividing_by_zero() {
        assert!((percentile_ms(&[], 0.95) - 0.0).abs() < f64::EPSILON);
        assert!((throughput(0, Duration::ZERO) - 0.0).abs() < f64::EPSILON);
    }
}
