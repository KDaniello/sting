//! One command, the whole published scope, from a clean clone.
//!
//! Argument parsing and an exit status; the run itself lives in the library. The exit status is the
//! part worth reading twice: a file the grammar read only in part ends the process non-zero even
//! though the report was written and the findings are in it. A run that could not read everything it
//! claimed to cover is not a clean run, and the only way to keep that from decaying into a footnote
//! is to make it fail.
#![allow(
    clippy::print_stdout,
    clippy::print_stderr,
    reason = "this file is the binary entry point, not library code: the report and its failures are what it exists to print"
)]

use std::path::PathBuf;
use std::process::ExitCode;

use clap::Parser;
use sting_hunt::report;

/// Run the detectors against the published bug bounty scopes of the TON ecosystem.
#[derive(Parser)]
#[command(name = "sting-hunt", version, about, long_about = None)]
struct Cli {
    /// Where to keep the clones. A working directory, never a product: no target is copied into
    /// this repository, and the report is the only thing the run publishes.
    #[arg(long, default_value = "target/hunt")]
    cache: PathBuf,
    /// Write the Markdown report here instead of stdout.
    #[arg(long)]
    output: Option<PathBuf>,
    /// Also write the run as JSON, for a triage entry that has to quote exact positions.
    #[arg(long)]
    json: Option<PathBuf>,
    /// Heading for the report, so a re-run at a later commit does not overwrite the question the
    /// previous one answered.
    #[arg(long, default_value = "The hunt")]
    heading: String,
    /// Discard the cached clones and resolve every reference again. Without it a target stays at
    /// the commit the cache holds, which keeps two reports comparable and hides a project that has
    /// moved since.
    #[arg(long)]
    refresh: bool,
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let report = match sting_hunt::run(&cli.cache, cli.refresh) {
        Ok(report) => report,
        Err(error) => {
            eprintln!("sting-hunt: {error}");
            return ExitCode::FAILURE;
        }
    };

    let rendered = report::markdown(&report, &cli.heading);
    if let Some(path) = &cli.output {
        if let Err(error) = std::fs::write(path, &rendered) {
            eprintln!("sting-hunt: cannot write {}: {error}", path.display());
            return ExitCode::FAILURE;
        }
        println!("{}", path.display());
    } else {
        print!("{rendered}");
    }

    if let Some(path) = &cli.json {
        match serde_json::to_string_pretty(&report) {
            Ok(json) => {
                if let Err(error) = std::fs::write(path, json) {
                    eprintln!("sting-hunt: cannot write {}: {error}", path.display());
                    return ExitCode::FAILURE;
                }
            }
            Err(error) => {
                eprintln!("sting-hunt: cannot serialise the run: {error}");
                return ExitCode::FAILURE;
            }
        }
    }

    if report.every_file_was_read_whole() {
        ExitCode::SUCCESS
    } else {
        eprintln!(
            "sting-hunt: {} files read only in part and {} unreadable; a scope that was not read \
             whole is not a scope that came back clean",
            report.unparsed_files(),
            report.unreadable_files()
        );
        ExitCode::FAILURE
    }
}
