//! Errors from resolving, running, or parsing the output of the vendored
//! native `tolk` compiler.

use std::path::PathBuf;
use std::time::Duration;

/// Everything that can go wrong driving [`crate::TolkCompiler`].
#[derive(Debug, thiserror::Error)]
pub enum TolkcError {
    /// [`crate::default_executable_path`] could not determine the running
    /// executable's own path (`std::env::current_exe` failed).
    #[error("failed to determine the running executable's own path: {0}")]
    CurrentExe(#[source] std::io::Error),
    /// Creating a scratch directory for `tolk`'s `--output` artifacts
    /// failed.
    #[error("failed to create a scratch directory for `tolk`'s output: {0}")]
    ScratchDir(#[source] std::io::Error),
    /// Spawning the `tolk` process itself failed (not found, not
    /// executable, permission denied, ...).
    #[error("failed to spawn `tolk` at {path}: {source}")]
    Spawn {
        /// The executable path that failed to spawn.
        path: PathBuf,
        /// The underlying OS error.
        #[source]
        source: std::io::Error,
    },
    /// `tolk` ran but exited non-zero — a real compile error, not an
    /// integration failure. Carries its stderr text verbatim (`tolk`'s own
    /// error format, confirmed empirically: plain human-readable text,
    /// `file:line: message` style, not JSON unless `--json-errors` is
    /// passed — this crate does not pass that flag, see
    /// `TolkCompiler::compile`'s own doc comment for why).
    #[error("`tolk` exited with a compile error:\n{stderr}")]
    CompileFailed {
        /// `tolk`'s stderr, verbatim.
        stderr: String,
    },
    /// `tolk` exited successfully but an expected sibling artifact
    /// (`.abi.json`/`.symbolTypes.json`) is missing — would indicate `tolk`
    /// changed its own output-file convention in a future version.
    #[error("`tolk` did not produce the expected artifact {path}")]
    MissingArtifact {
        /// The artifact path that should have existed.
        path: PathBuf,
    },
    /// Reading a produced artifact file failed.
    #[error("failed to read {path}: {source}")]
    ReadArtifact {
        /// The artifact path that failed to read.
        path: PathBuf,
        /// The underlying OS error.
        #[source]
        source: std::io::Error,
    },
    /// A produced artifact was not valid JSON, or not shaped the way this
    /// crate expects — would indicate `tolk` changed its artifact schema
    /// in a future version in a way this crate's types don't tolerate.
    #[error("failed to parse {path} as JSON: {source}")]
    ParseArtifact {
        /// The artifact path that failed to parse.
        path: PathBuf,
        /// The underlying parse error.
        #[source]
        source: serde_json::Error,
    },
    /// `tolk --version`'s output did not match the expected
    /// three-line format this crate parses — would indicate a `tolk`
    /// version whose `--version` output changed shape.
    #[error("unexpected `tolk --version` output, could not parse:\n{raw}")]
    VersionFormat {
        /// The raw, unparsed output.
        raw: String,
    },
    /// The compiler did not finish within [`crate::TolkCompiler`]'s
    /// configured timeout — killed rather than left running. Added for
    /// `sting-abi-service` (`docs/domains/service.md`):
    /// a public HTTP endpoint compiling untrusted source cannot rely on
    /// `tolk` finishing on its own, unlike this crate's original caller
    /// (`sting-pipeline::LintController`, a local trusted CLI where a
    /// pathologically slow compile was never a threat model, only ever an
    /// inconvenience).
    #[error("`tolk` did not finish within {timeout:?}, killed")]
    Timeout {
        /// The configured timeout that was exceeded.
        timeout: Duration,
    },
}
