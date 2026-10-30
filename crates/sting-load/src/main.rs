//! `surface-load` — the concurrency ladder, run against one of this platform's surfaces.
//!
//! Its own binary, and its own crate, for one reason: it is the only thing in this tree besides
//! `sting-chain` that names an HTTP client, and an exception is narrower when it lives alone. What
//! it talks to is this platform's own service over a loopback, which is not chain access — but the
//! rule is about what a manifest declares rather than what a caller means by it, so the exception is
//! recorded in `scripts/check-lints.py` with this reason rather than assumed to be obvious.

use clap::Parser;

/// What to measure, and how hard.
#[derive(Parser)]
#[command(
    name = "surface-load",
    about = "walks a running surface up a concurrency ladder"
)]
struct Cli {
    /// The endpoint of a service that is already running.
    #[arg(long, default_value = "http://127.0.0.1:8080/v1/decode")]
    url: String,
    /// Whether the endpoint is asked or told.
    ///
    /// Two surfaces, two shapes: a decode is a body posted, a score is an address in a path.
    #[arg(long, default_value = "post")]
    method: String,
    /// How many requests each step of the ladder sends.
    #[arg(long, default_value_t = 200)]
    requests: usize,
    /// A header to send with every request, as `Name: value`. Repeat it for more than one.
    #[arg(long = "header")]
    headers: Vec<String>,
    /// The message body every request carries, as base64, for a surface that takes one. A real
    /// captured jetton transfer notification by default, so what is timed is a reading rather than
    /// a refusal.
    #[arg(
        long,
        default_value = "te6cckEBAgEAPgABZHNi0Jzv5G1UHv2tpUEeGjAIAAxyuxvBmtyIDdYsj3iafEOsMHxyCuREQYSBefhi2qTlAQAOAAAAADMwMD/iE3A="
    )]
    body: String,
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    sting_load::run(&cli.url, &cli.method, &cli.headers, cli.requests, &cli.body)
}
