//! Errors from resolving, running, or reading the output of the vendored
//! native `fift` assembler.

use std::path::PathBuf;
use std::time::Duration;

/// Everything that can go wrong driving [`crate::FiftAssembler`].
#[derive(Debug, thiserror::Error)]
pub enum FiftError {
    /// [`crate::default_executable_path`] could not determine the running
    /// executable's own path (`std::env::current_exe` failed).
    #[error("failed to determine the running executable's own path: {0}")]
    CurrentExe(#[source] std::io::Error),
    /// Spawning the `fift` process itself failed (not found, not
    /// executable, permission denied, ...).
    #[error("failed to spawn `fift` at {path}: {source}")]
    Spawn {
        /// The executable path that failed to spawn.
        path: PathBuf,
        /// The underlying OS error.
        #[source]
        source: std::io::Error,
    },
    /// `fift` ran but exited non-zero — a real assembly error (e.g.
    /// malformed Fift source), not an integration failure. Carries its
    /// stderr text verbatim.
    #[error("`fift` exited with an assembly error:\n{stderr}")]
    AssembleFailed {
        /// `fift`'s stderr, verbatim.
        stderr: String,
    },
    /// `fift` exited successfully but the expected `.compiled.boc` sibling
    /// artifact is missing. Confirmed empirically (see [`crate`]'s own
    /// module doc comment) that this artifact's path is baked as a literal
    /// string constant into the Fift source text at `tolk --output` time —
    /// this error fires either if `fift_path` was not itself produced by
    /// `tolk --output` (no such baked `B>file` call exists in the source at
    /// all), or if the scratch directory that path pointed into was deleted
    /// before this call ran.
    #[error("`fift` did not produce the expected artifact {path}")]
    MissingArtifact {
        /// The artifact path that should have existed.
        path: PathBuf,
    },
    /// Reading the produced `.compiled.boc` artifact failed.
    #[error("failed to read {path}: {source}")]
    ReadArtifact {
        /// The artifact path that failed to read.
        path: PathBuf,
        /// The underlying OS error.
        #[source]
        source: std::io::Error,
    },
    /// The assembler did not finish within [`crate::FiftAssembler`]'s
    /// configured timeout — killed rather than left running. Same threat
    /// model as `sting_tolkc::TolkcError::Timeout`: a public HTTP endpoint
    /// assembling untrusted-derived Fift source cannot rely on `fift`
    /// finishing on its own.
    #[error("`fift` did not finish within {timeout:?}, killed")]
    Timeout {
        /// The configured timeout that was exceeded.
        timeout: Duration,
    },
}
