use serde::{Deserialize, Serialize};
use sting_karman_core::SourceError;
use sting_karman_types::{Amount, Tick};

use crate::{Wad, market::Market};

/// What a market charges its borrowers, per second, at [`crate::WAD`].
///
/// A port of this family rather than of the engine. The market is an argument because a borrow
/// rate reads the market it prices — utilisation is the ratio of what is borrowed to what is
/// supplied, and a rate that rises with it is the feedback loop a credit market exists to
/// exhibit. An exogenous path could not express that: it sees the clock and nothing else.
///
/// The shape follows the reference's own pluggable interface, so a model written against that
/// one substitutes here without translation.
pub trait BorrowRate {
    /// The rate `market` charges at `now`.
    ///
    /// # Errors
    ///
    /// Fails when the model cannot price this market — a recorded path asked past its end, a
    /// fixture missing the tick.
    fn per_second(&self, market: &Market, now: Tick) -> Result<Amount<Wad>, SourceError>;
}

/// A market that charges the same rate whatever its utilisation.
///
/// Not a model of anything. It is what lets accrual, share accounting and the health check be
/// exercised for what they do with a rate rather than for which rate they were handed, and it is
/// what a differential vector supplies to both sides so that the two are comparing arithmetic
/// rather than two rate curves.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct FixedRate(pub Amount<Wad>);

impl BorrowRate for FixedRate {
    fn per_second(&self, _market: &Market, _now: Tick) -> Result<Amount<Wad>, SourceError> {
        Ok(self.0)
    }
}

/// The rate a market actually charged, as a recording of it.
///
/// What a replay needs and a model cannot give: the curve that priced a real episode had state
/// of its own, and the only honest way to put an episode back through this mechanism is to hand
/// it the rates the episode was charged rather than the ones a model would have guessed.
///
/// The schedule holds forward. A rate is what it was until something changed it, which is how
/// the market itself treats it between one accrual and the next.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecordedRate(Vec<Charged>);

/// What was charged, and from when.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Charged {
    /// The first tick this rate answered for.
    pub from: Tick,
    /// The rate, per second, at [`crate::WAD`].
    pub per_second: Amount<Wad>,
}

impl RecordedRate {
    /// A recording, in the order it was charged.
    ///
    /// # Errors
    ///
    /// Refuses a schedule out of order rather than sorting one: a recording whose entries arrived
    /// shuffled is a recording somebody assembled wrongly, and quietly repairing it would put a
    /// rate against a tick that never carried it.
    pub fn new(charged: Vec<Charged>) -> Result<Self, SourceError> {
        if charged.windows(2).any(|pair| pair[1].from <= pair[0].from) {
            return Err(SourceError::new("a recorded rate is out of order"));
        }
        Ok(RecordedRate(charged))
    }
}

impl BorrowRate for RecordedRate {
    fn per_second(&self, _market: &Market, now: Tick) -> Result<Amount<Wad>, SourceError> {
        self.0
            .iter()
            .rev()
            .find(|charged| charged.from <= now)
            .map(|charged| charged.per_second)
            .ok_or_else(|| SourceError::new("the recording charged nothing at this tick"))
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

    /// A recording answers with what was charged then, and holds it forward until the next
    /// change — which is what the market does between one accrual and the next.
    #[test]
    fn a_recording_holds_a_rate_forward() {
        let charged = RecordedRate::new(vec![
            Charged {
                from: Tick(10),
                per_second: Amount::new(5),
            },
            Charged {
                from: Tick(20),
                per_second: Amount::new(7),
            },
        ])
        .unwrap();
        let market = Market::opening(Tick(0), Amount::ZERO).unwrap();
        assert!(charged.per_second(&market, Tick(9)).is_err());
        assert_eq!(
            charged.per_second(&market, Tick(10)).unwrap(),
            Amount::new(5)
        );
        assert_eq!(
            charged.per_second(&market, Tick(19)).unwrap(),
            Amount::new(5)
        );
        assert_eq!(
            charged.per_second(&market, Tick(20)).unwrap(),
            Amount::new(7)
        );
        assert_eq!(
            charged.per_second(&market, Tick(9_999)).unwrap(),
            Amount::new(7)
        );
    }

    /// A shuffled recording is one somebody assembled wrongly, and sorting it would put a rate
    /// against a tick that never carried it.
    #[test]
    fn a_recording_out_of_order_is_refused() {
        assert!(
            RecordedRate::new(vec![
                Charged {
                    from: Tick(20),
                    per_second: Amount::new(7)
                },
                Charged {
                    from: Tick(10),
                    per_second: Amount::new(5)
                },
            ])
            .is_err()
        );
    }

    #[test]
    fn a_fixed_rate_ignores_what_it_is_pricing() {
        let rate = FixedRate(Amount::new(7));
        let market = Market::opening(Tick(0), Amount::ZERO).unwrap();
        assert_eq!(rate.per_second(&market, Tick(0)).unwrap(), Amount::new(7));
        assert_eq!(
            rate.per_second(&market, Tick(9_999)).unwrap(),
            Amount::new(7)
        );
    }
}
