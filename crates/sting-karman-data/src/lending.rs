//! Pulling a live lending market into a fixture.
//!
//! What crosses this boundary is a node's answer to three questions — how a market is listed,
//! what it holds, and what one holder holds in it — and what leaves it is a plain value with its
//! origin attached.
//! The decoding is here because that is where a mistake is silent: a word read at the wrong
//! offset, or a balance wider than money is carried in, becomes a simulation whose initial
//! conditions were never what anybody thought they were.
//!
//! The transport is not here. Reaching a node is a handful of calls made by hand, once per
//! fixture, and
//! putting a TLS stack into the dependency path of every binary this workspace ships would be
//! paying for that once-in-a-while act on every build. What the rule protects is that a run
//! never fetches, and a run cannot fetch through a decoder.
//!
//! The calls, and what is done with what they answer:
//!
//! ```text
//! eth_call { to: <contract>, data: 0x2c3c9157 <market id> }              idToMarketParams(bytes32)
//! eth_call { to: <oracle>,   data: 0xa035b1fe }                          price()
//! eth_call { to: <token>,    data: 0x313ce567 }                          decimals()
//! eth_call { to: <contract>, data: 0x5c60e39a <market id> }              market(bytes32)
//! eth_call { to: <contract>, data: 0x93c52062 <market id> <holder> }     position(bytes32,address)
//!
//! { "origin": { … }, "terms": { "params": "0x…", "price": "0x…", "loan_decimals": "0x…",
//!                                 "collateral_decimals": "0x…" },
//!   "market": "0x…", "holders": [ { "holder": "0x…", "answer": "0x…" } ] }
//!   | cargo run -p sting-karman-data --bin pull-market corpus/karman/fixtures/<name>.json
//! ```
//!
//! Which holders to ask about is a modelling decision rather than a mechanical one — a live
//! market has more of them than anyone wants to name, and the ones worth naming are the ones a
//! study is about. Everybody else is [`PulledMarket::unaccounted`].

use serde::{Deserialize, Serialize};
use sting_karman_types::{Wide, u128_str};
use thiserror::Error;

/// A node's answer could not be read as the value it was asked for.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum PullError {
    /// The answer was not the number of words the getter returns.
    #[error("expected {expected} words in the answer, found {found}")]
    Shape {
        /// How many the getter returns.
        expected: usize,
        /// How many arrived.
        found: usize,
    },
    /// A word was not hexadecimal, or held more than the type it is read into.
    #[error("word {at} does not fit the value it stands for")]
    Width {
        /// Which word, counting from zero.
        at: usize,
    },
    /// The holders name more of the market than the market says exists.
    #[error("the holders hold more {of} than the market issued")]
    Oversubscribed {
        /// Which ledger disagrees.
        of: &'static str,
    },
    /// The oracle prices the collateral at nothing.
    #[error("the oracle prices this market's collateral at zero")]
    Unpriced,
    /// More is owed to the market than was ever supplied to it.
    #[error("the market is owed {owed} against {supplied} supplied")]
    Overdrawn {
        /// What the market says is owed.
        owed: u128,
        /// What it says was supplied.
        supplied: u128,
    },
}

/// Where a pulled market came from, in enough detail to ask the same question again.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Origin {
    /// The chain that answered.
    pub chain_id: u64,
    /// Which protocol's market this is.
    pub protocol: String,
    /// The contract that holds it.
    pub contract: String,
    /// The market's own identifier within that contract.
    pub market: String,
    /// The block the answers are as of. A block rather than a clock: it is exact, and it is what
    /// somebody checking this would have to quote anyway.
    pub block: u64,
}

/// What a node answered about how a market is listed.
///
/// Four calls, kept apart from one another by name rather than by position: two of them are a
/// token's decimals, and a pull that swapped them would produce a fixture that is wrong by two
/// orders of magnitude and readable throughout.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Listing {
    /// What `idToMarketParams` answered.
    pub params: String,
    /// What the oracle answered.
    pub price: String,
    /// What the loan token's `decimals` answered.
    pub loan_decimals: String,
    /// What the collateral token's `decimals` answered.
    pub collateral_decimals: String,
}

/// The terms a market is listed under, and what its oracle said.
///
/// Facts of the chain rather than choices of a study, which is why they travel with the market
/// instead of being restated wherever it is used. A price quoted against one market and a
/// threshold from another produce a simulation that looks entirely plausible and is about
/// nothing, and there is no way to notice from inside it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Terms {
    /// The asset lent and borrowed.
    pub loan_token: String,
    /// How many base units make one of it.
    pub loan_decimals: u8,
    /// The asset pledged.
    pub collateral_token: String,
    /// How many base units make one of that.
    pub collateral_decimals: u8,
    /// What prices the collateral.
    pub oracle: String,
    /// What prices the debt. Not modelled by this workspace, and named so a reader knows which
    /// curve a run is not reproducing.
    pub irm: String,
    /// The share of its collateral's value a position may owe, at WAD.
    #[serde(with = "u128_str")]
    pub lltv: u128,
    /// What the oracle answered at the block this was pulled at, at its own scale.
    ///
    /// Wider than money because the oracle interface is: this market quotes `6.48e38`.
    pub price: Wide,
}

impl Terms {
    /// Reads the four answers that say how a market is listed.
    ///
    /// # Errors
    ///
    /// Fails when an answer is not the shape its getter returns, when a word holds more than the
    /// field it lands in, or when the oracle prices the collateral at nothing — which is not a
    /// market under stress but a market whose collateral has stopped being collateral.
    pub fn decode(listing: &Listing) -> Result<Terms, PullError> {
        let listed = words(&listing.params, 5)?;
        let price = wide(&words(&listing.price, 1)?, 0)?;
        if price == Wide::ZERO {
            return Err(PullError::Unpriced);
        }
        Ok(Terms {
            loan_token: address(&listed, 0)?,
            loan_decimals: decimals(&listing.loan_decimals)?,
            collateral_token: address(&listed, 1)?,
            collateral_decimals: decimals(&listing.collateral_decimals)?,
            oracle: address(&listed, 2)?,
            irm: address(&listed, 3)?,
            lltv: number(&listed, 4)?,
            price,
        })
    }
}

/// What one holder holds, as the chain reports it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PulledPosition {
    /// The address, or a name for the holders a pull did not enumerate.
    pub holder: String,
    /// Its claim on the supply side.
    #[serde(with = "u128_str")]
    pub supply_shares: u128,
    /// Its share of the debt.
    #[serde(with = "u128_str")]
    pub borrow_shares: u128,
    /// What backs that debt.
    #[serde(with = "u128_str")]
    pub collateral: u128,
}

/// A lending market as a node answered for it, and the positions a pull could name.
///
/// A pull names the holders it could find, which is never all of them: enumerating every address
/// that ever touched a live market means reading its whole history. What the rest of them hold
/// is exactly what the totals do not account for, and [`PulledMarket::unaccounted`] is that
/// remainder — one holder standing for everybody the pull did not reach, which is also how a
/// simulation treats them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PulledMarket {
    /// Where this came from.
    pub origin: Origin,
    /// Everything supplied.
    #[serde(with = "u128_str")]
    pub total_supply_assets: u128,
    /// The shares dividing it.
    #[serde(with = "u128_str")]
    pub total_supply_shares: u128,
    /// Everything owed.
    #[serde(with = "u128_str")]
    pub total_borrow_assets: u128,
    /// The shares dividing that.
    #[serde(with = "u128_str")]
    pub total_borrow_shares: u128,
    /// The timestamp interest was last accrued to.
    pub last_update: u64,
    /// The share of interest the market takes.
    #[serde(with = "u128_str")]
    pub fee: u128,
    /// What it is listed under, and what it was quoted at, when a pull asked.
    ///
    /// Absent where nobody asked. A replay of a recorded episode already carries its threshold
    /// and every price it needs, so it never makes these calls; a study opening a market to run
    /// something new on it has nowhere else to get them, and refuses a market without them
    /// rather than inventing either.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub terms: Option<Terms>,
    /// The holders the pull could name.
    pub holders: Vec<PulledPosition>,
}

impl PulledMarket {
    /// Reads what the market getter answered: six totals, in the order it returns them.
    ///
    /// # Errors
    ///
    /// Fails when the answer is not six words, or when one of them holds more than the width
    /// money is carried in.
    /// Refuses a market owing more than it holds, which is the reference's own invariant and so
    /// is never what a chain answered. Left to be discovered later it would be a state the
    /// mechanism declares impossible — caught by an invariant check that a sweep turns off, and
    /// by nothing at all when it is off.
    pub fn decode(origin: Origin, terms: Option<Terms>, answer: &str) -> Result<Self, PullError> {
        let held = words(answer, 6)?;
        let (supplied, owed) = (number(&held, 0)?, number(&held, 2)?);
        if owed > supplied {
            return Err(PullError::Overdrawn { owed, supplied });
        }
        Ok(PulledMarket {
            origin,
            total_supply_assets: number(&held, 0)?,
            total_supply_shares: number(&held, 1)?,
            total_borrow_assets: number(&held, 2)?,
            total_borrow_shares: number(&held, 3)?,
            last_update: u64::try_from(number(&held, 4)?)
                .map_err(|_| PullError::Width { at: 4 })?,
            fee: number(&held, 5)?,
            terms,
            holders: Vec::new(),
        })
    }

    /// Reads what the position getter answered for one holder, and records it.
    ///
    /// A holder with nothing in the market is not recorded: an address that appears in a log and
    /// has since closed out is not a position, and carrying it would put an empty row into every
    /// state built from this.
    ///
    /// # Errors
    ///
    /// Fails when the answer is not three words, or when one of them is too wide.
    pub fn with_holder(mut self, holder: &str, answer: &str) -> Result<Self, PullError> {
        let held = words(answer, 3)?;
        let position = PulledPosition {
            holder: holder.to_owned(),
            supply_shares: number(&held, 0)?,
            borrow_shares: number(&held, 1)?,
            collateral: number(&held, 2)?,
        };
        if position.supply_shares != 0 || position.borrow_shares != 0 || position.collateral != 0 {
            self.holders.push(position);
        }
        Ok(self)
    }

    /// The one holder standing for everybody the pull did not name.
    ///
    /// Its collateral is zero and stays zero. Collateral belongs to positions rather than to the
    /// market, so the totals say nothing about how much of it the unnamed holders have pledged —
    /// and a residual that invented some would be putting a number nobody measured into the
    /// initial conditions of every run built from this.
    ///
    /// # Errors
    ///
    /// Fails when the named holders already hold more than the market issued, which would mean
    /// the answers came from different blocks.
    pub fn unaccounted(&self, name: &str) -> Result<PulledPosition, PullError> {
        let named = |take: fn(&PulledPosition) -> u128| {
            self.holders
                .iter()
                .map(take)
                .try_fold(0u128, u128::checked_add)
        };
        let supply = named(|held| held.supply_shares).ok_or(PullError::Oversubscribed {
            of: "supply shares",
        })?;
        let borrow = named(|held| held.borrow_shares)
            .ok_or(PullError::Oversubscribed { of: "debt shares" })?;
        Ok(PulledPosition {
            holder: name.to_owned(),
            supply_shares: self.total_supply_shares.checked_sub(supply).ok_or(
                PullError::Oversubscribed {
                    of: "supply shares",
                },
            )?,
            borrow_shares: self
                .total_borrow_shares
                .checked_sub(borrow)
                .ok_or(PullError::Oversubscribed { of: "debt shares" })?,
            collateral: 0,
        })
    }
}

/// One answer as the fixed-width words a getter returns it in.
fn words(answer: &str, expected: usize) -> Result<Vec<&str>, PullError> {
    let body = answer.strip_prefix("0x").unwrap_or(answer);
    let found = body.len() / 64;
    if found != expected || !body.len().is_multiple_of(64) {
        return Err(PullError::Shape { expected, found });
    }
    Ok((0..expected)
        .map(|at| &body[at * 64..at * 64 + 64])
        .collect())
}

/// One word as a value of the width a price is carried in.
fn wide(held: &[&str], at: usize) -> Result<Wide, PullError> {
    let word = held.get(at).ok_or(PullError::Width { at })?;
    Wide::from_hex(word).map_err(|_| PullError::Width { at })
}

/// One word as the address it holds, which is its last twenty bytes.
fn address(held: &[&str], at: usize) -> Result<String, PullError> {
    let word = held.get(at).ok_or(PullError::Width { at })?;
    let (padding, body) = word.split_at(24);
    if padding.bytes().any(|digit| digit != b'0') {
        return Err(PullError::Width { at });
    }
    Ok(format!("0x{body}"))
}

/// A `decimals` answer, which is one word holding a very small number.
fn decimals(answer: &str) -> Result<u8, PullError> {
    let held = words(answer, 1)?;
    u8::try_from(number(&held, 0)?).map_err(|_| PullError::Width { at: 0 })
}

/// One word as the number it stands for, refusing anything wider than money is carried in.
fn number(held: &[&str], at: usize) -> Result<u128, PullError> {
    let word = held.get(at).ok_or(PullError::Width { at })?;
    let (high, low) = word.split_at(32);
    if high.bytes().any(|digit| digit != b'0') {
        return Err(PullError::Width { at });
    }
    u128::from_str_radix(low, 16).map_err(|_| PullError::Width { at })
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

    fn terms() -> Terms {
        Terms {
            loan_token: "0xa0b8".into(),
            loan_decimals: 6,
            collateral_token: "0xcbb7".into(),
            collateral_decimals: 8,
            oracle: "0xa6d6".into(),
            irm: "0x870a".into(),
            lltv: 860_000_000_000_000_000,
            price: "648000000000000000000000000000000000000".parse().unwrap(),
        }
    }

    /// What the node actually answered for the market this workspace pulls, word for word.
    fn quoted() -> Listing {
        Listing {
            params: concat!(
                "0x000000000000000000000000a0b86991c6218b36c1d19d4a2e9eb0ce3606eb48",
                "000000000000000000000000cbb7c0000ab88b473b1f5afd9ef808440eed33bf",
                "000000000000000000000000a6d6950c9f177f1de7f7757fb33539e3ec60182a",
                "000000000000000000000000870ac11d48b15db9a138cf899d20f13f79ba00bc",
                "0000000000000000000000000000000000000000000000000bef55718ad60000",
            )
            .into(),
            price: "0x00000000000000000000000000000001e78042a954d9eeedd76aa08000000000".into(),
            loan_decimals: word(6),
            collateral_decimals: word(8),
        }
    }

    /// The five addresses and the threshold land where the getter puts them, and the price the
    /// oracle answered is read at a width no quantity has: this market quotes `6.48e38`.
    #[test]
    fn a_listing_reads_as_the_market_it_describes() {
        let read = Terms::decode(&quoted()).unwrap();
        assert_eq!(
            read.loan_token,
            "0xa0b86991c6218b36c1d19d4a2e9eb0ce3606eb48"
        );
        assert_eq!(
            read.collateral_token,
            "0xcbb7c0000ab88b473b1f5afd9ef808440eed33bf"
        );
        assert_eq!(read.oracle, "0xa6d6950c9f177f1de7f7757fb33539e3ec60182a");
        assert_eq!(read.irm, "0x870ac11d48b15db9a138cf899d20f13f79ba00bc");
        assert_eq!(read.lltv, 860_000_000_000_000_000);
        assert_eq!(
            read.price.to_string(),
            "648000000000000000000000000000000000000"
        );
        assert_eq!((read.loan_decimals, read.collateral_decimals), (6, 8));
    }

    /// The two decimals are the pair most easily read the wrong way round, and nothing in a
    /// number says which token it counts. They are named at the boundary, so a swap is a swap of
    /// two named answers rather than of two adjacent numbers.
    #[test]
    fn the_two_decimals_are_told_apart_by_name() {
        let swapped = Listing {
            loan_decimals: word(8),
            collateral_decimals: word(6),
            ..quoted()
        };
        let read = Terms::decode(&swapped).unwrap();
        assert_eq!((read.loan_decimals, read.collateral_decimals), (8, 6));
    }

    /// An oracle answering nothing is not a market under stress: collateral worth nothing is
    /// collateral that has stopped being collateral, and no study of it would mean anything.
    #[test]
    fn a_market_priced_at_nothing_is_refused() {
        let unpriced = Listing {
            price: word(0),
            ..quoted()
        };
        assert_eq!(Terms::decode(&unpriced), Err(PullError::Unpriced));
    }

    /// A word carrying more than an address is not an address with the extra ignored. Twelve
    /// zero bytes are what makes the last twenty an address, and a getter answering otherwise
    /// answered something else.
    #[test]
    fn a_word_that_is_not_an_address_is_refused() {
        let stuffed = Listing {
            params: format!("0x{}{}", "1".repeat(24), &quoted().params[26..]),
            ..quoted()
        };
        assert_eq!(Terms::decode(&stuffed), Err(PullError::Width { at: 0 }));
    }

    /// A getter's answer is as many words as the getter returns, and an answer of any other
    /// shape came from something else.
    #[test]
    fn a_listing_of_the_wrong_shape_is_refused() {
        let short = Listing {
            params: format!("0x{}", "0".repeat(64 * 4)),
            ..quoted()
        };
        assert_eq!(
            Terms::decode(&short),
            Err(PullError::Shape {
                expected: 5,
                found: 4
            })
        );
        let wide = Listing {
            price: format!("0x{}", "0".repeat(128)),
            ..quoted()
        };
        assert_eq!(
            Terms::decode(&wide),
            Err(PullError::Shape {
                expected: 1,
                found: 2
            })
        );
    }

    fn origin() -> Origin {
        Origin {
            chain_id: 1,
            protocol: "morpho-blue".into(),
            contract: "0xBBBB".into(),
            market: "0x64d6".into(),
            block: 25_789_266,
        }
    }

    fn word(value: u128) -> String {
        format!("{value:064x}")
    }

    fn answer(values: &[u128]) -> String {
        format!("0x{}", values.iter().map(|v| word(*v)).collect::<String>())
    }

    #[test]
    fn a_market_reads_as_the_six_totals_the_getter_returns() {
        let pulled =
            PulledMarket::decode(origin(), Some(terms()), &answer(&[10, 2, 3, 4, 5, 6])).unwrap();
        assert_eq!(pulled.total_supply_assets, 10);
        assert_eq!(pulled.total_supply_shares, 2);
        assert_eq!(pulled.total_borrow_assets, 3);
        assert_eq!(pulled.total_borrow_shares, 4);
        assert_eq!(pulled.last_update, 5);
        assert_eq!(pulled.fee, 6);
    }

    /// An answer of the wrong length is a different getter, or a node that failed while
    /// answering. Reading the words that happen to be there would be reading someone else's
    /// numbers into a market.
    #[test]
    fn an_answer_of_the_wrong_shape_is_refused() {
        assert_eq!(
            PulledMarket::decode(origin(), Some(terms()), &answer(&[1, 2, 3])),
            Err(PullError::Shape {
                expected: 6,
                found: 3
            })
        );
        assert_eq!(
            PulledMarket::decode(origin(), Some(terms()), "0xabc"),
            Err(PullError::Shape {
                expected: 6,
                found: 0
            })
        );
    }

    /// The chain carries these in words twice as wide as money is. A value using the upper half
    /// is not a quantity this workspace can hold, and truncating it would be inventing one.
    #[test]
    fn a_value_wider_than_money_is_refused() {
        let wide = format!("0x1{}", "0".repeat(63));
        let answer = format!("{wide}{}", word(0).repeat(5));
        assert_eq!(
            PulledMarket::decode(origin(), Some(terms()), &answer),
            Err(PullError::Width { at: 0 })
        );
    }

    /// A market owing more than it holds is the one thing the reference itself will not produce,
    /// and opening one would put a market that never existed under a run.
    #[test]
    fn a_market_owing_more_than_it_holds_is_refused() {
        assert_eq!(
            PulledMarket::decode(origin(), Some(terms()), &answer(&[100, 1, 101, 1, 1, 0])),
            Err(PullError::Overdrawn {
                owed: 101,
                supplied: 100
            })
        );
        assert!(
            PulledMarket::decode(origin(), Some(terms()), &answer(&[100, 1, 100, 1, 1, 0])).is_ok()
        );
    }

    #[test]
    fn a_holder_with_nothing_in_the_market_is_not_a_position() {
        let pulled = PulledMarket::decode(origin(), Some(terms()), &answer(&[9, 9, 9, 9, 1, 0]))
            .unwrap()
            .with_holder("0xaa", &answer(&[0, 0, 0]))
            .unwrap()
            .with_holder("0xbb", &answer(&[0, 0, 7]))
            .unwrap();
        assert_eq!(pulled.holders.len(), 1);
        assert_eq!(pulled.holders[0].holder, "0xbb");
    }

    /// The remainder is what makes a partial pull a whole market: the named holders plus this one
    /// account for every share the market issued, which is what any state built from it has to
    /// satisfy.
    #[test]
    fn the_unnamed_holders_account_for_the_rest() {
        let pulled = PulledMarket::decode(
            origin(),
            Some(terms()),
            &answer(&[100, 1_000, 40, 400, 1, 0]),
        )
        .unwrap()
        .with_holder("0xaa", &answer(&[600, 100, 5]))
        .unwrap();
        let rest = pulled.unaccounted("everyone else").unwrap();
        assert_eq!(rest.supply_shares, 400);
        assert_eq!(rest.borrow_shares, 300);
        assert_eq!(
            rest.collateral, 0,
            "a residual invented collateral nobody measured"
        );
    }

    /// Answers taken from different blocks can add up to more than the market issued, and a
    /// market that cannot be reconciled is refused rather than silently floored at zero.
    #[test]
    fn holders_holding_more_than_was_issued_are_refused() {
        let pulled = PulledMarket::decode(origin(), Some(terms()), &answer(&[100, 10, 0, 0, 1, 0]))
            .unwrap()
            .with_holder("0xaa", &answer(&[11, 0, 0]))
            .unwrap();
        assert_eq!(
            pulled.unaccounted("everyone else"),
            Err(PullError::Oversubscribed {
                of: "supply shares"
            })
        );
    }

    /// A fixture is read back by machines that never saw the pull, so the shape has to survive
    /// the boundary with its full width intact.
    #[test]
    fn a_pulled_market_survives_the_serialisation_boundary() {
        let pulled = PulledMarket::decode(
            origin(),
            Some(terms()),
            &answer(&[u128::MAX, 2, 3, 4, 5, 6]),
        )
        .unwrap()
        .with_holder("0xaa", &answer(&[u128::MAX, 1, 1]))
        .unwrap();
        let restored: PulledMarket =
            serde_json::from_str(&serde_json::to_string(&pulled).unwrap()).unwrap();
        assert_eq!(restored, pulled);
    }
}
