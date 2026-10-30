//! Rendering the hunt as a document somebody else can check.
//!
//! The reach table comes first and the findings second, in that order on purpose. A findings list
//! read without knowing how much of the scope was looked at invites the one conclusion this
//! repository may never publish — that what was not examined is safe. Putting the reach first makes
//! the reader pass the coverage before reaching the count.
//!
//! A target the analyser cannot read is printed with the language it is written in, never as a zero.
//! "No rule for this" and "the rule found nothing" are different claims, and a table that renders
//! them identically flatters whichever tool reads less.

use std::fmt::Write as _;

use crate::hunt::Report;

/// Render the whole hunt as Markdown.
///
/// `heading` names the run, so a report regenerated at a later commit does not overwrite the
/// question the previous one answered.
#[must_use]
pub fn markdown(report: &Report, heading: &str) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "# {heading}\n");
    reach_section(&mut out, report);
    findings_section(&mut out, report);
    out
}

/// The coverage table: every published scope entry, and how much of it is readable.
fn reach_section(out: &mut String, report: &Report) {
    let reach = &report.reach;
    let _ = writeln!(
        out,
        "## Reach\n\n{} of {} scope entries carry code this analyser reads, {} of {} files in \
         all. A further {} files sit outside the scope by the programmes' own exclusions.\n",
        reach.readable_targets,
        reach.total_targets,
        reach.readable_files,
        reach.total_files,
        reach.excluded_files
    );
    let _ = writeln!(
        out,
        "| Tier | Target | Programme | Commit | Files in scope | Readable | Written in |"
    );
    let _ = writeln!(out, "|---|---|---|---|---|---|---|");
    for target in &reach.targets {
        let commit = target
            .commit
            .as_ref()
            .map_or_else(|| "no repository named".to_owned(), |c| short(c));
        let readable = if target.commit.is_none() {
            "not resolvable".to_owned()
        } else if target.files_readable == 0 {
            "none".to_owned()
        } else {
            target.files_readable.to_string()
        };
        let written_in = languages(target);
        let _ = writeln!(
            out,
            "| {} | {} | {} | {} | {} | {} | {} |",
            target.tier.label(),
            target.target,
            target.programme,
            commit,
            target.files_in_scope,
            readable,
            written_in
        );
    }
    let _ = writeln!(out);
}

/// What a target is written in, as measured, or why the question does not apply.
fn languages(target: &crate::reach::Reach) -> String {
    if let Some(reason) = target.unresolved {
        return reason.to_owned();
    }
    let ranked = target.dominant_extensions(3);
    if ranked.is_empty() {
        return "nothing at the named path".to_owned();
    }
    ranked
        .iter()
        .map(|(extension, count)| format!("{extension} {count}"))
        .collect::<Vec<_>>()
        .join(", ")
}

/// What fired, per target, and what was not read.
fn findings_section(out: &mut String, report: &Report) {
    let _ = writeln!(out, "## The run\n");
    if report.runs.is_empty() {
        let _ = writeln!(
            out,
            "No entry of the published scope carries code the static contour reads, so nothing was \
             analysed. This is a statement about reach and not about the code: none of it was \
             examined, and none of it is therefore called clean.\n"
        );
        return;
    }
    let _ = writeln!(
        out,
        "{} files analysed, {} findings, {} files read only in part, {} unreadable.\n",
        report
            .runs
            .iter()
            .map(|run| run.files_analysed)
            .sum::<usize>(),
        report.hits(),
        report.unparsed_files(),
        report.unreadable_files()
    );
    for run in &report.runs {
        let _ = writeln!(
            out,
            "### {} — {}\n\nAt `{}`, {} files analysed.\n",
            run.target,
            run.programme,
            short(&run.commit),
            run.files_analysed
        );
        if !run.unparsed.is_empty() || !run.unreadable.is_empty() {
            let _ = writeln!(out, "| File | Not read |");
            let _ = writeln!(out, "|---|---|");
            for file in &run.unparsed {
                let _ = writeln!(
                    out,
                    "| {}:{} | {} node, {} in the file |",
                    cell(&file.file),
                    file.line,
                    cell(&file.kind),
                    file.regions
                );
            }
            for file in &run.unreadable {
                let _ = writeln!(out, "| {} | {} |", cell(&file.file), cell(&file.reason));
            }
            let _ = writeln!(out);
        }
        if run.hits.is_empty() {
            let _ = writeln!(
                out,
                "Nothing fired. The search did not reach a defect within this budget under this \
                 scope.\n"
            );
            continue;
        }
        let _ = writeln!(out, "| Detector | Severity | Where | What |");
        let _ = writeln!(out, "|---|---|---|---|");
        for hit in &run.hits {
            let _ = writeln!(
                out,
                "| {} | {} | {}:{} | {} |",
                hit.detector,
                hit.severity,
                hit.file,
                hit.line,
                hit.message.replace('|', "\\|")
            );
        }
        let _ = writeln!(out);
    }
}

/// One table cell. Every column goes through this: a pipe anywhere in a value splits the row, and
/// the row most likely to carry one is the one saying what could not be read.
fn cell(value: &str) -> String {
    value.replace('|', "\\|")
}

/// A commit short enough for a table and long enough to be unambiguous in a repository this size.
fn short(commit: &str) -> String {
    commit.chars().take(12).collect()
}

#[cfg(test)]
#[allow(
    clippy::expect_used,
    reason = "docs/ARCHITECTURE.md §3.2 scopes the no-unwrap/expect policy to library code outside tests"
)]
mod tests {
    use super::markdown;
    use crate::hunt::Report;
    use crate::reach::{Reach, ReachSummary};
    use crate::scope::{Programme, Scope, Target, Tier};

    const PROGRAMME: Programme = Programme {
        name: "programme",
        url: "https://example.invalid",
    };
    const PINNED: Target = Target {
        tier: Tier::Ecosystem,
        name: "pinned target",
        programme: PROGRAMME,
        scope: Scope::Repository {
            url: "https://example.invalid/repo",
            reference: "main",
            paths: &[],
            excluded: &[],
        },
    };

    /// A target written in a language the contour does not read must never render as a zero: a
    /// reader who sees "0" beside a name concludes the code was examined and found clean.
    #[test]
    fn an_unreadable_target_names_its_language_instead_of_reporting_zero() {
        let reach = Reach::measure(&PINNED, "c0ffeec0ffee00", &["src/node.cpp".to_owned()]);
        let report = Report {
            schema_version: crate::hunt::SCHEMA_VERSION,
            reach: ReachSummary::of(vec![reach]),
            runs: Vec::new(),
        };
        let rendered = markdown(&report, "run");
        assert!(rendered.contains("cpp 1"), "{rendered}");
        assert!(rendered.contains("none"), "{rendered}");
        assert!(
            rendered.contains("none of it is therefore called clean"),
            "{rendered}"
        );
    }

    /// The reach has to be stated before the findings, or the count is read as coverage.
    #[test]
    fn reach_is_rendered_before_the_findings() {
        let report = Report {
            schema_version: crate::hunt::SCHEMA_VERSION,
            reach: ReachSummary::of(vec![Reach::unpinned(&PINNED)]),
            runs: Vec::new(),
        };
        let rendered = markdown(&report, "run");
        let reach = rendered.find("## Reach").expect("reach section");
        let run = rendered.find("## The run").expect("run section");
        assert!(reach < run, "{rendered}");
    }
}
