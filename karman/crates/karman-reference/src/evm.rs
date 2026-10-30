//! The reference under an EVM: its deployed bytecode, the stubs it talks to, and the reads that
//! turn its storage back into a state this workspace can compare.

use std::collections::BTreeMap;

use alloy_sol_types::{sol, SolCall, SolEvent, SolValue};
use sting_karman_mech_lending::{LendingState, Market, Position};
use sting_karman_types::{AgentId, Amount, MarketId, Tick, Wide};
use revm::{
    context::{BlockEnv, CfgEnv, Context, Journal, TxEnv},
    context_interface::result::{ExecutionResult, Output},
    database::{CacheDB, EmptyDB},
    primitives::{address, keccak256, Address, Bytes, Log, TxKind, B256, U256},
    state::{AccountInfo, Bytecode},
    ExecuteCommitEvm, ExecuteEvm, MainBuilder, MainContext, MainnetEvm,
};

sol! {
    /// The terms a market is identified by. Its ABI encoding, hashed, is the market's id.
    #[derive(Debug)]
    struct MarketParams {
        address loanToken;
        address collateralToken;
        address oracle;
        address irm;
        uint256 lltv;
    }

    function owner() external view returns (address);
    function enableIrm(address irm) external;
    function enableLltv(uint256 lltv) external;
    function setFeeRecipient(address newFeeRecipient) external;
    function createMarket(MarketParams marketParams) external;
    function setFee(MarketParams marketParams, uint256 newFee) external;
    function accrueInterest(MarketParams marketParams) external;

    function supply(MarketParams marketParams, uint256 assets, uint256 shares, address onBehalf, bytes data)
        external returns (uint256, uint256);
    function withdraw(MarketParams marketParams, uint256 assets, uint256 shares, address onBehalf, address receiver)
        external returns (uint256, uint256);
    function borrow(MarketParams marketParams, uint256 assets, uint256 shares, address onBehalf, address receiver)
        external returns (uint256, uint256);
    function repay(MarketParams marketParams, uint256 assets, uint256 shares, address onBehalf, bytes data)
        external returns (uint256, uint256);
    function supplyCollateral(MarketParams marketParams, uint256 assets, address onBehalf, bytes data) external;
    function withdrawCollateral(MarketParams marketParams, uint256 assets, address onBehalf, address receiver)
        external;
    function liquidate(
        MarketParams marketParams, address borrower, uint256 seizedAssets, uint256 repaidShares, bytes data
    ) external returns (uint256, uint256);

    function market(bytes32 id)
        external view returns (uint128, uint128, uint128, uint128, uint128, uint128);
    function position(bytes32 id, address user) external view returns (uint256, uint128, uint128);

    event Supply(bytes32 indexed id, address indexed caller, address indexed onBehalf, uint256 assets, uint256 shares);
    event Withdraw(
        bytes32 indexed id, address caller, address indexed onBehalf, address indexed receiver,
        uint256 assets, uint256 shares
    );
    event Borrow(
        bytes32 indexed id, address caller, address indexed onBehalf, address indexed receiver,
        uint256 assets, uint256 shares
    );
    event Repay(bytes32 indexed id, address indexed caller, address indexed onBehalf, uint256 assets, uint256 shares);
    event SupplyCollateral(bytes32 indexed id, address indexed caller, address indexed onBehalf, uint256 assets);
    event WithdrawCollateral(
        bytes32 indexed id, address caller, address indexed onBehalf, address indexed receiver, uint256 assets
    );
    event Liquidate(
        bytes32 indexed id, address indexed caller, address indexed borrower, uint256 repaidAssets,
        uint256 repaidShares, uint256 seizedAssets, uint256 badDebtAssets, uint256 badDebtShares
    );
    event AccrueInterest(bytes32 indexed id, uint256 prevBorrowRate, uint256 interest, uint256 feeShares);
}

/// Where the reference sits. Its own mainnet address, so that a reader checking the vendored
/// code against a node is looking at the same place twice.
const MORPHO: Address = address!("BBBBBbbBBb9cC5e90e3b3Af64bdAF62C37EEFFCb");

/// Whoever may enable a rate model and a threshold. Nobody else acts as this.
const OWNER: Address = address!("00000000000000000000000000000000000000A1");

const LOAN_TOKEN: Address = address!("00000000000000000000000000000000000000B1");
const COLLATERAL_TOKEN: Address = address!("00000000000000000000000000000000000000B2");
const ORACLE: Address = address!("00000000000000000000000000000000000000C1");
const IRM: Address = address!("00000000000000000000000000000000000000C2");

/// The one tick a corpus starts from, as a wall clock.
///
/// Not zero: the reference reads a market's `lastUpdate` of zero as a market that was never
/// created, so a run that opened at the epoch would be a run with no market in it.
pub(crate) const OPENING_TIMESTAMP: u64 = 1_700_000_000;

/// A contract that answers every call with the word in its storage, and takes a new word from
/// any transaction exactly thirty-two bytes long.
///
/// One stub serves as both tokens, the oracle and the rate model, because that is all any of
/// them has to do here: a token that reports success, an oracle that reports a price, a model
/// that reports a rate. None of the reference's own calls into them is thirty-two bytes long —
/// the shortest is four and the next is sixty-eight — so the setter cannot be reached by
/// accident from inside a transition.
///
/// ```text
/// CALLDATASIZE PUSH1 0x20 EQ PUSH1 0x12 JUMPI     if the call is a new word, go and store it
/// PUSH1 0x00 SLOAD PUSH1 0x00 MSTORE              otherwise answer with the word held
/// PUSH1 0x20 PUSH1 0x00 RETURN
/// JUMPDEST PUSH1 0x00 CALLDATALOAD PUSH1 0x00 SSTORE STOP
/// ```
const ANSWERS_WITH_A_WORD: [u8; 26] = [
    0x36, 0x60, 0x20, 0x14, 0x60, 0x12, 0x57, 0x60, 0x00, 0x54, 0x60, 0x00, 0x52, 0x60, 0x20, 0x60,
    0x00, 0xf3, 0x5b, 0x60, 0x00, 0x35, 0x60, 0x00, 0x55, 0x00,
];

type Machine =
    MainnetEvm<Context<BlockEnv, TxEnv, CfgEnv, CacheDB<EmptyDB>, Journal<CacheDB<EmptyDB>>>>;

/// The address one agent acts from.
///
/// A page of its own, clear of the fixtures above it, so that the mapping back is total: every
/// address the corpus mentions is either an agent or one of the four named contracts.
pub(crate) fn acting_as(who: AgentId) -> Address {
    let mut bytes = [0u8; 20];
    bytes[16..].copy_from_slice(&(0x1000_0000_u32 + who.0).to_be_bytes());
    Address::from(bytes)
}

fn agent_at(address: Address) -> Option<AgentId> {
    let tail = u32::from_be_bytes(address.0[16..].try_into().ok()?);
    let leading_zeroes = address.0[..16].iter().all(|byte| *byte == 0);
    (leading_zeroes && tail >= 0x1000_0000).then(|| AgentId(tail - 0x1000_0000))
}

/// Turns a Morpho log into the event this workspace reports, whoever produced it.
///
/// Shared by the two things that read these: the corpus, whose logs come from a machine standing
/// the reference up, and a recorded episode, whose logs come from a chain. Which addresses are
/// which agent is the caller's, because those two answer it differently.
pub(crate) fn reported(
    logs: &[Log],
    market: MarketId,
    name: &dyn Fn(Address) -> Result<AgentId, String>,
) -> Result<Vec<sting_karman_mech_lending::LendingEvent>, String> {
    use sting_karman_mech_lending::LendingEvent as Reported;

    let mut events = Vec::new();
    for log in logs {
        let named = |who: Address| name(who);
        if let Ok(reported) = Supply::decode_log(log) {
            events.push(Reported::Supplied {
                market,
                holder: named(reported.onBehalf)?,
                assets: quantity(reported.assets)?,
                shares: quantity(reported.shares)?,
            });
        } else if let Ok(reported) = Withdraw::decode_log(log) {
            events.push(Reported::Withdrawn {
                market,
                holder: named(reported.onBehalf)?,
                assets: quantity(reported.assets)?,
                shares: quantity(reported.shares)?,
            });
        } else if let Ok(reported) = Borrow::decode_log(log) {
            events.push(Reported::Borrowed {
                market,
                holder: named(reported.onBehalf)?,
                assets: quantity(reported.assets)?,
                shares: quantity(reported.shares)?,
            });
        } else if let Ok(reported) = Repay::decode_log(log) {
            events.push(Reported::Repaid {
                market,
                holder: named(reported.onBehalf)?,
                assets: quantity(reported.assets)?,
                shares: quantity(reported.shares)?,
            });
        } else if let Ok(reported) = SupplyCollateral::decode_log(log) {
            events.push(Reported::CollateralAdded {
                market,
                holder: named(reported.onBehalf)?,
                assets: quantity(reported.assets)?,
            });
        } else if let Ok(reported) = WithdrawCollateral::decode_log(log) {
            events.push(Reported::CollateralRemoved {
                market,
                holder: named(reported.onBehalf)?,
                assets: quantity(reported.assets)?,
            });
        } else if let Ok(reported) = Liquidate::decode_log(log) {
            events.push(Reported::Liquidated {
                market,
                liquidator: named(reported.caller)?,
                borrower: named(reported.borrower)?,
                repaid: quantity(reported.repaidAssets)?,
                repaid_shares: quantity(reported.repaidShares)?,
                seized: quantity(reported.seizedAssets)?,
                bad_debt: quantity(reported.badDebtAssets)?,
                bad_debt_shares: quantity(reported.badDebtShares)?,
            });
        } else if let Ok(reported) = AccrueInterest::decode_log(log) {
            events.push(Reported::InterestAccrued {
                market,
                interest: quantity(reported.interest)?,
                fee_shares: quantity(reported.feeShares)?,
            });
        } else {
            // Silently dropping it would be the worst kind of agreement: the corpus would
            // record less than the reference said, and this side would match the smaller
            // record. Anything the reference reports that this workspace has no event for is
            // a gap in the model, and it stops the corpus rather than shrinking it.
            return Err(format!("the reference reported something unrecognised: {log:?}"));
        }
    }
    Ok(events)
}

/// One of the reference's own numbers, as a quantity of the unit its place in an event names.
fn quantity<U: sting_karman_types::Unit>(value: U256) -> Result<Amount<U>, String> {
    u128::try_from(value)
        .map(Amount::new)
        .map_err(|_| String::from("the reference reported an amount past what money can carry"))
}

/// A transition the reference either settled or reverted.
pub(crate) struct Settled {
    /// What it reported, in the order it reported it.
    pub(crate) logs: Vec<Log>,
}

/// The reference, running.
pub(crate) struct Reference {
    machine: Machine,
    nonces: BTreeMap<Address, u64>,
    params: MarketParams,
    id: B256,
    cast: Vec<AgentId>,
}

impl Reference {
    /// Stands the reference up: its own code at its own address, four stubs around it, and one
    /// market open at [`OPENING_TIMESTAMP`].
    ///
    /// The owner is written into storage rather than constructed, because what is vendored is
    /// the code as deployed and not the code that deploys it. That the slot written is the one
    /// the reference calls `owner` is not assumed: it is asked, before anything else happens.
    pub(crate) fn open(
        runtime_code: &[u8],
        lltv: u128,
        fee_recipient: AgentId,
        price: Wide,
        rate: u128,
        cast: Vec<AgentId>,
    ) -> Result<Self, String> {
        let mut db = CacheDB::<EmptyDB>::default();
        db.insert_account_info(
            MORPHO,
            AccountInfo {
                code_hash: keccak256(runtime_code),
                code: Some(Bytecode::new_raw(Bytes::copy_from_slice(runtime_code))),
                ..Default::default()
            },
        );
        db.insert_account_storage(MORPHO, U256::ZERO, U256::from_be_slice(OWNER.as_slice()))
            .map_err(|e| format!("could not write the owner: {e:?}"))?;

        let stub = Bytecode::new_raw(Bytes::from_static(&ANSWERS_WITH_A_WORD));
        for (at, word) in [
            (LOAN_TOKEN, U256::from(1)),
            (COLLATERAL_TOKEN, U256::from(1)),
            (ORACLE, U256::from_be_slice(&price.to_be_bytes())),
            (IRM, U256::from(rate)),
        ] {
            db.insert_account_info(
                at,
                AccountInfo {
                    code_hash: keccak256(ANSWERS_WITH_A_WORD),
                    code: Some(stub.clone()),
                    ..Default::default()
                },
            );
            db.insert_account_storage(at, U256::ZERO, word)
                .map_err(|e| format!("could not seed a stub: {e:?}"))?;
        }
        for who in std::iter::once(OWNER).chain(cast.iter().copied().map(acting_as)) {
            db.insert_account_info(
                who,
                AccountInfo { balance: U256::MAX >> 1, ..Default::default() },
            );
        }

        let machine = Context::mainnet()
            .with_db(db)
            .modify_block_chained(|block| block.timestamp = U256::from(OPENING_TIMESTAMP))
            .build_mainnet();

        let params = MarketParams {
            loanToken: LOAN_TOKEN,
            collateralToken: COLLATERAL_TOKEN,
            oracle: ORACLE,
            irm: IRM,
            lltv: U256::from(lltv),
        };
        let id = keccak256(params.abi_encode());
        let mut reference = Reference { machine, nonces: BTreeMap::new(), params, id, cast };

        let answered = reference.read(OWNER, ownerCall {}.abi_encode())?;
        let named = ownerCall::abi_decode_returns(&answered)
            .map_err(|e| format!("the reference did not answer with an owner: {e}"))?;
        if named != OWNER {
            return Err(format!("the owner slot is not slot zero: the reference names {named}"));
        }

        reference.must(OWNER, enableIrmCall { irm: IRM }.abi_encode())?;
        reference.must(OWNER, enableLltvCall { lltv: U256::from(lltv) }.abi_encode())?;
        reference.must(
            OWNER,
            setFeeRecipientCall { newFeeRecipient: acting_as(fee_recipient) }.abi_encode(),
        )?;
        reference.must(
            OWNER,
            createMarketCall { marketParams: reference.params.clone() }.abi_encode(),
        )?;
        Ok(reference)
    }

    /// The market's terms, as every call has to repeat them.
    pub(crate) fn params(&self) -> MarketParams {
        self.params.clone()
    }

    /// Moves the clock to `now`.
    pub(crate) fn at(&mut self, now: Tick) {
        self.machine.ctx.block.timestamp = U256::from(OPENING_TIMESTAMP + now.0);
    }

    /// Tells the oracle what the collateral is worth from here on.
    ///
    /// A transaction to the stub rather than a write into its storage: everything a corpus is
    /// built out of goes through the machine, so nothing in it depends on a storage layout this
    /// side guessed at.
    pub(crate) fn price(&mut self, price: Wide) -> Result<(), String> {
        match self.transact(OWNER, ORACLE, price.to_be_bytes().to_vec())? {
            ExecutionResult::Success { .. } => Ok(()),
            other => Err(format!("the oracle would not take a price: {other:?}")),
        }
    }

    /// Sets the share of interest the market keeps for its fee recipient.
    pub(crate) fn fee(&mut self, fee: u128) -> Result<(), String> {
        self.must(
            OWNER,
            setFeeCall { marketParams: self.params.clone(), newFee: U256::from(fee) }.abi_encode(),
        )
    }

    /// Sends one transaction to the reference and reports what it made of it.
    pub(crate) fn send(&mut self, from: AgentId, data: Vec<u8>) -> Result<Settled, String> {
        match self.transact(acting_as(from), MORPHO, data)? {
            ExecutionResult::Success { logs, .. } => Ok(Settled { logs }),
            ExecutionResult::Revert { .. } | ExecutionResult::Halt { .. } => {
                Err(String::from("reverted"))
            }
        }
    }

    /// The market and every position in it, as this workspace would hold them.
    pub(crate) fn state(&mut self) -> Result<LendingState, String> {
        let answered = self.read(OWNER, marketCall { id: self.id }.abi_encode())?;
        let held = marketCall::abi_decode_returns(&answered)
            .map_err(|e| format!("the reference did not answer with a market: {e}"))?;
        let opened = Market {
            total_supply_assets: Amount::new(held._0),
            total_supply_shares: Amount::new(held._1),
            total_borrow_assets: Amount::new(held._2),
            total_borrow_shares: Amount::new(held._3),
            last_update: Tick(
                held._4
                    .checked_sub(u128::from(OPENING_TIMESTAMP))
                    .and_then(|since| u64::try_from(since).ok())
                    .ok_or_else(|| {
                        String::from("the reference dated a market before the corpus")
                    })?,
            ),
            fee: Amount::new(held._5),
        };
        let mut state = LendingState::new()
            .with_market(MarketId(0), opened)
            .map_err(|e| format!("the market the reference holds is not one this can: {e}"))?;

        for who in self.cast.clone() {
            let answered =
                self.read(OWNER, positionCall { id: self.id, user: acting_as(who) }.abi_encode())?;
            let held = positionCall::abi_decode_returns(&answered)
                .map_err(|e| format!("the reference did not answer with a position: {e}"))?;
            state = state
                .with_position(
                    MarketId(0),
                    who,
                    Position {
                        supply_shares: Amount::new(u128::try_from(held._0).map_err(|_| {
                            String::from("a supply claim past what money can carry")
                        })?),
                        borrow_shares: Amount::new(held._1),
                        collateral: Amount::new(held._2),
                    },
                )
                .map_err(|e| format!("the reference holds a position this cannot: {e}"))?;
        }
        Ok(state)
    }

    /// Turns the reference's own log of a transition into the events this workspace reports.
    pub(crate) fn events(
        &self,
        settled: &Settled,
    ) -> Result<Vec<sting_karman_mech_lending::LendingEvent>, String> {
        reported(&settled.logs, MarketId(0), &|who| {
            agent_at(who).ok_or_else(|| format!("the reference named a stranger: {who}"))
        })
    }

    /// A call whose answer is wanted.
    ///
    /// Not committed. The getters behind it are declared `view` and could not write anything, but
    /// reading the state a case starts from is not the place to depend on that: a corpus is a
    /// record of what the transitions did, and a question asked in the middle of building one
    /// must not be able to be part of the answer.
    fn read(&mut self, from: Address, data: Vec<u8>) -> Result<Bytes, String> {
        let tx = self.envelope(from, MORPHO, data)?;
        let answered = self
            .machine
            .transact(tx)
            .map_err(|e| format!("the machine stopped on a question: {e:?}"))?;
        match answered.result {
            ExecutionResult::Success { output: Output::Call(answer), .. } => Ok(answer),
            other => Err(format!("the reference refused a question: {other:?}")),
        }
    }

    /// A transaction that has to succeed for the corpus to mean anything.
    fn must(&mut self, from: Address, data: Vec<u8>) -> Result<(), String> {
        match self.transact(from, MORPHO, data)? {
            ExecutionResult::Success { .. } => Ok(()),
            other => Err(format!("setting the corpus up failed: {other:?}")),
        }
    }

    /// One transaction, addressed and numbered. Does not advance the nonce: a question and a
    /// transition are built the same way and only one of them is sent.
    fn envelope(&self, from: Address, to: Address, data: Vec<u8>) -> Result<TxEnv, String> {
        TxEnv::builder()
            .caller(from)
            .kind(TxKind::Call(to))
            .data(Bytes::from(data))
            .gas_limit(16_000_000)
            .gas_price(0)
            .nonce(self.nonces.get(&from).copied().unwrap_or_default())
            .build()
            .map_err(|e| format!("the transaction could not be built: {e:?}"))
    }

    fn transact(
        &mut self,
        from: Address,
        to: Address,
        data: Vec<u8>,
    ) -> Result<ExecutionResult, String> {
        let tx = self.envelope(from, to, data)?;
        *self.nonces.entry(from).or_default() += 1;
        self.machine.transact_commit(tx).map_err(|e| format!("the machine stopped: {e:?}"))
    }
}
