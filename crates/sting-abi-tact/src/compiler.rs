//! Drives the real `tact` compiler by subprocess — same shape of
//! integration as `sting_tolkc::TolkCompiler`, but no shared crate exists
//! for Tact the way `sting-tolkc` already existed for Tolk before this
//! branch started, so this lives directly in `sting-abi-tact`.
//!
//! Exact confirmed call form (`tests/fixtures/SOURCE.md`,
//! `vendor/tact/README.md`): `node <script>
//! <entrypoint>` — no `tact.config.json` needed, artifacts land *next to
//! the source file*, named `<source-file-name>_<ContractName>.<ext>`.

use std::env;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

use wait_timeout::ChildExt;

use crate::abi_json::ContractAbi;

/// The conventional relative path from a vendored `vendor/tact/` install
/// (`vendor/tact/README.md`) to the real `tact.js` entrypoint —
/// `npm install` inside `vendor/tact/` produces exactly this layout.
const VENDORED_SCRIPT_SUFFIX: &str = "tact/node_modules/@tact-lang/compiler/bin/tact.js";

/// Resolves the default `tact.js` path: `STING_TACT_JS_PATH` if set,
/// otherwise [`VENDORED_SCRIPT_SUFFIX`] next to the running executable —
/// mirrors `sting_tolkc::default_executable_path`'s identical convention
/// (that crate's `STING_TOLKC_PATH`/`resolve_executable_path`). Separated
/// from [`TactCompiler::from_default_path`] so the resolution logic itself
/// is testable without touching the real environment (env vars are
/// process-global mutable state; asserting against them directly in a
/// test that may run in parallel with others is inherently flaky) — same
/// reasoning `sting_tolkc::resolve_executable_path` already documents.
fn resolve_default_script_path(env_override: Option<&str>, exe_path: &Path) -> PathBuf {
    if let Some(path) = env_override {
        return PathBuf::from(path);
    }
    let dir = exe_path.parent().unwrap_or_else(|| Path::new("."));
    dir.join(VENDORED_SCRIPT_SUFFIX)
}

/// Where this workspace vendors the compiler.
///
/// Resolved from the crate's own directory at compile time, so it holds from a workspace checkout
/// and simply does not exist for a shipped binary.
#[must_use]
pub fn vendored_script_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../vendor")
        .join(VENDORED_SCRIPT_SUFFIX)
}

/// Default wall-clock budget for one `compile` call. Strict but generous: it
/// insures against the caller's own typo rather than against an attacker
/// (`docs/domains/service.md`). The stricter, OS-isolated budget a public
/// endpoint needs for untrusted callers is a separate and harder problem, not
/// solved here.
pub const DEFAULT_COMPILE_TIMEOUT: Duration = Duration::from_secs(30);

/// Everything one successful `tact` invocation produces that this crate
/// cares about.
#[derive(Debug, Clone)]
pub struct TactCompileOutput {
    /// The parsed `<entrypoint>_<ContractName>.abi` artifact.
    pub abi: ContractAbi,
    /// The raw bytes of the `<entrypoint>_<ContractName>.code.boc` artifact
    /// — real deployable bytecode, confirmed to land next to `.abi` on
    /// every real compile call this crate already makes
    /// (`tests/fixtures/SOURCE.md`'s documented full artifact list:
    /// `.abi`, `.code.boc`, `.fc`, `.fif`, `.md`, `.pkg`, `.ts`). Added
    /// because `RegistryController` needs real bytecode to byte-compare
    /// against on-chain code, which `.abi` alone cannot provide.
    pub code_boc: Vec<u8>,
}

/// Errors from resolving, running, or parsing the output of `tact`.
#[derive(Debug, thiserror::Error)]
pub enum TactcError {
    /// [`TactCompiler::from_default_path`] could not determine the
    /// running executable's own path (`std::env::current_exe` failed).
    #[error("failed to determine the running executable's own path: {0}")]
    CurrentExe(#[source] std::io::Error),
    /// Building the scratch copy of the project directory failed.
    #[error("failed to prepare a scratch copy of {project_dir}: {source}")]
    ScratchCopy {
        /// The project directory that failed to copy.
        project_dir: PathBuf,
        /// The underlying I/O error.
        #[source]
        source: std::io::Error,
    },
    /// Spawning `node <script>` failed (`node` not found, not executable,
    /// permission denied, ...).
    #[error("failed to spawn `node {script}`: {source}")]
    Spawn {
        /// The `tact.js` script path that failed to spawn via `node`.
        script: PathBuf,
        /// The underlying OS error.
        #[source]
        source: std::io::Error,
    },
    /// `tact` ran but exited non-zero — a real compile error.
    #[error("`tact` exited with a compile error:\n{stderr}")]
    CompileFailed {
        /// `tact`'s stderr, verbatim.
        stderr: String,
    },
    /// The compiler did not finish within [`TactCompiler`]'s configured
    /// timeout — killed rather than left running. A subprocess call without
    /// a timeout is a subprocess that can never be relied on to return.
    #[error("`node {script}` did not finish within {timeout:?}, killed")]
    Timeout {
        /// The `tact.js` script path that timed out.
        script: PathBuf,
        /// The configured timeout that was exceeded.
        timeout: Duration,
    },
    /// No `<entrypoint>_*.abi` artifact was found after a successful
    /// compile — would indicate `tact` changed its own output-file
    /// convention in a future version.
    #[error("no {entrypoint}_*.abi artifact found in {project_dir} after a successful compile")]
    MissingArtifact {
        /// The project directory searched.
        project_dir: PathBuf,
        /// The entrypoint file name searched for.
        entrypoint: PathBuf,
    },
    /// More than one `<entrypoint>_*.abi` artifact was found — the source
    /// file declares more than one contract. Not resolved automatically
    /// (which one would the caller mean?) — an honest error instead of
    /// silently picking one, see this crate's own top-level doc comment
    /// for the full reasoning.
    #[error(
        "{entrypoint} declares {count} contracts ({names}) — TactExtractor extracts exactly one \
         ContractSchema per call and cannot guess which one you meant; v1 does not support \
         multi-contract source files, see crates/sting-abi-tact's own doc comment"
    )]
    MultipleContracts {
        /// The entrypoint file that produced more than one contract.
        entrypoint: PathBuf,
        /// How many distinct `.abi` artifacts were found.
        count: usize,
        /// The contract names found, comma-joined, for the error message.
        names: String,
    },
    /// A produced artifact was not valid JSON, or not shaped the way this
    /// crate expects.
    #[error("failed to parse {path} as JSON: {source}")]
    ParseArtifact {
        /// The artifact path that failed to parse.
        path: PathBuf,
        /// The underlying parse error.
        #[source]
        source: serde_json::Error,
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
}

/// Drives one vendored `tact.js` script via `node`.
#[derive(Debug, Clone)]
pub struct TactCompiler {
    script_path: PathBuf,
    timeout: Duration,
}

impl TactCompiler {
    /// Builds a compiler handle for an explicit `tact.js` script path
    /// (e.g. `vendor/tact/node_modules/@tact-lang/compiler/bin/tact.js`),
    /// with [`DEFAULT_COMPILE_TIMEOUT`].
    #[must_use]
    pub fn new(script_path: PathBuf) -> Self {
        Self::with_timeout(script_path, DEFAULT_COMPILE_TIMEOUT)
    }

    /// Builds a compiler handle with an explicit timeout — e.g. a short
    /// one for tests that want to fail fast, or a stricter one for a
    /// future untrusted-input caller.
    #[must_use]
    pub fn with_timeout(script_path: PathBuf, timeout: Duration) -> Self {
        Self {
            script_path,
            timeout,
        }
    }

    /// Builds a compiler handle for the default `tact.js` location: the
    /// `STING_TACT_JS_PATH` env var if set, otherwise
    /// `vendor/tact/node_modules/@tact-lang/compiler/bin/tact.js` next to
    /// the running executable — mirrors
    /// `sting_tolkc::TolkCompiler::from_default_path`'s identical
    /// convention.
    ///
    /// # Errors
    /// Returns [`TactcError::CurrentExe`] if the running executable's own
    /// path cannot be determined.
    pub fn from_default_path() -> Result<Self, TactcError> {
        let env_override = env::var("STING_TACT_JS_PATH").ok();
        let exe = env::current_exe().map_err(TactcError::CurrentExe)?;
        Ok(Self::new(resolve_default_script_path(
            env_override.as_deref(),
            &exe,
        )))
    }

    /// The compiler this workspace can actually reach, or `None`.
    ///
    /// [`Self::from_default_path`] looks next to the running executable, which is right for a
    /// shipped release and wrong from a workspace checkout, where the path points into
    /// `target/debug/`. So the vendored directory is tried after it — the same pair, in the same
    /// order, that `sting_tolkc::TolkCompiler::vendored_or_default` already states, and for the
    /// same reason: every caller needs both, and each of them keeping its own copy of the pair is
    /// how two of them come to disagree about where a compiler is.
    ///
    /// `None` means no compiler is reachable. `vendor/tact/` is gitignored and its `node_modules`
    /// are an `npm install` away, so a fresh checkout has none and a caller is expected to say so
    /// rather than to fail as though the contract were at fault.
    #[must_use]
    pub fn vendored_or_default() -> Option<Self> {
        if let Ok(compiler) = Self::from_default_path()
            && compiler.script_path().is_file()
        {
            return Some(compiler);
        }
        let vendored = vendored_script_path();
        vendored.is_file().then(|| Self::new(vendored))
    }

    /// The `tact.js` script path this handle drives — lets a caller (e.g.
    /// `sting-abi-service`) resolve the default path and then rebuild with
    /// a different timeout, without duplicating [`Self::from_default_path`]'s
    /// own env-var/`current_exe` resolution logic.
    #[must_use]
    pub fn script_path(&self) -> &Path {
        &self.script_path
    }

    /// Compiles `entrypoint` (a path relative to `project_dir`, matching
    /// `sting_tolkc::TolkCompiler::compile`'s own convention). Copies
    /// `project_dir` into a scratch directory first and compiles there —
    /// `tact` writes its output artifacts next to the source file with no
    /// way to redirect them elsewhere without changing where imports
    /// resolve from (`vendor/tact/README.md`), so compiling in place
    /// against a real corpus directory would litter it with `.abi`/`.boc`/
    /// `.fc`/`.fif`/`.md`/`.pkg`/`.ts` build artifacts on every call.
    ///
    /// # Errors
    /// See [`TactcError`]'s variants.
    pub fn compile(
        &self,
        project_dir: &Path,
        entrypoint: &Path,
    ) -> Result<TactCompileOutput, TactcError> {
        let scratch = tempfile::tempdir().map_err(|source| TactcError::ScratchCopy {
            project_dir: project_dir.to_path_buf(),
            source,
        })?;
        copy_dir_recursive(project_dir, scratch.path()).map_err(|source| {
            TactcError::ScratchCopy {
                project_dir: project_dir.to_path_buf(),
                source,
            }
        })?;

        // Must be absolute before spawning: the child's cwd is the
        // scratch copy, not wherever this process itself was launched
        // from, so a *relative* script_path (e.g. a relative
        // STING_TACT_JS_PATH) would resolve against the wrong directory —
        // real bug found by review, reproduced with a relative
        // STING_TACT_JS_PATH: "Cannot find module '<scratch-dir>/vendor/
        // tact/.../tact.js'".
        //
        // `std::path::absolute`, not `canonicalize()`: canonicalize()
        // touches the filesystem and, on Windows, returns a `\\?\`-prefixed
        // verbatim path — Node's own module resolver mishandles that
        // prefix and fails with a confusing `EISDIR: ... lstat 'C:'`
        // instead of loading the script. `std::path::absolute` is purely
        // lexical (join with cwd + normalize `.`/`..`), no filesystem
        // access, no verbatim prefix — a missing script still surfaces
        // via the `Spawn` error below when `node` itself can't find it.
        let script_path =
            std::path::absolute(&self.script_path).map_err(|source| TactcError::Spawn {
                script: self.script_path.clone(),
                source,
            })?;

        let mut child = Command::new("node")
            .current_dir(scratch.path())
            .arg(&script_path)
            .arg(entrypoint)
            // Never read below — `Stdio::null()`, not `Stdio::piped()`, so
            // a chatty child can't fill an unread pipe buffer and block on
            // its own `write()`. stderr *is* read, so it stays piped and is
            // drained concurrently by `wait_draining_stderr` below instead.
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|source| TactcError::Spawn {
                script: self.script_path.clone(),
                source,
            })?;

        // No async runtime here on purpose (`docs/domains/abi-ir.md`
        // §7: only `sting-abi-service` gets one) — `wait_timeout` is a
        // synchronous poll-with-deadline, the same shape of primitive a
        // blocking CLI/pipeline call is allowed to use.
        let Some((status, stderr)) =
            wait_draining_stderr(&mut child, self.timeout).map_err(|source| TactcError::Spawn {
                script: self.script_path.clone(),
                source,
            })?
        else {
            // Best-effort: a kill/wait failure here just means the process
            // leaks until the OS reaps it on scratch-dir cleanup — not
            // worth a second error variant for.
            return Err(TactcError::Timeout {
                script: self.script_path.clone(),
                timeout: self.timeout,
            });
        };

        if !status.success() {
            return Err(TactcError::CompileFailed { stderr });
        }

        // Artifacts land next to the *source file*, not at the invocation
        // cwd (confirmed, `tests/fixtures/SOURCE.md`) — for an entrypoint
        // with a directory component (`contracts/main.tact`, the standard
        // Blueprint/Tact project layout, not an edge case) that means
        // `scratch.path()/contracts/`, not `scratch.path()` itself. Real
        // bug found by review: searching the scratch root unconditionally
        // silently failed `MissingArtifact` on every non-flat project,
        // even though compilation itself had genuinely succeeded.
        let search_dir = match entrypoint.parent() {
            Some(parent) if parent.as_os_str().is_empty() => scratch.path().to_path_buf(),
            Some(parent) => scratch.path().join(parent),
            None => scratch.path().to_path_buf(),
        };
        let abi_path = find_single_artifact(&search_dir, entrypoint, ".abi")?;
        let text =
            std::fs::read_to_string(&abi_path).map_err(|source| TactcError::ReadArtifact {
                path: abi_path.clone(),
                source,
            })?;
        let abi = serde_json::from_str(&text).map_err(|source| TactcError::ParseArtifact {
            path: abi_path,
            source,
        })?;

        let boc_path = find_single_artifact(&search_dir, entrypoint, ".code.boc")?;
        let code_boc = std::fs::read(&boc_path).map_err(|source| TactcError::ReadArtifact {
            path: boc_path,
            source,
        })?;

        Ok(TactCompileOutput { abi, code_boc })
    }
}

/// Runs `child` to completion against `timeout`, draining its stderr pipe
/// on a background thread *while* waiting, not only afterward. A pipe is
/// bounded (~64KB on Linux/Windows) — a child that writes more than that to
/// stderr before exiting would otherwise block on its own `write()`
/// forever, and `wait_timeout` would spend its entire budget waiting on a
/// process that's alive but stuck, not actually hung. Real, exploitable
/// finding on a public HTTP endpoint that compiles untrusted source — see
/// Callers must not pipe stdout
/// (`Stdio::null()`) since nothing here drains it.
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

fn copy_dir_recursive(src: &Path, dst: &Path) -> std::io::Result<()> {
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let file_type = entry.file_type()?;
        let dst_path = dst.join(entry.file_name());
        if file_type.is_dir() {
            std::fs::create_dir_all(&dst_path)?;
            copy_dir_recursive(&entry.path(), &dst_path)?;
        } else if file_type.is_file() {
            std::fs::copy(entry.path(), &dst_path)?;
        }
    }
    Ok(())
}

/// Finds the single `<entrypoint>_<ContractName><suffix>` artifact in
/// `project_dir` — generalized from the original `.abi`-only search
/// — a second caller, `.code.boc`, arrived later, see
/// [`TactCompileOutput::code_boc`]'s doc comment. `suffix` is matched by
/// plain string suffix, not [`Path::extension`] — `.code.boc` is a
/// compound extension `Path::extension()` alone can't express (it would
/// only see `"boc"`).
fn find_single_artifact(
    project_dir: &Path,
    entrypoint: &Path,
    suffix: &str,
) -> Result<PathBuf, TactcError> {
    let Some(entrypoint_name) = entrypoint.file_name().and_then(|n| n.to_str()) else {
        return Err(TactcError::MissingArtifact {
            project_dir: project_dir.to_path_buf(),
            entrypoint: entrypoint.to_path_buf(),
        });
    };
    let prefix = format!("{entrypoint_name}_");

    let mut matches = Vec::new();
    if let Ok(entries) = std::fs::read_dir(project_dir) {
        for entry in entries.flatten() {
            let name = entry.file_name();
            let Some(name) = name.to_str() else { continue };
            if name.starts_with(&prefix) && name.ends_with(suffix) {
                matches.push(entry.path());
            }
        }
    }
    matches.sort();

    match matches.as_slice() {
        [] => Err(TactcError::MissingArtifact {
            project_dir: project_dir.to_path_buf(),
            entrypoint: entrypoint.to_path_buf(),
        }),
        [only] => Ok(only.clone()),
        many => {
            let names = many
                .iter()
                .filter_map(|p| p.file_name())
                .map(|n| n.to_string_lossy().into_owned())
                .collect::<Vec<_>>()
                .join(", ");
            Err(TactcError::MultipleContracts {
                entrypoint: entrypoint.to_path_buf(),
                count: many.len(),
                names,
            })
        }
    }
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "docs/ARCHITECTURE.md §3.2 scopes the no-unwrap/expect/panic policy to library code outside tests"
)]
mod tests {
    use std::path::{Path, PathBuf};
    use std::time::{Duration, Instant};

    use super::{TactCompiler, TactcError, resolve_default_script_path};

    #[test]
    fn resolve_default_script_path_prefers_the_env_override() {
        let resolved =
            resolve_default_script_path(Some("/custom/tact.js"), Path::new("/bin/sting"));
        assert_eq!(resolved, PathBuf::from("/custom/tact.js"));
    }

    #[test]
    fn resolve_default_script_path_falls_back_to_next_to_the_running_executable() {
        let resolved = resolve_default_script_path(None, Path::new("/usr/local/bin/sting"));
        assert_eq!(
            resolved,
            PathBuf::from("/usr/local/bin/tact/node_modules/@tact-lang/compiler/bin/tact.js")
        );
    }

    /// Empirically confirms the timeout actually fires and kills the
    /// process, not just that the code compiles — a fake "compiler" script
    /// that ignores its arguments and sleeps far longer than the
    /// configured timeout, run via the real `node` this crate always
    /// requires. Requires `node` in `PATH`, same precondition as every
    /// other test in this crate.
    #[test]
    fn compile_times_out_and_kills_a_slow_process() {
        let scratch = tempfile::tempdir().expect("scratch dir");
        let slow_script = scratch.path().join("slow.js");
        std::fs::write(&slow_script, "setTimeout(() => {}, 5000);\n").expect("writable");
        let project_dir = scratch.path().join("project");
        std::fs::create_dir_all(&project_dir).expect("writable");

        let compiler = TactCompiler::with_timeout(slow_script, Duration::from_millis(300));
        let started = Instant::now();
        let result = compiler.compile(&project_dir, std::path::Path::new("ignored.tact"));
        let elapsed = started.elapsed();

        assert!(
            matches!(result, Err(TactcError::Timeout { .. })),
            "expected Timeout, got {result:?}"
        );
        assert!(
            elapsed < Duration::from_secs(2),
            "compile() must return promptly once the timeout fires, not wait for the full 5s sleep \
             (took {elapsed:?}) — a slow return here would mean the process wasn't actually killed"
        );
    }

    /// Regression test for the pipe-buffer deadlock found in
    /// : a "compiler" that writes far more than
    /// one OS pipe buffer (~64KB) to stderr before failing must still
    /// return promptly, well under the configured timeout — with the old
    /// code (stderr read only *after* `wait_timeout` returned), the child
    /// would block on its own `write()` and this call would eat the entire
    /// timeout budget instead of returning almost immediately.
    #[test]
    fn compile_does_not_deadlock_on_a_chatty_failing_process() {
        let scratch = tempfile::tempdir().expect("scratch dir");
        let noisy_script = scratch.path().join("noisy.js");
        std::fs::write(
            &noisy_script,
            "process.stderr.write('x'.repeat(300000));\nprocess.exit(1);\n",
        )
        .expect("writable");
        let project_dir = scratch.path().join("project");
        std::fs::create_dir_all(&project_dir).expect("writable");

        let compiler = TactCompiler::with_timeout(noisy_script, Duration::from_secs(20));
        let started = Instant::now();
        let result = compiler.compile(&project_dir, std::path::Path::new("ignored.tact"));
        let elapsed = started.elapsed();

        match result {
            Err(TactcError::CompileFailed { stderr }) => {
                assert_eq!(
                    stderr.len(),
                    300_000,
                    "stderr must be drained in full, not truncated or dropped"
                );
            }
            other => panic!("expected CompileFailed with the full stderr, got {other:?}"),
        }
        assert!(
            elapsed < Duration::from_secs(10),
            "compile() must not deadlock waiting for a full pipe buffer to be read (took \
             {elapsed:?} against a 20s timeout) — this is exactly the DoS vector the fix closes"
        );
    }
}
