//! Sweeps an indexer for one census of a market that is held rather than traded.
//!
//! Writes the sweep one stage at a time, one JSON object per line, and stops on the first stage it
//! cannot finish. The counting is `pull-ton-census`, which folds these files into a fixture — the
//! half where a mistake is silent, and therefore the half that is checked.
//!
//! ```text
//! sweep-ton-census --into out/census --days 90 --served-by nft.fragment.com \
//!                  --also 0:B774D95EB20543F186C06B371AB88AD704F7E256130CAF96189368A7D0CB6CCF \
//!                  --issuer 0:158136239ADB15DD59DF90C641F9EFD312CFEB8664F218F4C3E5FCE9D95E6C07
//! ```
//!
//! `--issuer` may be given more than once. An issuer is an address whose holdings are stock rather
//! than ownership, and a census covering several classes of asset covers several of them.
//!
//! Running it again after any failure continues where it stopped: a stage already written is
//! skipped, and one that was interrupted is a `.part` nobody reads.

use std::path::PathBuf;
use std::process::ExitCode;

use sting_karman_data::SweepPlan;

fn main() -> ExitCode {
    let plan = match plan_from(std::env::args().skip(1)) {
        Ok(plan) => plan,
        Err(reason) => {
            eprintln!("{reason}");
            return ExitCode::from(2);
        }
    };
    eprintln!("sweeping {} days into {}", plan.days, plan.into.display());
    match sting_karman_data::sweep_census(&plan) {
        Ok(()) => {
            eprintln!("done");
            ExitCode::SUCCESS
        }
        Err(error) => {
            // Named rather than summarised: the stage that failed is the stage a second run
            // resumes from, and a sweep that says only "failed" is a sweep started over.
            eprintln!("stopped: {error}");
            ExitCode::FAILURE
        }
    }
}

/// Reads the arguments, or says what is missing.
fn plan_from(arguments: impl Iterator<Item = String>) -> Result<SweepPlan, String> {
    let mut into = PathBuf::from("out/census");
    let mut days = 90_u32;
    let mut served_by = None;
    let mut also = Vec::new();
    let mut issuers = Vec::new();

    let mut arguments = arguments.peekable();
    while let Some(argument) = arguments.next() {
        let mut value = || {
            arguments
                .next()
                .ok_or_else(|| format!("{argument} needs a value"))
        };
        match argument.as_str() {
            "--into" => into = PathBuf::from(value()?),
            "--days" => {
                days = value()?
                    .parse()
                    .map_err(|_| "--days takes a whole number of days".to_string())?;
            }
            "--served-by" => served_by = Some(value()?),
            "--also" => also.push(value()?),
            "--issuer" => issuers.push(value()?),
            other => return Err(format!("unknown argument: {other}")),
        }
    }

    Ok(SweepPlan {
        into,
        days,
        served_by,
        also,
        issuers,
    })
}
