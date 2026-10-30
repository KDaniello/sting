//! Shared domain types for the Karman workspace.
//!
//! Everything here is a plain value: no I/O, no clocks, no allocation beyond what the type
//! itself owns. Money is integer base units carrying the unit it counts, so two quantities that
//! must never be added cannot be — a mechanism state transition never touches a float, because
//! level 0 of the verification ladder demands bit-exact agreement with on-chain arithmetic.
//!
//! The widths, the absence of any scale, and the rounding rules are
//! [the numeric contract](https://github.com/karman-dev/karman/blob/main/karman/docs/ARITHMETIC.md), and
//! each of them is read out of a reference implementation rather than chosen.

mod arith;
mod money;
mod wire;

use std::num::NonZeroU64;

pub use arith::{ArithError, ParseWideError, Rounding, Wide};
pub use money::{Amount, Delta, Unit};
use serde::{Deserialize, Serialize};
pub use wire::{i128_str, option_u128_str, u128_str};

/// A step of logical simulation time.
///
/// The scheduler owns the clock; wall time never enters a run. What one tick means in seconds
/// is a property of the scenario, not of the engine.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default, Serialize, Deserialize,
)]
#[serde(transparent)]
pub struct Tick(pub u64);

/// Identifier of an agent within one scenario.
///
/// Assignment order is part of the reproducibility contract: agent 0 always acts before agent 1
/// on the same tick, and an agent receives its own identifier as the handle on whatever part of
/// the state belongs to it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct AgentId(pub u32);

/// Identifier of a market within one scenario.
///
/// A venue hosts more than one market at a time — a lending protocol one per collateral and
/// loan pair, an exchange one per instrument — and every action names the one it acts on. The
/// ordering is part of the reproducibility contract: state keyed by this is walked in numeric
/// order, so a run that touches several markets touches them in the same sequence every time.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct MarketId(pub u32);

/// How much wall-clock time one [`Tick`] stands for.
///
/// The engine's clock is logical, and a mechanism whose transitions are dated — interest that
/// compounds per second, funding that pays per hour — needs the conversion the scenario chose.
/// Non-zero, because a tick standing for no time makes every per-second rate meaningless.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct SecondsPerTick(pub NonZeroU64);

impl SecondsPerTick {
    /// The wall-clock seconds `ticks` ticks stand for.
    ///
    /// Widened on the way out, because the product of two 64-bit counts does not fit one: a run
    /// long enough to exceed it would otherwise report a duration it did not have.
    #[must_use]
    pub fn seconds(self, ticks: u64) -> u128 {
        u128::from(ticks) * u128::from(self.0.get())
    }
}

#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "karman/docs/ARCHITECTURE.md §3.2 scopes the no-unwrap/expect/panic policy to library code outside tests"
)]
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn logical_time_and_identifiers_survive_the_boundary() {
        let tick: Tick = serde_json::from_str("7").unwrap();
        assert_eq!(tick, Tick(7));
        assert_eq!(serde_json::to_string(&AgentId(3)).unwrap(), "3");
        assert_eq!(serde_json::from_str::<MarketId>("2").unwrap(), MarketId(2));
        assert_eq!(
            serde_json::to_string(&SecondsPerTick(NonZeroU64::new(12).unwrap())).unwrap(),
            "12"
        );
    }

    /// A tick that stands for no time is refused where it is read, not where it is used: every
    /// per-second rate divides by it.
    #[test]
    fn a_tick_standing_for_no_time_is_refused_at_the_boundary() {
        assert!(serde_json::from_str::<SecondsPerTick>("0").is_err());
    }

    #[test]
    fn a_span_of_ticks_is_measured_past_the_width_of_either_factor() {
        let per_tick = SecondsPerTick(NonZeroU64::new(12).unwrap());
        assert_eq!(per_tick.seconds(10), 120);
        assert_eq!(per_tick.seconds(u64::MAX), u128::from(u64::MAX) * 12);
    }
}
