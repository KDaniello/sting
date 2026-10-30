//! The agent library: behaviours that populate a world.
//!
//! An agent implements [`sting_karman_core::Agent`] for one concrete mechanism — behaviour is
//! mechanism-specific by nature, so this crate depends on the mechanism crates, never the other
//! way round. Calibration is what separates an agent here from a toy: a behaviour's parameters
//! are estimated from recorded data (levels 2 and up of karman/docs/VERIFICATION.md), and an agent
//! carries its parameters as an explicit, serialisable struct so a calibrated population is a
//! versioned artifact, not code.
//!
//! Two families. For an automated market maker, three behaviours, and the division between them
//! is the one every account of one makes. [`NoiseTrader`] is uninformed flow, which pays the fees a
//! liquidity provider earns. [`Arbitrageur`] is informed flow, which takes the price back to the
//! market and costs a provider what loss-versus-rebalancing measures. [`MomentumTrader`] is
//! technical flow, which has no view on where the price belongs and follows where it went — the
//! half a model needs before a return series clusters the way a real one does.
//!
//! For a lending market, three again, and the division is the one a loss is decided by.
//! [`Borrower`] is a position opened at a leverage and put back together after part of it is
//! taken. [`Liquidator`] is whoever takes it, and carries the two parameters that decide whether
//! anybody got there in time — a reaction time and a capacity. [`Depositor`] is the side that
//! bears what nobody got to in time, and is the baseline of the three.
//!
//! For a showcase, one. [`Holder`] is somebody who owns things and decides what to say they are
//! worth, and a collector is not a second behaviour but the same one with a stronger attachment:
//! two types differing only in their constants would be two copies of one decision drifting apart.
//! What it declares comes from Weyl and Zhang rather than from a taste of ours, which is why the
//! curve it produces can fall as well as rise.
//!
//! Each states what its parameters are estimated from, and which of them is a measurement rather
//! than a fit. A parameter with no such story is a knob, and a population of knobs will
//! reproduce anything asked of it.

mod borrower;
mod depositor;
mod holder;
mod informed;
mod liquidator;
mod member;
mod noise;
#[cfg(test)]
mod testing;
mod trend;

pub use borrower::Borrower;
pub use depositor::Depositor;
pub use holder::Holder;
pub use informed::Arbitrageur;
pub use liquidator::Liquidator;
pub use member::Member;
pub use noise::NoiseTrader;
pub use trend::MomentumTrader;

/// One whole in basis points: the unit every share in this crate is quoted against.
///
/// The scale lives here rather than in the foundation, which owns the operation and no scale
/// (karman/docs/ARITHMETIC.md §3). Basis points because that is what a fee, a band and a threshold are
/// quoted in wherever these behaviours are calibrated from.
pub(crate) const BPS: u128 = 10_000;

/// A share of the whole, clamped to what a share can be.
///
/// The fields these come from are public and a scenario is not the only way to build a
/// behaviour, so the bound is enforced where the arithmetic depends on it rather than only at
/// the boundary that already refuses to cross it. A share of a whole or more would take
/// `BPS - share` through zero, which in a release build wraps rather than stops.
pub(crate) fn share(bps: u32) -> u128 {
    u128::from(bps).min(BPS - 1)
}
