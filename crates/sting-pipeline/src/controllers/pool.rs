//! `EmulationPoolController` — `docs/ARCHITECTURE.md`
//!
//! Tier 1, in-process, only: a full Tier 2 for `sting scan` is a separate
//! piece of work. `BridgeController` runs against a corpus we control — our
//! own synthetic and testnet fixtures — rather than an arbitrary untrusted
//! address, which is exactly the case that is safe for
//! Tier 1's lighter defense (`gas_limit` as the primary guard, no subprocess
//! isolation).
//!
//! Every logical unit of work (one `send_internal_message` call, one
//! get-method call) gets a **freshly created** `TvmEmulator` instance,
//! matching §4.1's conservative default (ABI thread-safety of a *shared*
//! handle across calls is undocumented upstream) — nothing here is a "pool"
//! of reused handles in the literal sense; the name is kept for consistency
//! with the other controllers, and Tier 1 has nothing to pool.

use sting_emulator::TvmEmulator;
use sting_emulator_sys::LibEmulator;

/// Normalized emulation input: one contract instance's code+data plus the
/// address it is emulated as. Deliberately a distinct type from
/// `sting_corpus::ContractCode` (which `CorpusController` resolves to) —
/// `EmulationTarget` additionally carries the address `set_c7` needs, which
/// `ContractCode` alone does not (an address is not "the contract's code",
/// it's *where* it is being emulated).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EmulationTarget {
    /// Base64-encoded code cell `BoC`.
    pub code_boc_base64: String,
    /// Base64-encoded data cell `BoC`.
    pub data_boc_base64: String,
    /// The contract's own raw address (`"<workchain>:<64 hex chars>"`),
    /// used for `set_c7`'s `MYADDR`.
    pub address: String,
}

impl EmulationTarget {
    /// A target identical to `self` but with `data_boc_base64` replaced —
    /// used to chain a follow-up call against the state a previous
    /// `send_internal_message` produced (its `new_data`), without needing a
    /// single `TvmEmulator` instance to stay alive across two separate
    /// `EmulatorProvider` calls — which would not help anyway, since
    /// `new_data` is already the full post-call state.
    #[must_use]
    pub fn with_data(&self, data_boc_base64: String) -> Self {
        Self {
            code_boc_base64: self.code_boc_base64.clone(),
            data_boc_base64,
            address: self.address.clone(),
        }
    }
}

/// Outcome of [`EmulatorProvider::send_internal_message`], reduced to the
/// fields `BridgeController`'s strategies actually branch on — not a
/// re-export of `sting_emulator::SendMessageResponse` directly, so the
/// provider trait's contract does not silently change shape if that type
/// grows fields not every provider implementation can supply the same way
/// (e.g. a future Tier 2 subprocess provider communicating over a narrower
/// wire protocol).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SendOutcome {
    /// Whether the emulator call itself completed (not the same as the
    /// *contract* accepting the message — see [`Self::accepted`]).
    pub success: bool,
    /// Whether the contract accepted the message (did not immediately reject
    /// it before any state change).
    pub accepted: bool,
    /// The VM's own exit code, when `success` is `true`.
    pub vm_exit_code: Option<i32>,
    /// The resulting data cell `BoC`, when `success` is `true` — feed this
    /// into [`EmulationTarget::with_data`] for a follow-up call against the
    /// post-message state.
    pub new_data_boc_base64: Option<String>,
    /// Whether the response carried any queued output actions (e.g. an
    /// outbound message) — `UBM`'s strategy checks this specifically.
    pub has_actions: bool,
}

/// Outcome of [`EmulatorProvider::run_get_method`], reduced the same way as
/// [`SendOutcome`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GetMethodOutcome {
    /// Whether the call completed.
    pub success: bool,
    /// The VM's own exit code, when `success` is `true`.
    pub vm_exit_code: Option<i32>,
    /// The raw returned `VmStack` `BoC`, when `success` is `true` — decode
    /// with [`crate::controllers::bridge::decode_stack_ints`] for the
    /// fixed-shape small-integer results `BridgeController`'s strategies expect.
    pub stack_boc_base64: Option<String>,
}

/// Errors from an [`EmulatorProvider`] call.
#[derive(Debug, thiserror::Error)]
pub enum EmulationError {
    /// The `TvmEmulator` instance could not be constructed (malformed code/data).
    #[error("failed to construct emulator instance: {0}")]
    Create(#[source] sting_emulator::EmulatorError),
    /// `set_c7` failed (malformed address).
    #[error("failed to set c7 context: {0}")]
    SetC7(#[source] sting_emulator::EmulatorError),
    /// `send_internal_message` failed.
    #[error("send_internal_message failed: {0}")]
    Send(#[source] sting_emulator::EmulatorError),
    /// The get-method call failed.
    #[error("get-method call failed: {0}")]
    GetMethod(#[source] sting_emulator::EmulatorError),
}

/// A provider of emulation calls — the mocking boundary
/// `docs/ARCHITECTURE.md` principle 1 calls for by name
/// (`&dyn EmulatorProvider`): production code uses [`EmulationPoolController`];
/// `BridgeController`'s own tests substitute a fake with scripted responses,
/// never a real `libemulator`.
///
/// Deliberately narrower than the illustrative sketch this started from
/// (`run_get_method` with an arbitrary `StackValue` argument list, plus
/// `emulate_transaction`): every `BridgeController` strategy implemented in
/// only ever needs a no-argument get-method call and a
/// `send_internal_message` call, and `emulate_transaction` specifically is
/// not reachable without a `ShardAccount` encoder that is not built here
/// (`docs/domains/emulator.md` §6). A trait with methods no
/// caller uses yet is exactly the kind of speculative surface
/// `docs/ARCHITECTURE.md` argues against elsewhere in this project.
pub trait EmulatorProvider: Send + Sync {
    /// Sends an internal message body (with `amount_nanotons` attached) to `target`.
    ///
    /// # Errors
    /// See [`EmulationError`].
    fn send_internal_message(
        &self,
        target: &EmulationTarget,
        body_boc_base64: &str,
        amount_nanotons: u64,
    ) -> Result<SendOutcome, EmulationError>;

    /// Runs a no-argument get-method against `target`.
    ///
    /// # Errors
    /// See [`EmulationError`].
    fn run_get_method(
        &self,
        target: &EmulationTarget,
        method_name: &str,
    ) -> Result<GetMethodOutcome, EmulationError>;

    /// Checks whether `target` is structurally valid TVM state — the code
    /// and data cells parse and a `TvmEmulator` instance can actually be
    /// constructed and given a `c7` context — without sending any message or
    /// calling any get-method (`docs/API.md`'s `sting
    /// scan`: the one thing that can be checked about an arbitrary,
    /// unknown-protocol on-chain address without guessing at its message
    /// dispatch convention or get-method names, and that guess is not
    /// attempted here either — `docs/domains/emulator.md` §6).
    ///
    /// # Errors
    /// [`EmulationError::Create`]/[`EmulationError::SetC7`] specifically —
    /// never [`EmulationError::Send`]/[`EmulationError::GetMethod`], since
    /// this never reaches either call.
    fn check_structural_validity(&self, target: &EmulationTarget) -> Result<(), EmulationError>;
}

/// Unix timestamp / balance passed to every `set_c7` call this controller
/// makes — fixed, not fuzzed or derived from `target`: every
/// `BridgeController` strategy's hypothesis is about a *storage/action*
/// effect, never about time- or balance-dependent branching, so a stable
/// constant keeps every verification run reproducible without adding a
/// parameter no current strategy needs.
const FIXED_UNIXTIME: u32 = 1_784_240_154;
const FIXED_BALANCE: u64 = 1_000_000_000;

/// The Tier 1 (in-process) [`EmulatorProvider`] — real `libemulator` calls,
/// a fresh `TvmEmulator` per call (see this module's doc comment for why).
pub struct EmulationPoolController<'lib> {
    lib: &'lib LibEmulator,
}

impl<'lib> EmulationPoolController<'lib> {
    /// Builds a controller against an already-loaded `libemulator`.
    #[must_use]
    pub fn new(lib: &'lib LibEmulator) -> Self {
        Self { lib }
    }
}

impl EmulatorProvider for EmulationPoolController<'_> {
    #[tracing::instrument(level = "debug", skip(self, body_boc_base64), fields(address = %target.address))]
    fn send_internal_message(
        &self,
        target: &EmulationTarget,
        body_boc_base64: &str,
        amount_nanotons: u64,
    ) -> Result<SendOutcome, EmulationError> {
        let emulator = TvmEmulator::new(self.lib, &target.code_boc_base64, &target.data_boc_base64)
            .map_err(EmulationError::Create)?;
        emulator
            .set_c7(&target.address, FIXED_UNIXTIME, FIXED_BALANCE, None)
            .map_err(EmulationError::SetC7)?;
        let response = emulator
            .send_internal_message(body_boc_base64, amount_nanotons)
            .map_err(EmulationError::Send)?;
        tracing::debug!(
            success = response.success,
            vm_exit_code = response.vm_exit_code,
            "send_internal_message completed"
        );
        Ok(SendOutcome {
            success: response.success,
            accepted: response.accepted.unwrap_or(false),
            vm_exit_code: response.vm_exit_code,
            new_data_boc_base64: response.new_data,
            has_actions: response.actions.is_some(),
        })
    }

    #[tracing::instrument(level = "debug", skip(self), fields(address = %target.address))]
    fn run_get_method(
        &self,
        target: &EmulationTarget,
        method_name: &str,
    ) -> Result<GetMethodOutcome, EmulationError> {
        let emulator = TvmEmulator::new(self.lib, &target.code_boc_base64, &target.data_boc_base64)
            .map_err(EmulationError::Create)?;
        emulator
            .set_c7(&target.address, FIXED_UNIXTIME, FIXED_BALANCE, None)
            .map_err(EmulationError::SetC7)?;
        let response = emulator
            .run_get_method_no_args(method_name)
            .map_err(EmulationError::GetMethod)?;
        tracing::debug!(
            success = response.success,
            vm_exit_code = response.vm_exit_code,
            "run_get_method completed"
        );
        Ok(GetMethodOutcome {
            success: response.success,
            vm_exit_code: response.vm_exit_code,
            stack_boc_base64: response.stack,
        })
    }

    #[tracing::instrument(level = "debug", skip(self), fields(address = %target.address))]
    fn check_structural_validity(&self, target: &EmulationTarget) -> Result<(), EmulationError> {
        let emulator = TvmEmulator::new(self.lib, &target.code_boc_base64, &target.data_boc_base64)
            .map_err(EmulationError::Create)?;
        emulator
            .set_c7(&target.address, FIXED_UNIXTIME, FIXED_BALANCE, None)
            .map_err(EmulationError::SetC7)?;
        tracing::debug!("structural validity check passed");
        Ok(())
    }
}
