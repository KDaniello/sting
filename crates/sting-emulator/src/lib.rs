//! Safe RAII wrapper over `sting-emulator-sys`.
//!
//! Covers both handle types `libemulator` exposes: [`TvmEmulator`] (single
//! get-method / message-send calls against one code+data pair) and
//! [`TransactionEmulator`] (full transaction emulation against a
//! `ShardAccount` + inbound `Message`), plus [`EmulatorConfig`] (a
//! pre-parsed blockchain config object, shareable across many calls to
//! either emulator type without re-parsing the config `BoC` each time).
//!
//! `tvm_emulator_emulate_run_method`/`_detailed` (the single-call
//! create+run+destroy optimization) are deliberately **not** wrapped here.
//! The non-detailed variant leaks internally in the C++ implementation (see
//! `sting-emulator-sys`'s module doc comment) and must never be called. The
//! detailed variant is fully bound and memory-safe at the `sting-emulator-sys`
//! layer (see [`sting_emulator_sys::LibEmulator::emulate_run_method_detailed`]),
//! but building its `params_boc` correctly requires serializing a TVM c7
//! tuple by hand — a real TL-B/TVM-internals undertaking with no immediate
//! consumer. Designing that API before a caller has a concrete throughput
//! requirement driving its shape is exactly the unjustified-by-a-caller work
//! `docs/ARCHITECTURE.md` argues against. Deferred rather than forgotten:
//! `docs/domains/emulator.md` §6 carries the reasoning.

pub mod config;
pub mod stack;
pub mod tlb;

pub use config::{ConfigError, PINNED_GLOBAL_VERSION, global_version_of, pinned_global_version_of};

use std::ffi::{CString, c_void};
use std::path::{Path, PathBuf};

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64;
use sting_emulator_sys::{LibEmulator, LoadError};
use tonlib_core::cell::{BagOfCells, Cell, CellBuilder};

/// Loads `libemulator` from `path` — the safe entry point every crate
/// *other than* `sting-emulator`/`sting-emulator-sys` must use.
/// `sting-emulator-sys::LibEmulator::load` is an `unsafe fn` (loading an
/// arbitrary shared library runs its static initializers as a side effect);
/// `docs/ARCHITECTURE.md` §3.1 scopes `unsafe` to exactly these two crates,
/// so a downstream consumer (e.g. `sting-fuzz`'s tests, or its `fuzz/`
/// targets) cannot call that `unsafe fn` directly without its own exemption.
/// This function asserts the same "trust that `path` names a genuine
/// `libemulator` build" contract *once*, here, so every other crate gets a
/// plain safe function instead of needing its own copy of that trust
/// boundary.
///
/// # Errors
/// See [`LoadError`].
pub fn load_library(path: &Path) -> Result<LibEmulator, LoadError> {
    // SAFETY: this is the one designated place outside sting-emulator-sys
    // itself that accepts the "path names a genuine libemulator build"
    // trust boundary — see this function's own doc comment.
    unsafe { LibEmulator::load(path) }
}

/// The library this workspace can actually reach, or `None`.
///
/// `default_library_path` resolves next to the running executable, which is
/// right for a shipped release and wrong from a workspace checkout. The
/// vendored directory is tried after it, for the same reason and with the same
/// shape as `sting_tolkc::TolkCompiler::vendored_or_default`.
///
/// `None` means no library is reachable: `vendor/libemulator/` is gitignored,
/// so a fresh checkout has none.
#[must_use]
pub fn vendored_or_default_library_path() -> Option<PathBuf> {
    if let Ok(path) = LibEmulator::default_library_path()
        && path.is_file()
    {
        return Some(path);
    }
    let vendored = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../vendor/libemulator")
        .join(sting_emulator_sys::platform_library_filename());
    vendored.is_file().then_some(vendored)
}

/// [`load_library`], but resolving the path itself through
/// [`vendored_or_default_library_path`] instead of taking one.
///
/// Every caller that does not name a path goes through here, so the
/// resolution rule has one implementation. A caller keeping its own copy of
/// the rule is how a workspace checkout ends up finding the library from one
/// command and not from the next.
///
/// # Errors
/// See [`LoadError`]. A missing library reports the release path it looked
/// for, since that is the one a shipped binary is expected to have.
pub fn load_default_library() -> Result<LibEmulator, LoadError> {
    match vendored_or_default_library_path() {
        Some(path) => load_library(&path),
        None => load_library(&LibEmulator::default_library_path()?),
    }
}

/// Computes a get-method numeric id from its name, per the TON convention
/// documented at <https://docs.ton.org/tvm/get-method>: `(crc16(name) & 0xffff)
/// | 0x10000`, using CRC-16/XMODEM (poly `0x1021`, init `0x0000`, no
/// reflection, no final XOR).
///
/// Cross-checked against the worked example on that page: `method_id("get_x")
/// == 97865` (see the unit test below).
#[must_use]
pub fn method_id(name: &str) -> i32 {
    let mut crc: u16 = 0x0000;
    for &byte in name.as_bytes() {
        crc ^= u16::from(byte) << 8;
        for _ in 0..8 {
            crc = if crc & 0x8000 == 0 {
                crc << 1
            } else {
                (crc << 1) ^ 0x1021
            };
        }
    }
    (i32::from(crc) & 0xffff) | 0x10000
}

/// Errors constructing inputs for the emulator (as opposed to errors from the
/// emulator call itself, which surface as `success: false` in its JSON reply).
#[derive(Debug, thiserror::Error)]
pub enum BuildError {
    /// Cell construction failed (should not happen for the fixed-shape,
    /// always-in-range values this crate builds).
    #[error("failed to build cell: {0}")]
    Cell(#[source] tonlib_core::cell::TonCellError),
    /// A base64-encoded `BoC` string unexpectedly contained an interior NUL
    /// byte, so it cannot be passed to the C API as a `CStr`. Base64's
    /// alphabet cannot produce NUL, so this should be unreachable in practice.
    #[error("BoC string contained an interior NUL byte: {0}")]
    InteriorNul(#[source] std::ffi::NulError),
}

/// The empty `VmStack` cell (`depth = 0`, no elements): a single cell holding
/// 24 zero bits and no references, per
/// `vm_stack#_ depth:(## 24) stack:(VmStackList depth) = VmStack;` with
/// `vm_stk_nil$_ = VmStackList 0;` contributing no further bits/refs.
fn empty_stack_cell() -> Result<Cell, BuildError> {
    let mut builder = CellBuilder::new();
    builder.store_u32(24, 0).map_err(BuildError::Cell)?;
    builder.build().map_err(BuildError::Cell)
}

/// The base64-encoded `BoC` serialization of `empty_stack_cell`.
///
/// # Errors
/// See [`BuildError`].
pub fn empty_stack_boc() -> Result<String, BuildError> {
    let cell = empty_stack_cell()?;
    let bytes = BagOfCells::from_root(cell)
        .serialize(false)
        .map_err(BuildError::Cell)?;
    Ok(BASE64.encode(bytes))
}

/// The raw JSON shape returned by `tvm_emulator_run_get_method`, per the
/// doc-comment on that function in `emulator/emulator-extern.h`:
/// `{"success": false, "error": "..."}` or `{"success": true, "vm_log": "...",
/// "vm_exit_code": 0, "stack": "...", "missing_library": null, "gas_used": N}`.
#[derive(Debug, serde::Deserialize)]
pub struct RunGetMethodResponse {
    /// Whether the call completed (not whether the *contract* considered
    /// itself successful — that is `vm_exit_code`).
    pub success: bool,
    /// Present when `success` is `false`.
    pub error: Option<String>,
    /// Present when `success` is `true`.
    pub vm_exit_code: Option<i32>,
    /// Present when `success` is `true`: base64-encoded `BoC` of the
    /// resulting `VmStack`. Decoding its contents beyond [`Self::stack_depth`]
    /// is left to callers — `sting-emulator` does not reimplement a full
    /// `VmStack`/`VmStackValue` decoder.
    pub stack: Option<String>,
    /// Present when `success` is `true`.
    pub gas_used: Option<serde_json::Value>,
}

impl RunGetMethodResponse {
    /// Decodes just the 24-bit `depth` prefix of the returned stack `BoC`
    /// (the number of values on the resulting stack), without decoding the
    /// individual `VmStackValue`s themselves.
    ///
    /// # Errors
    /// Returns an error if `stack` is absent or is not a well-formed `BoC`.
    pub fn stack_depth(&self) -> Result<u32, String> {
        let stack = self.stack.as_deref().ok_or("no stack field in response")?;
        let bag = BagOfCells::parse_base64(stack).map_err(|e| e.to_string())?;
        let root = bag.single_root().map_err(|e| e.to_string())?;
        root.parser().load_u32(24).map_err(|e| e.to_string())
    }
}

/// The raw JSON shape returned by `tvm_emulator_send_external_message`/
/// `tvm_emulator_send_internal_message`, per their doc comments in
/// `emulator/emulator-extern.h`.
#[derive(Debug, serde::Deserialize)]
pub struct SendMessageResponse {
    /// Whether the call completed.
    pub success: bool,
    /// Present when `success` is `false`.
    pub error: Option<String>,
    /// Present when `success` is `true`: base64-decoded new code cell.
    pub new_code: Option<String>,
    /// Present when `success` is `true`: base64-decoded new data cell.
    pub new_data: Option<String>,
    /// Present when `success` is `true`: whether the message was accepted.
    pub accepted: Option<bool>,
    /// Present when `success` is `true`.
    pub vm_exit_code: Option<i32>,
    /// Present when `success` is `true`.
    pub vm_log: Option<String>,
    /// Present when `success` is `true`.
    pub gas_used: Option<serde_json::Value>,
    /// Present when `success` is `true`: base64-decoded actions cell
    /// (`OutList n`).
    pub actions: Option<String>,
}

/// The raw JSON shape returned by `transaction_emulator_emulate_transaction`/
/// `transaction_emulator_emulate_tick_tock_transaction`, per their doc
/// comments in `emulator/emulator-extern.h`.
#[derive(Debug, serde::Deserialize)]
pub struct TransactionResponse {
    /// Whether the call completed.
    pub success: bool,
    /// Present when `success` is `false`.
    pub error: Option<String>,
    /// Present when `success` is `false`: whether an external message was
    /// rejected before the compute phase even started.
    pub external_not_accepted: Option<bool>,
    /// Present when `success` is `true`: base64-encoded `Transaction` `BoC`.
    pub transaction: Option<String>,
    /// Present when `success` is `true`: base64-encoded new `ShardAccount` `BoC`.
    pub shard_account: Option<String>,
    /// Present on success, and on the external-not-accepted failure path.
    pub vm_log: Option<String>,
    /// Present when `success` is `true`: base64-encoded compute-phase
    /// actions `BoC` (`OutList n`).
    pub actions: Option<String>,
    /// Present on both success and the external-not-accepted failure path.
    pub elapsed_time: Option<f64>,
    /// Present on the external-not-accepted failure path.
    pub vm_exit_code: Option<i32>,
}

/// Errors from any `sting-emulator` operation. Deliberately grouped by
/// *kind* of failure rather than one variant per one of the ~30 underlying C
/// functions — `{operation}` carries the specific C function name, so
/// diagnostics stay precise without an enum that just mirrors the FFI surface.
#[derive(Debug, thiserror::Error)]
pub enum EmulatorError {
    /// The config handed to this emulation names a global version this tree is
    /// not ruled against, or names none at all.
    #[error("this emulation was given a config it may not run against: {0}")]
    Config(#[source] config::ConfigError),
    /// Input construction failed before any FFI call was made.
    #[error(transparent)]
    Build(#[from] BuildError),
    /// A `CString` could not be built from an input string (see
    /// [`BuildError::InteriorNul`] — should be unreachable in practice for
    /// base64/hex inputs).
    #[error("failed to build C string: {0}")]
    CString(#[source] std::ffi::NulError),
    /// A `*_create` function returned a null pointer (malformed input `BoC`
    /// or internal allocation failure — the C API does not distinguish the two).
    #[error("{what} returned null (malformed input BoC)")]
    CreateFailed {
        /// Name of the C function that failed.
        what: &'static str,
    },
    /// A `*_set_*` function returned `false`.
    #[error("{operation} returned false")]
    SetterFailed {
        /// Name of the C function that failed.
        operation: &'static str,
    },
    /// A call function returned a null pointer.
    #[error("{operation} returned null")]
    CallFailed {
        /// Name of the C function that failed.
        operation: &'static str,
    },
    /// The response was not valid JSON, or not shaped as documented in
    /// `emulator/emulator-extern.h`.
    #[error("failed to parse emulator response as JSON: {0}")]
    Json(#[source] serde_json::Error),
}

fn cstring(s: &str) -> Result<CString, EmulatorError> {
    CString::new(s).map_err(EmulatorError::CString)
}

/// Refuses a config whose global version this tree has not been ruled against.
///
/// Every path that hands a config to `libemulator` goes through here, and that
/// is the whole point: a version checked in one entry point and not in the next
/// is a version nobody checked. Emulating semantics without saying which is the
/// same defect as parsing a file without saying it failed.
fn refuse_unpinned(config_boc: &str) -> Result<(), EmulatorError> {
    config::pinned_global_version_of(config_boc)
        .map(|_| ())
        .map_err(EmulatorError::Config)
}

/// A pre-parsed blockchain config object (`emulator_config_create`), shared
/// across many [`TvmEmulator::set_config_object`]/
/// [`TransactionEmulator::set_config_object`] calls without re-parsing the
/// config `BoC` on every call — the natural pattern when many emulations run
/// against the same fixed config.
pub struct EmulatorConfig<'lib> {
    lib: &'lib LibEmulator,
    handle: *mut c_void,
}

impl<'lib> EmulatorConfig<'lib> {
    /// Parses `config_params_boc` (base64-encoded `BoC` of a `Hashmap 32
    /// ^Cell` config dictionary) into a reusable config object.
    ///
    /// # Errors
    /// See [`EmulatorError`].
    pub fn new(lib: &'lib LibEmulator, config_params_boc: &str) -> Result<Self, EmulatorError> {
        refuse_unpinned(config_params_boc)?;
        let config_c = cstring(config_params_boc)?;
        // SAFETY: `lib` is a live `LibEmulator`; `config_c` is a valid
        // NUL-terminated C string for the duration of this call.
        let handle = unsafe { lib.create_emulator_config(&config_c) }.ok_or(
            EmulatorError::CreateFailed {
                what: "emulator_config_create",
            },
        )?;
        Ok(Self { lib, handle })
    }
}

impl Drop for EmulatorConfig<'_> {
    fn drop(&mut self) {
        // SAFETY: `self.handle` was created by `self.lib` in `new` and is
        // destroyed here exactly once, per the `Drop` contract.
        unsafe { self.lib.destroy_emulator_config(self.handle) };
    }
}

/// A safe, RAII-managed TVM emulator instance: [`Drop`] calls
/// `tvm_emulator_destroy` exactly once.
pub struct TvmEmulator<'lib> {
    lib: &'lib LibEmulator,
    handle: *mut c_void,
}

impl<'lib> TvmEmulator<'lib> {
    /// Creates a TVM emulator instance from base64-encoded code/data cell `BoC`s.
    ///
    /// # Errors
    /// See [`EmulatorError`].
    pub fn new(
        lib: &'lib LibEmulator,
        code_boc_base64: &str,
        data_boc_base64: &str,
    ) -> Result<Self, EmulatorError> {
        let code = cstring(code_boc_base64)?;
        let data = cstring(data_boc_base64)?;
        // SAFETY: `lib` is a live `LibEmulator`; `code`/`data` are valid
        // NUL-terminated C strings for the duration of this call.
        let handle = unsafe { lib.create_tvm_emulator(&code, &data, 0) }.ok_or(
            EmulatorError::CreateFailed {
                what: "tvm_emulator_create",
            },
        )?;
        Ok(Self { lib, handle })
    }

    /// Sets shared libraries (`HashmapE 256 ^Cell`, base64-encoded)
    /// resolvable during execution.
    ///
    /// # Errors
    /// See [`EmulatorError`].
    pub fn set_libraries(&self, libs_boc: &str) -> Result<(), EmulatorError> {
        let libs_c = cstring(libs_boc)?;
        // SAFETY: `self.handle` is live (see `run_get_method`'s safety note
        // for why `&self` rules out a concurrent `Drop`); `libs_c` is a valid
        // C string for the call.
        let ok = unsafe { self.lib.set_tvm_libraries(self.handle, &libs_c) };
        ok.then_some(()).ok_or(EmulatorError::SetterFailed {
            operation: "tvm_emulator_set_libraries",
        })
    }

    /// Sets c7 context (own address, unixtime, balance; random seed defaults
    /// to all-zero) on this emulator instance, and optionally a blockchain
    /// config `BoC` (`config_boc`) — many get-methods rely on this: `MYADDR`/
    /// `config.*` opcodes read from c7 and fail (VM exit code 9, "cannot
    /// parse a `MsgAddress`") if it was never set, discovered the hard way
    /// against a real jetton master whose `get_jetton_data()` embeds its own
    /// address into a
    /// generated content URL. Call this before [`Self::run_get_method`] for
    /// any method that might need it; harmless to call even if it turns out
    /// not to be needed.
    ///
    /// `address` is the contract's raw address (`"<workchain>:<64 hex
    /// chars>"`, e.g. `"0:000011a5...ae6"`).
    ///
    /// # Errors
    /// See [`EmulatorError`].
    pub fn set_c7(
        &self,
        address: &str,
        unixtime: u32,
        balance: u64,
        config_boc: Option<&str>,
    ) -> Result<(), EmulatorError> {
        if let Some(config_boc) = config_boc {
            refuse_unpinned(config_boc)?;
        }
        let address_c = cstring(address)?;
        let rand_seed_c = cstring(&"0".repeat(64))?;
        let config_c = config_boc.map(cstring).transpose()?;
        // SAFETY: `self.handle` was created by `self.lib` in `new` and has
        // not yet been destroyed (see `run_get_method`'s safety note).
        // `address_c`/`rand_seed_c`/`config_c` are valid C strings for the call.
        let ok = unsafe {
            self.lib.set_c7(
                self.handle,
                &address_c,
                unixtime,
                balance,
                &rand_seed_c,
                config_c.as_deref(),
            )
        };
        ok.then_some(()).ok_or(EmulatorError::SetterFailed {
            operation: "tvm_emulator_set_c7",
        })
    }

    /// Sets extra-currency balances (`"currency_id1=balance1 ..."` format).
    ///
    /// # Errors
    /// See [`EmulatorError`].
    pub fn set_extra_currencies(&self, extra_currencies: &str) -> Result<(), EmulatorError> {
        let currencies_c = cstring(extra_currencies)?;
        // SAFETY: `self.handle` is live; `currencies_c` is a valid C string
        // for the call.
        let ok = unsafe {
            self.lib
                .set_tvm_extra_currencies(self.handle, &currencies_c)
        };
        ok.then_some(()).ok_or(EmulatorError::SetterFailed {
            operation: "tvm_emulator_set_extra_currencies",
        })
    }

    /// Sets a pre-parsed [`EmulatorConfig`] as this emulator's blockchain
    /// config, avoiding re-parsing the config `BoC` on every call.
    ///
    /// # Errors
    /// See [`EmulatorError`].
    pub fn set_config_object(&self, config: &EmulatorConfig<'lib>) -> Result<(), EmulatorError> {
        // SAFETY: `self.handle` is live; `config.handle` is live (borrowed
        // for the call's duration, and `EmulatorConfig` outlives `'lib`
        // alongside this `TvmEmulator`).
        let ok = unsafe { self.lib.set_tvm_config_object(self.handle, config.handle) };
        ok.then_some(()).ok_or(EmulatorError::SetterFailed {
            operation: "tvm_emulator_set_config_object",
        })
    }

    /// Sets the tuple of previous blocks (13th element of c7).
    ///
    /// # Errors
    /// See [`EmulatorError`].
    pub fn set_prev_blocks_info(&self, info_boc: &str) -> Result<(), EmulatorError> {
        let info_c = cstring(info_boc)?;
        // SAFETY: `self.handle` is live; `info_c` is a valid C string for the call.
        let ok = unsafe { self.lib.set_tvm_prev_blocks_info(self.handle, &info_c) };
        ok.then_some(()).ok_or(EmulatorError::SetterFailed {
            operation: "tvm_emulator_set_prev_blocks_info",
        })
    }

    /// Sets the TVM gas limit.
    ///
    /// # Errors
    /// See [`EmulatorError`].
    pub fn set_gas_limit(&self, gas_limit: i64) -> Result<(), EmulatorError> {
        // SAFETY: `self.handle` is live.
        let ok = unsafe { self.lib.set_tvm_gas_limit(self.handle, gas_limit) };
        ok.then_some(()).ok_or(EmulatorError::SetterFailed {
            operation: "tvm_emulator_set_gas_limit",
        })
    }

    /// Enables or disables TVM debug primitives.
    ///
    /// # Errors
    /// See [`EmulatorError`].
    pub fn set_debug_enabled(&self, enabled: bool) -> Result<(), EmulatorError> {
        // SAFETY: `self.handle` is live.
        let ok = unsafe { self.lib.set_tvm_debug_enabled(self.handle, enabled) };
        ok.then_some(()).ok_or(EmulatorError::SetterFailed {
            operation: "tvm_emulator_set_debug_enabled",
        })
    }

    /// Runs a get-method by numeric id (see [`method_id`]) against this
    /// emulator instance, with the given base64-encoded `VmStack` `BoC` as
    /// the argument stack.
    ///
    /// # Errors
    /// See [`EmulatorError`].
    pub fn run_get_method(
        &self,
        method_id: i32,
        stack_boc_base64: &str,
    ) -> Result<RunGetMethodResponse, EmulatorError> {
        let stack_c = cstring(stack_boc_base64)?;
        // SAFETY: `self.handle` was created by this same `self.lib` in `new`
        // and has not yet been destroyed (destruction only happens in `Drop`,
        // which requires `&mut self`/ownership, so cannot have run yet while
        // `&self` is held here). `stack_c` is a valid C string for the call.
        let raw = unsafe { self.lib.run_get_method(self.handle, method_id, &stack_c) }.ok_or(
            EmulatorError::CallFailed {
                operation: "tvm_emulator_run_get_method",
            },
        )?;
        serde_json::from_str(&raw).map_err(EmulatorError::Json)
    }

    /// Runs a get-method by name (see [`method_id`]) against this emulator
    /// instance, with an empty argument stack (see [`empty_stack_boc`]).
    ///
    /// # Errors
    /// See [`EmulatorError`].
    pub fn run_get_method_no_args(
        &self,
        method_name: &str,
    ) -> Result<RunGetMethodResponse, EmulatorError> {
        let stack = empty_stack_boc()?;
        self.run_get_method(method_id(method_name), &stack)
    }

    /// Sends an external message body (base64-encoded `BoC`) to this contract.
    ///
    /// # Errors
    /// See [`EmulatorError`].
    pub fn send_external_message(
        &self,
        message_body_boc_base64: &str,
    ) -> Result<SendMessageResponse, EmulatorError> {
        let body_c = cstring(message_body_boc_base64)?;
        // SAFETY: `self.handle` is live; `body_c` is a valid C string for the call.
        let raw = unsafe { self.lib.send_external_message(self.handle, &body_c) }.ok_or(
            EmulatorError::CallFailed {
                operation: "tvm_emulator_send_external_message",
            },
        )?;
        serde_json::from_str(&raw).map_err(EmulatorError::Json)
    }

    /// Sends an internal message body (base64-encoded `BoC`) with `amount`
    /// nanotons attached to this contract.
    ///
    /// # Errors
    /// See [`EmulatorError`].
    pub fn send_internal_message(
        &self,
        message_body_boc_base64: &str,
        amount: u64,
    ) -> Result<SendMessageResponse, EmulatorError> {
        let body_c = cstring(message_body_boc_base64)?;
        // SAFETY: `self.handle` is live; `body_c` is a valid C string for the call.
        let raw = unsafe { self.lib.send_internal_message(self.handle, &body_c, amount) }.ok_or(
            EmulatorError::CallFailed {
                operation: "tvm_emulator_send_internal_message",
            },
        )?;
        serde_json::from_str(&raw).map_err(EmulatorError::Json)
    }
}

impl Drop for TvmEmulator<'_> {
    fn drop(&mut self) {
        // SAFETY: `self.handle` was created by `self.lib` in `new` and is
        // destroyed here exactly once, per the `Drop` contract (this method
        // cannot run twice for the same value).
        unsafe { self.lib.destroy_tvm_emulator(self.handle) };
    }
}

/// A safe, RAII-managed `TransactionEmulator` instance: [`Drop`] calls
/// `transaction_emulator_destroy` exactly once.
pub struct TransactionEmulator<'lib> {
    lib: &'lib LibEmulator,
    handle: *mut c_void,
}

impl<'lib> TransactionEmulator<'lib> {
    /// Creates a transaction emulator from a base64-encoded blockchain
    /// config `BoC`.
    ///
    /// # Errors
    /// See [`EmulatorError`].
    pub fn new(
        lib: &'lib LibEmulator,
        config_params_boc: &str,
        vm_log_verbosity: i32,
    ) -> Result<Self, EmulatorError> {
        refuse_unpinned(config_params_boc)?;
        let config_c = cstring(config_params_boc)?;
        // SAFETY: `lib` is a live `LibEmulator`; `config_c` is a valid
        // NUL-terminated C string for the duration of this call.
        let handle = unsafe { lib.create_transaction_emulator(&config_c, vm_log_verbosity) }
            .ok_or(EmulatorError::CreateFailed {
                what: "transaction_emulator_create",
            })?;
        Ok(Self { lib, handle })
    }

    /// Sets the unix timestamp used for emulation.
    ///
    /// # Errors
    /// See [`EmulatorError`].
    pub fn set_unixtime(&self, unixtime: u32) -> Result<(), EmulatorError> {
        // SAFETY: `self.handle` is live (see `TvmEmulator::run_get_method`'s
        // safety note for why `&self` rules out a concurrent `Drop`).
        let ok = unsafe { self.lib.set_unixtime(self.handle, unixtime) };
        ok.then_some(()).ok_or(EmulatorError::SetterFailed {
            operation: "transaction_emulator_set_unixtime",
        })
    }

    /// Sets the logical time used for emulation.
    ///
    /// # Errors
    /// See [`EmulatorError`].
    pub fn set_lt(&self, lt: u64) -> Result<(), EmulatorError> {
        // SAFETY: `self.handle` is live.
        let ok = unsafe { self.lib.set_lt(self.handle, lt) };
        ok.then_some(()).ok_or(EmulatorError::SetterFailed {
            operation: "transaction_emulator_set_lt",
        })
    }

    /// Sets the random seed (64 hex chars) used for emulation.
    ///
    /// # Errors
    /// See [`EmulatorError`].
    pub fn set_rand_seed(&self, rand_seed_hex: &str) -> Result<(), EmulatorError> {
        let rand_seed_c = cstring(rand_seed_hex)?;
        // SAFETY: `self.handle` is live; `rand_seed_c` is a valid C string
        // for the call.
        let ok = unsafe { self.lib.set_rand_seed(self.handle, &rand_seed_c) };
        ok.then_some(()).ok_or(EmulatorError::SetterFailed {
            operation: "transaction_emulator_set_rand_seed",
        })
    }

    /// Sets whether emulation should always succeed on `CHKSIG`.
    ///
    /// # Errors
    /// See [`EmulatorError`].
    pub fn set_ignore_chksig(&self, ignore_chksig: bool) -> Result<(), EmulatorError> {
        // SAFETY: `self.handle` is live.
        let ok = unsafe { self.lib.set_ignore_chksig(self.handle, ignore_chksig) };
        ok.then_some(()).ok_or(EmulatorError::SetterFailed {
            operation: "transaction_emulator_set_ignore_chksig",
        })
    }

    /// Sets the blockchain config (base64-encoded `BoC`) used for emulation.
    ///
    /// # Errors
    /// See [`EmulatorError`].
    pub fn set_config(&self, config_boc: &str) -> Result<(), EmulatorError> {
        refuse_unpinned(config_boc)?;
        let config_c = cstring(config_boc)?;
        // SAFETY: `self.handle` is live; `config_c` is a valid C string for the call.
        let ok = unsafe { self.lib.set_config(self.handle, &config_c) };
        ok.then_some(()).ok_or(EmulatorError::SetterFailed {
            operation: "transaction_emulator_set_config",
        })
    }

    /// Sets a pre-parsed [`EmulatorConfig`] as this emulator's config,
    /// avoiding re-parsing the config `BoC` on every call.
    ///
    /// # Errors
    /// See [`EmulatorError`].
    pub fn set_config_object(&self, config: &EmulatorConfig<'lib>) -> Result<(), EmulatorError> {
        // SAFETY: `self.handle` is live; `config.handle` is live (borrowed
        // for the call's duration).
        let ok = unsafe {
            self.lib
                .set_transaction_config_object(self.handle, config.handle)
        };
        ok.then_some(()).ok_or(EmulatorError::SetterFailed {
            operation: "transaction_emulator_set_config_object",
        })
    }

    /// Sets shared libraries (`HashmapE 256 ^Cell`, base64-encoded) for emulation.
    ///
    /// # Errors
    /// See [`EmulatorError`].
    pub fn set_libs(&self, libs_boc: &str) -> Result<(), EmulatorError> {
        let libs_c = cstring(libs_boc)?;
        // SAFETY: `self.handle` is live; `libs_c` is a valid C string for the call.
        let ok = unsafe { self.lib.set_libs(self.handle, &libs_c) };
        ok.then_some(()).ok_or(EmulatorError::SetterFailed {
            operation: "transaction_emulator_set_libs",
        })
    }

    /// Enables or disables TVM debug primitives for transaction emulation.
    ///
    /// # Errors
    /// See [`EmulatorError`].
    pub fn set_debug_enabled(&self, enabled: bool) -> Result<(), EmulatorError> {
        // SAFETY: `self.handle` is live.
        let ok = unsafe { self.lib.set_transaction_debug_enabled(self.handle, enabled) };
        ok.then_some(()).ok_or(EmulatorError::SetterFailed {
            operation: "transaction_emulator_set_debug_enabled",
        })
    }

    /// Sets the tuple of previous blocks (13th element of c7).
    ///
    /// # Errors
    /// See [`EmulatorError`].
    pub fn set_prev_blocks_info(&self, info_boc: &str) -> Result<(), EmulatorError> {
        let info_c = cstring(info_boc)?;
        // SAFETY: `self.handle` is live; `info_c` is a valid C string for the call.
        let ok = unsafe {
            self.lib
                .set_transaction_prev_blocks_info(self.handle, &info_c)
        };
        ok.then_some(()).ok_or(EmulatorError::SetterFailed {
            operation: "transaction_emulator_set_prev_blocks_info",
        })
    }

    /// Emulates a transaction: `shard_account_boc` (base64-encoded
    /// `ShardAccount`) receiving `message_boc` (base64-encoded internal or
    /// external `Message`).
    ///
    /// # Errors
    /// See [`EmulatorError`].
    pub fn emulate_transaction(
        &self,
        shard_account_boc: &str,
        message_boc: &str,
    ) -> Result<TransactionResponse, EmulatorError> {
        let shard_account_c = cstring(shard_account_boc)?;
        let message_c = cstring(message_boc)?;
        // SAFETY: `self.handle` is live; both `BoC` arguments are valid C
        // strings for the call.
        let raw = unsafe {
            self.lib
                .emulate_transaction(self.handle, &shard_account_c, &message_c)
        }
        .ok_or(EmulatorError::CallFailed {
            operation: "transaction_emulator_emulate_transaction",
        })?;
        serde_json::from_str(&raw).map_err(EmulatorError::Json)
    }

    /// Emulates a tick/tock transaction on a special account.
    ///
    /// # Errors
    /// See [`EmulatorError`].
    pub fn emulate_tick_tock_transaction(
        &self,
        shard_account_boc: &str,
        is_tock: bool,
    ) -> Result<TransactionResponse, EmulatorError> {
        let shard_account_c = cstring(shard_account_boc)?;
        // SAFETY: `self.handle` is live; `shard_account_c` is a valid C
        // string for the call.
        let raw = unsafe {
            self.lib
                .emulate_tick_tock_transaction(self.handle, &shard_account_c, is_tock)
        }
        .ok_or(EmulatorError::CallFailed {
            operation: "transaction_emulator_emulate_tick_tock_transaction",
        })?;
        serde_json::from_str(&raw).map_err(EmulatorError::Json)
    }
}

impl Drop for TransactionEmulator<'_> {
    fn drop(&mut self) {
        // SAFETY: `self.handle` was created by `self.lib` in `new` and is
        // destroyed here exactly once, per the `Drop` contract.
        unsafe { self.lib.destroy_transaction_emulator(self.handle) };
    }
}

/// Sets the library's global verbosity level (0 = never, 4 = debug). Affects
/// every emulator instance process-wide, not just one handle — a free
/// function rather than a method on [`TvmEmulator`]/[`TransactionEmulator`]
/// because it has no per-instance receiver in the C API either.
#[must_use]
pub fn set_verbosity_level(lib: &LibEmulator, level: i32) -> bool {
    // SAFETY: `lib` is a live `LibEmulator`; the call takes no pointer
    // arguments beyond `self`/a plain integer.
    unsafe { lib.set_verbosity_level(level) }
}

#[cfg(test)]
mod tests {
    use super::method_id;

    #[test]
    fn method_id_matches_the_worked_example_from_ton_docs() {
        // https://docs.ton.org/tvm/get-method: method_id("get_x") == 97865.
        assert_eq!(method_id("get_x"), 97_865);
    }

    #[test]
    fn method_id_get_jetton_data_matches_independently_computed_value() {
        // Cross-checked in Python against the same CRC-16/XMODEM parameters
        // (docs/ARCHITECTURE.md).
        assert_eq!(method_id("get_jetton_data"), 106_029);
    }
}
