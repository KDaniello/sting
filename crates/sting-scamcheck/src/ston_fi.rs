//! STON.fi LP-concentration primitives (`docs/domains/scamcheck.md`).
//! A STON.fi pool contract, v1 and v2 alike, doubles as the TEP-74
//! jetton master for its own LP token, and the standard `get_wallet_address
//! (slice owner_address)` get-method resolves any candidate address's LP
//! jetton wallet, exactly the way it would for any other TEP-74 jetton (a
//! pool is not a special case there).
//!
//! **Correction found empirically, not assumed**: `get_pool_data()`'s field
//! order is version-dependent (`docs.ston.fi` documents `is_locked,
//! router_address, total_supply, ...` for v2 but a shorter, `total_supply`-
//! less order for v1) — a real test against the real fixture below (a v1
//! pool, it turned out) decoded index 2 as a non-`Int` value, contradicting
//! the assumed v2 schema. Rather than pin this module to one pool version's
//! `get_pool_data()` layout, `total_supply` is read via the *already
//! version-independent* `get_jetton_data()` (every TEP-74 contract, this
//! pool included, implements it identically regardless of pool-version
//! internals) — the same call and the same decoder already proved against
//! real data, just index 0 instead of index 2. Ground truth:
//! `corpus/fixtures/emulator/SOURCE.md`.
//!
//! **Three things this module deliberately does not do**, named explicitly so
//! that its real scope is not mistaken for the full signal
//! `docs/domains/scamcheck.md` describes:
//! 1. **No pool discovery.** Every function here takes an already-known
//!    pool's [`TvmEmulator`] — there is no STON.fi-router-based lookup from
//!    a jetton address to its pool address. A caller must already have the
//!    pool.
//! 2. **No cross-contract balance fetch.** [`candidate_lp_wallet_address`]
//!    resolves *which address* holds a candidate's LP wallet, but does not
//!    fetch that (separately-deployed) wallet's own code+data or read its
//!    balance — that cross-contract hop belongs with the product-wiring
//!    work that already owns `sting-corpus` integration for cross-cutting
//!    fetch-by-address, rather than being duplicated here.
//! 3. **No classification.** This module ships two raw primitives
//!    (`total_supply`, wallet-address resolution), not a composed
//!    deployer-controlled/burn-like/third-party-locker verdict — that
//!    judgment call is scoring logic, out of scope for a primitive.

use sting_emulator::{EmulatorError, TvmEmulator, method_id};
use thiserror::Error;
use tonlib_core::{TonAddress, TonAddressParseError};

use crate::stack::{self, StackError};

/// `get_jetton_data()`'s `total_supply` — see this module's doc comment
/// for why this is used instead of the version-dependent `get_pool_data()`.
const POOL_TOTAL_SUPPLY_STACK_INDEX: u32 = 0;
/// `get_wallet_address(slice owner_address)` returns exactly one value.
const WALLET_ADDRESS_STACK_INDEX: u32 = 0;

/// Errors calling or decoding a STON.fi pool's get-methods.
#[derive(Debug, Error)]
pub enum StonFiError {
    /// The get-method call itself failed at the emulator layer.
    #[error("{method} call failed: {source}")]
    Emulator {
        /// Which get-method was being called.
        method: &'static str,
        /// The underlying emulator failure.
        #[source]
        source: EmulatorError,
    },
    /// The call completed but did not succeed, or returned no stack.
    #[error("{method} did not succeed: exit_code={exit_code:?} error={error:?}")]
    ContractFailure {
        /// Which get-method was being called.
        method: &'static str,
        /// The VM exit code, if the call itself completed.
        exit_code: Option<i32>,
        /// The emulator-reported error, if any.
        error: Option<String>,
    },
    /// The returned stack could not be decoded, or decoded to an
    /// unexpected shape (e.g. `total_supply` was not an `Int`).
    #[error("failed to decode {method}'s stack: {source}")]
    Stack {
        /// Which get-method was being called.
        method: &'static str,
        /// The underlying decode failure.
        #[source]
        source: StackError,
    },
    /// The candidate's LP wallet address is not a standard address.
    #[error("resolved LP wallet address is not a standard address: {0}")]
    UnsupportedAddress(#[source] TonAddressParseError),
}

/// Reads the pool's own LP jetton `total_supply` via `get_jetton_data()`
/// (see this module's doc comment for why not `get_pool_data()`).
///
/// # Errors
/// See [`StonFiError`].
pub fn pool_total_supply(pool: &TvmEmulator<'_>) -> Result<i64, StonFiError> {
    const METHOD: &str = "get_jetton_data";
    let response = pool
        .run_get_method_no_args(METHOD)
        .map_err(|source| StonFiError::Emulator {
            method: METHOD,
            source,
        })?;
    if !response.success || response.vm_exit_code != Some(0) {
        return Err(StonFiError::ContractFailure {
            method: METHOD,
            exit_code: response.vm_exit_code,
            error: response.error,
        });
    }
    let stack = response
        .stack
        .as_deref()
        .ok_or(StonFiError::ContractFailure {
            method: METHOD,
            exit_code: response.vm_exit_code,
            error: Some("success response carried no stack field".to_owned()),
        })?;
    let value = stack::decode_entry(stack, POOL_TOTAL_SUPPLY_STACK_INDEX).map_err(|source| {
        StonFiError::Stack {
            method: METHOD,
            source,
        }
    })?;
    value
        .as_int(POOL_TOTAL_SUPPLY_STACK_INDEX)
        .map_err(|source| StonFiError::Stack {
            method: METHOD,
            source,
        })
}

/// Resolves `candidate`'s LP jetton wallet address for this pool, via the
/// standard TEP-74 `get_wallet_address(slice owner_address)` get-method —
/// the pool contract *is* the LP jetton's master, so this call is no
/// different from resolving any other jetton's wallet address.
///
/// # Errors
/// See [`StonFiError`].
pub fn candidate_lp_wallet_address(
    pool: &TvmEmulator<'_>,
    candidate: &TonAddress,
) -> Result<TonAddress, StonFiError> {
    const METHOD: &str = "get_wallet_address";
    let arg_stack = stack::encode_address_arg(candidate).map_err(|source| StonFiError::Stack {
        method: METHOD,
        source,
    })?;
    let response = pool
        .run_get_method(method_id(METHOD), &arg_stack)
        .map_err(|source| StonFiError::Emulator {
            method: METHOD,
            source,
        })?;
    if !response.success || response.vm_exit_code != Some(0) {
        return Err(StonFiError::ContractFailure {
            method: METHOD,
            exit_code: response.vm_exit_code,
            error: response.error,
        });
    }
    let stack = response
        .stack
        .as_deref()
        .ok_or(StonFiError::ContractFailure {
            method: METHOD,
            exit_code: response.vm_exit_code,
            error: Some("success response carried no stack field".to_owned()),
        })?;
    let value = stack::decode_entry(stack, WALLET_ADDRESS_STACK_INDEX).map_err(|source| {
        StonFiError::Stack {
            method: METHOD,
            source,
        }
    })?;
    let msg_addr = value
        .as_msg_address(WALLET_ADDRESS_STACK_INDEX)
        .map_err(|source| StonFiError::Stack {
            method: METHOD,
            source,
        })?;
    TonAddress::from_msg_address(msg_addr).map_err(StonFiError::UnsupportedAddress)
}
