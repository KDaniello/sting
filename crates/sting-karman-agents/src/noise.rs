use serde::{Deserialize, Serialize};
use sting_karman_core::{Agent, RunRng};
use sting_karman_mech_curve::{Cpamm, CpammAction, CpammState};
use sting_karman_types::{AgentId, Amount, Tick};

/// Uninformed order flow against a curve market: each tick, a fair coin between buying and
/// selling, and a uniform size below the cap.
///
/// The zero-intelligence baseline of the agent-based literature. It exists to exercise a
/// mechanism, to seed conformance runs, and to be the null hypothesis a calibrated population
/// is measured against — never to claim realism by itself.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct NoiseTrader {
    /// Upper bound on a single trade, in the base units of whichever side the trade takes.
    /// The bound is a size and not a value: this behaviour reads no price, so it has no way to
    /// convert one side into the other.
    pub trade_cap: u64,
}

impl Agent<Cpamm> for NoiseTrader {
    fn decide(
        &mut self,
        _me: AgentId,
        _state: &CpammState,
        _now: Tick,
        rng: &mut RunRng,
    ) -> Option<CpammAction> {
        let size = u128::from(rng.below(self.trade_cap) + 1);
        if rng.below(2) == 0 {
            Some(CpammAction::BuyToken {
                quote_in: Amount::new(size),
            })
        } else {
            Some(CpammAction::SellToken {
                token_in: Amount::new(size),
            })
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

    fn flow(trader: &mut NoiseTrader, draws: usize, stream: u64) -> Vec<CpammAction> {
        let state =
            CpammState::new(Amount::new(1_000_000), Amount::new(1_000_000)).expect("a live pool");
        let mut rng = RunRng::new(7, stream);
        (0..draws)
            .filter_map(|tick| trader.decide(AgentId(0), &state, Tick(tick as u64), &mut rng))
            .collect()
    }

    fn size(action: &CpammAction) -> u128 {
        match action {
            CpammAction::BuyToken { quote_in } => quote_in.base_units(),
            CpammAction::SellToken { token_in } => token_in.base_units(),
        }
    }

    #[test]
    fn every_trade_is_positive_and_within_the_cap() {
        let cap = 1_000;
        for action in flow(&mut NoiseTrader { trade_cap: cap }, 500, 0) {
            let size = size(&action);
            assert!(
                size >= 1,
                "a swap of {size} would be rejected before it reached the pool"
            );
            assert!(
                size <= u128::from(cap),
                "a swap of {size} broke the cap of {cap}"
            );
        }
    }

    #[test]
    fn the_flow_is_a_function_of_the_stream_alone() {
        let first = flow(&mut NoiseTrader { trade_cap: 1_000 }, 200, 3);
        let second = flow(&mut NoiseTrader { trade_cap: 1_000 }, 200, 3);
        assert_eq!(first, second);
        assert_ne!(first, flow(&mut NoiseTrader { trade_cap: 1_000 }, 200, 4));
    }

    #[test]
    fn a_cap_of_one_leaves_exactly_one_trade_size() {
        for action in flow(&mut NoiseTrader { trade_cap: 1 }, 50, 0) {
            assert_eq!(size(&action), 1);
        }
    }

    /// Parameters are the versioned artifact of a calibrated population, so they have to survive
    /// the wire without the behaviour changing shape.
    #[test]
    fn parameters_round_trip_as_an_artifact() {
        let trader = NoiseTrader { trade_cap: 4_200 };
        let json = serde_json::to_string(&trader).unwrap();
        assert_eq!(
            serde_json::from_str::<NoiseTrader>(&json)
                .unwrap()
                .trade_cap,
            4_200
        );
    }
}
