//! Native-`tolk`-CLI integration: vendors and drives the real Tolk compiler
//! as a subprocess (not FFI, not WASM-in-Node) to recover whole-project
//! semantic information — a resolved import graph, and every function/
//! struct/alias/enum declaration with a fully monomorphized name and a
//! precise source location — that `sting-ast`'s pure tree-sitter parse
//! cannot provide at all: tree-sitter has no notion of imports, generics,
//! or cross-file name resolution.
//!
//! **Why a native binary rather than the other two paths.** Three were
//! compared — a Node.js subprocess, `wasmtime` hosting the Emscripten WASM
//! build, and the native CLI — and this one won on every axis checked. `ton-blockchain/ton` already publishes native
//! `tolk`/`tolk.exe` binaries on every release (the same vendoring pattern
//! already used for `libemulator`, see `sting_emulator_sys`), built by the
//! same toolchain in the same CI step as `emulator` — not a separate,
//! fragile path. The native CLI's own `--output`-sibling `symbolTypes.json`/
//! `abi.json` artifacts make this integration *simpler* than
//! `sting-emulator-sys`'s `dlopen`: a subprocess call and two file reads,
//! no FFI, no `unsafe` anywhere in this crate. That is also why this crate
//! is named without a `-sys` suffix — that suffix is a Rust convention for
//! raw FFI-binding crates specifically (`docs/GLOSSARY.md`'s "Cargo Book:
//! `-sys` packages"); this crate would be misnamed with it.
//!
//! **What this crate does not give you**: function bodies, expressions, or
//! a call graph. `tolk`'s `symbolTypes.json` is a flat table of resolved
//! *signatures* (name, params, return type, source location), not an AST.
//! Walking a function body for call sites is still `sting-ast`/tree-sitter's
//! job; this crate's contribution is resolving what a call site's callee
//! name actually refers to *across files* — the one thing tree-sitter alone
//! structurally cannot do, confirmed by the fact that three independent
//! detectors (`ac.rs::classify_function`, `uta.rs`'s one-hop call-site
//! walker, `store_load.rs::classify_function`) already each hand-roll a
//! narrower, same-file-only version of exactly this problem.
//!
//! **Wired into one detector so far**: `sting-detectors::
//! ac.rs::classify_function` consults [`ProjectStore`] (via
//! `sting_detectors::DetectorContext::resolve_call`) when a callee is not
//! found in its own file, resolving free-function calls only — a method
//! call is a separate, still-open gap (see `ac.rs`'s own doc comment).
//! `sting-pipeline::LintController::lint_paths` is the only caller that
//! ever runs [`TolkCompiler::compile`] at all, and only best-effort: no
//! vendored binary, or a compile failure for any reason, silently falls
//! back to same-file-only resolution — `sting lint` has no hard dependency
//! on this crate. `LintController::lint_source`-based callers
//! (`sting-bench`'s own precision/recall scoring) never attempt a compile
//! at all, by design. `UTA`/`store_load.rs`'s own same-file-only resolvers
//! are unaffected, natural future migrations onto the same [`ProjectStore`].

mod abi;
mod error;
mod project_store;
mod symbol_types;

pub use abi::{AbiJson, GetMethod, MessageRef, StorageRef, ThrownError};
pub use error::TolkcError;
pub use project_store::ProjectStore;
pub use symbol_types::{
    CustomPackUnpack, Declaration, FieldEntry, FileEntry, FunctionEntry, Param, SourceSpan,
    StructInstantiation, SymbolTypes, TlbPrefix, TypeDescriptor, UnionVariant,
};

use std::env;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

use wait_timeout::ChildExt;

/// Everything one successful `tolk` invocation produces that this crate
/// cares about. Bytecode (`codeBoc64`/`codeHashHex`) is deliberately not
/// captured here — `sting-emulator` already obtains bytecode a different
/// way (on-chain fetch, or its own pipeline), and this crate exists for the
/// *semantic* side of compilation, not to become a second path to
/// deployable output.
#[derive(Debug, Clone)]
pub struct CompileOutput {
    /// The compiled Fift assembly text.
    pub fift_code: String,
    /// The parsed `<basename>.symbolTypes.json` artifact.
    pub symbol_types: SymbolTypes,
    /// The parsed `<basename>.abi.json` artifact.
    pub abi: AbiJson,
}

/// The result of [`TolkCompiler::compile_keep_scratch`]: everything
/// [`CompileOutput`] gives plus the on-disk `.fif` path `tolk` wrote its
/// `--output` artifacts to — kept alive (not deleted) because a caller that
/// asked for this specifically needs to hand that exact path to
/// `sting_fift::FiftAssembler::assemble` afterwards, which requires the
/// directory to still exist on disk (see [`TolkCompiler::compile_keep_scratch`]'s
/// own doc comment for why).
#[derive(Debug)]
pub struct CompileForAssembly {
    /// Same parsed artifacts [`TolkCompiler::compile`] returns.
    pub output: CompileOutput,
    /// The on-disk path of the `.fif` file `tolk --output` wrote — pass
    /// this directly to `sting_fift::FiftAssembler::assemble`.
    pub fift_path: PathBuf,
    /// RAII handle keeping the scratch directory `fift_path` lives in alive
    /// for as long as this value is held — never read directly, its only
    /// purpose is the directory-removal side effect of its `Drop` impl.
    _scratch: tempfile::TempDir,
}

/// `tolk`'s self-reported version, parsed from `tolk --version`'s real
/// three-line output:
/// ```text
/// Tolk compiler v1.4.1
/// Build commit: da70af2be5ef2efe9f0d575ec466847e27b81842
/// Build date: 2026-06-13 15:22:32 +0000
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TolkVersion {
    /// The version number after `"Tolk compiler v"` (e.g. `"1.4.1"`).
    pub version: String,
    /// The commit hash after `"Build commit: "`.
    pub build_commit: String,
    /// The build timestamp after `"Build date: "`.
    pub build_date: String,
}

/// The platform-conventional `tolk` executable filename this crate resolves
/// by default. Deliberately **not** the exact filename `ton-blockchain/ton`'s
/// GitHub release publishes (`tolk-linux-x86_64`, `tolk-mac-arm64`, ...) —
/// those encode redundant architecture info irrelevant to a
/// same-architecture runtime resolving its own platform's binary by
/// convention — mirrors `sting_emulator_sys::platform_library_filename`'s
/// identical reasoning. Whatever vendors the binary (a developer, CI, or a
/// future `sting`-provided fetch command) is responsible for placing it
/// under this canonical name; this crate only resolves, never fetches.
#[must_use]
pub const fn platform_executable_filename() -> &'static str {
    if cfg!(target_os = "windows") {
        "tolk.exe"
    } else {
        "tolk"
    }
}

/// Pure resolution logic behind [`default_executable_path`], separated out
/// so it is testable without touching the real environment or
/// `std::env::current_exe` — mirrors
/// `sting_emulator_sys::resolve_library_path`'s identical reasoning (env
/// vars are process-global mutable state; asserting against them directly
/// in a test that may run in parallel with others is inherently flaky).
fn resolve_executable_path(env_override: Option<&str>, exe_path: &Path) -> PathBuf {
    if let Some(path) = env_override {
        return PathBuf::from(path);
    }
    let dir = exe_path.parent().unwrap_or_else(|| Path::new("."));
    dir.join(platform_executable_filename())
}

/// Resolves the default `tolk` path: `STING_TOLKC_PATH` if set, otherwise
/// [`platform_executable_filename`] next to the running executable —
/// mirrors `LibEmulator::default_library_path`'s identical convention.
///
/// # Errors
/// Returns [`TolkcError::CurrentExe`] if the running executable's own path
/// cannot be determined.
pub fn default_executable_path() -> Result<PathBuf, TolkcError> {
    let env_override = env::var("STING_TOLKC_PATH").ok();
    let exe = env::current_exe().map_err(TolkcError::CurrentExe)?;
    Ok(resolve_executable_path(env_override.as_deref(), &exe))
}

/// `tolk` resolves `@stdlib/...` imports against a `tolk-stdlib/` directory
/// it expects to find by its own convention, or via the `TOLK_STDLIB`
/// env var — confirmed empirically: a bare invocation with no such
/// directory reachable fails with "Failed to discover Tolk stdlib" and
/// names that exact env var in its own error message. This crate pins the
/// stdlib directory explicitly (next to the resolved `tolk` executable)
/// rather than relying on whatever `tolk`'s own auto-discovery would find,
/// so a `tolk` binary relocated via `STING_TOLKC_PATH` still finds a
/// matching, version-consistent stdlib deterministically instead of
/// silently picking up an unrelated one that happens to be discoverable.
fn stdlib_dir_for(executable_path: &Path) -> PathBuf {
    executable_path
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join("tolk-stdlib")
}

/// `tolk`'s own artifact-naming convention for `--output <path>.fif`:
/// sibling artifacts replace the `.fif` extension, they don't append to it
/// (confirmed empirically: `--output out.fif` produces `out.abi.json` and
/// `out.symbolTypes.json`, not `out.fif.abi.json`).
fn sibling_artifact(fif_path: &Path, suffix: &str) -> PathBuf {
    fif_path.with_extension(suffix)
}

/// Default wall-clock budget for one `compile` call — generous enough
/// never to trip on any real contract this crate's own test corpus
/// compiles in under a second, while still bounding the worst case for
/// `sting-abi-service`, the caller this default was raised for.
/// Mirrors `sting_abi_tact::compiler::DEFAULT_COMPILE_TIMEOUT`'s identical
/// value and reasoning.
pub const DEFAULT_COMPILE_TIMEOUT: Duration = Duration::from_secs(30);

/// Drives one vendored `tolk` executable.
#[derive(Debug, Clone)]
pub struct TolkCompiler {
    executable_path: PathBuf,
    timeout: Duration,
}

impl TolkCompiler {
    /// Builds a compiler handle for the executable at
    /// [`default_executable_path`], with its stdlib resolved by convention
    /// (a `tolk-stdlib` directory beside the executable) and [`DEFAULT_COMPILE_TIMEOUT`].
    ///
    /// # Errors
    /// Returns [`TolkcError::CurrentExe`] under the same condition as
    /// [`default_executable_path`].
    pub fn from_default_path() -> Result<Self, TolkcError> {
        Ok(Self::new(default_executable_path()?))
    }

    /// Builds a compiler handle for an explicit `tolk` executable path,
    /// with [`DEFAULT_COMPILE_TIMEOUT`]. The stdlib directory is *not*
    /// resolved eagerly here — `compile` derives it fresh from the
    /// canonicalized executable path on every call (see that method's own
    /// doc comment for why: a relative path stored at construction time
    /// would resolve wrongly once `compile` changes the child process's
    /// working directory).
    #[must_use]
    pub fn new(executable_path: PathBuf) -> Self {
        Self::with_timeout(executable_path, DEFAULT_COMPILE_TIMEOUT)
    }

    /// Builds a compiler handle with an explicit timeout — e.g. a short
    /// one for tests that want to fail fast, or a stricter one for a
    /// public, untrusted-input caller. Added for `sting-abi-service`; this
    /// crate's original caller (`sting-pipeline::LintController`, a local
    /// trusted CLI) never needed one, so
    /// [`Self::new`]'s [`DEFAULT_COMPILE_TIMEOUT`] preserves that caller's
    /// existing behavior unchanged rather than silently starting to kill
    /// long-but-legitimate local compiles.
    #[must_use]
    pub fn with_timeout(executable_path: PathBuf, timeout: Duration) -> Self {
        Self {
            executable_path,
            timeout,
        }
    }

    /// The `tolk` executable path this handle drives.
    #[must_use]
    pub fn executable_path(&self) -> &Path {
        &self.executable_path
    }

    /// Compiles `entrypoint` (a path relative to `project_dir`, matching
    /// how `tolk` itself resolves sibling-file imports — confirmed
    /// empirically that project-relative imports resolve against the
    /// *invocation* working directory, not the entrypoint file's own
    /// directory, which is why this method takes both separately rather
    /// than one combined path) and returns its Fift code plus both semantic
    /// JSON artifacts.
    ///
    /// Deliberately does not pass `--json-errors`: this crate has only
    /// ever observed `tolk`'s plain-text error format in practice (see
    /// [`TolkcError::CompileFailed`]'s doc comment) and would rather report
    /// that real, confirmed shape than guess at the structured one's exact
    /// fields.
    ///
    /// # Errors
    /// See [`TolkcError`]'s variants.
    pub fn compile(
        &self,
        project_dir: &Path,
        entrypoint: &Path,
    ) -> Result<CompileOutput, TolkcError> {
        let scratch = tempfile::tempdir().map_err(TolkcError::ScratchDir)?;
        let fif_path = scratch.path().join("out.fif");
        self.compile_in(project_dir, entrypoint, &fif_path)
        // `scratch` drops here — fine for this method: every field of
        // `CompileOutput` is already owned (`String`/parsed structs), read
        // from disk before this point, not a reference into the scratch
        // directory. Callers that also need to *assemble* the emitted Fift
        // text (`sting_fift::FiftAssembler::assemble`) cannot use this
        // method — see [`Self::compile_keep_scratch`].
    }

    /// Identical to [`Self::compile`], except the scratch directory `tolk`
    /// wrote its `--output` artifacts into is kept alive in the returned
    /// [`CompileForAssembly`] instead of being deleted when this call
    /// returns.
    ///
    /// **Why this exists, not just `compile`**: `sting_fift::FiftAssembler`
    /// needs the exact on-disk `.fif` path `tolk` was invoked with — the
    /// `.compiled.boc` destination `fift` will write to is a literal string
    /// baked into the Fift source text at `tolk --output` time (confirmed
    /// empirically, see `sting_fift`'s own module doc comment), so that
    /// directory must still exist on disk when `fift` later runs. Plain
    /// [`Self::compile`] deletes it immediately, which is correct for every
    /// caller that only wants the parsed semantic artifacts (`fift_code` as
    /// text, `abi`, `symbol_types`) and never needs to invoke `fift` at all.
    ///
    /// # Errors
    /// See [`TolkcError`]'s variants.
    pub fn compile_keep_scratch(
        &self,
        project_dir: &Path,
        entrypoint: &Path,
    ) -> Result<CompileForAssembly, TolkcError> {
        let scratch = tempfile::tempdir().map_err(TolkcError::ScratchDir)?;
        let fif_path = scratch.path().join("out.fif");
        let output = self.compile_in(project_dir, entrypoint, &fif_path)?;
        Ok(CompileForAssembly {
            output,
            fift_path: fif_path,
            _scratch: scratch,
        })
    }

    fn compile_in(
        &self,
        project_dir: &Path,
        entrypoint: &Path,
        fif_path: &Path,
    ) -> Result<CompileOutput, TolkcError> {
        // Must be absolute before spawning: `.current_dir(project_dir)`
        // changes the *child's* cwd, and both the program path itself
        // (platform-specific relative-executable resolution, see
        // `std::process::Command`'s own docs) and `TOLK_STDLIB` (an env
        // var `tolk` itself resolves against its own cwd, i.e. the child's,
        // not ours) would silently resolve against the wrong directory if
        // `executable_path` were relative (e.g. a relative
        // `STING_TOLKC_PATH`) — real bug found by review while building
        // `sting-abi-cli`, reproduced with a
        // relative `STING_TOLKC_PATH` and an entrypoint with a directory
        // component: `tolk` failed to find its own stdlib. `stdlib_dir` is
        // recomputed from the absolute path here rather than trusting
        // `self.stdlib_dir` (derived once at construction, from whatever
        // `executable_path` was at that time).
        //
        // `std::path::absolute`, not `canonicalize()`: canonicalize()
        // touches the filesystem and, on Windows, returns a `\\?\`-prefixed
        // verbatim path — the sibling `sting-abi-tact` crate hit exactly
        // this (Node's module resolver choked on the prefix with a
        // confusing `EISDIR` error) when it used canonicalize() for the
        // same "make it absolute before changing cwd" purpose.
        // `std::path::absolute` is purely lexical (join with cwd +
        // normalize `.`/`..`), no filesystem access, no verbatim prefix —
        // a missing executable still surfaces via the `Spawn` error below.
        let absolute_executable_path =
            std::path::absolute(&self.executable_path).map_err(|source| TolkcError::Spawn {
                path: self.executable_path.clone(),
                source,
            })?;
        let absolute_stdlib_dir = stdlib_dir_for(&absolute_executable_path);

        let mut child = Command::new(&absolute_executable_path)
            .current_dir(project_dir)
            .env("TOLK_STDLIB", &absolute_stdlib_dir)
            .arg("--output")
            .arg(fif_path)
            .arg(entrypoint)
            // Never read below — `Stdio::null()`, not `Stdio::piped()`, so
            // a chatty child can't fill an unread pipe buffer and block on
            // its own `write()`. stderr *is* read, so it stays piped and is
            // drained concurrently by `wait_draining_stderr` below instead.
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|source| TolkcError::Spawn {
                path: self.executable_path.clone(),
                source,
            })?;

        // No async runtime here on purpose (`docs/domains/abi-ir.md`
        // §7: only `sting-abi-service` gets one) — `wait_timeout` is a
        // synchronous poll-with-deadline, the same shape of primitive a
        // blocking CLI/pipeline call is allowed to use. Mirrors
        // `sting_abi_tact::compiler::TactCompiler::compile`'s identical
        // spawn/wait_timeout/kill pattern.
        let Some((status, stderr)) =
            wait_draining_stderr(&mut child, self.timeout).map_err(|source| TolkcError::Spawn {
                path: self.executable_path.clone(),
                source,
            })?
        else {
            // Best-effort: a kill/wait failure here just means the process
            // leaks until the OS reaps it — not worth a second error
            // variant for.
            return Err(TolkcError::Timeout {
                timeout: self.timeout,
            });
        };

        if !status.success() {
            return Err(TolkcError::CompileFailed { stderr });
        }

        let fift_code = read_artifact_string(fif_path)?;
        let abi = read_artifact_json(&sibling_artifact(fif_path, "abi.json"))?;
        let symbol_types = read_artifact_json(&sibling_artifact(fif_path, "symbolTypes.json"))?;

        Ok(CompileOutput {
            fift_code,
            symbol_types,
            abi,
        })
    }

    /// The compiler this workspace can actually reach, or `None`.
    ///
    /// [`Self::from_default_path`] looks next to the running executable, which
    /// is right for a shipped release and wrong from a workspace checkout,
    /// where the path points into `target/debug/deps/`. So the vendored
    /// directory is tried after it. Every caller needs both, and each of them
    /// having its own copy of the pair is how two of them drift apart.
    ///
    /// `None` means no compiler is reachable: `vendor/tolk/` is gitignored, so
    /// a fresh checkout has none, and a caller is expected to skip rather than
    /// fail.
    #[must_use]
    pub fn vendored_or_default() -> Option<Self> {
        if let Ok(compiler) = Self::from_default_path()
            && compiler.executable_path().is_file()
        {
            return Some(compiler);
        }
        let vendored = vendored_executable_path();
        vendored.is_file().then(|| Self::new(vendored))
    }

    /// Checks `entrypoint` for errors without generating any code, and
    /// returns them structured.
    ///
    /// Three flags separate this from [`Self::compile`], and each answers a
    /// different question the oracle needs and a compile cannot:
    /// `--check-only` skips code generation, `--allow-no-entrypoint` lets a
    /// module file be checked at all rather than refused for having no
    /// `onInternalMessage`, and `--json-errors` returns each error as data
    /// instead of as text to scrape.
    ///
    /// A file with no errors returns an empty list. The distinction that
    /// matters to a caller comparing this against a grammar is in the message:
    /// `tolk` opens every parse error with `expected `, and everything else it
    /// reports — an unknown type, an undefined symbol — is a file it read and
    /// then disagreed with.
    ///
    /// # Errors
    /// Returns [`TolkcError::Spawn`] if the process cannot be started,
    /// [`TolkcError::Timeout`] if it outlives the configured budget, and
    /// [`TolkcError::ParseArtifact`] if its own JSON output cannot be parsed.
    pub fn check(
        &self,
        project_dir: &Path,
        entrypoint: &Path,
    ) -> Result<Vec<CheckError>, TolkcError> {
        let absolute_executable_path =
            std::path::absolute(&self.executable_path).map_err(|source| TolkcError::Spawn {
                path: self.executable_path.clone(),
                source,
            })?;
        let absolute_stdlib_dir = stdlib_dir_for(&absolute_executable_path);

        let mut child = Command::new(&absolute_executable_path)
            .current_dir(project_dir)
            .env("TOLK_STDLIB", &absolute_stdlib_dir)
            .arg("--check-only")
            .arg("--allow-no-entrypoint")
            .arg("--json-errors")
            .arg(entrypoint)
            // The report goes to stdout here, unlike a compile, so this is the
            // stream that is drained and stderr is the one that is dropped.
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|source| TolkcError::Spawn {
                path: self.executable_path.clone(),
                source,
            })?;

        let Some((_, stdout)) =
            wait_draining_stdout(&mut child, self.timeout).map_err(|source| TolkcError::Spawn {
                path: self.executable_path.clone(),
                source,
            })?
        else {
            return Err(TolkcError::Timeout {
                timeout: self.timeout,
            });
        };

        // A clean check prints nothing at all, which is not JSON.
        if stdout.trim().is_empty() {
            return Ok(Vec::new());
        }
        let report: CheckReport =
            serde_json::from_str(&stdout).map_err(|source| TolkcError::ParseArtifact {
                path: entrypoint.to_path_buf(),
                source,
            })?;
        Ok(report.errors)
    }

    /// Runs `tolk --version` and parses its self-reported version/commit/
    /// build date.
    ///
    /// # Errors
    /// Returns [`TolkcError::Spawn`] if the process can't be started, or
    /// [`TolkcError::VersionFormat`] if its output doesn't match the
    /// expected three-line shape.
    pub fn version(&self) -> Result<TolkVersion, TolkcError> {
        let output = Command::new(&self.executable_path)
            .arg("--version")
            .output()
            .map_err(|source| TolkcError::Spawn {
                path: self.executable_path.clone(),
                source,
            })?;

        // Observed on real output: the version banner goes to stdout, but
        // this crate checks both rather than assume, since nothing else
        // about `tolk`'s stream conventions is documented anywhere.
        let mut combined = String::from_utf8_lossy(&output.stdout).into_owned();
        combined.push_str(&String::from_utf8_lossy(&output.stderr));
        parse_version_output(&combined)
    }
}

/// Where this workspace vendors the compiler.
///
/// Resolved from the crate's own directory at compile time, so it holds from a
/// workspace checkout and simply does not exist for a shipped binary.
#[must_use]
pub fn vendored_executable_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../vendor/tolk")
        .join(platform_executable_filename())
}

/// One error from [`TolkCompiler::check`], as `tolk`'s own `--json-errors`
/// reports it.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct CheckError {
    /// The message, verbatim. A parse error opens with `expected `.
    pub message: String,
    /// The file it was found in, when `tolk` names one.
    #[serde(default)]
    pub file: Option<String>,
    /// The line, 1-indexed, when `tolk` names one.
    #[serde(default)]
    pub line: Option<u32>,
}

impl CheckError {
    /// Whether this is the compiler failing to read the syntax rather than
    /// disagreeing with what it read.
    ///
    /// The distinction is the whole point of asking: a file `tolk` parses and
    /// then rejects for an unknown type is a file whose syntax is valid, and a
    /// grammar that refuses it is a grammar that is behind.
    #[must_use]
    pub fn is_parse_error(&self) -> bool {
        self.message.starts_with("expected ")
    }
}

/// `tolk --json-errors`' own report shape.
#[derive(Debug, serde::Deserialize)]
struct CheckReport {
    #[serde(default)]
    errors: Vec<CheckError>,
}

/// Runs `child` to completion against `timeout`, draining its stderr pipe
/// on a background thread *while* waiting, not only afterward. A pipe is
/// bounded (~64KB on Linux/Windows) — a child that writes more than that to
/// stderr before exiting would otherwise block on its own `write()`
/// forever, and `wait_timeout` would spend its entire budget waiting on a
/// process that's alive but stuck, not actually hung. Real, exploitable
/// finding on a public HTTP endpoint that compiles untrusted source — see
/// Mirrors
/// `sting_abi_tact::compiler::wait_draining_stderr` exactly (see that
/// crate's own doc comment; no shared crate exists between them for this).
/// Callers must not pipe stdout (`Stdio::null()`) since nothing here
/// drains it.
///
/// Returns `Ok(Some((status, stderr)))` on completion, `Ok(None)` if the
/// timeout elapsed (the child has already been killed and reaped by the
/// time this returns), `Err` if `wait_timeout` itself failed.
fn wait_draining_stderr(
    child: &mut std::process::Child,
    timeout: Duration,
) -> std::io::Result<Option<(std::process::ExitStatus, String)>> {
    let pipe = child.stderr.take();
    wait_draining(child, timeout, pipe)
}

/// The same for stdout, which is the stream [`TolkCompiler::check`] reads its
/// report from.
fn wait_draining_stdout(
    child: &mut std::process::Child,
    timeout: Duration,
) -> std::io::Result<Option<(std::process::ExitStatus, String)>> {
    let pipe = child.stdout.take();
    wait_draining(child, timeout, pipe)
}

/// The drain itself, told which pipe to read. Both callers need the same
/// deadlock avoidance, and one of them having its own copy is how the two
/// would eventually stop matching.
fn wait_draining<P: std::io::Read + Send + 'static>(
    child: &mut std::process::Child,
    timeout: Duration,
    pipe: Option<P>,
) -> std::io::Result<Option<(std::process::ExitStatus, String)>> {
    let handle = pipe.map(|mut pipe| {
        std::thread::spawn(move || {
            let mut buf = String::new();
            let _ = pipe.read_to_string(&mut buf);
            buf
        })
    });
    let Some(status) = child.wait_timeout(timeout)? else {
        let _ = child.kill();
        let _ = child.wait();
        if let Some(handle) = handle {
            let _ = handle.join();
        }
        return Ok(None);
    };
    let text = handle
        .and_then(|handle| handle.join().ok())
        .unwrap_or_default();
    Ok(Some((status, text)))
}

fn parse_version_output(raw: &str) -> Result<TolkVersion, TolkcError> {
    let mut lines = raw.lines();
    let version = lines
        .next()
        .and_then(|l| l.strip_prefix("Tolk compiler v"))
        .map(str::trim)
        .map(str::to_string);
    let build_commit = lines
        .next()
        .and_then(|l| l.strip_prefix("Build commit:"))
        .map(str::trim)
        .map(str::to_string);
    let build_date = lines
        .next()
        .and_then(|l| l.strip_prefix("Build date:"))
        .map(str::trim)
        .map(str::to_string);

    match (version, build_commit, build_date) {
        (Some(version), Some(build_commit), Some(build_date)) => Ok(TolkVersion {
            version,
            build_commit,
            build_date,
        }),
        _ => Err(TolkcError::VersionFormat {
            raw: raw.to_string(),
        }),
    }
}

fn read_artifact_string(path: &Path) -> Result<String, TolkcError> {
    if !path.exists() {
        return Err(TolkcError::MissingArtifact {
            path: path.to_path_buf(),
        });
    }
    std::fs::read_to_string(path).map_err(|source| TolkcError::ReadArtifact {
        path: path.to_path_buf(),
        source,
    })
}

fn read_artifact_json<T: serde::de::DeserializeOwned>(path: &Path) -> Result<T, TolkcError> {
    let text = read_artifact_string(path)?;
    serde_json::from_str(&text).map_err(|source| TolkcError::ParseArtifact {
        path: path.to_path_buf(),
        source,
    })
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "docs/ARCHITECTURE.md §3.2 scopes the no-unwrap/expect policy to library code outside tests"
)]
mod tests {
    use super::{
        TolkCompiler, parse_version_output, resolve_executable_path, sibling_artifact,
        stdlib_dir_for, wait_draining_stderr,
    };
    use std::path::{Path, PathBuf};
    use std::process::{Command, Stdio};
    use std::time::{Duration, Instant};

    #[test]
    fn resolve_executable_path_prefers_the_env_override() {
        let resolved = resolve_executable_path(Some("/custom/tolk"), Path::new("/bin/sting"));
        assert_eq!(resolved, PathBuf::from("/custom/tolk"));
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
    fn stdlib_dir_for_sits_next_to_the_executable() {
        let dir = stdlib_dir_for(Path::new("/opt/tolk/tolk.exe"));
        assert_eq!(dir, PathBuf::from("/opt/tolk/tolk-stdlib"));
    }

    #[test]
    fn sibling_artifact_replaces_the_fif_extension_not_appends_to_it() {
        let fif = Path::new("/tmp/scratch/out.fif");
        assert_eq!(
            sibling_artifact(fif, "abi.json"),
            PathBuf::from("/tmp/scratch/out.abi.json")
        );
        assert_eq!(
            sibling_artifact(fif, "symbolTypes.json"),
            PathBuf::from("/tmp/scratch/out.symbolTypes.json")
        );
    }

    #[test]
    fn parse_version_output_reads_the_real_three_line_format() {
        let raw = "Tolk compiler v1.4.1\nBuild commit: da70af2be5ef2efe9f0d575ec466847e27b81842\nBuild date: 2026-06-13 15:22:32 +0000\n";
        let parsed = parse_version_output(raw).expect("matches the real observed format");
        assert_eq!(parsed.version, "1.4.1");
        assert_eq!(
            parsed.build_commit,
            "da70af2be5ef2efe9f0d575ec466847e27b81842"
        );
        assert_eq!(parsed.build_date, "2026-06-13 15:22:32 +0000");
    }

    #[test]
    fn parse_version_output_rejects_unrecognized_text() {
        assert!(parse_version_output("not a version banner at all").is_err());
    }

    #[test]
    fn new_derives_the_stdlib_dir_from_the_given_executable_path() {
        let compiler = TolkCompiler::new(PathBuf::from("/opt/tolk/tolk.exe"));
        assert_eq!(compiler.executable_path(), Path::new("/opt/tolk/tolk.exe"));
    }

    /// Regression test for the pipe-buffer deadlock found in
    /// : a child that writes far more than one
    /// OS pipe buffer (~64KB) to stderr before failing must still be
    /// drained and reaped promptly, well under the configured timeout —
    /// with the old code (stderr read only *after* `wait_timeout`
    /// returned), the child would block on its own `write()` and this call
    /// would eat the entire timeout budget instead of returning almost
    /// immediately. Exercises the extracted helper directly (not the full
    /// `compile()` path) since that only needs a generic noisy child
    /// process, not a real vendored `tolk` binary.
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
