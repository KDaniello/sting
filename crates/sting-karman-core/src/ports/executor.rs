use std::borrow::Cow;

use thiserror::Error;

/// A contract could not answer. Never a rejection: a get-method that fails is a question the run
/// asked and did not get an answer to, so the run stops rather than continuing on a number it
/// invented.
#[derive(Debug, Error)]
#[error("contract call failed: {reason}")]
pub struct ExecutorError {
    /// Which call failed and how, in enough detail to find the contract and the method.
    pub reason: Cow<'static, str>,
}

impl ExecutorError {
    /// States what the contract could not answer.
    #[must_use]
    pub fn new(reason: impl Into<Cow<'static, str>>) -> Self {
        ExecutorError {
            reason: reason.into(),
        }
    }
}

/// A contract that answers questions: run a get-method, take a number off the stack.
///
/// The port exists because a mechanism modelled after a real venue and the real venue itself are
/// two implementations of the same shape, and a run that wants to know whether the model agrees
/// with the contract has to be able to ask both. What crosses the boundary is a number rather
/// than a stack, a cell or a virtual machine: a mechanism that decoded cells would be a mechanism
/// that knows which chain it is on.
///
/// Deterministic like everything else the core touches. A get-method against a fixed account
/// state answers the same way every time, which is what lets a run that consults a contract still
/// reproduce from its seed.
pub trait TvmExecutor {
    /// The unsigned integer `method` puts at `index` of its result stack.
    ///
    /// # Errors
    /// [`ExecutorError`] when the contract refuses the call, exits non-zero, or answers with
    /// something that is not a non-negative integer at that position.
    fn call_number(&self, method: &str, index: u32) -> Result<u128, ExecutorError>;
}
