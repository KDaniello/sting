//! The program the corpus is generated from.
//!
//! Chosen for the edges rather than for realism. Every case is a transition where two
//! implementations of this arithmetic could first part company: a supply stated in shares
//! instead of assets, a borrow one unit past the threshold and one unit inside it, a repayment
//! that names more than the market records as owed, a liquidation that leaves collateral and one
//! that takes the last of it.

use alloy_sol_types::SolCall;
use sting_karman_mech_lending::{Given, LendingAction, Position, Wad};
use sting_karman_types::{AgentId, Amount, MarketId, Tick, Wide};
use sting_karman_verify::differential::Outcome;

use crate::{
    evm::{acting_as, MarketParams, Reference},
    Case, Corpus, Provenance, Quoted,
};

/// The threshold the corpus market is opened at: eighty percent, where a mainstream collateral
/// pair is listed.
pub const LLTV: Amount<Wad> = Amount::new(800_000_000_000_000_000);

/// The rate the model answers with, unchanged throughout: ten percent a year, per second.
///
/// Level 0 checks the arithmetic around a rate, not which rate a curve would have produced, and
/// a rate that moved would put the curve into every case without adding a single edge.
pub const RATE: Amount<Wad> = Amount::new(3_170_979_198);

/// Who the market pays its fee to. Agent zero, which also acts, so the case where the recipient
/// is the one transacting is in the corpus rather than only in the properties.
pub const FEE_RECIPIENT: AgentId = AgentId(0);

/// Everyone the corpus reads a position for.
pub const ACTORS: [AgentId; 4] = [AgentId(0), AgentId(1), AgentId(2), AgentId(3)];

const TREASURY: AgentId = AgentId(0);
const ALICE: AgentId = AgentId(1);
const BOB: AgentId = AgentId(2);
const CAROL: AgentId = AgentId(3);

const MARKET: MarketId = MarketId(0);

/// One collateral unit for one loan unit.
const PARITY: u128 = 1_000_000_000_000_000_000_000_000_000_000_000_000;

/// Half of that: far enough that a position opened at the threshold and paid down a third is
/// still worth less than it owes, which is what a liquidation needs to be allowed at all.
const FALLEN: u128 = 500_000_000_000_000_000_000_000_000_000_000_000;

/// A tenth of the interest to the fee recipient.
const FEE: u128 = 100_000_000_000_000_000;

/// A price the width a quantity is carried in cannot hold, and the fall that follows it.
///
/// `6.48e38` is what a market of cbBTC against USDC quotes: `1e36` of scale, eight decimals of
/// collateral against six of loan, and a coin worth tens of thousands. It is 1.9 times what 128
/// bits hold, so a workspace that valued a position in that width could not run this market at
/// all — and the only way to know it values one correctly is to ask the reference.
const HUGE: &str = "648000000000000000000000000000000000000";
const HUGE_FALLEN: &str = "341000000000000000000000000000000000000";

/// Drives the reference through the program and collects what it did.
pub(crate) fn run(code: &[u8], reference: Provenance) -> Result<Corpus, String> {
    let mut machine = Reference::open(
        code,
        LLTV.base_units(),
        FEE_RECIPIENT,
        Wide::from_u128(PARITY),
        RATE.base_units(),
        ACTORS.to_vec(),
    )?;
    let mut program = Program {
        reference: &mut machine,
        cases: Vec::new(),
        prices: vec![Quoted { from: Tick(0), price: Wide::from_u128(PARITY) }],
    };
    program.play()?;
    Ok(Corpus {
        reference,
        lltv: LLTV,
        fee_recipient: FEE_RECIPIENT,
        rate: RATE,
        prices: program.prices,
        cases: program.cases,
    })
}

struct Program<'a> {
    reference: &'a mut Reference,
    cases: Vec<Case>,
    prices: Vec<Quoted>,
}

impl Program<'_> {
    fn play(&mut self) -> Result<(), String> {
        self.act("a first supply prices against the virtual pair", 0, ALICE, supply(10_000_000))?;
        self.act(
            "a supply stated in shares rather than assets",
            0,
            BOB,
            LendingAction::Supply {
                market: MARKET,
                given: Given::Shares(Amount::new(5_000_000_000_000)),
            },
        )?;
        self.act("a supply of nothing", 0, CAROL, supply(0))?;
        self.act(
            "collateral, which is not liquidity",
            0,
            BOB,
            LendingAction::SupplyCollateral { market: MARKET, assets: Amount::new(2_000_000) },
        )?;
        self.act("a borrow one unit past the threshold", 0, BOB, borrow(1_600_001))?;
        self.act("a borrow exactly at the threshold", 0, BOB, borrow(1_600_000))?;
        self.act(
            "a withdrawal against what is left idle",
            0,
            ALICE,
            LendingAction::Withdraw { market: MARKET, given: Given::Assets(Amount::new(1)) },
        )?;
        self.act(
            "a withdrawal of more than is held",
            0,
            CAROL,
            LendingAction::Withdraw { market: MARKET, given: Given::Shares(Amount::new(1)) },
        )?;

        self.reference.fee(FEE)?;
        self.act("the first transition of a tick accrues", 1, ALICE, supply(1))?;
        self.act("the second transition of the same tick does not", 1, ALICE, supply(1))?;
        self.act("accrual over a long gap, with a fee to mint", 12_345, CAROL, supply(3))?;

        let owed = self.held(BOB)?.borrow_shares.base_units();
        self.act(
            "a repayment stated in shares",
            12_345,
            BOB,
            LendingAction::Repay { market: MARKET, given: Given::Shares(Amount::new(owed / 3)) },
        )?;
        self.act(
            "a repayment stated in assets",
            12_345,
            BOB,
            LendingAction::Repay { market: MARKET, given: Given::Assets(Amount::new(7)) },
        )?;
        self.act(
            "a borrow stated in shares",
            12_345,
            BOB,
            LendingAction::Borrow {
                market: MARKET,
                given: Given::Shares(Amount::new(1_000_000_000)),
            },
        )?;
        self.act(
            "collateral taken back from a position that can spare it",
            12_345,
            BOB,
            LendingAction::WithdrawCollateral { market: MARKET, assets: Amount::new(1) },
        )?;
        self.act(
            "collateral the debt still needs",
            12_345,
            BOB,
            LendingAction::WithdrawCollateral { market: MARKET, assets: Amount::new(1_000_000) },
        )?;

        self.price(12_346, Wide::from_u128(FALLEN))?;
        self.act(
            "a liquidation that leaves collateral behind",
            12_346,
            CAROL,
            seize(BOB, 500_000),
        )?;
        let left = self.held(BOB)?.collateral.base_units();
        self.act(
            "a liquidation that takes the last of the collateral",
            12_346,
            CAROL,
            seize(BOB, left),
        )?;
        self.act("a liquidation of a position that is gone", 12_346, CAROL, seize(BOB, 1))?;

        let treasury = self.held(TREASURY)?.supply_shares.base_units();
        self.act(
            "the fee recipient withdrawing what accrual minted it",
            12_347,
            TREASURY,
            LendingAction::Withdraw { market: MARKET, given: Given::Shares(Amount::new(treasury)) },
        )?;
        let last = self.held(ALICE)?.supply_shares;
        self.act(
            "the last supplier withdrawing everything left",
            12_400,
            ALICE,
            LendingAction::Withdraw { market: MARKET, given: Given::Shares(last) },
        )?;

        // Everything from here is quoted above the width a quantity is carried in. Valuing a
        // position, buying collateral with a repayment and writing off what is left are three
        // different pieces of arithmetic at that width, and each one is answered by the
        // reference rather than assumed to carry over from the narrow prices above.
        self.price(12_401, quoted(HUGE))?;
        self.act(
            "a supply into a market quoted wider than money",
            12_401,
            ALICE,
            supply(10_000_000),
        )?;
        self.act(
            "collateral pledged at a price wider than money",
            12_401,
            BOB,
            LendingAction::SupplyCollateral { market: MARKET, assets: Amount::new(20_000) },
        )?;
        self.act(
            "a borrow one unit past a threshold wider than money",
            12_401,
            BOB,
            borrow(10_368_001),
        )?;
        self.act(
            "a borrow against collateral quoted wider than money",
            12_401,
            BOB,
            borrow(9_000_000),
        )?;

        self.price(12_402, quoted(HUGE_FALLEN))?;
        self.act("a liquidation at a price wider than money", 12_402, CAROL, seize(BOB, 15_000))?;
        self.act("the write-off a wide price leaves behind", 12_402, CAROL, seize(BOB, 5_000))?;
        Ok(())
    }

    /// Puts one transition to the reference and records what it made of it.
    fn act(
        &mut self,
        probes: &str,
        at: u64,
        actor: AgentId,
        action: LendingAction,
    ) -> Result<(), String> {
        let now = Tick(at);
        self.reference.at(now);
        let state = self.reference.state()?;
        let data = calldata(&self.reference.params(), actor, action)?;
        let outcome = match self.reference.send(actor, data) {
            Ok(settled) => {
                let events = self.reference.events(&settled)?;
                Outcome::Applied { state: self.reference.state()?, events }
            }
            Err(_) => Outcome::Rejected,
        };
        self.cases.push(Case { probes: probes.to_owned(), state, actor, at: now, action, outcome });
        Ok(())
    }

    fn price(&mut self, at: u64, price: Wide) -> Result<(), String> {
        self.reference.at(Tick(at));
        self.reference.price(price)?;
        self.prices.push(Quoted { from: Tick(at), price });
        Ok(())
    }

    fn held(&mut self, who: AgentId) -> Result<Position, String> {
        Ok(self.reference.state()?.position(MARKET, who))
    }
}

/// The side an action states, as the reference's pair of an asset amount and a share amount
/// with exactly one of them zero.
fn either<A: sting_karman_types::Unit, S: sting_karman_types::Unit>(
    given: Given<A, S>,
) -> (alloy_sol_types::private::U256, alloy_sol_types::private::U256) {
    use alloy_sol_types::private::U256;
    match given {
        Given::Assets(assets) => (U256::from(assets.base_units()), U256::ZERO),
        Given::Shares(shares) => (U256::ZERO, U256::from(shares.base_units())),
    }
}

/// One of this program's own prices, which are written down rather than computed.
fn quoted(price: &str) -> Wide {
    price.parse().expect("the prices this program states are readable")
}

fn supply(assets: u128) -> LendingAction {
    LendingAction::Supply { market: MARKET, given: Given::Assets(Amount::new(assets)) }
}

fn borrow(assets: u128) -> LendingAction {
    LendingAction::Borrow { market: MARKET, given: Given::Assets(Amount::new(assets)) }
}

fn seize(borrower: AgentId, collateral: u128) -> LendingAction {
    LendingAction::Liquidate {
        market: MARKET,
        borrower,
        given: Given::Assets(Amount::new(collateral)),
    }
}

/// One action as the reference's own interface takes it.
///
/// The duality the action carries is the duality the reference asks for: it takes an asset
/// amount and a share amount together and requires exactly one of them to be zero, so a side
/// stated here is that side and the other is a zero.
fn calldata(
    params: &MarketParams,
    actor: AgentId,
    action: LendingAction,
) -> Result<Vec<u8>, String> {
    use alloy_sol_types::private::U256;

    use crate::evm::{
        borrowCall, liquidateCall, repayCall, supplyCall, supplyCollateralCall, withdrawCall,
        withdrawCollateralCall,
    };

    let me = acting_as(actor);
    let terms = params.clone();
    Ok(match action {
        LendingAction::Supply { given, .. } => {
            let (assets, shares) = either(given);
            supplyCall {
                marketParams: terms,
                assets,
                shares,
                onBehalf: me,
                data: Default::default(),
            }
            .abi_encode()
        }
        LendingAction::Withdraw { given, .. } => {
            let (assets, shares) = either(given);
            withdrawCall { marketParams: terms, assets, shares, onBehalf: me, receiver: me }
                .abi_encode()
        }
        LendingAction::Borrow { given, .. } => {
            let (assets, shares) = either(given);
            borrowCall { marketParams: terms, assets, shares, onBehalf: me, receiver: me }
                .abi_encode()
        }
        LendingAction::Repay { given, .. } => {
            let (assets, shares) = either(given);
            repayCall {
                marketParams: terms,
                assets,
                shares,
                onBehalf: me,
                data: Default::default(),
            }
            .abi_encode()
        }
        LendingAction::SupplyCollateral { assets, .. } => supplyCollateralCall {
            marketParams: terms,
            assets: U256::from(assets.base_units()),
            onBehalf: me,
            data: Default::default(),
        }
        .abi_encode(),
        LendingAction::WithdrawCollateral { assets, .. } => withdrawCollateralCall {
            marketParams: terms,
            assets: U256::from(assets.base_units()),
            onBehalf: me,
            receiver: me,
        }
        .abi_encode(),
        LendingAction::Liquidate { borrower, given, .. } => {
            let (seized, repaid) = either(given);
            liquidateCall {
                marketParams: terms,
                borrower: acting_as(borrower),
                seizedAssets: seized,
                repaidShares: repaid,
                data: Default::default(),
            }
            .abi_encode()
        }
    })
}
