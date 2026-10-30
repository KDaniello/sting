//! CLI-level integration test: actually spawns the compiled `sting` binary
//! and runs `sting lint` against a real corpus fixture, checking exit code
//! and stdout — closes the gap between "the controllers are tested" and
//! "the CLI actually wires them up correctly," which nothing previously
//! covered . Every other test in this crate is a unit test of a pure
//! function or a `clap` definition smoke test — none of them actually runs
//! the binary.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "docs/ARCHITECTURE.md §3.2 scopes the no-unwrap/expect policy to library code outside tests"
)]

use std::path::PathBuf;
use std::process::Command;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// Runs `sting lint --format human` on the canonical positive `AC` fixture,
/// optionally with a `Sting.toml` written next to it, and returns stdout.
fn lint_ac_fixture(config: Option<&str>) -> String {
    let fixture = repo_root()
        .join("corpus/fixtures/detectors/AC/positive/01-withdraw-with-no-check-at-all.tolk");
    assert!(
        fixture.is_file(),
        "fixture must exist at {}",
        fixture.display()
    );

    let dir = tempfile::tempdir().expect("creating a temp dir must succeed");
    let target = dir.path().join("contract.tolk");
    std::fs::copy(&fixture, &target).expect("copying the fixture must succeed");
    if let Some(config) = config {
        std::fs::write(dir.path().join("Sting.toml"), config).expect("writing config must succeed");
    }

    let output = Command::new(env!("CARGO_BIN_EXE_sting"))
        .arg("lint")
        .arg("--format")
        .arg("human")
        .arg(&target)
        .current_dir(dir.path())
        .output()
        .expect("spawning the compiled sting binary must succeed");

    assert!(
        output.status.success(),
        "sting lint must exit 0 even when findings are present (only file-analysis failures \
         are a non-zero exit, see main.rs::lint's own doc comment) — stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).into_owned()
}

/// `AC` is experimental: compiled in and measured, but off unless asked for. This test and the next are a pair:
/// one would pass on a detector that had simply been deleted.
#[test]
fn lint_does_not_report_the_experimental_ac_detector_by_default() {
    let stdout = lint_ac_fixture(None);
    assert!(
        !stdout.contains("AC"),
        "AC is off by default (Detector::is_default_enabled) — it must not appear without \
         being named in Sting.toml, got:\n{stdout}"
    );
}

#[test]
fn lint_reports_ac_when_sting_toml_enables_it_explicitly() {
    let stdout = lint_ac_fixture(Some("[detectors]\nenable = [\"AC\"]\n"));
    assert!(
        stdout.contains("AC"),
        "expected the AC detector's finding once enabled explicitly, got:\n{stdout}"
    );
}

#[test]
fn lint_on_a_directory_with_no_tolk_files_exits_nonzero() {
    let empty_dir = repo_root().join("corpus/fixtures/detectors/AC/positive/does-not-exist");
    let output = Command::new(env!("CARGO_BIN_EXE_sting"))
        .arg("lint")
        .arg(&empty_dir)
        .output()
        .expect("spawning the compiled sting binary must succeed");

    assert!(
        !output.status.success(),
        "a nonexistent/empty path must fail loudly, not silently succeed with no output"
    );
}

/// A file the grammar reads only in part must not come back looking clean.
///
/// Every detector still runs over whatever parsed and the report still renders,
/// which is exactly the shape that would let an unread file pass for an empty
/// one. The input is written here rather than taken from `corpus/`, which by
/// its own gate contains nothing the grammar cannot read.
#[test]
fn lint_on_a_partly_unparsed_file_exits_nonzero_and_names_the_position() {
    let dir = tempfile::tempdir().expect("creating a temp dir must succeed");
    let target = dir.path().join("broken.tolk");
    std::fs::write(
        &target,
        "fun onInternalMessage(in: InMessage) {\n    var x = ;\n}\n",
    )
    .expect("writing the fixture must succeed");

    let output = Command::new(env!("CARGO_BIN_EXE_sting"))
        .arg("lint")
        .arg(&target)
        .output()
        .expect("spawning the compiled sting binary must succeed");

    assert!(
        !output.status.success(),
        "a file with an ERROR node must fail loudly; a clean report on it would be \
         the grammar's silence, not the code's"
    );

    let stderr = strip_ansi(&String::from_utf8_lossy(&output.stderr));
    assert!(
        stderr.contains("line=2") && stderr.contains("kind=\"ERROR\""),
        "the diagnostic must name the position the grammar stopped at, got:\n{stderr}"
    );
}

/// Drops the terminal escape sequences `tracing` colours its fields with, so an
/// assertion reads the diagnostic's content rather than its presentation.
fn strip_ansi(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars();
    while let Some(c) = chars.next() {
        if c != '\u{1b}' {
            out.push(c);
            continue;
        }
        for c in chars.by_ref() {
            if c.is_ascii_alphabetic() {
                break;
            }
        }
    }
    out
}

/// `sting doctor` names every version and refuses when one is unknown.
///
/// The versions themselves are not asserted — they move with the vendored
/// toolchain, and a test that pinned them would be a second place to update.
/// What is asserted is the shape: four components, each with a source, and an
/// exit code that follows from whether any of them is unknown.
#[test]
fn doctor_names_four_components_and_its_exit_code_follows_from_them() {
    let output = Command::new(env!("CARGO_BIN_EXE_sting"))
        .arg("doctor")
        .output()
        .expect("spawning the compiled sting binary must succeed");

    let stdout = strip_ansi(&String::from_utf8_lossy(&output.stdout));
    for component in [
        "libemulator",
        "Tolk compiler",
        "Tolk grammar",
        "global version",
    ] {
        assert!(
            stdout.contains(component),
            "the report must name {component}, got:\n{stdout}"
        );
    }

    let unknown = stdout.matches("unknown").count();
    assert_eq!(
        output.status.success(),
        unknown == 0,
        "the exit code must follow from the report: {unknown} unknown, exit {:?}\n{stdout}",
        output.status.code()
    );
}
