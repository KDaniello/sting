use serde::{Deserialize, Serialize};
use sting_karman_core::{Agent, RunRng};
use sting_karman_mech_curve::{Cpamm, CpammAction, CpammState, Quote, Token};
use sting_karman_types::{AgentId, Amount, Rounding, Tick};

use crate::{BPS, informed::commit, share};

/// Technical flow: follows the pool's own recent move, with no view on where the price belongs.
///
/// The chartist of the agent-based literature, which divides a market into traders who form
/// expectations from fundamentals and traders who form them from the price series itself. This
/// half is destabilising by construction — it buys what has risen — and a population mixing the
/// two is the standard way a model produces the clustering and fat tails a real return series
/// has, neither of which uninformed flow alone will generate.
///
/// The signal is the move since the start of the current window rather than a distance from a
/// sliding average. A stepwise window carries one remembered price instead of a history, which
/// keeps a sweep of millions of runs free of a per-agent buffer, and the resolution a sliding
/// window buys is resolution this behaviour does not act on.
///
/// **Calibration.** `lookback_ticks` and `entry_band_bps` are read off the autocorrelation of
/// signed order flow in the venue being modelled: the horizon at which that autocorrelation
/// stops being distinguishable from zero is the lookback, and the move size at which flow starts
/// following is the band. `aggression_bps` is fitted against the size distribution of the flow
/// that follows.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct MomentumTrader {
    /// How many ticks one window lasts. The move is measured against the price the window
    /// opened at.
    pub lookback_ticks: u32,
    /// The move that must have happened before the trader follows it, in basis points.
    pub entry_band_bps: u32,
    /// The share of the incoming reserve committed when it does act, in basis points.
    pub aggression_bps: u32,
    /// The pool as the current window found it. Absent before the first tick.
    #[serde(skip)]
    opened_at: Option<Window>,
}

/// The pool's price at the opening of a window, kept as the ratio it is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
struct Window {
    token: Amount<Token>,
    quote: Amount<Quote>,
    opened: Tick,
}

impl MomentumTrader {
    /// A trader with the given window, band and size, holding no memory yet.
    #[must_use]
    pub fn new(lookback_ticks: u32, entry_band_bps: u32, aggression_bps: u32) -> Self {
        MomentumTrader {
            lookback_ticks,
            entry_band_bps,
            aggression_bps,
            opened_at: None,
        }
    }

    /// The window in force at `now`, opening a fresh one when the previous has run its length.
    fn window(&mut self, state: &CpammState, now: Tick) -> Window {
        let current = Window {
            token: state.token_reserve,
            quote: state.quote_reserve,
            opened: now,
        };
        match self.opened_at {
            // Saturating because the comparison is against a window that opened earlier; the
            // engine only ever advances, and a clock that did not would keep the window rather
            // than wrap its age.
            Some(open) if now.0.saturating_sub(open.opened.0) < u64::from(self.lookback_ticks) => {
                open
            }
            _ => {
                self.opened_at = Some(current);
                current
            }
        }
    }
}

impl Agent<Cpamm> for MomentumTrader {
    fn decide(
        &mut self,
        _me: AgentId,
        state: &CpammState,
        now: Tick,
        _rng: &mut RunRng,
    ) -> Option<CpammAction> {
        let open = self.window(state, now);
        if open.opened == now {
            // A window that opened on this tick has no move to report yet.
            return None;
        }

        // Cross products again: the price has risen exactly when `quote * open.token` exceeds
        // `open.quote * token`, with the band applied to the opening price rather than to a
        // quotient that would have to be rounded first.
        let unit = Amount::<Quote>::new(BPS);
        let band = share(self.entry_band_bps);
        let risen = open
            .quote
            .mul_div(Amount::new(BPS + band), unit, Rounding::Up)
            .ok()?;
        let fallen = open
            .quote
            .mul_div(Amount::new(BPS - band), unit, Rounding::Down)
            .ok()?;
        let pool = state.quote_reserve.wide_mul(open.token);

        if pool > risen.wide_mul(state.token_reserve) {
            // The token has become dearer, so the trader buys more of it.
            commit(state.quote_reserve, self.aggression_bps)
                .map(|quote_in| CpammAction::BuyToken { quote_in })
        } else if pool < fallen.wide_mul(state.token_reserve) {
            commit(state.token_reserve, self.aggression_bps)
                .map(|token_in| CpammAction::SellToken { token_in })
        } else {
            None
        }
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

    fn pool(token: u128, quote: u128) -> CpammState {
        CpammState::new(Amount::new(token), Amount::new(quote)).expect("a live pool")
    }

    fn decide(agent: &mut MomentumTrader, state: &CpammState, now: u64) -> Option<CpammAction> {
        agent.decide(AgentId(0), state, Tick(now), &mut RunRng::new(1, 0))
    }

    #[test]
    fn the_tick_a_window_opens_on_has_no_move_to_follow() {
        let mut agent = MomentumTrader::new(4, 50, 100);
        assert!(decide(&mut agent, &pool(1_000_000, 1_000_000), 0).is_none());
    }

    #[test]
    fn a_price_that_has_risen_is_followed_up() {
        let mut agent = MomentumTrader::new(4, 50, 100);
        decide(&mut agent, &pool(1_000_000, 1_000_000), 0);
        assert!(matches!(
            decide(&mut agent, &pool(1_000_000, 1_100_000), 1),
            Some(CpammAction::BuyToken { .. })
        ));
    }

    #[test]
    fn a_price_that_has_fallen_is_followed_down() {
        let mut agent = MomentumTrader::new(4, 50, 100);
        decide(&mut agent, &pool(1_000_000, 1_000_000), 0);
        assert!(matches!(
            decide(&mut agent, &pool(1_000_000, 900_000), 1),
            Some(CpammAction::SellToken { .. })
        ));
    }

    #[test]
    fn a_move_inside_the_band_is_not_a_trend() {
        let mut agent = MomentumTrader::new(4, 200, 100);
        decide(&mut agent, &pool(1_000_000, 1_000_000), 0);
        // One percent, against a band of two.
        assert!(decide(&mut agent, &pool(1_000_000, 1_010_000), 1).is_none());
    }

    /// The window is what the behaviour is parameterised by, so its length has to be the length
    /// the comparison actually spans.
    #[test]
    fn a_window_holds_its_opening_price_for_its_whole_length() {
        let mut agent = MomentumTrader::new(3, 50, 100);
        decide(&mut agent, &pool(1_000_000, 1_000_000), 0);
        for tick in 1..3 {
            assert_eq!(
                agent.opened_at.unwrap().opened,
                Tick(0),
                "the window reopened early"
            );
            decide(
                &mut agent,
                &pool(1_000_000, 1_000_000 + u128::from(tick)),
                tick,
            );
        }
        decide(&mut agent, &pool(1_000_000, 1_500_000), 3);
        assert_eq!(
            agent.opened_at.unwrap().opened,
            Tick(3),
            "the window never reopened"
        );
    }

    #[test]
    fn a_trader_reads_the_pool_and_never_the_run_stream() {
        // Two identical pools decided on with different streams must give the same answer: this
        // behaviour is technical, and nothing about it is stochastic.
        let mut first = MomentumTrader::new(4, 50, 100);
        let mut second = MomentumTrader::new(4, 50, 100);
        first.decide(
            AgentId(0),
            &pool(1_000_000, 1_000_000),
            Tick(0),
            &mut RunRng::new(1, 0),
        );
        second.decide(
            AgentId(0),
            &pool(1_000_000, 1_000_000),
            Tick(0),
            &mut RunRng::new(9, 5),
        );
        let a = first.decide(
            AgentId(0),
            &pool(1_000_000, 1_200_000),
            Tick(1),
            &mut RunRng::new(1, 0),
        );
        let b = second.decide(
            AgentId(0),
            &pool(1_000_000, 1_200_000),
            Tick(1),
            &mut RunRng::new(9, 5),
        );
        assert_eq!(a, b);
    }

    /// A band no share can be is held at what a share can be, so the comparison stays a
    /// comparison rather than wrapping. A doubling clears even the widest expressible band, so
    /// the answer is still a trade.
    #[test]
    fn a_band_beyond_a_whole_cannot_wrap_the_move_test() {
        let mut agent = MomentumTrader::new(4, u32::MAX, 100);
        decide(&mut agent, &pool(1_000_000, 1_000_000), 0);
        assert!(matches!(
            decide(&mut agent, &pool(1_000_000, 2_000_000), 1),
            Some(CpammAction::BuyToken { .. })
        ));
    }

    #[test]
    fn parameters_round_trip_as_an_artifact() {
        let agent = MomentumTrader::new(12, 75, 250);
        let json = serde_json::to_string(&agent).unwrap();
        let back: MomentumTrader = serde_json::from_str(&json).unwrap();
        assert_eq!(back.lookback_ticks, 12);
        assert_eq!(back.entry_band_bps, 75);
        assert_eq!(back.aggression_bps, 250);
        assert!(
            back.opened_at.is_none(),
            "a restored trader must start without memory"
        );
    }
}
