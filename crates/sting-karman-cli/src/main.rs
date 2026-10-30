//! The `karman` binary: the terminal surface over the composition.
//!
//! Everything this binary can do, the Python bindings can do, because both go through
//! `sting-karman-app` and neither resolves a scenario itself. What lives here is what a terminal owns
//! and a library must not: the argument grammar, the exit code, and the counting allocator the
//! profiler measures with.

mod alloc;
mod profile;

use std::{path::PathBuf, process::ExitCode};

use clap::{Parser, Subcommand};
use sting_karman_app::{
    AppError, read_scenario, run, showcase_world_builder, sweep, verify, vouch, world_builder,
};
use sting_karman_scenario::{Knob, MechanismConfig};

#[global_allocator]
static ALLOCATOR: alloc::CountingAlloc = alloc::CountingAlloc;

#[derive(Parser)]
#[command(
    name = "karman",
    about = "Agent-based wind tunnel for crypto-economic mechanisms"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Run a scenario as a Monte Carlo sweep, write the run report, print a JSON summary.
    Run {
        /// Path to a scenario JSON file.
        #[arg(long)]
        scenario: PathBuf,
        /// Number of independent runs.
        #[arg(long, default_value_t = 100)]
        runs: u64,
        /// Overrides the scenario's seed.
        #[arg(long)]
        seed: Option<u64>,
        /// Directory the run report is written into.
        #[arg(long, default_value = "out")]
        report_dir: PathBuf,
        /// Path to a claim registered in `findings/` before this sweep ran. Without one the
        /// report states no verdict, which is the honest outcome rather than a missing feature.
        #[arg(long)]
        claim: Option<PathBuf>,
    },
    /// Sweep one parameter of a scenario across values, and report the designs side by side.
    Sweep {
        /// Path to a scenario JSON file.
        #[arg(long)]
        scenario: PathBuf,
        /// Which parameter to vary: n-ticks, token-reserve, quote-reserve, lltv, charge, lag,
        /// commission, share, shape, spread, items, floor, premium, venue-share, ring,
        /// group-count:N, shock-depth:N, sells-share:N, liquidator-every:N or
        /// liquidator-capacity:N.
        #[arg(long)]
        vary: Knob,
        /// The values to turn it to, comma separated.
        #[arg(long, value_delimiter = ',')]
        values: Vec<u128>,
        /// Number of independent runs at each point.
        #[arg(long, default_value_t = 100)]
        runs: u64,
        /// Overrides the scenario's seed. Every point uses it, so points differ by the knob
        /// alone and never by which stream they happened to draw.
        #[arg(long)]
        seed: Option<u64>,
        /// Directory the sweep report is written into.
        #[arg(long, default_value = "out")]
        report_dir: PathBuf,
    },
    /// Drive a scenario through the executable levels of the verification ladder.
    Verify {
        /// Path to a scenario JSON file.
        #[arg(long)]
        scenario: PathBuf,
        /// Overrides the scenario's seed.
        #[arg(long)]
        seed: Option<u64>,
        /// Directory the verification report is written into.
        #[arg(long, default_value = "out")]
        report_dir: PathBuf,
    },
    /// Profile a scenario: phase timings, single- vs multi-thread throughput, memory.
    Profile {
        /// Path to a scenario JSON file.
        #[arg(long)]
        scenario: PathBuf,
        /// Number of runs in the throughput and memory sweeps.
        #[arg(long, default_value_t = 1000)]
        runs: u64,
        /// Directory the profile is written into.
        #[arg(long, default_value = "out")]
        report_dir: PathBuf,
    },
}

fn main() -> ExitCode {
    match dispatch(Cli::parse()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("error: {error}");
            ExitCode::FAILURE
        }
    }
}

fn dispatch(cli: Cli) -> Result<(), AppError> {
    // Every command needs the same scenario, validated the same way, before they diverge.
    let path = match &cli.command {
        Command::Run { scenario, .. }
        | Command::Sweep { scenario, .. }
        | Command::Verify { scenario, .. }
        | Command::Profile { scenario, .. } => scenario.clone(),
    };
    let config = read_scenario(&path)?;

    match cli.command {
        Command::Run {
            runs,
            seed,
            report_dir,
            claim,
            ..
        } => {
            let report = run(&path, &config, runs, seed, &report_dir, claim.as_deref())?;
            println!("{report}");
        }
        Command::Sweep {
            vary,
            values,
            runs,
            seed,
            report_dir,
            ..
        } => {
            let report = sweep(&path, &config, vary, &values, runs, seed, &report_dir)?;
            println!("{report}");
        }
        Command::Verify {
            seed, report_dir, ..
        } => {
            let report = verify(&path, &config, seed, &report_dir)?;
            println!("{report}");
        }
        // Every family this workspace can build a world for can be profiled, because a phase
        // timing and an allocation discipline are properties of the engine and not of any one
        // mechanism — and the family whose worlds are large is the one that needs profiling most.
        Command::Profile {
            runs, report_dir, ..
        } => match config.mechanism {
            MechanismConfig::Showcase { .. } => {
                profile::profile(&config, showcase_world_builder(&config)?, runs, &report_dir)?;
            }
            MechanismConfig::Vouch { .. } => {
                profile::profile(&config, vouch::world_builder(&config)?, runs, &report_dir)?;
            }
            _ => profile::profile(&config, world_builder(&config)?, runs, &report_dir)?,
        },
    }
    Ok(())
}
