//! `ReportController`: findings → rendered report
//! (`docs/ARCHITECTURE.md`).

use std::cmp::Reverse;
use std::collections::HashMap;
use std::fmt::Write as _;
use std::path::Path;

use sting_core::{Finding, Location, Severity};

use crate::PipelineError;

/// Output format for a rendered report.
///
/// `Json` is `--format {human,sarif,json}`, consistent across `lint` and
/// `scan` (`docs/API.md`) — a plain
/// JSON array of [`Finding`]s, deliberately **not** the same thing as
/// `sting-bench`'s own `to_json_string(BenchReport)`
/// (`docs/ARCHITECTURE.md`'s older note called `json` "internal
/// format for `sting bench`"; that note predates this variant and is now
/// stale — `sting bench` keeps its own `--report`-gated `BenchReport` JSON,
/// a structurally different report shape (aggregate metrics, not a finding
/// list), unrelated to this enum). This variant exists for `lint`/`scan`
/// callers that want machine-readable output without SARIF's schema
/// overhead.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReportFormat {
    /// One line per finding, for a terminal.
    Human,
    /// SARIF 2.1.0 JSON (`sting-report`).
    Sarif,
    /// A plain JSON array of `Finding`s (`serde_json`, not SARIF).
    Json,
}

/// Renders `Vec<Finding>` into a [`ReportFormat`].
///
/// `docs/ARCHITECTURE.md`: today a thin pass-through to
/// `sting-report`/a human formatter over a plain `&[Finding]`; grows to also
/// fold in `DynamicVerification`/`Confidence` once
/// `docs/domains/emulator.md` exists — the controller (not a
/// bare function in `sting-cli`) is the seam that later addition attaches
/// to without reshaping every caller.
pub struct ReportController;

impl ReportController {
    /// Renders `findings` as `format`.
    ///
    /// # Errors
    /// Returns [`PipelineError::Report`] if SARIF serialization fails (see
    /// [`sting_report::to_json_string`]'s doc comment for when that can
    /// happen in practice — essentially never, for a `Sarif` value built
    /// entirely from this workspace's own types) or [`PipelineError::Json`]
    /// if plain JSON serialization fails (same practical non-occurrence,
    /// `Finding` derives `serde::Serialize` structurally).
    ///
    /// `color` only ever affects `ReportFormat::Human` — ANSI escape codes
    /// in `sarif`/`json` output would corrupt the format they're supposed
    /// to be, so it is silently ignored for those two
    /// . The caller
    /// (`sting-cli`) decides the bool — whether stdout is a real terminal,
    /// `NO_COLOR`, `--color` — rather than this controller doing that
    /// terminal-dependent detection itself, keeping it a pure function of
    /// its arguments and directly testable (`docs/ARCHITECTURE.md`
    /// §0).
    pub fn render(
        &self,
        findings: &[Finding],
        format: ReportFormat,
        color: bool,
    ) -> Result<String, PipelineError> {
        match format {
            ReportFormat::Human => Ok(render_human(findings, color)),
            ReportFormat::Sarif => Ok(sting_report::to_json_string(findings)?),
            ReportFormat::Json => Ok(serde_json::to_string_pretty(findings)?),
        }
    }
}

/// Per-file source text, read once and reused across every finding that
/// points into the same file — most real runs report several findings per
/// file, and re-reading the same source from disk once per finding would be
/// wasted I/O for no benefit. `None` means the read was already attempted
/// and failed (or the path is not a real readable file at all — e.g. `sting
/// scan`'s [`Location::file`] is the on-chain address string itself, not a
/// filesystem path, see `address_level_finding` in `sting-cli`) — cached so
/// a second finding in the same unreadable "file" doesn't retry the read.
type FileCache<'a> = HashMap<&'a Path, Option<Vec<String>>>;

fn render_human(findings: &[Finding], color: bool) -> String {
    if findings.is_empty() {
        return "no findings".to_string();
    }
    let mut file_cache: FileCache<'_> = HashMap::new();
    let mut blocks: Vec<String> = findings
        .iter()
        .map(|f| render_finding_human(f, &mut file_cache, color))
        .collect();
    blocks.push(summary_line(findings));
    blocks.join("\n\n")
}

/// The trailing summary line — `rustc`'s own "N warnings emitted", extended
/// with a severity breakdown and a per-detector breakdown
/// : a developer
/// scanning CI output wants "how bad is this, at a glance" and "which
/// detector do I go read about" without re-reading every individual line.
/// Deliberately starts with the exact `"{n} finding(s)"` prefix the old,
/// simpler line already had — call sites/tests that only cared about the
/// count keep working unchanged. Never colorized, matching `rustc`'s own
/// convention of coloring only individual diagnostic level-words, not its
/// final tally line.
fn summary_line(findings: &[Finding]) -> String {
    let file_count = findings
        .iter()
        .map(|f| f.location.file.as_path())
        .collect::<std::collections::HashSet<_>>()
        .len();

    let mut by_severity: Vec<(Severity, usize)> = Vec::new();
    for f in findings {
        match by_severity.iter_mut().find(|(s, _)| *s == f.severity) {
            Some((_, count)) => *count += 1,
            None => by_severity.push((f.severity, 1)),
        }
    }
    // Most severe first — `Severity`'s derived `Ord` follows declaration
    // order (Info < ... < Critical), so descending puts Critical first.
    by_severity.sort_by_key(|(severity, _)| Reverse(*severity));
    let severity_breakdown = by_severity
        .iter()
        .map(|(s, count)| format!("{count} {s:?}"))
        .collect::<Vec<_>>()
        .join(", ");

    let mut by_detector: Vec<(&'static str, usize)> = Vec::new();
    for f in findings {
        match by_detector.iter_mut().find(|(id, _)| *id == f.detector_id) {
            Some((_, count)) => *count += 1,
            None => by_detector.push((f.detector_id, 1)),
        }
    }
    by_detector.sort_by_key(|(id, _)| *id);
    let detector_breakdown = by_detector
        .iter()
        .map(|(id, count)| format!("{id}×{count}"))
        .collect::<Vec<_>>()
        .join(", ");

    format!(
        "{} finding(s) ({severity_breakdown}) across {file_count} file(s): {detector_breakdown}",
        findings.len()
    )
}

/// One finding's full human-readable block: the existing `file:line:col:
/// [ID] Severity: message` header line, then — best-effort, only when
/// `f.location.file` is actually readable on this machine at render time —
/// a `rustc`/`clippy`-style source snippet with a caret under the exact
/// column range, then always a trailing `help:` line from
/// [`Finding::remediation`]. Snippet rendering is single-line only: a
/// finding spanning multiple lines gets a caret under just the first line,
/// from `start_column` to that line's end, not a full multi-line
/// highlight — a deliberately narrower scope than `rustc`'s own renderer,
/// not a bug (every detector in this workspace flags single-expression/
/// single-statement spans in practice, so this has not needed to be richer
/// yet).
fn render_finding_human<'a>(f: &'a Finding, file_cache: &mut FileCache<'a>, color: bool) -> String {
    let ansi_code = severity_ansi_code(f.severity);
    let prefix = colorize(
        &format!("[{}] {:?}:", f.detector_id, f.severity),
        ansi_code,
        color,
    );
    let mut out = format!(
        "{}:{}:{}: {prefix} {}",
        f.location.file.display(),
        f.location.start_line,
        f.location.start_column,
        f.message
    );
    let cached = file_cache
        .entry(f.location.file.as_path())
        .or_insert_with(|| {
            std::fs::read_to_string(&f.location.file)
                .ok()
                .map(|text| text.lines().map(str::to_string).collect())
        });
    if let Some(lines) = cached.as_deref()
        && let Some(snippet) = source_snippet(lines, &f.location, ansi_code, color)
    {
        out.push('\n');
        out.push_str(&snippet);
    }
    let _ = write!(out, "\n  = help: {}", f.remediation);
    out
}

/// ANSI SGR parameters (no leading `\x1b[`/trailing `m`) for `severity` —
/// 's fixed palette
/// (Critical=red, High=orange, Medium=yellow, Low=blue, Info=grey). High
/// gets a 256-color code (`38;5;208`, true orange) rather than reusing
/// standard yellow — the whole point of a 5-tier severity scale is that
/// High reads as visibly distinct from Medium, not just bold/plain of the
/// same color.
fn severity_ansi_code(severity: Severity) -> &'static str {
    match severity {
        Severity::Critical => "1;31",
        Severity::High => "38;5;208",
        Severity::Medium => "33",
        Severity::Low => "34",
        Severity::Info => "90",
    }
}

/// Wraps `text` in the ANSI escape sequence for `ansi_code` when `color` is
/// set, otherwise returns it unchanged — the one place that decides whether
/// escape codes exist at all, so every other renderer function stays a
/// plain string builder.
fn colorize(text: &str, ansi_code: &str, color: bool) -> String {
    if color {
        format!("\x1b[{ansi_code}m{text}\x1b[0m")
    } else {
        text.to_string()
    }
}

/// A `rustc`-style two/three-line snippet: a blank gutter, the flagged
/// line prefixed with its (1-indexed) line number, and a caret line
/// underlining the finding's column range on that line. `None` if
/// `f.location.start_line` is out of range for `lines` (a finding on a
/// source that changed since it was reported, or a non-file `Location`
/// like `sting scan`'s address-level findings, whose caller already
/// filtered those out via the `Option` in [`FileCache`]).
fn source_snippet(
    lines: &[String],
    loc: &Location,
    ansi_code: &str,
    color: bool,
) -> Option<String> {
    let index = loc.start_line.checked_sub(1)?;
    let line = lines.get(usize::try_from(index).ok()?)?;
    let line_num = loc.start_line.to_string();
    let gutter = " ".repeat(line_num.len());
    let char_offset = byte_column_to_char_offset(line, loc.start_column);
    let caret_len = if loc.end_line == loc.start_line {
        caret_span_width(line, loc.start_column, loc.end_column)
    } else {
        1
    };
    let carets = colorize(&"^".repeat(caret_len), ansi_code, color);
    Some(format!(
        "{gutter} |\n{line_num} | {line}\n{gutter} | {}{carets}",
        " ".repeat(char_offset)
    ))
}

/// Converts a 1-indexed, **byte-offset** column (`Location::start_column`'s
/// actual unit — tree-sitter's own convention, see
/// `sting_ast::SourceRange::from_node`'s doc comment) into a character
/// count, so padding a caret with `' '.repeat(n)` lines it up under the
/// right on-screen column even when a multi-byte UTF-8 character (e.g. a
/// Cyrillic identifier or comment) appears earlier on the same line —
/// `chars().count()` up to the byte offset, not the byte offset itself.
fn byte_column_to_char_offset(line: &str, byte_column_1_indexed: u32) -> usize {
    let Ok(byte_index) = usize::try_from(byte_column_1_indexed.saturating_sub(1)) else {
        return 0;
    };
    let mut index = byte_index.min(line.len());
    while index > 0 && !line.is_char_boundary(index) {
        index -= 1;
    }
    line[..index].chars().count()
}

/// Character-count width of the span between two 1-indexed byte columns on
/// the same line — at least 1, so a zero-width span (shouldn't happen for a
/// real finding, but not asserted away) still renders a visible caret
/// rather than nothing.
fn caret_span_width(line: &str, start_column: u32, end_column: u32) -> usize {
    let start = byte_column_to_char_offset(line, start_column);
    let end = byte_column_to_char_offset(line, end_column);
    end.saturating_sub(start).max(1)
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "docs/ARCHITECTURE.md §3.2 scopes the no-unwrap/expect policy to library code outside tests"
)]
mod tests {
    use std::path::PathBuf;

    use sting_core::{Finding, Location, Severity};

    use super::{ReportController, ReportFormat};

    fn sample_finding() -> Finding {
        Finding {
            detector_id: "GVR",
            severity: Severity::High,
            message: "shadowed field".to_string(),
            remediation: "rename the local variable".to_string(),
            location: Location {
                file: PathBuf::from("contract.tolk"),
                start_line: 5,
                start_column: 9,
                end_line: 5,
                end_column: 14,
            },
            confidence: None,
            dynamic_verification: None,
            suggestion: None,
        }
    }

    #[test]
    fn human_format_reports_no_findings_explicitly() {
        let rendered = ReportController
            .render(&[], ReportFormat::Human, false)
            .expect("renders");
        assert_eq!(rendered, "no findings");
    }

    #[test]
    fn human_format_ends_with_a_severity_and_detector_breakdown_summary() {
        let high = sample_finding();
        let mut low = sample_finding();
        low.detector_id = "SUA";
        low.severity = Severity::Low;
        low.location.file = PathBuf::from("other.tolk");

        let rendered = ReportController
            .render(&[high, low], ReportFormat::Human, false)
            .expect("renders");
        let summary = rendered.lines().last().expect("a summary line");
        assert_eq!(
            summary,
            "2 finding(s) (1 High, 1 Low) across 2 file(s): GVR×1, SUA×1"
        );
    }

    #[test]
    fn human_format_includes_file_line_column_and_message() {
        let rendered = ReportController
            .render(&[sample_finding()], ReportFormat::Human, false)
            .expect("renders");
        assert!(rendered.contains("contract.tolk:5:9"));
        assert!(rendered.contains("GVR"));
        assert!(rendered.contains("shadowed field"));
        assert!(rendered.contains("1 finding(s)"));
    }

    #[test]
    fn sarif_format_produces_parseable_sarif_json() {
        let rendered = ReportController
            .render(&[sample_finding()], ReportFormat::Sarif, false)
            .expect("renders");
        let value: serde_json::Value = serde_json::from_str(&rendered).expect("valid json");
        assert_eq!(value["version"], "2.1.0");
        assert_eq!(
            value["runs"][0]["results"]
                .as_array()
                .expect("results array")
                .len(),
            1
        );
    }

    #[test]
    fn human_format_includes_a_help_line() {
        let rendered = ReportController
            .render(&[sample_finding()], ReportFormat::Human, false)
            .expect("renders");
        assert!(rendered.contains("= help: rename the local variable"));
    }

    #[test]
    fn human_format_omits_the_snippet_when_the_file_is_not_readable() {
        // sample_finding()'s file ("contract.tolk") does not exist relative
        // to wherever tests run from — the exact shape `sting scan`'s
        // address-level findings hit too (`Location::file` is not a real
        // path at all there). The help line must still show up regardless.
        let rendered = ReportController
            .render(&[sample_finding()], ReportFormat::Human, false)
            .expect("renders");
        assert!(
            !rendered.contains(" | "),
            "no snippet gutter should appear for an unreadable file: {rendered:?}"
        );
        assert!(rendered.contains("= help: rename the local variable"));
    }

    #[test]
    fn human_format_is_byte_identical_whether_or_not_a_readable_file_backs_it_when_color_is_off() {
        // Locks in that `color: false` is a true no-op, not just "no visible
        // color" — no stray escape bytes at all, since a saved --output
        // file must never contain them.
        let rendered = ReportController
            .render(&[sample_finding()], ReportFormat::Human, false)
            .expect("renders");
        assert!(!rendered.contains('\x1b'));
    }

    #[test]
    fn human_format_wraps_the_severity_prefix_and_caret_in_ansi_codes_when_color_is_on() {
        let dir = tempfile::tempdir().expect("creates a temp dir");
        let file = dir.path().join("contract.tolk");
        std::fs::write(&file, "fun f() {\n    var owner = 0;\n}\n").expect("writes fixture file");
        let finding = Finding {
            location: Location {
                file: file.clone(),
                start_line: 2,
                start_column: 9,
                end_line: 2,
                end_column: 14,
            },
            ..sample_finding()
        };

        let rendered = ReportController
            .render(&[finding], ReportFormat::Human, true)
            .expect("renders");
        // High severity -> the 256-color orange code around "[GVR] High:".
        assert!(rendered.contains("\x1b[38;5;208m[GVR] High:\x1b[0m"));
        // The caret line is wrapped in the same code, reset afterward.
        assert!(rendered.contains("\x1b[38;5;208m^^^^^\x1b[0m"));
        // The message/help text itself is never colorized.
        assert!(rendered.contains("\x1b[0m shadowed field"));
        // Exactly two colorized regions (prefix, caret) -> 4 escape codes
        // (open+reset each) — no stray extra coloring elsewhere.
        assert_eq!(rendered.matches('\x1b').count(), 4);
    }

    #[test]
    fn human_format_shows_a_source_snippet_with_a_caret_when_the_file_is_readable() {
        let dir = tempfile::tempdir().expect("creates a temp dir");
        let file = dir.path().join("contract.tolk");
        std::fs::write(&file, "fun f() {\n    var owner = 0;\n}\n").expect("writes fixture file");

        let finding = Finding {
            detector_id: "GVR",
            severity: Severity::High,
            message: "shadowed field".to_string(),
            remediation: "rename the local variable".to_string(),
            location: Location {
                file: file.clone(),
                start_line: 2,
                start_column: 9,
                end_line: 2,
                end_column: 14,
            },
            confidence: None,
            dynamic_verification: None,
            suggestion: None,
        };
        let rendered = ReportController
            .render(&[finding], ReportFormat::Human, false)
            .expect("renders");
        assert!(
            rendered.contains("var owner = 0;"),
            "expected the flagged source line verbatim: {rendered:?}"
        );
        // Column 9 (1-indexed, byte offset) on "    var owner = 0;" is where
        // `owner` starts; end_column 14 is one past where it ends — a
        // 5-character caret span (`^^^^^`) offset by 8 spaces (the char
        // count of "    var " before it).
        let caret_line = rendered
            .lines()
            .find(|l| l.contains('^'))
            .expect("a caret line must be present");
        assert_eq!(
            caret_line.matches('^').count(),
            5,
            "expected exactly 5 carets under `owner`: {caret_line:?}"
        );
        let before_carets = caret_line.trim_end_matches('^');
        assert!(
            before_carets.ends_with("        "),
            "expected 8 spaces of offset immediately before the carets: {caret_line:?}"
        );
        assert!(rendered.contains("= help: rename the local variable"));
    }

    #[test]
    fn caret_offset_counts_characters_not_bytes_before_a_multi_byte_prefix() {
        // "привет " is 7 Cyrillic/space characters but 13 UTF-8 bytes (each
        // Cyrillic letter is 2 bytes) — a byte-offset caret would land 6
        // columns too far right on this line if not converted to a char
        // count first.
        let line = "привет owner = 0;";
        let byte_column_of_owner =
            u32::try_from(line.find("owner").expect("substring present")).expect("fits in u32") + 1;
        let char_offset = super::byte_column_to_char_offset(line, byte_column_of_owner);
        assert_eq!(
            char_offset,
            line.chars().take_while(|&c| c != 'o').count(),
            "char offset must match the character (not byte) position of `owner`"
        );
    }
}
