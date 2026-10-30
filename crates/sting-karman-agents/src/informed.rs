use serde::{Deserialize, Serialize};
use sting_karman_core::{Agent, RunRng};
use sting_karman_mech_curve::{Cpamm, CpammAction, CpammState, Quote, Token};
use sting_karman_types::{AgentId, Amount, Rounding, Tick, Unit};

use crate::{BPS, share};

/// Informed flow: trades the pool toward a reference price the pool cannot see.
///
/// The counterpart of uninformed flow in every account of how an automated market maker earns
/// and loses. Liquidity providers are paid by traders who do not know where the price is going
/// and are picked off by traders who do; the second half is this behaviour, and the loss it
/// inflicts is what loss-versus-rebalancing measures.
///
/// Its reference wanders on the run stream rather than being read from a recording, which makes
/// a population of these a stress generator rather than a replay. Replay against a real
/// reference is a different question, asked by the verification ladder rather than here.
///
/// **Calibration.** `entry_band_bps` is not a taste: it is the round trip this trader must pay
/// to realise the gap — the pool's fee plus the cost of the offsetting leg — and it is
/// observable per venue rather than fitted. `volatility_bps` is the reference market's realised
/// volatility over one tick, which is measured from that market and not from the pool.
/// `aggression_bps` is the only fitted parameter, and the quantity it should be fitted against
/// is the size distribution of arbitrage trades in the pool being modelled.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct Arbitrageur {
    /// The reference price as a ratio: this much quote per `reference_token` of token. At
    /// construction it is the parameter; from the first tick it is the agent's own belief.
    pub reference_quote: Amount<Quote>,
    /// The token side of that ratio, which the walk leaves alone.
    pub reference_token: Amount<Token>,
    /// The furthest the reference may move in one tick, in basis points, drawn uniformly in
    /// both directions each tick.
    pub volatility_bps: u32,
    /// The deviation that must open before a round trip pays for itself, in basis points.
    pub entry_band_bps: u32,
    /// The share of the incoming reserve committed when it does act, in basis points.
    pub aggression_bps: u32,
}

impl Arbitrageur {
    /// Moves the reference one tick along its walk.
    ///
    /// A step that has no representable answer leaves the reference where it was: a belief no
    /// number can hold is not one worth acting on, and refusing to move is the only outcome that
    /// does not invent a price.
    fn wander(&mut self, rng: &mut RunRng) {
        let reach = share(self.volatility_bps);
        // `reach` is a share in ten-thousandths and twice it plus one is far inside a word;
        // a volatility that overflowed here would be a scenario the validator refused.
        let span = u64::try_from(reach * 2 + 1).unwrap_or(u64::MAX);
        let step = u128::from(rng.below(span));
        let multiplier = BPS + step - reach;
        if let Ok(moved) = self.reference_quote.mul_div(
            Amount::<Quote>::new(multiplier),
            Amount::<Quote>::new(BPS),
            Rounding::Down,
        ) && !moved.is_zero()
        {
            self.reference_quote = moved;
        }
    }
}

impl Agent<Cpamm> for Arbitrageur {
    fn decide(
        &mut self,
        _me: AgentId,
        state: &CpammState,
        _now: Tick,
        rng: &mut RunRng,
    ) -> Option<CpammAction> {
        self.wander(rng);

        // Every comparison below is a cross product held at full width, so the deviation test
        // needs no price type, no scale and no rounding: the pool's quote-per-token exceeds the
        // reference exactly when `quote * reference_token` exceeds `reference_quote * token`.
        let pool = state.quote_reserve.wide_mul(self.reference_token);
        let band = share(self.entry_band_bps);
        let unit = Amount::<Quote>::new(BPS);
        let upper = self
            .reference_quote
            .mul_div(Amount::new(BPS + band), unit, Rounding::Up)
            .ok()?;
        let lower = self
            .reference_quote
            .mul_div(Amount::new(BPS - band), unit, Rounding::Down)
            .ok()?;

        if pool > upper.wide_mul(state.token_reserve) {
            // The pool pays too much quote for a token, so the token is sold into it.
            let size = commit(state.token_reserve, self.aggression_bps)?;
            Some(CpammAction::SellToken { token_in: size })
        } else if pool < lower.wide_mul(state.token_reserve) {
            let size = commit(state.quote_reserve, self.aggression_bps)?;
            Some(CpammAction::BuyToken { quote_in: size })
        } else {
            None
        }
    }
}

/// The share of a reserve an agent commits, in basis points. Never larger than the reserve, so
/// this cannot be the arithmetic that overflows.
pub(crate) fn commit<U: Unit>(reserve: Amount<U>, share_bps: u32) -> Option<Amount<U>> {
    // The share is a pure ratio; naming its unit `U` is what makes it cancel rather than a claim
    // that a basis point is a quantity of anything.
    let size = reserve
        .mul_div(
            Amount::<U>::new(share(share_bps)),
            Amount::<U>::new(BPS),
            Rounding::Down,
        )
        .ok()?;
    (!size.is_zero()).then_some(size)
}

#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "karman/docs/ARCHITECTURE.md §3.2 scopes the no-unwrap/expect/panic policy to library code outside tests"
)]
#[cfg(test)]
mod tests {
    use sting_karman_core::Mechanism as _;

    use super::*;

    fn pool(token: u128, quote: u128) -> CpammState {
        CpammState::new(Amount::new(token), Amount::new(quote)).expect("a live pool")
    }

    fn arbitrageur(reference_quote: u128, band: u32) -> Arbitrageur {
        Arbitrageur {
            reference_quote: Amount::new(reference_quote),
            reference_token: Amount::new(1_000_000),
            volatility_bps: 0,
            entry_band_bps: band,
            aggression_bps: 100,
        }
    }

    fn decide(agent: &mut Arbitrageur, state: &CpammState) -> Option<CpammAction> {
        agent.decide(AgentId(0), state, Tick(0), &mut RunRng::new(1, 0))
    }

    #[test]
    fn a_pool_at_the_reference_is_left_alone() {
        let state = pool(1_000_000, 1_000_000);
        assert!(decide(&mut arbitrageur(1_000_000, 50), &state).is_none());
    }

    #[test]
    fn a_pool_paying_too_much_quote_is_sold_into() {
        // The pool holds twice the quote per token that the reference says it should.
        let state = pool(1_000_000, 2_000_000);
        assert!(matches!(
            decide(&mut arbitrageur(1_000_000, 50), &state),
            Some(CpammAction::SellToken { .. })
        ));
    }

    #[test]
    fn a_pool_paying_too_little_quote_is_bought_from() {
        let state = pool(2_000_000, 1_000_000);
        assert!(matches!(
            decide(&mut arbitrageur(1_000_000, 50), &state),
            Some(CpammAction::BuyToken { .. })
        ));
    }

    /// The band is the round trip: a deviation smaller than it is not worth paying for, and the
    /// same deviation is worth acting on once the band is lowered below it.
    #[test]
    fn a_deviation_inside_the_band_is_not_worth_the_round_trip() {
        // The pool is one percent above the reference: a hundred basis points.
        let state = pool(1_000_000, 1_010_000);
        assert!(decide(&mut arbitrageur(1_000_000, 200), &state).is_none());
        assert!(decide(&mut arbitrageur(1_000_000, 50), &state).is_some());
    }

    /// Trading toward the reference means the deviation the agent acts on shrinks, which is the
    /// property that makes a population of these hold a pool near the market.
    #[test]
    fn acting_moves_the_pool_toward_the_reference() {
        let mut state = pool(1_000_000, 2_000_000);
        let mut agent = arbitrageur(1_000_000, 50);
        let before = state.quote_reserve.wide_mul(agent.reference_token);
        let action = decide(&mut agent, &state).expect("a gap this wide is worth closing");
        let mut events = Vec::new();
        sting_karman_mech_curve::Cpamm
            .apply(&mut state, AgentId(0), action, Tick(0), &mut events)
            .expect("the pool can absorb one percent of a reserve");
        let after = state.quote_reserve.wide_mul(agent.reference_token);
        assert!(after < before, "the pool did not move toward the reference");
    }

    #[test]
    fn a_still_reference_does_not_wander() {
        let mut agent = arbitrageur(1_000_000, 50);
        let before = agent.reference_quote;
        agent.wander(&mut RunRng::new(9, 0));
        assert_eq!(
            agent.reference_quote, before,
            "a zero volatility moved the reference"
        );
    }

    #[test]
    fn a_wandering_reference_stays_a_function_of_the_stream() {
        let walk = |stream: u64| {
            let mut agent = arbitrageur(1_000_000, 50);
            agent.volatility_bps = 500;
            let mut rng = RunRng::new(7, stream);
            (0..64)
                .map(|_| {
                    agent.wander(&mut rng);
                    agent.reference_quote
                })
                .collect::<Vec<_>>()
        };
        assert_eq!(walk(0), walk(0));
        assert_ne!(walk(0), walk(1));
    }

    /// The fields are public, so a behaviour can be built with a volatility no share can be.
    /// The walk has to stop at what a share is rather than take its multiplier through zero,
    /// which a release build would wrap rather than refuse.
    #[test]
    fn a_volatility_beyond_a_whole_cannot_wrap_the_walk() {
        let mut agent = arbitrageur(1_000_000, 50);
        agent.volatility_bps = u32::MAX;
        let mut rng = RunRng::new(3, 0);
        for _ in 0..256 {
            agent.wander(&mut rng);
            assert!(
                !agent.reference_quote.is_zero(),
                "the reference walked into nothing"
            );
        }
    }

    /// The same bound on the other side of the same arithmetic: a doubling still clears the
    /// widest expressible band, and nothing wraps on the way to saying so.
    #[test]
    fn a_band_beyond_a_whole_cannot_wrap_the_deviation_test() {
        let mut agent = arbitrageur(1_000_000, u32::MAX);
        assert!(matches!(
            decide(&mut agent, &pool(1_000_000, 2_000_000)),
            Some(CpammAction::SellToken { .. })
        ));
    }

    #[test]
    fn a_commitment_too_small_to_trade_is_no_trade_at_all() {
        // A hundredth of a basis point of a tiny reserve rounds to nothing, and offering the
        // pool nothing is not a decision.
        assert!(commit(Amount::<Quote>::new(10), 1).is_none());
        assert_eq!(
            commit(Amount::<Quote>::new(1_000_000), 100),
            Some(Amount::new(10_000))
        );
    }
}
