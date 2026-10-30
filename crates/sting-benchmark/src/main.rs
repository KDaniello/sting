//! `sting-benchmark` — several analysers over one dataset, scored by one rule.
//!
//! Two steps with a container between them, because `acton` neither builds nor runs on Windows: its
//! `ton-objs` artefacts are published for macOS and Linux only (their
//! `crates/ton-objs/artifacts_manifest.toml`).
//!
//! ```text
//! sting-benchmark export --dir <dir>
//! docker run ... acton check --output-format json   # once per unit, into <dir>/<unit>/out.json
//! sting-benchmark score --dir <dir>
//! ```
//!
//! The `score` step does not run the participant: it reads what the participant printed. That makes
//! the run reproducible on a machine with no Docker and no Windows build of `acton`, and lets an
//! outsider substitute their own tool's output under the same file name.
//!
//! **What this does not do.** It does not decide which of two analysers is better. It reports what
//! each found on one dataset under one scoring rule, with the units the participant could not
//! compile excluded and counted — a comparison is a table, and the reading of it belongs to whoever
//! reads it.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "a measurement binary rather than shipped library code (docs/ARCHITECTURE.md §3.2)"
)]

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use clap::{Parser, Subcommand};
use sting_bench::{DatasetUnit, UnitOrigin};
use sting_benchmark::external::{acton_rule_mapping, parse_acton_json};
use sting_benchmark::{Analyzer, StingAnalyzer};

/// Where the dataset is written and where a participant's output is read from.
#[derive(Parser)]
#[command(
    name = "sting-benchmark",
    about = "several analysers over one dataset, scored by one rule"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Writes every dataset unit as its own project, ready for a participant to analyse.
    Export {
        /// Where to write them.
        #[arg(long, default_value = "bench-run")]
        dir: PathBuf,
        /// The corpus to build the dataset from.
        #[arg(long, default_value = "corpus")]
        corpus: PathBuf,
    },
    /// Reads a participant's output beside each unit and scores both participants by one rule.
    Score {
        /// Where the units and the participant's output are.
        #[arg(long, default_value = "bench-run")]
        dir: PathBuf,
        /// The corpus the dataset was built from.
        #[arg(long, default_value = "corpus")]
        corpus: PathBuf,
        /// The file name the participant's output is under, inside each unit's directory.
        ///
        /// One name per configuration: a run with a preview rule enabled is a different
        /// measurement from a run with the defaults, and both belong in the report rather than one
        /// quietly replacing the other.
        #[arg(long, default_value = "out.json")]
        participant_out: String,
        /// What that configuration was, printed with the numbers.
        #[arg(long, default_value = "as shipped")]
        configuration: String,
        /// Where to write the report. Printed to standard output when absent.
        #[arg(long)]
        report: Option<PathBuf>,
    },
}

fn main() {
    let cli = Cli::parse();
    match cli.command {
        Command::Export { dir, corpus } => {
            let dataset = sting_bench::load_dataset(&corpus).expect("corpus discoverable");
            export(&dataset, &dir);
        }
        Command::Score {
            dir,
            corpus,
            participant_out,
            configuration,
            report,
        } => {
            let dataset = sting_bench::load_dataset(&corpus).expect("corpus discoverable");
            let text = score(&dataset, &dir, &participant_out, &configuration);
            match report {
                Some(path) => {
                    std::fs::write(&path, &text).expect("writing the report");
                    println!("wrote {}", path.display());
                }
                None => print!("{text}"),
            }
        }
    }
}

/// The file name for a dataset unit, deterministic so that the `score` step
/// finds the participant's output under the same name.
fn file_stem(unit: &DatasetUnit) -> String {
    unit.name
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect()
}

/// Every unit goes **in its own directory with its own manifest**.
///
/// The first attempt put all 547 in one directory under one `Acton.toml`, and
/// that was a measurement error rather than the participant's: `acton` resolves
/// a project as a whole, saw 547 files at once, and answered
/// `redefinition of symbol` pointing at neighbouring units, then a cascade of
/// `unknown type name` — 292 of 547 files did not compile. Publishing the zeros
/// that produced would have declared somebody else's tool blind because of our
/// own directory layout.
fn export(dataset: &[DatasetUnit], dir: &Path) {
    std::fs::create_dir_all(dir).expect("creating output dir");
    for unit in dataset {
        let stem = file_stem(unit);
        let unit_dir = dir.join(&stem);
        std::fs::create_dir_all(&unit_dir).expect("creating unit dir");

        // The real per-file layout rather than the splice: a participant with
        // a resolver gets the project as it was, not text full of redefined
        // symbols. The imports stay intact, because the sibling files are back
        // in place.
        let multi_file = unit.files.len() > 1;
        for (name, source) in &unit.files {
            let body = if multi_file {
                source.clone()
            } else {
                sting_bench::strip_sibling_imports(source)
            };
            std::fs::write(unit_dir.join(name), body).expect("writing unit file");
        }

        // `description` is mandatory: the participant refuses to parse a
        // manifest without it, and a refusal looks like "zero findings" unless
        // one looks
        // stderr.
        let mut manifest = String::from(
            "[package]\nname = \"unit\"\n\
             description = \"Benchmark dataset unit\"\nversion = \"0.1.0\"\nlicense = \"MIT\"\n\n",
        );
        for (name, _) in &unit.files {
            let contract = name.trim_end_matches(".tolk").replace(['-', '.'], "_");
            let _ = writeln!(
                manifest,
                "[contracts.{contract}]\nsrc = \"{name}\"\ndepends = []\n"
            );
        }
        std::fs::write(unit_dir.join("Acton.toml"), manifest).expect("writing Acton.toml");
    }
    println!(
        "exported {} units to {} (one directory each)",
        dataset.len(),
        dir.display()
    );
}

/// Reads the participant's output for each unit, and notes which of them it
/// managed to compile at all.
fn read_participant(
    dataset: &[DatasetUnit],
    dir: &Path,
    participant_out: &str,
) -> (Vec<Vec<sting_core::Finding>>, Vec<bool>, usize, usize) {
    let mapping = acton_rule_mapping();
    let mut findings = Vec::with_capacity(dataset.len());
    let mut compiles = Vec::with_capacity(dataset.len());
    let (mut missing, mut unparsed) = (0usize, 0usize);
    for unit in dataset {
        let path = dir.join(file_stem(unit)).join(participant_out);
        let Ok(text) = std::fs::read_to_string(&path) else {
            missing += 1;
            findings.push(Vec::new());
            compiles.push(false);
            continue;
        };
        // Read out of the diagnostics rather than searched for in the text: a rule's own message is
        // free to contain the string `C001`, and excluding a unit because of a word in a sentence
        // would drop it from the comparison for no reason at all.
        compiles.push(!refused_to_compile(&text));
        if let Ok(parsed) = parse_acton_json(&text, &mapping) {
            findings.push(parsed);
        } else {
            unparsed += 1;
            findings.push(Vec::new());
        }
    }
    (findings, compiles, missing, unparsed)
}

/// Whether the participant refused this unit rather than analysing it.
///
/// Their compiler's own refusals — a failed import, a syntax error — arrive as diagnostics like any
/// other, and a unit they refused is a unit nobody was measured on.
fn refused_to_compile(text: &str) -> bool {
    let Some(start) = text.find('{') else {
        return true;
    };
    let Ok(report) = serde_json::from_str::<serde_json::Value>(&text[start..]) else {
        return true;
    };
    report
        .get("diagnostics")
        .and_then(serde_json::Value::as_array)
        .is_some_and(|diagnostics| {
            diagnostics.iter().any(|diagnostic| {
                let code = diagnostic.get("code").and_then(serde_json::Value::as_str);
                let name = diagnostic.get("name").and_then(serde_json::Value::as_str);
                matches!(code, Some("C001")) || matches!(name, Some("parse-error"))
            })
        })
}

/// Keeps only the units the participant could compile — the call site explains
/// why the rest are excluded.
fn keep_comparable(
    dataset: &[DatasetUnit],
    sting: &[Vec<sting_core::Finding>],
    acton: &[Vec<sting_core::Finding>],
    compiles: &[bool],
) -> (
    Vec<DatasetUnit>,
    Vec<Vec<sting_core::Finding>>,
    Vec<Vec<sting_core::Finding>>,
) {
    let pick = |src: &[Vec<sting_core::Finding>]| {
        src.iter()
            .zip(compiles)
            .filter(|(_, ok)| **ok)
            .map(|(f, _)| f.clone())
            .collect()
    };
    let units = dataset
        .iter()
        .zip(compiles)
        .filter(|(_, ok)| **ok)
        .map(|(u, _)| u.clone())
        .collect();
    (units, pick(sting), pick(acton))
}

fn score(
    dataset: &[DatasetUnit],
    dir: &Path,
    participant_out: &str,
    configuration: &str,
) -> String {
    // Our own tool is an ordinary participant, through the crate's own runner rather than a second
    // loop that would drift from it.
    let sting = StingAnalyzer::new(env!("CARGO_PKG_VERSION"));
    let sting_findings: Vec<Vec<sting_core::Finding>> = dataset
        .iter()
        .map(|unit| sting.analyze(unit).unwrap_or_default())
        .collect();

    let (acton_findings, compiles, missing, unparsed) =
        read_participant(dataset, dir, participant_out);
    // A run nobody made is not a comparison anybody can read: an empty table beside a full one would
    // be published as "the participant found nothing" by whoever quoted it.
    assert!(
        missing < dataset.len(),
        "no participant output under {participant_out} for any of the {} units: run the participant first",
        dataset.len()
    );
    let (comparable, sting_sub, acton_sub) =
        keep_comparable(dataset, &sting_findings, &acton_findings, &compiles);
    // A public comparison scores at unit level — see MatchGranularity for why
    // a line-for-line match is unfair to everyone but us.
    let g = sting_bench::MatchGranularity::Unit;
    let sting_cmp = sting_bench::score_dataset_with(&comparable, &sting_sub, g);
    let acton_cmp = sting_bench::score_dataset_with(&comparable, &acton_sub, g);

    let mut out = String::new();
    let _ = writeln!(out, "# One dataset, two analysers\n");
    let _ = writeln!(
        out,
        "Participant configuration: **{configuration}**, read from `{participant_out}`.\n"
    );
    let _ = writeln!(
        out,
        "Dataset units: {} (real code {}, injections {}, fixtures {}).",
        dataset.len(),
        dataset
            .iter()
            .filter(|u| u.origin == UnitOrigin::RealCode)
            .count(),
        dataset
            .iter()
            .filter(|u| u.origin == UnitOrigin::InjectedIntoRealCode)
            .count(),
        dataset
            .iter()
            .filter(|u| u.origin == UnitOrigin::HandWritten)
            .count(),
    );
    let _ = writeln!(
        out,
        "No participant output for {missing} units, {unparsed} unparsed.\n"
    );

    let dropped = dataset.len() - comparable.len();
    let _ = writeln!(
        out,
        "**{dropped} of {} units excluded**: the participant could not compile them (`C001` or a parse error). The comparison below is over the remaining {}.\n",
        dataset.len(),
        comparable.len()
    );

    print_table(&mut out, &sting_cmp, &acton_cmp);
    out
}

/// The comparison itself: a row per class, then the aggregate over the classes both cover.
fn print_table(
    out: &mut String,
    sting_cmp: &sting_bench::BenchReport,
    acton_cmp: &sting_bench::BenchReport,
) {
    let _ = writeln!(
        out,
        "| Class | sting TP/FP/FN | sting precision | sting recall | acton TP/FP/FN | acton precision | acton recall |"
    );
    let _ = writeln!(out, "|---|---|---|---|---|---|---|");
    let mut classes: Vec<&String> = sting_cmp
        .by_detector
        .keys()
        .chain(acton_cmp.by_detector.keys())
        .collect();
    classes.sort_unstable();
    classes.dedup();
    let mapping = acton_rule_mapping();
    let declared: BTreeMap<&str, &str> = mapping
        .codes
        .iter()
        .map(|(k, v)| (v.as_str(), k.as_str()))
        .collect();
    for class in &classes {
        let ours = sting_cmp.by_detector.get(*class);
        let theirs = acton_cmp.by_detector.get(*class);
        // A class outside the declared mapping is missing from the participant not because it erred
        // but because it has no such rule.
        let covered = declared.contains_key(class.as_str());
        let _ = writeln!(
            out,
            "| {class} | {} | {} | {} | {} | {} | {} |",
            counts(ours),
            share(ours.and_then(precision)),
            share(ours.and_then(recall)),
            if covered {
                counts(theirs)
            } else {
                "no rule".to_string()
            },
            if covered {
                share(theirs.and_then(precision))
            } else {
                "—".to_string()
            },
            if covered {
                share(theirs.and_then(recall))
            } else {
                "—".to_string()
            },
        );
    }

    // The aggregate is taken over the classes the participant has a rule for, on both sides. Summing
    // ours over classes they do not cover and calling the two numbers comparable would be a
    // comparison of coverage dressed as a comparison of accuracy.
    let shared: Vec<&String> = classes
        .iter()
        .copied()
        .filter(|class| declared.contains_key(class.as_str()))
        .collect();
    let _ = writeln!(out);
    let _ = writeln!(
        out,
        "| Over the {} shared class(es) | TP | FP | FN | Precision | Recall |",
        shared.len()
    );
    let _ = writeln!(out, "|---|---|---|---|---|---|");
    for (name, report) in [("sting", &sting_cmp), ("acton", &acton_cmp)] {
        let total = shared
            .iter()
            .filter_map(|class| report.by_detector.get(*class))
            .fold((0u32, 0u32, 0u32), |(tp, fp, fn_), m| {
                (
                    tp + m.true_positives,
                    fp + m.false_positives,
                    fn_ + m.false_negatives,
                )
            });
        let _ = writeln!(
            out,
            "| {name} | {} | {} | {} | {} | {} |",
            total.0,
            total.1,
            total.2,
            share(ratio(total.0, total.0 + total.1)),
            share(ratio(total.0, total.0 + total.2)),
        );
    }
}

/// `TP/FP/FN`, or the honest absence of a measurement.
fn counts(metrics: Option<&sting_bench::DetectorMetrics>) -> String {
    metrics.map_or_else(
        || "not covered".to_string(),
        |m| {
            format!(
                "{}/{}/{}",
                m.true_positives, m.false_positives, m.false_negatives
            )
        },
    )
}

/// Of what it reported, how much was labelled.
fn precision(metrics: &sting_bench::DetectorMetrics) -> Option<f64> {
    ratio(
        metrics.true_positives,
        metrics.true_positives + metrics.false_positives,
    )
}

/// Of what was labelled, how much it reported.
fn recall(metrics: &sting_bench::DetectorMetrics) -> Option<f64> {
    ratio(
        metrics.true_positives,
        metrics.true_positives + metrics.false_negatives,
    )
}

/// A share, or nothing at all where the denominator is zero.
///
/// `None` rather than zero, and the difference is the whole point: a participant that reported
/// nothing in a class has not achieved a precision of zero, it has not been measured in that class.
fn ratio(part: u32, whole: u32) -> Option<f64> {
    (whole > 0).then(|| f64::from(part) / f64::from(whole))
}

/// A share as a percentage, and `n/a` where there was nothing to take a share of.
fn share(value: Option<f64>) -> String {
    match value {
        Some(share) => format!("{:.1}%", share * 100.0),
        None => "n/a".to_string(),
    }
}
