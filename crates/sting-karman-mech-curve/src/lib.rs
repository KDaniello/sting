//! Inventory-curve markets: the mechanism family where price is a formula over reserves.
//!
//! The first implementation is a constant-product AMM. All arithmetic is integer; output
//! amounts round down, which keeps the product invariant non-decreasing — the direction real
//! AMMs round, and the direction the conservatism contract demands (karman/docs/VERIFICATION.md).

use serde::{Deserialize, Serialize};
use sting_karman_core::{Abort, InvariantViolation, MechError, Mechanism};
use sting_karman_types::{AgentId, Amount, Delta, Rounding, Tick, Unit};

/// The asset the pool is priced in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Token;

impl Unit for Token {
    const SYMBOL: &'static str = "token";
}

/// The asset the pool is priced against.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Quote;

impl Unit for Quote {
    const SYMBOL: &'static str = "quote";
}

/// Reserves of a constant-product pool.
///
/// The opening reserves are kept alongside the current ones because the non-decreasing-product
/// invariant needs a baseline to be measured against. They are kept as reserves rather than as
/// their product so that the state carries no width the numeric contract keeps inside a single
/// operation (karman/docs/ARITHMETIC.md §2).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CpammState {
    /// Token held by the pool right now.
    pub token_reserve: Amount<Token>,
    /// Quote held by the pool right now. Its ratio to the token reserve is the price.
    pub quote_reserve: Amount<Quote>,
    opening_token_reserve: Amount<Token>,
    opening_quote_reserve: Amount<Quote>,
}

impl CpammState {
    /// Builds a pool. Both reserves must be positive: an empty pool has no price.
    ///
    /// # Errors
    /// [`MechError`] when either reserve is zero.
    pub fn new(
        token_reserve: Amount<Token>,
        quote_reserve: Amount<Quote>,
    ) -> Result<Self, MechError> {
        if token_reserve.is_zero() || quote_reserve.is_zero() {
            return Err(MechError::new("both reserves must be positive"));
        }
        Ok(CpammState {
            token_reserve,
            quote_reserve,
            opening_token_reserve: token_reserve,
            opening_quote_reserve: quote_reserve,
        })
    }
}

/// A swap request against the pool.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CpammAction {
    /// Pay quote into the pool and take token out.
    BuyToken {
        /// What the trader pays in.
        quote_in: Amount<Quote>,
    },
    /// Pay token into the pool and take quote out.
    SellToken {
        /// What the trader pays in.
        token_in: Amount<Token>,
    },
}

/// An executed swap, from the trader's point of view: what went in and what came out.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Swap {
    /// How the trader's token position moved: positive on a buy, negative on a sell.
    pub token_delta: Delta<Token>,
    /// How the trader's quote position moved, with the opposite sign.
    pub quote_delta: Delta<Quote>,
}

/// One swap resolved in full — both new reserves and the output — before any of it reaches the
/// state. Computing everything first is what makes rejection safety structural: no path exists
/// on which one reserve moves and the other does not.
struct SwapLeg<In: Unit, Out: Unit> {
    reserve_in: Amount<In>,
    reserve_out: Amount<Out>,
    amount_out: Amount<Out>,
}

/// The constant-product mechanism. Stateless itself: everything mutable lives in
/// [`CpammState`], so one mechanism value can drive any number of worlds.
#[derive(Debug, Clone, Copy, Default)]
pub struct Cpamm;

impl Cpamm {
    /// Resolves one swap direction: `amount_in` goes into `reserve_in`, and the trader receives
    /// `floor(reserve_out * amount_in / (reserve_in + amount_in))` out of `reserve_out`.
    ///
    /// One function serves both directions because the units are parameters: buying names quote
    /// as the incoming side and token as the outgoing one, selling names them the other way, and
    /// nothing else about the arithmetic differs.
    ///
    /// The floor is the whole conservatism story of this mechanism. Rounding the output down
    /// leaves the truncated remainder inside the pool, so the product can only grow; rounding it
    /// up would let a sequence of dust swaps drain the pool one unit at a time.
    fn resolve<In: Unit, Out: Unit>(
        reserve_in: Amount<In>,
        reserve_out: Amount<Out>,
        amount_in: Amount<In>,
    ) -> Result<SwapLeg<In, Out>, MechError> {
        if amount_in.is_zero() {
            return Err(MechError::new("swap input must be positive"));
        }
        let new_in = reserve_in.checked_add(amount_in)?;
        let amount_out = reserve_out.mul_div(amount_in, new_in, Rounding::Down)?;
        if amount_out.is_zero() {
            return Err(MechError::new("swap too small for any output"));
        }
        let new_out = reserve_out.checked_sub(amount_out)?;
        Ok(SwapLeg {
            reserve_in: new_in,
            reserve_out: new_out,
            amount_out,
        })
    }
}

impl Mechanism for Cpamm {
    type State = CpammState;
    type Action = CpammAction;
    type Event = Swap;

    /// A pool keeps no accounts, so it has no use for the actor: a swap is settled against the
    /// reserves and whoever asked for it is credited outside this mechanism's knowledge.
    fn apply(
        &self,
        state: &mut CpammState,
        _actor: AgentId,
        action: CpammAction,
        _now: Tick,
        events: &mut Vec<Swap>,
    ) -> Result<(), MechError> {
        let swap = match action {
            CpammAction::BuyToken { quote_in } => {
                let leg = Self::resolve(state.quote_reserve, state.token_reserve, quote_in)?;
                state.quote_reserve = leg.reserve_in;
                state.token_reserve = leg.reserve_out;
                Swap {
                    token_delta: Delta::positive(leg.amount_out)?,
                    quote_delta: Delta::negative(quote_in)?,
                }
            }
            CpammAction::SellToken { token_in } => {
                let leg = Self::resolve(state.token_reserve, state.quote_reserve, token_in)?;
                state.token_reserve = leg.reserve_in;
                state.quote_reserve = leg.reserve_out;
                Swap {
                    token_delta: Delta::negative(token_in)?,
                    quote_delta: Delta::positive(leg.amount_out)?,
                }
            }
        };
        events.push(swap);
        Ok(())
    }

    /// A pool has no clock of its own: price moves only when someone trades, so there is nothing
    /// here to accrue and nothing that can fail.
    fn tick(
        &self,
        _state: &mut CpammState,
        _now: Tick,
        _events: &mut Vec<Swap>,
    ) -> Result<(), Abort> {
        Ok(())
    }

    fn check_invariants(&self, state: &CpammState) -> Result<(), InvariantViolation> {
        if state.token_reserve.is_zero() || state.quote_reserve.is_zero() {
            return Err(InvariantViolation::new("a reserve was drained to zero"));
        }
        // Both products are compared at their full 256-bit width, so the invariant is total: a
        // pool deep enough to overflow a 128-bit product still has an answer here.
        let opening = state
            .opening_token_reserve
            .wide_mul(state.opening_quote_reserve);
        if state.token_reserve.wide_mul(state.quote_reserve) < opening {
            return Err(InvariantViolation::new("constant product decreased"));
        }
        Ok(())
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
    use proptest::prelude::*;
    use sting_karman_core::{Agent, NullObserver, RunRng, World, conformance, monte_carlo};
    use sting_karman_types::AgentId;

    use super::*;

    const BASELINE: &str = include_str!("../../../benches/baseline.toml");

    /// The engine supplies the actor; a direct call has to name one, and which one it is cannot
    /// matter to a pool.
    const TRADER: AgentId = AgentId(0);

    /// The engine hands the mechanism a buffer it reuses; a direct call brings its own.
    fn swap(state: &mut CpammState, action: CpammAction) -> Result<Vec<Swap>, MechError> {
        let mut events = Vec::new();
        Cpamm.apply(state, TRADER, action, Tick(0), &mut events)?;
        Ok(events)
    }

    type Tokens = Amount<Token>;
    type Quotes = Amount<Quote>;

    /// Test-local trader: buys or sells a random small amount each tick. Lives here so the
    /// mechanism crate's conformance run does not depend on the agent library above it.
    struct CoinFlipTrader;

    impl Agent<Cpamm> for CoinFlipTrader {
        fn decide(
            &mut self,
            _me: AgentId,
            _state: &CpammState,
            _now: Tick,
            rng: &mut RunRng,
        ) -> Option<CpammAction> {
            let size = u128::from(rng.below(1_000_000) + 1);
            if rng.below(2) == 0 {
                Some(CpammAction::BuyToken {
                    quote_in: Quotes::new(size),
                })
            } else {
                Some(CpammAction::SellToken {
                    token_in: Tokens::new(size),
                })
            }
        }
    }

    fn pool(token: u128, quote: u128) -> CpammState {
        CpammState::new(Tokens::new(token), Quotes::new(quote)).unwrap()
    }

    fn world() -> World<Cpamm> {
        World::new(
            Cpamm,
            pool(1_000_000_000, 1_000_000_000),
            vec![Box::new(CoinFlipTrader)],
        )
    }

    #[test]
    fn swap_output_is_never_generous() {
        // 10% of the quote reserve in buys strictly less than 10% of the token reserve out:
        // floor rounding always favours the pool.
        let mut state = pool(1_000, 1_000);
        let events = swap(
            &mut state,
            CpammAction::BuyToken {
                quote_in: Quotes::new(100),
            },
        );
        assert_eq!(
            events.unwrap()[0].token_delta,
            Delta::positive(Tokens::new(90)).unwrap()
        );
        assert!(Cpamm.check_invariants(&state).is_ok());
    }

    #[test]
    fn a_swap_too_small_to_pay_out_is_rejected() {
        // One unit against a pool a thousand times deeper floors to nothing; taking the input
        // while paying nothing out would be a free donation to the pool.
        let mut state = pool(1_000, 1_000_000);
        let before = state.clone();
        assert!(
            swap(
                &mut state,
                CpammAction::BuyToken {
                    quote_in: Quotes::new(1)
                }
            )
            .is_err()
        );
        assert_eq!(state, before);
    }

    #[test]
    fn rejection_leaves_state_unchanged() {
        let mut state = pool(1_000, 1_000);
        let before = state.clone();
        let result = swap(
            &mut state,
            CpammAction::BuyToken {
                quote_in: Quotes::ZERO,
            },
        );
        assert!(result.is_err());
        assert_eq!(state, before);
    }

    #[test]
    fn an_empty_pool_has_no_price() {
        assert!(CpammState::new(Tokens::ZERO, Quotes::new(1)).is_err());
        assert!(CpammState::new(Tokens::new(1), Quotes::ZERO).is_err());
    }

    /// The product of two full-width reserves has no 128-bit answer, and the invariant still
    /// does: widening the comparison removed a failure mode rather than papering over one.
    #[test]
    fn a_pool_whose_product_exceeds_128_bits_is_still_checkable() {
        let state = pool(u128::MAX, u128::MAX);
        assert!(Cpamm.check_invariants(&state).is_ok());
    }

    #[test]
    fn a_swap_the_reserves_cannot_absorb_is_rejected() {
        let mut state = pool(1_000, u128::MAX);
        let before = state.clone();
        let result = swap(
            &mut state,
            CpammAction::BuyToken {
                quote_in: Quotes::new(u128::MAX),
            },
        );
        assert!(result.is_err());
        assert_eq!(state, before);
    }

    /// State and events cross a serialisation boundary under the hand-written encoding of
    /// `Amount` and `Delta`, and a fixture has to be able to carry a pool back into a run.
    #[test]
    fn state_and_events_round_trip_through_json() {
        let mut state = pool(1_000, 1_000);
        let events = swap(
            &mut state,
            CpammAction::BuyToken {
                quote_in: Quotes::new(100),
            },
        )
        .unwrap();

        let restored: CpammState =
            serde_json::from_str(&serde_json::to_string(&state).unwrap()).unwrap();
        assert_eq!(
            restored, state,
            "a restored pool must carry its opening reserves too"
        );
        assert!(Cpamm.check_invariants(&restored).is_ok());

        let restored: Vec<Swap> =
            serde_json::from_str(&serde_json::to_string(&events).unwrap()).unwrap();
        assert_eq!(restored, events);
    }

    #[test]
    fn conformance_deterministic() {
        conformance::assert_deterministic(500, 42, world);
    }

    /// The reproducibility gate at sweep scale: a thousand streams of one seed, twice, with the
    /// same vector both times.
    #[test]
    fn conformance_deterministic_across_a_thousand_streams() {
        let sweep = || {
            monte_carlo(1_000, 100, 42, world, |s: &CpammState, _| {
                (s.token_reserve, s.quote_reserve)
            })
            .into_iter()
            .map(Result::ok)
            .collect::<Vec<_>>()
        };
        let first = sweep();
        assert!(
            first.iter().all(Option::is_some),
            "a run aborted on an invariant violation"
        );
        assert_eq!(first, sweep());
    }

    #[test]
    fn conformance_golden() {
        let mut rng = RunRng::new(42, 0);
        let mut w = world();
        w.run(500, &mut rng, &mut NullObserver).unwrap();
        let state = w.state();
        conformance::assert_golden(
            BASELINE,
            "cpamm.golden_token_reserve",
            i128::try_from(state.token_reserve.base_units()).unwrap(),
        );
        conformance::assert_golden(
            BASELINE,
            "cpamm.golden_quote_reserve",
            i128::try_from(state.quote_reserve.base_units()).unwrap(),
        );
    }

    /// Swap sizes and pool depths across the range a scenario can reach, so the properties are
    /// not asserted only where the hand-written cases happen to look.
    fn pool_and_swaps() -> impl Strategy<Value = (u128, u128, Vec<(bool, u128)>)> {
        (
            1_000u128..1_000_000_000_000u128,
            1_000u128..1_000_000_000_000u128,
            prop::collection::vec((any::<bool>(), 1u128..1_000_000_000u128), 1..32),
        )
    }

    proptest! {
        /// The invariant the mechanism exists to preserve: floor rounding leaves the truncated
        /// remainder in the pool, so no sequence of swaps can shrink the product, and no
        /// rejection can move it.
        #[test]
        fn the_product_never_decreases((token, quote, swaps) in pool_and_swaps()) {
            let mut state = pool(token, quote);
            for (buy, size) in swaps {
                let action = if buy {
                    CpammAction::BuyToken { quote_in: Quotes::new(size) }
                } else {
                    CpammAction::SellToken { token_in: Tokens::new(size) }
                };
                let before = state.clone();
                if swap(&mut state, action).is_err() {
                    prop_assert!(state == before, "a rejection moved the pool");
                    continue;
                }
                prop_assert!(Cpamm.check_invariants(&state).is_ok());
            }
        }

        /// A trader never receives more than the curve owes, and never less than one unit below
        /// it: the output is exactly the floor of the exact quotient.
        #[test]
        fn the_output_is_the_floor_of_the_exact_quotient(
            token in 1_000u128..1_000_000_000_000u128,
            quote in 1_000u128..1_000_000_000_000u128,
            size in 1u128..1_000_000_000u128,
        ) {
            let mut state = pool(token, quote);
            let action = CpammAction::BuyToken { quote_in: Quotes::new(size) };
            let Ok(events) = swap(&mut state, action) else {
                return Ok(());
            };
            let paid = u128::try_from(events[0].token_delta.base_units()).unwrap();
            // The exact quotient, reconstructed by the division contract: paid * den <= num,
            // and one more unit would have exceeded it.
            let den = quote + size;
            prop_assert!(paid * den <= token * size);
            prop_assert!((paid + 1) * den > token * size);
        }

        /// Buying then selling the tokens back cannot leave the trader ahead: the round trip is
        /// where a rounding direction that favoured the trader would show up as free money.
        #[test]
        fn a_round_trip_never_profits_the_trader(
            token in 1_000_000u128..1_000_000_000_000u128,
            quote in 1_000_000u128..1_000_000_000_000u128,
            size in 1_000u128..1_000_000_000u128,
        ) {
            let mut state = pool(token, quote);
            let buy = CpammAction::BuyToken { quote_in: Quotes::new(size) };
            let Ok(bought) = swap(&mut state, buy) else {
                return Ok(());
            };
            let received = u128::try_from(bought[0].token_delta.base_units()).unwrap();
            let sell = CpammAction::SellToken { token_in: Tokens::new(received) };
            let Ok(sold) = swap(&mut state, sell) else {
                return Ok(());
            };
            let returned = u128::try_from(sold[0].quote_delta.base_units()).unwrap();
            prop_assert!(returned <= size, "the round trip returned a profit");
        }
    }
}
