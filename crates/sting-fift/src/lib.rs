//! Native-`fift`-CLI integration: assembles the Fift assembly text
//! `sting_tolkc::TolkCompiler::compile` produces into real, deployable
//! bytecode — the one piece `sting_tolkc` itself deliberately does not
//! provide (see that crate's own module doc comment: "Bytecode... is
//! deliberately not captured here").
//!
//! Same vendoring and naming reasoning as `sting_tolkc`: a subprocess
//! call plus a file read, no FFI, no `unsafe` anywhere in this crate — hence
//! no `-sys` suffix. `vendor/fift/` (binary + `fift-lib/` standard library
//! sources, `ton-blockchain/ton` release, same build as `tolk`/`libemulator`)
//! is `.gitignore`d, vendored the same way as `vendor/tolk/`.
//!
//! **The key empirical finding this crate's design depends on** (confirmed
//! live against a real corpus contract before writing any code, not
//! assumed): the Fift assembly text `tolk --output <path>.fif` produces
//! ends with a literal, already-baked instruction —
//! `dup boc>B "<path>.compiled.boc" B>file` — where `<path>.compiled.boc`
//! is `tolk`'s own sibling-artifact convention applied to *the exact
//! `--output` path tolk was invoked with*, embedded as a fixed string
//! constant in the Fift source itself. Running `fift` against that source
//! does not print the resulting bytecode to stdout for this crate to
//! capture; it writes it straight to that baked path. Consequently
//! [`FiftAssembler::assemble`] takes the *path* to the `.fif` file `tolk`
//! wrote (not the Fift text as a `&str`), and the directory that path lives
//! in must still exist on disk when `fift` runs — the caller (typically
//! `sting_abi_registry::TolkBytecodeSource`) is responsible for keeping
//! `tolk`'s scratch directory alive across both the compile step and this
//! crate's assemble step, exactly the reason `sting_tolkc::TolkCompiler`
//! exposes `compile_keep_scratch` alongside its ordinary `compile`.

mod error;

pub use error::FiftError;

use std::io::Read as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

use wait_timeout::ChildExt;

/// The platform-conventional `fift` executable filename this crate resolves
/// by default — mirrors `sting_tolkc::platform_executable_filename`'s
/// identical reasoning.
#[must_use]
pub const fn platform_executable_filename() -> &'static str {
    if cfg!(target_os = "windows") {
        "fift.exe"
    } else {
        "fift"
    }
}

/// Pure resolution logic behind [`default_executable_path`] — mirrors
/// `sting_tolkc::resolve_executable_path`'s identical reasoning (testable
/// without touching real process-global env state).
fn resolve_executable_path(env_override: Option<&str>, exe_path: &Path) -> PathBuf {
    if let Some(path) = env_override {
        return PathBuf::from(path);
    }
    let dir = exe_path.parent().unwrap_or_else(|| Path::new("."));
    dir.join(platform_executable_filename())
}

/// Resolves the default `fift` path: `STING_FIFT_PATH` if set, otherwise
/// [`platform_executable_filename`] next to the running executable — mirrors
/// `sting_tolkc::default_executable_path`'s identical convention.
///
/// # Errors
/// Returns [`FiftError::CurrentExe`] if the running executable's own path
/// cannot be determined.
pub fn default_executable_path() -> Result<PathBuf, FiftError> {
    let env_override = std::env::var("STING_FIFT_PATH").ok();
    let exe = std::env::current_exe().map_err(FiftError::CurrentExe)?;
    Ok(resolve_executable_path(env_override.as_deref(), &exe))
}

/// Where this workspace vendors the assembler.
///
/// Resolved from the crate's own directory at compile time, so it holds from a workspace checkout
/// and simply does not exist for a shipped binary.
#[must_use]
pub fn vendored_executable_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../vendor/fift")
        .join(platform_executable_filename())
}

/// `fift` resolves `"Asm.fif" include` (and every other Fift-library
/// include `tolk`-generated source needs) against a library search path —
/// confirmed empirically via `-I<path>` against the vendored
/// `fift-lib/` directory this crate expects to find next to the resolved
/// `fift` executable, the same "pin explicitly rather than trust
/// auto-discovery" reasoning as `sting_tolkc::stdlib_dir_for`.
fn lib_dir_for(executable_path: &Path) -> PathBuf {
    executable_path
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join("fift-lib")
}

/// `tolk`'s sibling-artifact convention (`sting_tolkc::sibling_artifact`)
/// applied to the `.compiled.boc` output baked into the Fift source by
/// `tolk --output` — see this crate's own module doc comment for why this
/// path, not `fift`'s stdout, is where the assembled bytecode lands.
fn sibling_boc_artifact(fift_path: &Path) -> PathBuf {
    fift_path.with_extension("compiled.boc")
}

/// Default wall-clock budget for one [`FiftAssembler::assemble`] call —
/// mirrors `sting_tolkc::DEFAULT_COMPILE_TIMEOUT`'s identical value and
/// reasoning; assembly is a smaller, strictly-cheaper step than the `tolk`
/// compile that precedes it, so the same budget is generous, not tight.
pub const DEFAULT_ASSEMBLE_TIMEOUT: Duration = Duration::from_secs(30);

/// Drives one vendored `fift` executable.
#[derive(Debug, Clone)]
pub struct FiftAssembler {
    executable_path: PathBuf,
    timeout: Duration,
}

impl FiftAssembler {
    /// Builds an assembler handle for the executable at
    /// [`default_executable_path`], with [`DEFAULT_ASSEMBLE_TIMEOUT`].
    ///
    /// # Errors
    /// Returns [`FiftError::CurrentExe`] under the same condition as
    /// [`default_executable_path`].
    pub fn from_default_path() -> Result<Self, FiftError> {
        Ok(Self::new(default_executable_path()?))
    }

    /// Builds an assembler handle for an explicit `fift` executable path,
    /// with [`DEFAULT_ASSEMBLE_TIMEOUT`].
    #[must_use]
    pub fn new(executable_path: PathBuf) -> Self {
        Self::with_timeout(executable_path, DEFAULT_ASSEMBLE_TIMEOUT)
    }

    /// Builds an assembler handle with an explicit timeout — e.g. a
    /// stricter one for a public, untrusted-input-derived caller. Mirrors
    /// `sting_tolkc::TolkCompiler::with_timeout`'s identical reasoning.
    #[must_use]
    pub fn with_timeout(executable_path: PathBuf, timeout: Duration) -> Self {
        Self {
            executable_path,
            timeout,
        }
    }

    /// The assembler this workspace can actually reach, or `None`.
    ///
    /// [`Self::from_default_path`] looks next to the running executable, which is right for a
    /// shipped release and wrong from a workspace checkout, where that path is `target/debug/`. The
    /// vendored directory is tried after it — the same pair, in the same order, that
    /// `sting_tolkc::TolkCompiler::vendored_or_default` states.
    ///
    /// `None` means no assembler is reachable: `vendor/fift/` is gitignored, so a fresh checkout
    /// has none, and a caller is expected to say which tool is missing rather than to fail as
    /// though the input were at fault.
    #[must_use]
    pub fn vendored_or_default() -> Option<Self> {
        if let Ok(assembler) = Self::from_default_path()
            && assembler.executable_path().is_file()
        {
            return Some(assembler);
        }
        let vendored = vendored_executable_path();
        vendored.is_file().then(|| Self::new(vendored))
    }

    /// The `fift` executable path this handle drives.
    #[must_use]
    pub fn executable_path(&self) -> &Path {
        &self.executable_path
    }

    /// Assembles the Fift source file at `fift_path` — which **must** be a
    /// path `tolk --output` itself wrote (see this crate's own module doc
    /// comment for why) and whose parent directory must still exist on disk
    /// — into real bytecode, returning the raw `.boc` bytes.
    ///
    /// # Errors
    /// See [`FiftError`]'s variants.
    pub fn assemble(&self, fift_path: &Path) -> Result<Vec<u8>, FiftError> {
        // Absolute before spawning, same reasoning as
        // `sting_tolkc::TolkCompiler::compile`: a relative `executable_path`
        // (e.g. a relative `STING_FIFT_PATH`) would otherwise resolve
        // `lib_dir_for` against the wrong base once this process's own cwd
        // differs from where the caller expects.
        let absolute_executable_path =
            std::path::absolute(&self.executable_path).map_err(|source| FiftError::Spawn {
                path: self.executable_path.clone(),
                source,
            })?;
        let lib_dir = lib_dir_for(&absolute_executable_path);

        let mut child = Command::new(&absolute_executable_path)
            .arg("-I")
            .arg(&lib_dir)
            .arg(fift_path)
            // Never read below — `Stdio::null()`, not `Stdio::piped()`, so
            // a chatty child can't fill an unread pipe buffer and block on
            // its own `write()`. stderr *is* read, so it stays piped and is
            // drained concurrently by `wait_draining_stderr` below instead.
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|source| FiftError::Spawn {
                path: self.executable_path.clone(),
                source,
            })?;

        let Some((status, stderr)) =
            wait_draining_stderr(&mut child, self.timeout).map_err(|source| FiftError::Spawn {
                path: self.executable_path.clone(),
                source,
            })?
        else {
            return Err(FiftError::Timeout {
                timeout: self.timeout,
            });
        };

        if !status.success() {
            return Err(FiftError::AssembleFailed { stderr });
        }

        let boc_path = sibling_boc_artifact(fift_path);
        if !boc_path.is_file() {
            return Err(FiftError::MissingArtifact { path: boc_path });
        }
        std::fs::read(&boc_path).map_err(|source| FiftError::ReadArtifact {
            path: boc_path,
            source,
        })
    }
}

/// Runs `child` to completion against `timeout`, draining its stderr pipe
/// on a background thread *while* waiting, not only afterward. A pipe is
/// bounded (~64KB on Linux/Windows) — a child that writes more than that to
/// stderr before exiting would otherwise block on its own `write()`
/// forever, and `wait_timeout` would spend its entire budget waiting on a
/// process that's alive but stuck, not actually hung. Real, exploitable
/// finding on a public HTTP endpoint that assembles untrusted-derived
/// source — see Mirrors
/// `sting_abi_tact::compiler::wait_draining_stderr`/
/// `sting_tolkc::wait_draining_stderr` exactly (see those crates' own doc
/// comments; no shared crate exists between them for this). Callers must
/// not pipe stdout (`Stdio::null()`) since nothing here drains it.
///
/// Returns `Ok(Some((status, stderr)))` on completion, `Ok(None)` if the
/// timeout elapsed (the child has already been killed and reaped by the
/// time this returns), `Err` if `wait_timeout` itself failed.
fn wait_draining_stderr(
    child: &mut std::process::Child,
    timeout: Duration,
) -> std::io::Result<Option<(std::process::ExitStatus, String)>> {
    let stderr_handle = child.stderr.take().map(|mut pipe| {
        std::thread::spawn(move || {
            let mut buf = String::new();
            let _ = pipe.read_to_string(&mut buf);
            buf
        })
    });
    let Some(status) = child.wait_timeout(timeout)? else {
        let _ = child.kill();
        let _ = child.wait();
        if let Some(handle) = stderr_handle {
            let _ = handle.join();
        }
        return Ok(None);
    };
    let stderr = stderr_handle
        .and_then(|handle| handle.join().ok())
        .unwrap_or_default();
    Ok(Some((status, stderr)))
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "docs/ARCHITECTURE.md §3.2 scopes the no-unwrap/expect policy to library code outside tests"
)]
mod tests {
    use super::{resolve_executable_path, sibling_boc_artifact, wait_draining_stderr};
    use std::path::{Path, PathBuf};
    use std::process::{Command, Stdio};
    use std::time::{Duration, Instant};

    #[test]
    fn resolve_executable_path_prefers_the_env_override() {
        let resolved = resolve_executable_path(Some("/custom/fift"), Path::new("/bin/sting"));
        assert_eq!(resolved, PathBuf::from("/custom/fift"));
    }

    #[test]
    fn resolve_executable_path_falls_back_to_next_to_the_running_executable() {
        let resolved = resolve_executable_path(None, Path::new("/usr/local/bin/sting"));
        assert_eq!(
            resolved,
            PathBuf::from("/usr/local/bin").join(super::platform_executable_filename())
        );
    }

    #[test]
    fn sibling_boc_artifact_replaces_the_fif_extension_not_appends_to_it() {
        let fif = Path::new("/tmp/scratch/out.fif");
        assert_eq!(
            sibling_boc_artifact(fif),
            PathBuf::from("/tmp/scratch/out.compiled.boc")
        );
    }

    /// Regression test for the pipe-buffer deadlock found in
    /// : a child that writes far more than one
    /// OS pipe buffer (~64KB) to stderr before failing must still be
    /// drained and reaped promptly, well under the configured timeout —
    /// with the old code (stderr read only *after* `wait_timeout`
    /// returned), the child would block on its own `write()` and this call
    /// would eat the entire timeout budget instead of returning almost
    /// immediately. Exercises the extracted helper directly (not the full
    /// `assemble()` path) since that only needs a generic noisy child
    /// process, not a real vendored `fift` binary.
    #[test]
    fn wait_draining_stderr_does_not_deadlock_on_a_chatty_failing_process() {
        let mut child = if cfg!(target_os = "windows") {
            Command::new("powershell")
                .args([
                    "-NoProfile",
                    "-Command",
                    "[Console]::Error.Write('x' * 300000); exit 7",
                ])
                .stdout(Stdio::null())
                .stderr(Stdio::piped())
                .spawn()
                .expect("spawn powershell")
        } else {
            Command::new("sh")
                .args(["-c", "yes x | head -c 300000 1>&2; exit 7"])
                .stdout(Stdio::null())
                .stderr(Stdio::piped())
                .spawn()
                .expect("spawn sh")
        };

        let started = Instant::now();
        let result = wait_draining_stderr(&mut child, Duration::from_secs(20));
        let elapsed = started.elapsed();

        let (status, stderr) = result
            .expect("wait_timeout should not itself error")
            .expect("must not time out — this is exactly the deadlock this fix closes");
        assert_eq!(stderr.len(), 300_000, "stderr must be drained in full");
        assert!(!status.success());
        assert!(
            elapsed < Duration::from_secs(10),
            "wait_draining_stderr must not block on a full pipe buffer (took {elapsed:?} \
             against a 20s timeout)"
        );
    }
}
