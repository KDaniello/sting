//! Raw unsafe FFI bindings to `libemulator`, loaded dynamically via `libloading`.
//!
//! All 33 symbols exported by `emulator/emulator-extern.h` (verified against
//! `ton-blockchain/ton@master`, see `docs/domains/emulator.md`) are
//! bound here — not just the five the first probe needed, and not just the
//! "recommended core" of fourteen. Every
//! signature below is transcribed by hand, field by field, from that header;
//! there is no `bindgen` step (see the module's rationale in
//! `docs/domains/emulator.md` §3: the Windows release ships a `.dll` and no
//! import library).
//!
//! Dynamic loading (not link-time `bindgen`+`cc`) is a deliberate choice: the
//! Windows release of `libemulator` ships a bare `.dll` with no `.lib`/`.pdb`,
//! so there is nothing to link against at build time. See
//! `docs/domains/emulator.md` for the full rationale.
//!
//! # Design deviation from the block doc's sketch
//!
//! `docs/domains/emulator.md` sketches [`LibEmulator`] with public
//! `pub tvm_emulator_create: unsafe extern "C" fn(...)`-shaped fields. This
//! implementation instead keeps every resolved function pointer private and
//! exposes one `unsafe fn` wrapper method per C function. That is
//! a strictly *tighter* encapsulation than the sketch: no raw C function
//! pointer of any kind is ever part of this crate's public API, only
//! `unsafe fn` methods with a documented `# Safety` contract each — kept
//! because it was already working, tested code, not because the sketch was
//! wrong.
//!
//! # Memory ownership
//!
//! Every function returning `const char *` here does so via `strdup()`
//! inside the C++ implementation; the header states plainly that calling the
//! caller's own `free()` on that pointer is unsafe (different allocator).
//! The only correct release path is `string_destroy()` — or, for
//! `tvm_emulator_emulate_run_method_detailed`'s two-pointer struct result,
//! `run_method_detailed_result_destroy()`. Every method in this crate that
//! receives such a pointer copies it into an owned Rust value and releases
//! the original via the correct `*_destroy` call *before returning* — no raw
//! `*const c_char` from this crate's FFI calls is ever handed back to a
//! caller. This is the exact step that all three surveyed third-party
//! bindings (`pytvm`, `tongo`, `ton4j`) skip or get wrong; see
//! `docs/domains/emulator.md` for the full comparison.
//!
//! `tvm_emulator_emulate_run_method` (the *non-detailed* single-call
//! optimization) is bound here for completeness, since all 33 symbols have to
//! resolve, but deliberately has **no** safe
//! wrapper method, and `sting-emulator` never calls it. Its `vm_log` field is
//! allocated by the C++ side and never freed by any code path in
//! `emulator-extern.cpp` itself (confirmed by reading the implementation:
//! `emulate_run_method`'s `TvmEulatorEmulateRunMethodResponse{rn, strdup(...)}`
//! is returned by value, and the non-detailed wrapper only returns
//! `result.response`, silently dropping `result.log` on the floor with no
//! `free`) — a leak in the upstream C++ that no caller-side discipline can
//! fix. Use `tvm_emulator_emulate_run_method_detailed` instead, which returns
//! both pointers so this crate can free both.

use std::ffi::{CStr, c_char, c_int, c_void};
use std::path::{Path, PathBuf};

/// Parsed `emulator_version()` payload.
///
/// The real (not assumed) shape, captured by running this crate's own
/// `run_get_jetton_data` example against the vendored `v2026.06` Windows
/// build: `{"emulatorLibCommitHash":"3f598aa2f0911317b626f4093177241e61798c61","emulatorLibCommitDate":"2026-07-15 13:49:38 +0300"}`.
/// There is no semantic version here — TON's ABI moves per-commit, not per
/// release — so "compatibility" at load time means exactly two things: (1)
/// every one of the 33 symbols this crate needs resolved by name, and (2)
/// this call returns JSON matching this exact shape. Neither is a
/// mathematical proof of ABI compatibility, but a library that fails either
/// check is definitely not a usable `libemulator` build, and failing fast
/// here beats segfaulting on the first real call.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct EmulatorVersion {
    /// Git commit hash the loaded `libemulator` was built from.
    #[serde(rename = "emulatorLibCommitHash")]
    pub commit_hash: String,
    /// Human-readable commit date/timezone string, as reported by the library.
    #[serde(rename = "emulatorLibCommitDate")]
    pub commit_date: String,
}

/// The `libemulator` build this crate accepts, as `emulator_version()` reports
/// it.
///
/// Written once in `Cargo.toml` under `[package.metadata.libemulator]` and
/// compiled in by `build.rs`. An emulation is a statement about semantics, and
/// a statement whose emulator nobody named is a statement nobody can check.
pub const PINNED_COMMIT: &str = env!("STING_LIBEMULATOR_COMMIT");
/// When [`PINNED_COMMIT`] was committed, as the library itself reports it.
pub const PINNED_COMMIT_DATE: &str = env!("STING_LIBEMULATOR_DATE");
/// The node release [`PINNED_COMMIT`] was published in — provenance, not
/// identity: TON's emulator moves per-commit rather than per release.
pub const PINNED_RELEASE: &str = env!("STING_LIBEMULATOR_RELEASE");
/// Where the pinned build is published, named in the refusal so the message
/// says what to do rather than only what went wrong.
pub const PINNED_SOURCE: &str = env!("STING_LIBEMULATOR_SOURCE");

/// Refuses any build but [`PINNED_COMMIT`], naming both commits.
///
/// Pure, so the refusal is testable without a real library — the same reason
/// [`parse_version`] is separate.
fn pinned_or_refused(version: EmulatorVersion, path: &Path) -> Result<EmulatorVersion, LoadError> {
    if version.commit_hash == PINNED_COMMIT {
        return Ok(version);
    }
    Err(LoadError::UnpinnedBuild {
        path: path.display().to_string(),
        found: version.commit_hash,
        expected: PINNED_COMMIT,
        release: PINNED_RELEASE,
        fetch_from: PINNED_SOURCE,
    })
}

/// Parses the raw `emulator_version()` string into [`EmulatorVersion`], as a
/// pure function so the "clearly rejects an incompatible bare string, not a
/// crash" requirement (`docs/domains/emulator.md`) is testable
/// without loading any real library.
fn parse_version(raw: &str) -> Result<EmulatorVersion, serde_json::Error> {
    serde_json::from_str(raw)
}

/// Resolved function pointers from a loaded `libemulator` shared library.
///
/// Owns the [`libloading::Library`] alongside the resolved symbols: the
/// symbols are raw pointers into the library's mapped code, so the library
/// must outlive every call made through them. Dropping `LibEmulator` unloads
/// the library.
pub struct LibEmulator {
    _lib: libloading::Library,

    // TransactionEmulator: 13 functions.
    transaction_emulator_create: unsafe extern "C" fn(*const c_char, c_int) -> *mut c_void,
    transaction_emulator_set_unixtime: unsafe extern "C" fn(*mut c_void, u32) -> bool,
    transaction_emulator_set_lt: unsafe extern "C" fn(*mut c_void, u64) -> bool,
    transaction_emulator_set_rand_seed: unsafe extern "C" fn(*mut c_void, *const c_char) -> bool,
    transaction_emulator_set_ignore_chksig: unsafe extern "C" fn(*mut c_void, bool) -> bool,
    transaction_emulator_set_config: unsafe extern "C" fn(*mut c_void, *const c_char) -> bool,
    transaction_emulator_set_config_object: unsafe extern "C" fn(*mut c_void, *mut c_void) -> bool,
    transaction_emulator_set_libs: unsafe extern "C" fn(*mut c_void, *const c_char) -> bool,
    transaction_emulator_set_debug_enabled: unsafe extern "C" fn(*mut c_void, bool) -> bool,
    transaction_emulator_set_prev_blocks_info:
        unsafe extern "C" fn(*mut c_void, *const c_char) -> bool,
    transaction_emulator_emulate_transaction:
        unsafe extern "C" fn(*mut c_void, *const c_char, *const c_char) -> *const c_char,
    transaction_emulator_emulate_tick_tock_transaction:
        unsafe extern "C" fn(*mut c_void, *const c_char, bool) -> *const c_char,
    transaction_emulator_destroy: unsafe extern "C" fn(*mut c_void),

    // The config object and the global: 3 functions.
    emulator_config_create: unsafe extern "C" fn(*const c_char) -> *mut c_void,
    emulator_config_destroy: unsafe extern "C" fn(*mut c_void),
    emulator_set_verbosity_level: unsafe extern "C" fn(c_int) -> bool,

    // TvmEmulator: 15 functions.
    tvm_emulator_create: unsafe extern "C" fn(*const c_char, *const c_char, c_int) -> *mut c_void,
    tvm_emulator_set_libraries: unsafe extern "C" fn(*mut c_void, *const c_char) -> bool,
    #[allow(
        clippy::type_complexity,
        reason = "transcribed 1:1 from emulator-extern.h; splitting it would obscure the ABI, not clarify it"
    )]
    tvm_emulator_set_c7: unsafe extern "C" fn(
        *mut c_void,
        *const c_char,
        u32,
        u64,
        *const c_char,
        *const c_char,
    ) -> bool,
    tvm_emulator_set_extra_currencies: unsafe extern "C" fn(*mut c_void, *const c_char) -> bool,
    tvm_emulator_set_config_object: unsafe extern "C" fn(*mut c_void, *mut c_void) -> bool,
    tvm_emulator_set_prev_blocks_info: unsafe extern "C" fn(*mut c_void, *const c_char) -> bool,
    tvm_emulator_set_gas_limit: unsafe extern "C" fn(*mut c_void, i64) -> bool,
    tvm_emulator_set_debug_enabled: unsafe extern "C" fn(*mut c_void, bool) -> bool,
    tvm_emulator_run_get_method:
        unsafe extern "C" fn(*mut c_void, c_int, *const c_char) -> *const c_char,
    /// Bound for completeness only, and never called. See this module's doc
    /// comment for why: upstream leaks `vm_log` in this variant.
    #[allow(
        dead_code,
        reason = "resolved to satisfy 'all 33 symbols bound', deliberately never invoked"
    )]
    tvm_emulator_emulate_run_method: unsafe extern "C" fn(u32, *const c_char, i64) -> *const c_char,
    tvm_emulator_emulate_run_method_detailed:
        unsafe extern "C" fn(u32, *const c_char, i64) -> *mut c_void,
    run_method_detailed_result_destroy: unsafe extern "C" fn(*mut c_void),
    tvm_emulator_send_external_message:
        unsafe extern "C" fn(*mut c_void, *const c_char) -> *const c_char,
    tvm_emulator_send_internal_message:
        unsafe extern "C" fn(*mut c_void, *const c_char, u64) -> *const c_char,
    tvm_emulator_destroy: unsafe extern "C" fn(*mut c_void),

    // Common to both: 2 functions.
    string_destroy: unsafe extern "C" fn(*const c_char),
    emulator_version: unsafe extern "C" fn() -> *const c_char,

    /// Parsed result of calling [`emulator_version`](LibEmulator::emulator_version_raw)
    /// once at load time — see [`LibEmulator::version`].
    version: EmulatorVersion,
}

/// Errors from [`LibEmulator::load`] or [`LibEmulator::default_library_path`].
#[derive(Debug, thiserror::Error)]
pub enum LoadError {
    /// The shared library itself could not be opened (missing file, wrong
    /// architecture, missing runtime dependency such as the MSVC redistributable).
    #[error("failed to load library at {path}: {source}")]
    Library {
        /// The path that was passed to [`LibEmulator::load`].
        path: String,
        /// The underlying `libloading`/OS loader error.
        #[source]
        source: libloading::Error,
    },
    /// The library loaded, but one of the expected exported symbols is
    /// missing — a strong signal of an ABI-incompatible `libemulator` build.
    #[error("failed to resolve symbol {symbol:?}: {source}")]
    Symbol {
        /// Name of the symbol that failed to resolve.
        symbol: &'static str,
        /// The underlying `libloading`/OS loader error.
        #[source]
        source: libloading::Error,
    },
    /// All 33 symbols resolved, but `emulator_version()` did not return the
    /// expected JSON shape — a weaker but still meaningful compatibility
    /// signal (see [`EmulatorVersion`]'s doc comment for why this can only
    /// ever be a shape check, not a semantic version comparison).
    #[error("emulator_version() returned unexpected content {raw:?}: {source}")]
    IncompatibleVersion {
        /// The raw string `emulator_version()` returned.
        raw: String,
        /// The JSON parse error.
        #[source]
        source: serde_json::Error,
    },
    /// The library is a genuine `libemulator`, but not the build this tree is
    /// pinned to. Two emulator builds can disagree about semantics, so a run
    /// against an unnamed build is a run nobody can reproduce.
    #[error(
        "libemulator at {path} is build {found}, and this tree is pinned to {expected}          ({release}, {fetch_from})"
    )]
    UnpinnedBuild {
        /// The path that was loaded.
        path: String,
        /// The commit the loaded library reports.
        found: String,
        /// The commit this crate is pinned to.
        expected: &'static str,
        /// The node release the pinned commit was published in.
        release: &'static str,
        /// Where the pinned build is published.
        fetch_from: &'static str,
    },
    /// [`LibEmulator::default_library_path`] could not determine the running
    /// executable's own path (`std::env::current_exe` failed).
    #[error("failed to determine the running executable's path: {0}")]
    CurrentExe(#[source] std::io::Error),
}

/// The platform-conventional `libemulator` shared library filename this
/// crate resolves by default. Deliberately **not** the exact filename
/// `ton-blockchain/ton`'s GitHub release publishes (`libemulator-linux-x86_64.so`,
/// `libemulator-mac-arm64.dylib`, ...) — those encode redundant
/// architecture info irrelevant to a same-architecture runtime resolving its
/// own platform's library by convention. Whatever fetches/vendors the binary
/// (a developer, CI, or a future `sting`-provided fetch command) is
/// responsible for placing it under this canonical name; this crate only
/// resolves, never fetches.
#[must_use]
pub const fn platform_library_filename() -> &'static str {
    if cfg!(target_os = "windows") {
        "libemulator.dll"
    } else if cfg!(target_os = "macos") {
        "libemulator.dylib"
    } else {
        "libemulator.so"
    }
}

/// Pure resolution logic behind [`LibEmulator::default_library_path`],
/// separated out so it is testable without touching the real environment or
/// `std::env::current_exe` (env vars are process-global mutable state —
/// asserting against them directly in a test that may run in parallel with
/// others is inherently flaky).
fn resolve_library_path(env_override: Option<&str>, exe_path: &Path) -> PathBuf {
    if let Some(path) = env_override {
        return PathBuf::from(path);
    }
    let dir = exe_path.parent().unwrap_or_else(|| Path::new("."));
    dir.join(platform_library_filename())
}

impl LibEmulator {
    /// Resolves the default `libemulator` path: `STING_LIBEMULATOR_PATH` if
    /// set, otherwise [`platform_library_filename`] next to the running
    /// executable (`docs/domains/emulator.md`).
    ///
    /// # Errors
    /// Returns [`LoadError::CurrentExe`] if the running executable's own path
    /// cannot be determined.
    pub fn default_library_path() -> Result<PathBuf, LoadError> {
        let env_override = std::env::var("STING_LIBEMULATOR_PATH").ok();
        let exe = std::env::current_exe().map_err(LoadError::CurrentExe)?;
        Ok(resolve_library_path(env_override.as_deref(), &exe))
    }

    /// Dynamically loads `libemulator` from [`Self::default_library_path`]
    /// and resolves the symbols this crate needs. See [`Self::load`] for the
    /// safety contract this inherits.
    ///
    /// # Errors
    /// See [`LoadError`].
    ///
    /// # Safety
    /// Same contract as [`Self::load`].
    pub unsafe fn load_default() -> Result<Self, LoadError> {
        let path = Self::default_library_path()?;
        // SAFETY: propagated to our own caller via this function's own
        // safety doc, identical to `load`'s contract.
        unsafe { Self::load(&path) }
    }

    /// Dynamically loads `libemulator` from `path` and resolves the symbols
    /// this crate needs.
    ///
    /// # Errors
    /// See [`LoadError`].
    ///
    /// # Safety
    /// Loading a shared library runs its static initializers as a side effect,
    /// which is inherently unsafe for a library not under the caller's
    /// control. The caller must ensure `path` names a genuine `libemulator`
    /// build; this function's version-shape check afterwards is evidence *a*
    /// compatible-enough library loaded, not a guarantee.
    #[allow(
        clippy::too_many_lines,
        reason = "one function resolving 33 symbols by name is inherently long; splitting it into per-group helper functions would just move the same lines around, not reduce them"
    )]
    pub unsafe fn load(path: &Path) -> Result<Self, LoadError> {
        // SAFETY: propagated to our own caller via this function's own safety
        // doc — loading an arbitrary shared library is unsafe by nature.
        let lib =
            unsafe { libloading::Library::new(path) }.map_err(|source| LoadError::Library {
                path: path.display().to_string(),
                source,
            })?;

        // SAFETY: each `get::<T>` is only unsafe because a wrong `T` would
        // read past/misinterpret the symbol's real signature; every `T` below
        // is transcribed by hand from `emulator/emulator-extern.h` at the
        // release tag matching the `.dll` this was tested against (see
        // docs/domains/emulator.md for the full, checked ABI).
        unsafe {
            macro_rules! resolve {
                ($name:literal, $ty:ty) => {
                    *lib.get::<$ty>(concat!($name, "\0").as_bytes())
                        .map_err(|source| LoadError::Symbol {
                            symbol: $name,
                            source,
                        })?
                };
            }

            let transaction_emulator_create = resolve!(
                "transaction_emulator_create",
                unsafe extern "C" fn(*const c_char, c_int) -> *mut c_void
            );
            let transaction_emulator_set_unixtime = resolve!(
                "transaction_emulator_set_unixtime",
                unsafe extern "C" fn(*mut c_void, u32) -> bool
            );
            let transaction_emulator_set_lt = resolve!(
                "transaction_emulator_set_lt",
                unsafe extern "C" fn(*mut c_void, u64) -> bool
            );
            let transaction_emulator_set_rand_seed = resolve!(
                "transaction_emulator_set_rand_seed",
                unsafe extern "C" fn(*mut c_void, *const c_char) -> bool
            );
            let transaction_emulator_set_ignore_chksig = resolve!(
                "transaction_emulator_set_ignore_chksig",
                unsafe extern "C" fn(*mut c_void, bool) -> bool
            );
            let transaction_emulator_set_config = resolve!(
                "transaction_emulator_set_config",
                unsafe extern "C" fn(*mut c_void, *const c_char) -> bool
            );
            let transaction_emulator_set_config_object = resolve!(
                "transaction_emulator_set_config_object",
                unsafe extern "C" fn(*mut c_void, *mut c_void) -> bool
            );
            let transaction_emulator_set_libs = resolve!(
                "transaction_emulator_set_libs",
                unsafe extern "C" fn(*mut c_void, *const c_char) -> bool
            );
            let transaction_emulator_set_debug_enabled = resolve!(
                "transaction_emulator_set_debug_enabled",
                unsafe extern "C" fn(*mut c_void, bool) -> bool
            );
            let transaction_emulator_set_prev_blocks_info = resolve!(
                "transaction_emulator_set_prev_blocks_info",
                unsafe extern "C" fn(*mut c_void, *const c_char) -> bool
            );
            let transaction_emulator_emulate_transaction = resolve!(
                "transaction_emulator_emulate_transaction",
                unsafe extern "C" fn(*mut c_void, *const c_char, *const c_char) -> *const c_char
            );
            let transaction_emulator_emulate_tick_tock_transaction = resolve!(
                "transaction_emulator_emulate_tick_tock_transaction",
                unsafe extern "C" fn(*mut c_void, *const c_char, bool) -> *const c_char
            );
            let transaction_emulator_destroy = resolve!(
                "transaction_emulator_destroy",
                unsafe extern "C" fn(*mut c_void)
            );

            let emulator_config_create = resolve!(
                "emulator_config_create",
                unsafe extern "C" fn(*const c_char) -> *mut c_void
            );
            let emulator_config_destroy =
                resolve!("emulator_config_destroy", unsafe extern "C" fn(*mut c_void));
            let emulator_set_verbosity_level = resolve!(
                "emulator_set_verbosity_level",
                unsafe extern "C" fn(c_int) -> bool
            );

            let tvm_emulator_create = resolve!(
                "tvm_emulator_create",
                unsafe extern "C" fn(*const c_char, *const c_char, c_int) -> *mut c_void
            );
            let tvm_emulator_set_libraries = resolve!(
                "tvm_emulator_set_libraries",
                unsafe extern "C" fn(*mut c_void, *const c_char) -> bool
            );
            let tvm_emulator_set_c7 = *lib
                .get::<unsafe extern "C" fn(
                    *mut c_void,
                    *const c_char,
                    u32,
                    u64,
                    *const c_char,
                    *const c_char,
                ) -> bool>(b"tvm_emulator_set_c7\0")
                .map_err(|source| LoadError::Symbol {
                    symbol: "tvm_emulator_set_c7",
                    source,
                })?;
            let tvm_emulator_set_extra_currencies = resolve!(
                "tvm_emulator_set_extra_currencies",
                unsafe extern "C" fn(*mut c_void, *const c_char) -> bool
            );
            let tvm_emulator_set_config_object = resolve!(
                "tvm_emulator_set_config_object",
                unsafe extern "C" fn(*mut c_void, *mut c_void) -> bool
            );
            let tvm_emulator_set_prev_blocks_info = resolve!(
                "tvm_emulator_set_prev_blocks_info",
                unsafe extern "C" fn(*mut c_void, *const c_char) -> bool
            );
            let tvm_emulator_set_gas_limit = resolve!(
                "tvm_emulator_set_gas_limit",
                unsafe extern "C" fn(*mut c_void, i64) -> bool
            );
            let tvm_emulator_set_debug_enabled = resolve!(
                "tvm_emulator_set_debug_enabled",
                unsafe extern "C" fn(*mut c_void, bool) -> bool
            );
            let tvm_emulator_run_get_method = resolve!(
                "tvm_emulator_run_get_method",
                unsafe extern "C" fn(*mut c_void, c_int, *const c_char) -> *const c_char
            );
            let tvm_emulator_emulate_run_method = resolve!(
                "tvm_emulator_emulate_run_method",
                unsafe extern "C" fn(u32, *const c_char, i64) -> *const c_char
            );
            let tvm_emulator_emulate_run_method_detailed = resolve!(
                "tvm_emulator_emulate_run_method_detailed",
                unsafe extern "C" fn(u32, *const c_char, i64) -> *mut c_void
            );
            let run_method_detailed_result_destroy = resolve!(
                "run_method_detailed_result_destroy",
                unsafe extern "C" fn(*mut c_void)
            );
            let tvm_emulator_send_external_message = resolve!(
                "tvm_emulator_send_external_message",
                unsafe extern "C" fn(*mut c_void, *const c_char) -> *const c_char
            );
            let tvm_emulator_send_internal_message = resolve!(
                "tvm_emulator_send_internal_message",
                unsafe extern "C" fn(*mut c_void, *const c_char, u64) -> *const c_char
            );
            let tvm_emulator_destroy =
                resolve!("tvm_emulator_destroy", unsafe extern "C" fn(*mut c_void));

            let string_destroy = resolve!("string_destroy", unsafe extern "C" fn(*const c_char));
            let emulator_version =
                resolve!("emulator_version", unsafe extern "C" fn() -> *const c_char);

            // SAFETY: `emulator_version` takes no arguments and returns a
            // `const char*` per emulator-extern.h; non-null per every
            // observed real build (a static build-info string).
            let version_ptr = emulator_version();
            let version_raw = if version_ptr.is_null() {
                String::new()
            } else {
                // SAFETY: non-null pointer just returned by the library, per
                // contract a NUL-terminated C string valid for this call.
                CStr::from_ptr(version_ptr).to_string_lossy().into_owned()
            };
            let version =
                parse_version(&version_raw).map_err(|source| LoadError::IncompatibleVersion {
                    raw: version_raw,
                    source,
                })?;
            let version = pinned_or_refused(version, path)?;

            Ok(Self {
                _lib: lib,
                transaction_emulator_create,
                transaction_emulator_set_unixtime,
                transaction_emulator_set_lt,
                transaction_emulator_set_rand_seed,
                transaction_emulator_set_ignore_chksig,
                transaction_emulator_set_config,
                transaction_emulator_set_config_object,
                transaction_emulator_set_libs,
                transaction_emulator_set_debug_enabled,
                transaction_emulator_set_prev_blocks_info,
                transaction_emulator_emulate_transaction,
                transaction_emulator_emulate_tick_tock_transaction,
                transaction_emulator_destroy,
                emulator_config_create,
                emulator_config_destroy,
                emulator_set_verbosity_level,
                tvm_emulator_create,
                tvm_emulator_set_libraries,
                tvm_emulator_set_c7,
                tvm_emulator_set_extra_currencies,
                tvm_emulator_set_config_object,
                tvm_emulator_set_prev_blocks_info,
                tvm_emulator_set_gas_limit,
                tvm_emulator_set_debug_enabled,
                tvm_emulator_run_get_method,
                tvm_emulator_emulate_run_method,
                tvm_emulator_emulate_run_method_detailed,
                run_method_detailed_result_destroy,
                tvm_emulator_send_external_message,
                tvm_emulator_send_internal_message,
                tvm_emulator_destroy,
                string_destroy,
                emulator_version,
                version,
            })
        }
    }

    /// The parsed, load-time-cached `emulator_version()` result — see
    /// [`EmulatorVersion`]. Safe: this is cached data from [`Self::load`],
    /// not a fresh FFI call.
    #[must_use]
    pub fn version(&self) -> &EmulatorVersion {
        &self.version
    }

    /// Re-invokes `emulator_version()` fresh, unlike [`Self::version`] (which
    /// returns the result parsed and cached once at [`Self::load`] time).
    /// Ordinary callers should prefer [`Self::version`]; this exists for
    /// diagnostic tooling that wants to double-check the loaded library
    /// hasn't changed underneath a long-lived process.
    ///
    /// # Safety
    /// `self` must have been loaded from a genuine, ABI-compatible
    /// `libemulator` build.
    #[must_use]
    pub unsafe fn emulator_version_raw(&self) -> String {
        // SAFETY: `emulator_version` takes no arguments; `self` is assumed
        // ABI-compatible per this function's own safety contract.
        let ptr = unsafe { (self.emulator_version)() };
        if ptr.is_null() {
            return String::new();
        }
        // SAFETY: non-null pointer just returned by the library, per
        // contract a NUL-terminated C string valid for this call (a static
        // build-info string, not requiring `string_destroy`).
        unsafe { CStr::from_ptr(ptr) }
            .to_string_lossy()
            .into_owned()
    }

    // TvmEmulator.

    /// Creates a TVM emulator instance from base64-encoded code/data cell `BoC`s.
    ///
    /// Returns `None` if the underlying library returns a null pointer
    /// (malformed `BoC` or internal allocation failure — the C API does not
    /// distinguish the two).
    ///
    /// # Safety
    /// `self` must have been loaded from a genuine, ABI-compatible
    /// `libemulator` build. The returned handle must be destroyed exactly
    /// once via [`Self::destroy_tvm_emulator`], and never used after that call.
    #[must_use]
    pub unsafe fn create_tvm_emulator(
        &self,
        code_boc_base64: &CStr,
        data_boc_base64: &CStr,
        vm_log_verbosity: i32,
    ) -> Option<*mut c_void> {
        // SAFETY: both arguments are valid NUL-terminated C strings for the
        // duration of this call (borrowed as `&CStr`); `self` is assumed
        // ABI-compatible per this function's own safety contract.
        let handle = unsafe {
            (self.tvm_emulator_create)(
                code_boc_base64.as_ptr(),
                data_boc_base64.as_ptr(),
                vm_log_verbosity,
            )
        };
        (!handle.is_null()).then_some(handle)
    }

    /// Sets shared libraries (`HashmapE 256 ^Cell`) resolvable during
    /// execution (e.g. TEP-based library cells).
    ///
    /// # Safety
    /// `handle` must be a live pointer previously returned by
    /// [`Self::create_tvm_emulator`] on this same `self`, not yet destroyed.
    #[must_use]
    pub unsafe fn set_tvm_libraries(&self, handle: *mut c_void, libs_boc: &CStr) -> bool {
        // SAFETY: `handle` validity is this function's own safety contract;
        // `libs_boc` is a valid NUL-terminated C string for the call.
        unsafe { (self.tvm_emulator_set_libraries)(handle, libs_boc.as_ptr()) }
    }

    /// Sets c7 context (own address, unixtime, balance, random seed, and
    /// optionally blockchain config) on a TVM emulator instance. Many
    /// get-methods rely on this — `MYADDR`/`config.*` opcodes read from c7 and
    /// fail (VM exit code 9, "cannot parse a `MsgAddress`") if it was never set,
    /// discovered the hard way against a real jetton master whose
    /// `get_jetton_data()` embeds its own
    /// address into a generated content URL.
    ///
    /// `config` is `Option` because `emulator-extern.h` documents it as
    /// optional — the C++ side itself checks for `nullptr` — confirmed by
    /// research, not obvious from the header signature alone
    /// (`docs/domains/emulator.md`).
    ///
    /// # Safety
    /// `handle` must be a live pointer previously returned by
    /// [`Self::create_tvm_emulator`] on this same `self`, not yet destroyed.
    #[must_use]
    #[allow(
        clippy::too_many_arguments,
        reason = "transcribed 1:1 from tvm_emulator_set_c7's own 6-argument C signature plus &self; splitting into a builder would obscure the ABI, not clarify it"
    )]
    pub unsafe fn set_c7(
        &self,
        handle: *mut c_void,
        address: &CStr,
        unixtime: u32,
        balance: u64,
        rand_seed_hex: &CStr,
        config: Option<&CStr>,
    ) -> bool {
        let config_ptr = config.map_or(std::ptr::null(), CStr::as_ptr);
        // SAFETY: `handle` validity is this function's own safety contract;
        // `address`/`rand_seed_hex` are valid NUL-terminated C strings for the
        // call; `config_ptr` is either null or a valid NUL-terminated C
        // string borrowed from `config` for the call's duration.
        unsafe {
            (self.tvm_emulator_set_c7)(
                handle,
                address.as_ptr(),
                unixtime,
                balance,
                rand_seed_hex.as_ptr(),
                config_ptr,
            )
        }
    }

    /// Sets extra-currency balances (`"currency_id1=balance1 ..."` format).
    ///
    /// # Safety
    /// `handle` must be a live pointer previously returned by
    /// [`Self::create_tvm_emulator`] on this same `self`, not yet destroyed.
    #[must_use]
    pub unsafe fn set_tvm_extra_currencies(
        &self,
        handle: *mut c_void,
        extra_currencies: &CStr,
    ) -> bool {
        // SAFETY: `handle` validity is this function's own safety contract;
        // `extra_currencies` is a valid NUL-terminated C string for the call.
        unsafe { (self.tvm_emulator_set_extra_currencies)(handle, extra_currencies.as_ptr()) }
    }

    /// Sets a pre-parsed [`Self::create_emulator_config`] object as the TVM
    /// emulator's blockchain config, avoiding re-parsing the config `BoC` on
    /// every call — useful when running many get-methods against the same
    /// config, which is what the fuzzing harness and the bridge both do.
    ///
    /// # Safety
    /// `handle` must be a live pointer previously returned by
    /// [`Self::create_tvm_emulator`] on this same `self`, not yet destroyed.
    /// `config` must be a live pointer previously returned by
    /// [`Self::create_emulator_config`], not yet destroyed.
    #[must_use]
    pub unsafe fn set_tvm_config_object(&self, handle: *mut c_void, config: *mut c_void) -> bool {
        // SAFETY: both pointers' validity is this function's own safety contract.
        unsafe { (self.tvm_emulator_set_config_object)(handle, config) }
    }

    /// Sets the tuple of previous blocks (13th element of c7).
    ///
    /// # Safety
    /// `handle` must be a live pointer previously returned by
    /// [`Self::create_tvm_emulator`] on this same `self`, not yet destroyed.
    #[must_use]
    pub unsafe fn set_tvm_prev_blocks_info(&self, handle: *mut c_void, info_boc: &CStr) -> bool {
        // SAFETY: `handle` validity is this function's own safety contract;
        // `info_boc` is a valid NUL-terminated C string for the call.
        unsafe { (self.tvm_emulator_set_prev_blocks_info)(handle, info_boc.as_ptr()) }
    }

    /// Sets the TVM gas limit.
    ///
    /// # Safety
    /// `handle` must be a live pointer previously returned by
    /// [`Self::create_tvm_emulator`] on this same `self`, not yet destroyed.
    #[must_use]
    pub unsafe fn set_tvm_gas_limit(&self, handle: *mut c_void, gas_limit: i64) -> bool {
        // SAFETY: `handle` validity is this function's own safety contract.
        unsafe { (self.tvm_emulator_set_gas_limit)(handle, gas_limit) }
    }

    /// Enables or disables TVM debug primitives.
    ///
    /// # Safety
    /// `handle` must be a live pointer previously returned by
    /// [`Self::create_tvm_emulator`] on this same `self`, not yet destroyed.
    #[must_use]
    pub unsafe fn set_tvm_debug_enabled(&self, handle: *mut c_void, debug_enabled: bool) -> bool {
        // SAFETY: `handle` validity is this function's own safety contract.
        unsafe { (self.tvm_emulator_set_debug_enabled)(handle, debug_enabled) }
    }

    /// Runs a get-method and returns the raw JSON response as an owned
    /// `String`, with `string_destroy` already applied to the library's
    /// buffer before returning (see the memory-ownership note in
    /// `docs/domains/emulator.md` — this is the step every prior
    /// third-party binding skips).
    ///
    /// # Safety
    /// `handle` must be a live pointer previously returned by
    /// [`Self::create_tvm_emulator`] on this same `self`, not yet destroyed.
    #[must_use]
    pub unsafe fn run_get_method(
        &self,
        handle: *mut c_void,
        method_id: i32,
        stack_boc_base64: &CStr,
    ) -> Option<String> {
        // SAFETY: `handle` validity is this function's own safety contract;
        // `stack_boc_base64` is a valid NUL-terminated C string for the call.
        let result_ptr = unsafe {
            (self.tvm_emulator_run_get_method)(handle, method_id, stack_boc_base64.as_ptr())
        };
        if result_ptr.is_null() {
            return None;
        }
        // SAFETY: non-null pointer just returned by the library; per
        // emulator-extern.h it is a `strdup`'d NUL-terminated JSON string
        // that we alone now own and must release via `string_destroy`.
        let owned = unsafe { CStr::from_ptr(result_ptr) }
            .to_string_lossy()
            .into_owned();
        // SAFETY: releases exactly the pointer just returned, exactly once,
        // via the library's own allocator (never the caller's `free`) — the
        // documented, correct release path per emulator-extern.h.
        unsafe { (self.string_destroy)(result_ptr) };
        Some(owned)
    }

    /// Sanity ceiling on [`Self::emulate_run_method_detailed`]'s
    /// `len_prefix` before trusting it enough to build a slice from it —
    /// see that function's own comment at the check site. Real TVM
    /// run-method response `BoC`s are bounded by TON's own protocol-level
    /// cell-count/depth limits and sit many orders of magnitude below this;
    /// 64 MiB exists purely as a last-resort guard against a corrupted or
    /// ABI-mismatched length turning into an unbounded out-of-bounds read.
    const MAX_PLAUSIBLE_RESPONSE_LEN: u32 = 64 * 1024 * 1024;

    /// Runs `tvm_emulator_emulate_run_method_detailed`: a self-contained,
    /// single-call optimization that builds its own internal `TvmEmulator`
    /// from `params_boc` (a `BoC` encoding
    /// `request$_ code:^Cell data:^Cell stack:^VmStack params:^[c7:^VmStack libs:^Cell] method_id:(## 32)`,
    /// see `emulator-extern.h`), runs the method, and returns both the
    /// length-prefixed result `BoC` and the VM log — as opposed to
    /// [`Self::run_get_method`], which requires a separately-created,
    /// separately-destroyed `TvmEmulator` handle. Both pointers in the
    /// library's returned struct are copied into owned Rust values and the
    /// struct itself released via `run_method_detailed_result_destroy`
    /// before this returns — no raw pointer crosses this call's boundary.
    ///
    /// Returns `None` if `params_boc` was malformed (the library reports
    /// this via null inner fields, not a null outer struct pointer — see
    /// `emulate_run_method`'s `return {nullptr, nullptr};` paths in
    /// `emulator-extern.cpp`).
    ///
    /// # Safety
    /// `self` must have been loaded from a genuine, ABI-compatible
    /// `libemulator` build.
    #[must_use]
    pub unsafe fn emulate_run_method_detailed(
        &self,
        params_boc: &[u8],
        gas_limit: i64,
    ) -> Option<DetailedRunMethodResult> {
        let len = u32::try_from(params_boc.len()).ok()?;
        // SAFETY: `params_boc` is a valid, `len`-byte buffer for the
        // duration of this call; `self` is assumed ABI-compatible per this
        // function's own safety contract.
        let ptr = unsafe {
            (self.tvm_emulator_emulate_run_method_detailed)(
                len,
                params_boc.as_ptr().cast::<c_char>(),
                gas_limit,
            )
        };
        if ptr.is_null() {
            // Not observed in practice (the C++ side always heap-allocates
            // the struct, even for an internal parse failure — see the
            // module doc comment) but handled defensively.
            return None;
        }
        // SAFETY: non-null pointer to a heap-allocated 2-pointer struct
        // (`response`, `log`, in that order), matching
        // `TvmEulatorEmulateRunMethodResponse` in `emulator-extern.cpp`
        // exactly (verified by reading that file, not assumed).
        let raw = unsafe { &*ptr.cast::<RawDetailedResult>() };
        let result = if raw.response.is_null() {
            None
        } else {
            // SAFETY: non-null `response` is a `malloc`'d buffer of
            // `4 + len` bytes per `emulate_run_method`'s
            // `memcpy(rn, &sz, 4); memcpy(rn + 4, sok.data(), sz);` — a
            // little-endian `u32` length prefix followed by exactly that
            // many bytes of BoC payload. Read unaligned: the buffer has no
            // alignment guarantee beyond `malloc`'s (1-byte for our purposes).
            let len_prefix = unsafe { raw.response.cast::<u32>().read_unaligned() };
            if len_prefix > Self::MAX_PLAUSIBLE_RESPONSE_LEN {
                // `len_prefix` has no independently-verifiable source of
                // truth from Rust's side (the C API hands back a raw
                // pointer, not a paired allocation size) — this bound does
                // not, and cannot, *prove* the buffer is actually this long
                // (that still rests entirely on this function's own
                // `# Safety` contract that `self` is a genuine,
                // ABI-compatible `libemulator` build). What it does do:
                // turn a corrupted or ABI-mismatched `len_prefix` into a
                // clean `None` instead of an unbounded
                // `slice::from_raw_parts` read — found during a full
                // project review, not previously guarded at all.
                None
            } else {
                // SAFETY: `len_prefix` bytes of BoC payload follow the
                // 4-byte header, per the same contract as above — bounded
                // to a plausible size by the check just above, though that
                // check is a sanity net, not an independent safety proof
                // (see its own comment).
                let response_boc = unsafe {
                    std::slice::from_raw_parts(
                        raw.response.add(4).cast::<u8>(),
                        len_prefix as usize,
                    )
                }
                .to_vec();
                let vm_log = if raw.log.is_null() {
                    String::new()
                } else {
                    // SAFETY: non-null `log` is `strdup(result.vm_log.data())`
                    // per `emulate_run_method` — a NUL-terminated C string.
                    unsafe { CStr::from_ptr(raw.log) }
                        .to_string_lossy()
                        .into_owned()
                };
                Some(DetailedRunMethodResult {
                    response_boc,
                    vm_log,
                })
            }
        };
        // SAFETY: releases exactly the struct (and both inner buffers,
        // `free`-ing null inner pointers is a documented C no-op) returned
        // above, exactly once, via the library's own destroy function.
        unsafe { (self.run_method_detailed_result_destroy)(ptr.cast()) };
        result
    }

    /// Sends an external message and returns the raw JSON response.
    ///
    /// # Safety
    /// `handle` must be a live pointer previously returned by
    /// [`Self::create_tvm_emulator`] on this same `self`, not yet destroyed.
    #[must_use]
    pub unsafe fn send_external_message(
        &self,
        handle: *mut c_void,
        message_body_boc: &CStr,
    ) -> Option<String> {
        // SAFETY: `handle` validity is this function's own safety contract;
        // `message_body_boc` is a valid NUL-terminated C string for the call.
        let result_ptr =
            unsafe { (self.tvm_emulator_send_external_message)(handle, message_body_boc.as_ptr()) };
        if result_ptr.is_null() {
            return None;
        }
        // SAFETY: see `run_get_method` — identical ownership contract.
        let owned = unsafe { CStr::from_ptr(result_ptr) }
            .to_string_lossy()
            .into_owned();
        // SAFETY: see `run_get_method` — identical release contract.
        unsafe { (self.string_destroy)(result_ptr) };
        Some(owned)
    }

    /// Sends an internal message with `amount` nanotons attached and returns
    /// the raw JSON response.
    ///
    /// # Safety
    /// `handle` must be a live pointer previously returned by
    /// [`Self::create_tvm_emulator`] on this same `self`, not yet destroyed.
    #[must_use]
    pub unsafe fn send_internal_message(
        &self,
        handle: *mut c_void,
        message_body_boc: &CStr,
        amount: u64,
    ) -> Option<String> {
        // SAFETY: `handle` validity is this function's own safety contract;
        // `message_body_boc` is a valid NUL-terminated C string for the call.
        let result_ptr = unsafe {
            (self.tvm_emulator_send_internal_message)(handle, message_body_boc.as_ptr(), amount)
        };
        if result_ptr.is_null() {
            return None;
        }
        // SAFETY: see `run_get_method` — identical ownership contract.
        let owned = unsafe { CStr::from_ptr(result_ptr) }
            .to_string_lossy()
            .into_owned();
        // SAFETY: see `run_get_method` — identical release contract.
        unsafe { (self.string_destroy)(result_ptr) };
        Some(owned)
    }

    /// Destroys a TVM emulator instance created by [`Self::create_tvm_emulator`].
    ///
    /// # Safety
    /// `handle` must be a live pointer previously returned by
    /// [`Self::create_tvm_emulator`] on this same `self`, and must not be
    /// used (including calling this function again with it) after this call.
    pub unsafe fn destroy_tvm_emulator(&self, handle: *mut c_void) {
        // SAFETY: handle validity/single-destroy is this function's own
        // safety contract, passed through to the caller.
        unsafe { (self.tvm_emulator_destroy)(handle) };
    }

    // TransactionEmulator.

    /// Creates a `TransactionEmulator` object from a base64-encoded config `BoC`.
    ///
    /// Returns `None` on a null pointer (malformed config).
    ///
    /// # Safety
    /// `self` must have been loaded from a genuine, ABI-compatible
    /// `libemulator` build. The returned handle must be destroyed exactly
    /// once via [`Self::destroy_transaction_emulator`], and never used after.
    #[must_use]
    pub unsafe fn create_transaction_emulator(
        &self,
        config_params_boc: &CStr,
        vm_log_verbosity: i32,
    ) -> Option<*mut c_void> {
        // SAFETY: `config_params_boc` is a valid NUL-terminated C string for
        // the call; `self` is assumed ABI-compatible per this function's own
        // safety contract.
        let handle = unsafe {
            (self.transaction_emulator_create)(config_params_boc.as_ptr(), vm_log_verbosity)
        };
        (!handle.is_null()).then_some(handle)
    }

    /// Sets the unix timestamp used for emulation.
    ///
    /// # Safety
    /// `handle` must be a live pointer previously returned by
    /// [`Self::create_transaction_emulator`] on this same `self`, not yet destroyed.
    #[must_use]
    pub unsafe fn set_unixtime(&self, handle: *mut c_void, unixtime: u32) -> bool {
        // SAFETY: `handle` validity is this function's own safety contract.
        unsafe { (self.transaction_emulator_set_unixtime)(handle, unixtime) }
    }

    /// Sets the logical time used for emulation.
    ///
    /// # Safety
    /// `handle` must be a live pointer previously returned by
    /// [`Self::create_transaction_emulator`] on this same `self`, not yet destroyed.
    #[must_use]
    pub unsafe fn set_lt(&self, handle: *mut c_void, lt: u64) -> bool {
        // SAFETY: `handle` validity is this function's own safety contract.
        unsafe { (self.transaction_emulator_set_lt)(handle, lt) }
    }

    /// Sets the random seed (64 hex chars) used for emulation.
    ///
    /// # Safety
    /// `handle` must be a live pointer previously returned by
    /// [`Self::create_transaction_emulator`] on this same `self`, not yet destroyed.
    #[must_use]
    pub unsafe fn set_rand_seed(&self, handle: *mut c_void, rand_seed_hex: &CStr) -> bool {
        // SAFETY: `handle` validity is this function's own safety contract;
        // `rand_seed_hex` is a valid NUL-terminated C string for the call.
        unsafe { (self.transaction_emulator_set_rand_seed)(handle, rand_seed_hex.as_ptr()) }
    }

    /// Sets whether emulation should always succeed on `CHKSIG`.
    ///
    /// # Safety
    /// `handle` must be a live pointer previously returned by
    /// [`Self::create_transaction_emulator`] on this same `self`, not yet destroyed.
    #[must_use]
    pub unsafe fn set_ignore_chksig(&self, handle: *mut c_void, ignore_chksig: bool) -> bool {
        // SAFETY: `handle` validity is this function's own safety contract.
        unsafe { (self.transaction_emulator_set_ignore_chksig)(handle, ignore_chksig) }
    }

    /// Sets the blockchain config (base64-encoded `BoC`) used for emulation.
    ///
    /// # Safety
    /// `handle` must be a live pointer previously returned by
    /// [`Self::create_transaction_emulator`] on this same `self`, not yet destroyed.
    #[must_use]
    pub unsafe fn set_config(&self, handle: *mut c_void, config_boc: &CStr) -> bool {
        // SAFETY: `handle` validity is this function's own safety contract;
        // `config_boc` is a valid NUL-terminated C string for the call.
        unsafe { (self.transaction_emulator_set_config)(handle, config_boc.as_ptr()) }
    }

    /// Sets a pre-parsed [`Self::create_emulator_config`] object as this
    /// transaction emulator's config, avoiding re-parsing the config `BoC`
    /// on every call.
    ///
    /// # Safety
    /// `handle` must be a live pointer previously returned by
    /// [`Self::create_transaction_emulator`] on this same `self`, not yet
    /// destroyed. `config` must be a live pointer previously returned by
    /// [`Self::create_emulator_config`], not yet destroyed.
    #[must_use]
    pub unsafe fn set_transaction_config_object(
        &self,
        handle: *mut c_void,
        config: *mut c_void,
    ) -> bool {
        // SAFETY: both pointers' validity is this function's own safety contract.
        unsafe { (self.transaction_emulator_set_config_object)(handle, config) }
    }

    /// Sets shared libraries (`HashmapE 256 ^Cell`) for emulation.
    ///
    /// # Safety
    /// `handle` must be a live pointer previously returned by
    /// [`Self::create_transaction_emulator`] on this same `self`, not yet destroyed.
    #[must_use]
    pub unsafe fn set_libs(&self, handle: *mut c_void, libs_boc: &CStr) -> bool {
        // SAFETY: `handle` validity is this function's own safety contract;
        // `libs_boc` is a valid NUL-terminated C string for the call.
        unsafe { (self.transaction_emulator_set_libs)(handle, libs_boc.as_ptr()) }
    }

    /// Enables or disables TVM debug primitives for transaction emulation.
    ///
    /// # Safety
    /// `handle` must be a live pointer previously returned by
    /// [`Self::create_transaction_emulator`] on this same `self`, not yet destroyed.
    #[must_use]
    pub unsafe fn set_transaction_debug_enabled(
        &self,
        handle: *mut c_void,
        debug_enabled: bool,
    ) -> bool {
        // SAFETY: `handle` validity is this function's own safety contract.
        unsafe { (self.transaction_emulator_set_debug_enabled)(handle, debug_enabled) }
    }

    /// Sets the tuple of previous blocks (13th element of c7).
    ///
    /// # Safety
    /// `handle` must be a live pointer previously returned by
    /// [`Self::create_transaction_emulator`] on this same `self`, not yet destroyed.
    #[must_use]
    pub unsafe fn set_transaction_prev_blocks_info(
        &self,
        handle: *mut c_void,
        info_boc: &CStr,
    ) -> bool {
        // SAFETY: `handle` validity is this function's own safety contract;
        // `info_boc` is a valid NUL-terminated C string for the call.
        unsafe { (self.transaction_emulator_set_prev_blocks_info)(handle, info_boc.as_ptr()) }
    }

    /// Emulates a transaction and returns the raw JSON response.
    ///
    /// # Safety
    /// `handle` must be a live pointer previously returned by
    /// [`Self::create_transaction_emulator`] on this same `self`, not yet destroyed.
    #[must_use]
    pub unsafe fn emulate_transaction(
        &self,
        handle: *mut c_void,
        shard_account_boc: &CStr,
        message_boc: &CStr,
    ) -> Option<String> {
        // SAFETY: `handle` validity is this function's own safety contract;
        // both `BoC` arguments are valid NUL-terminated C strings for the call.
        let result_ptr = unsafe {
            (self.transaction_emulator_emulate_transaction)(
                handle,
                shard_account_boc.as_ptr(),
                message_boc.as_ptr(),
            )
        };
        if result_ptr.is_null() {
            return None;
        }
        // SAFETY: see `run_get_method` — identical ownership contract.
        let owned = unsafe { CStr::from_ptr(result_ptr) }
            .to_string_lossy()
            .into_owned();
        // SAFETY: see `run_get_method` — identical release contract.
        unsafe { (self.string_destroy)(result_ptr) };
        Some(owned)
    }

    /// Emulates a tick/tock transaction on a special account and returns the
    /// raw JSON response.
    ///
    /// # Safety
    /// `handle` must be a live pointer previously returned by
    /// [`Self::create_transaction_emulator`] on this same `self`, not yet destroyed.
    #[must_use]
    pub unsafe fn emulate_tick_tock_transaction(
        &self,
        handle: *mut c_void,
        shard_account_boc: &CStr,
        is_tock: bool,
    ) -> Option<String> {
        // SAFETY: `handle` validity is this function's own safety contract;
        // `shard_account_boc` is a valid NUL-terminated C string for the call.
        let result_ptr = unsafe {
            (self.transaction_emulator_emulate_tick_tock_transaction)(
                handle,
                shard_account_boc.as_ptr(),
                is_tock,
            )
        };
        if result_ptr.is_null() {
            return None;
        }
        // SAFETY: see `run_get_method` — identical ownership contract.
        let owned = unsafe { CStr::from_ptr(result_ptr) }
            .to_string_lossy()
            .into_owned();
        // SAFETY: see `run_get_method` — identical release contract.
        unsafe { (self.string_destroy)(result_ptr) };
        Some(owned)
    }

    /// Destroys a `TransactionEmulator` instance created by
    /// [`Self::create_transaction_emulator`].
    ///
    /// # Safety
    /// `handle` must be a live pointer previously returned by
    /// [`Self::create_transaction_emulator`], and must not be used (including
    /// calling this function again with it) after this call.
    pub unsafe fn destroy_transaction_emulator(&self, handle: *mut c_void) {
        // SAFETY: handle validity/single-destroy is this function's own
        // safety contract, passed through to the caller.
        unsafe { (self.transaction_emulator_destroy)(handle) };
    }

    // The config object.

    /// Creates a `Config` object from a base64-encoded config `BoC`, for
    /// reuse across multiple `*_set_config_object` calls without re-parsing.
    ///
    /// Returns `None` on a null pointer (malformed config).
    ///
    /// # Safety
    /// `self` must have been loaded from a genuine, ABI-compatible
    /// `libemulator` build. The returned handle must be destroyed exactly
    /// once via [`Self::destroy_emulator_config`], and never used after.
    #[must_use]
    pub unsafe fn create_emulator_config(&self, config_params_boc: &CStr) -> Option<*mut c_void> {
        // SAFETY: `config_params_boc` is a valid NUL-terminated C string for
        // the call; `self` is assumed ABI-compatible per this function's own
        // safety contract.
        let handle = unsafe { (self.emulator_config_create)(config_params_boc.as_ptr()) };
        (!handle.is_null()).then_some(handle)
    }

    /// Destroys a `Config` object created by [`Self::create_emulator_config`].
    ///
    /// # Safety
    /// `handle` must be a live pointer previously returned by
    /// [`Self::create_emulator_config`], and must not be used (including
    /// calling this function again with it, or via any `*_set_config_object`
    /// call) after this call.
    pub unsafe fn destroy_emulator_config(&self, handle: *mut c_void) {
        // SAFETY: handle validity/single-destroy is this function's own
        // safety contract, passed through to the caller.
        unsafe { (self.emulator_config_destroy)(handle) };
    }

    /// Sets the library's global verbosity level (0 = never, 4 = debug).
    ///
    /// # Safety
    /// `self` must have been loaded from a genuine, ABI-compatible
    /// `libemulator` build. This affects all emulator instances process-wide.
    #[must_use]
    pub unsafe fn set_verbosity_level(&self, verbosity_level: i32) -> bool {
        // SAFETY: no arguments beyond a plain integer; `self` is assumed
        // ABI-compatible per this function's own safety contract.
        unsafe { (self.emulator_set_verbosity_level)(verbosity_level) }
    }
}

/// `repr(C)` mirror of the anonymous 2-pointer struct
/// `TvmEulatorEmulateRunMethodResponse` (`{const char *response; const char
/// *log;}`, upstream's own spelling, not a typo introduced here) returned by
/// `tvm_emulator_emulate_run_method_detailed`. Field order matters — it must
/// match the C++ struct's declaration order exactly.
#[repr(C)]
struct RawDetailedResult {
    response: *const c_char,
    log: *const c_char,
}

/// Owned result of [`LibEmulator::emulate_run_method_detailed`] — both
/// buffers already copied out of the library's memory and released.
#[derive(Debug, Clone)]
pub struct DetailedRunMethodResult {
    /// Raw bytes of the `result$_ exit_code:(## 32) gas_used:(## 32)
    /// stack:^VmStack` `BoC` payload (the length prefix itself is stripped —
    /// its only purpose was telling this crate how many bytes to copy).
    pub response_boc: Vec<u8>,
    /// The VM log text produced during emulation.
    pub vm_log: String,
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "docs/ARCHITECTURE.md §3.2 scopes the no-unwrap/expect policy to library code outside tests"
)]
mod tests {
    use std::path::Path;

    use super::{
        EmulatorVersion, LoadError, PINNED_COMMIT, parse_version, pinned_or_refused,
        resolve_library_path,
    };

    #[test]
    fn load_reports_a_readable_error_for_a_missing_file() {
        // SAFETY: exercising the error path only; no library is actually
        // loaded because the path does not exist.
        let result =
            unsafe { super::LibEmulator::load(Path::new("definitely-not-a-real-library.dll")) };
        assert!(matches!(result, Err(LoadError::Library { .. })));
    }

    #[test]
    fn parse_version_accepts_the_real_shape_observed_from_v2026_06() {
        let raw = r#"{"emulatorLibCommitHash":"3f598aa2f0911317b626f4093177241e61798c61","emulatorLibCommitDate":"2026-07-15 13:49:38 +0300"}"#;
        let version: EmulatorVersion = parse_version(raw).expect("parses");
        assert_eq!(
            version.commit_hash,
            "3f598aa2f0911317b626f4093177241e61798c61"
        );
        assert_eq!(version.commit_date, "2026-07-15 13:49:38 +0300");
    }

    #[test]
    fn the_pinned_commit_is_a_real_commit_hash() {
        assert_eq!(PINNED_COMMIT.len(), 40, "pin: {PINNED_COMMIT}");
        assert!(PINNED_COMMIT.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn a_build_other_than_the_pinned_one_is_refused_by_name() {
        let other = EmulatorVersion {
            commit_hash: "0".repeat(40),
            commit_date: "2026-08-17 00:00:00 +0300".to_string(),
        };
        let refusal = pinned_or_refused(other, Path::new("vendor/libemulator/libemulator.dll"))
            .expect_err("a different build is not this build");
        let message = refusal.to_string();
        // Both commits in the message: "wrong build" without saying which is
        // a message nobody can act on.
        assert!(message.contains(PINNED_COMMIT), "{message}");
        assert!(message.contains(&"0".repeat(40)), "{message}");
    }

    #[test]
    fn the_pinned_build_passes_the_same_check() {
        let pinned = EmulatorVersion {
            commit_hash: PINNED_COMMIT.to_string(),
            commit_date: "2026-07-15 13:49:38 +0300".to_string(),
        };
        assert!(pinned_or_refused(pinned, Path::new("vendor/libemulator/libemulator.dll")).is_ok());
    }

    #[test]
    fn parse_version_rejects_plain_garbage_instead_of_panicking() {
        assert!(parse_version("not json at all").is_err());
    }

    #[test]
    fn parse_version_rejects_valid_json_missing_the_expected_fields() {
        // A hypothetical wildly different library that happens to also
        // return JSON, but not this shape — still must be rejected, not
        // silently accepted as "close enough".
        assert!(parse_version(r#"{"unrelated_field": 42}"#).is_err());
    }

    #[test]
    fn resolve_library_path_prefers_the_env_override_when_set() {
        let resolved =
            resolve_library_path(Some("/custom/path/lib.so"), Path::new("/usr/bin/sting"));
        assert_eq!(resolved, Path::new("/custom/path/lib.so"));
    }

    #[test]
    fn resolve_library_path_falls_back_to_exe_directory_plus_platform_filename() {
        let resolved = resolve_library_path(None, Path::new("/usr/local/bin/sting"));
        assert_eq!(
            resolved,
            Path::new("/usr/local/bin").join(super::platform_library_filename())
        );
    }
}
