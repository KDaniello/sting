//! The wind tunnel's executor port, over a real virtual machine.
//!
//! A mechanism in the tunnel is a model of a venue, and the venue itself is a contract. Whether
//! the two agree is the only question that makes a simulation worth running, and answering it
//! needs both to be reachable from one place: this crate is the second half of that, a contract
//! answering the same way the model does.
//!
//! What crosses the boundary is a number. The core's port asks for one integer off a get-method's
//! stack, so nothing above it learns about cells, stacks or a virtual machine — a mechanism that
//! decoded a cell would be a mechanism that knows which chain it is on, and the tunnel is
//! deliberately not about one chain.
//!
//! Determinism survives the addition. A get-method against a fixed account state answers the same
//! way every time it is asked, so a run that consults a contract still reproduces from its seed —
//! which is the property the whole wind tunnel rests on.
//!
//! # What this crate does not do
//!
//! It sends no message and applies no transaction: a get-method reads, and a run that could
//! change the state it is reading from would not be reproducible. It fetches no account from a
//! chain — the code and data it runs against are handed to it, so a run depends on a checked-in
//! state rather than on what mainnet happens to hold today.

use sting_emulator::stack::{self, StackError};
use sting_emulator::{EmulatorError, TvmEmulator};
use sting_emulator_sys::LibEmulator;
use sting_karman_core::ports::{ExecutorError, TvmExecutor};

/// What a contract is allowed to know about the world it runs in.
///
/// Real contracts read their own address, the clock and their balance, and one that reads a
/// context it was never given fails at the first instruction that needs it. Supplied rather than
/// invented, and fixed rather than taken from the machine's clock: a run that read the wall clock
/// would answer differently tomorrow.
#[derive(Debug, Clone)]
pub struct ContractContext {
    /// The contract's own address, in the raw `workchain:hex` form.
    pub address_hex: String,
    /// The clock the contract sees, in epoch seconds.
    pub unixtime_secs: u32,
    /// The balance it sees, in nanotons.
    pub balance_nanoton: u64,
}

/// A contract at a fixed state, answering get-methods.
pub struct EmulatedContract<'lib> {
    emulator: TvmEmulator<'lib>,
}

impl<'lib> EmulatedContract<'lib> {
    /// Loads a contract from its code and data, as base64 bags of cells, and tells it where and
    /// when it is.
    ///
    /// # Errors
    /// [`EmulatorError`] when the virtual machine refuses to build an instance from them, or
    /// refuses the context.
    pub fn new(
        lib: &'lib LibEmulator,
        code_boc_base64: &str,
        data_boc_base64: &str,
        context: &ContractContext,
    ) -> Result<Self, EmulatorError> {
        let emulator = TvmEmulator::new(lib, code_boc_base64, data_boc_base64)?;
        emulator.set_c7(
            &context.address_hex,
            context.unixtime_secs,
            context.balance_nanoton,
            None,
        )?;
        Ok(Self { emulator })
    }
}

impl TvmExecutor for EmulatedContract<'_> {
    fn call_number(&self, method: &str, index: u32) -> Result<u128, ExecutorError> {
        let response = self
            .emulator
            .run_get_method_no_args(method)
            .map_err(|err| ExecutorError::new(format!("{method} could not be run: {err}")))?;

        if !response.success || response.vm_exit_code != Some(0) {
            return Err(ExecutorError::new(format!(
                "{method} exited with {exit:?}{detail}",
                exit = response.vm_exit_code,
                detail = response
                    .error
                    .as_deref()
                    .map(|text| format!(": {text}"))
                    .unwrap_or_default(),
            )));
        }

        let stack_boc = response.stack.as_deref().ok_or_else(|| {
            ExecutorError::new(format!("{method} succeeded and returned no stack"))
        })?;
        let value = stack::decode_entry(stack_boc, index)
            .map_err(|err| number_refused(method, index, &err))?;
        let number = value
            .as_int(index)
            .map_err(|err| number_refused(method, index, &err))?;

        u128::try_from(number).map_err(|_| {
            ExecutorError::new(format!(
                "{method} answered {number} at position {index}, and a negative amount is not a \
                 quantity a mechanism can hold"
            ))
        })
    }
}

fn number_refused(method: &str, index: u32, err: &StackError) -> ExecutorError {
    ExecutorError::new(format!(
        "{method} answered with something that is not a number at position {index}: {err}"
    ))
}
