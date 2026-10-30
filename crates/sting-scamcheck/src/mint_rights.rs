//! Mint-rights signal (`docs/domains/scamcheck.md`):
//! calls TEP-74's `get_jetton_data()` against a jetton master's code+data
//! via [`sting_emulator::TvmEmulator`] and reads `admin_address` — the
//! cheapest and most reliable of the four planned signals, a direct
//! get-method call rather than an inference over indirect evidence.

use sting_emulator::{EmulatorError, TvmEmulator};
use thiserror::Error;
use tonlib_core::tlb_types::block::msg_address::MsgAddress;
use tonlib_core::{TonAddress, TonAddressParseError};

use crate::stack::{self, StackError};

/// TEP-74's `get_jetton_data()` returns `(int total_supply, int mintable,
/// slice admin_address, cell jetton_content, cell jetton_wallet_code)` —
/// `admin_address` is index 2. Ground truth for the exact stack shape:
/// `corpus/fixtures/emulator/SOURCE.md`.
const ADMIN_ADDRESS_STACK_INDEX: u32 = 2;

/// Errors calling or decoding `get_jetton_data()`.
#[derive(Debug, Error)]
pub enum MintRightsError {
    /// The get-method call itself failed at the emulator layer.
    #[error("get_jetton_data call failed: {0}")]
    Emulator(#[source] EmulatorError),
    /// The call completed but did not succeed, or returned no stack — most
    /// likely this contract does not implement `get_jetton_data()` at all
    /// (i.e. it is not a TEP-74 jetton master).
    #[error("get_jetton_data did not succeed: exit_code={exit_code:?} error={error:?}")]
    ContractFailure {
        /// The VM exit code, if the call itself completed.
        exit_code: Option<i32>,
        /// The emulator-reported error, if any.
        error: Option<String>,
    },
    /// The returned stack could not be decoded into `admin_address`.
    #[error("failed to decode get_jetton_data's stack: {0}")]
    Stack(#[source] StackError),
    /// `admin_address` was present but not a shape [`TonAddress`] can
    /// represent (e.g. `addr_extern`) — surfaced rather than silently
    /// treated as either "renounced" or "has an admin".
    #[error("admin_address is not a standard address or addr_none: {0}")]
    UnsupportedAddress(#[source] TonAddressParseError),
}

/// The mint-rights signal: whether the admin can still mint new supply.
#[derive(Debug, Clone)]
pub struct MintRightsSignal {
    /// The admin's standard address, or `None` if mint rights have been
    /// renounced (`admin_address` is `addr_none`).
    pub admin_address: Option<TonAddress>,
}

impl MintRightsSignal {
    /// Whether mint rights have been renounced — the admin can never mint
    /// new supply again.
    #[must_use]
    pub fn renounced(&self) -> bool {
        self.admin_address.is_none()
    }
}

/// Calls `get_jetton_data()` against `emulator` and derives the mint-rights
/// signal from its `admin_address`.
///
/// # Errors
/// See [`MintRightsError`].
pub fn check_mint_rights(emulator: &TvmEmulator<'_>) -> Result<MintRightsSignal, MintRightsError> {
    let response = emulator
        .run_get_method_no_args("get_jetton_data")
        .map_err(MintRightsError::Emulator)?;
    if !response.success || response.vm_exit_code != Some(0) {
        return Err(MintRightsError::ContractFailure {
            exit_code: response.vm_exit_code,
            error: response.error,
        });
    }
    let stack = response
        .stack
        .as_deref()
        .ok_or(MintRightsError::ContractFailure {
            exit_code: response.vm_exit_code,
            error: Some("success response carried no stack field".to_owned()),
        })?;

    let value =
        stack::decode_entry(stack, ADMIN_ADDRESS_STACK_INDEX).map_err(MintRightsError::Stack)?;
    let msg_addr: MsgAddress = value
        .as_msg_address(ADMIN_ADDRESS_STACK_INDEX)
        .map_err(MintRightsError::Stack)?;

    let admin_address = match TonAddress::from_msg_address(msg_addr) {
        Ok(addr) if addr == TonAddress::NULL => None,
        Ok(addr) => Some(addr),
        Err(source) => return Err(MintRightsError::UnsupportedAddress(source)),
    };

    Ok(MintRightsSignal { admin_address })
}
