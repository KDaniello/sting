//! One synthetic world, shared by everything that needs a realistic scenario.
//!
//! Both the Rust snapshot and the Python bindings' test read this same scenario, so the two
//! proofs are about one world rather than two invented ones. It is deliberately public rather
//! than test-only: the bindings are a separate compilation unit and cannot see this crate's test
//! code.
//!
//! Synthetic, but pushed through the real extraction rather than hand-written features. It holds
//! a hub funding twenty-five addresses, and among them a wallet that fires on a metronome, one
//! that fires irregularly, and one that was funded and then did nothing at all — plus a chain of
//! pass-through funders off to the side.

use sting_sybil_graph::FundingGraphs;
use sting_sybil_ingestion::{Direction, RawMessage, RawTx};
use sting_sybil_types::{Finality, FundingEdge, OwnerId, TonAddress, TxHash};

/// The world under construction: the graphs, the rows, and the hash counter that keeps every
/// transaction in it distinct.
struct Scenario {
    graphs: FundingGraphs,
    txs: Vec<RawTx>,
    messages: Vec<RawMessage>,
    hashes: u8,
}

impl Scenario {
    fn new() -> Self {
        Self {
            graphs: FundingGraphs::with_dust_threshold(0),
            txs: Vec::new(),
            messages: Vec::new(),
            hashes: 0,
        }
    }

    fn next_hash(&mut self) -> TxHash {
        self.hashes = self.hashes.wrapping_add(1);
        [self.hashes; 32]
    }

    /// A funding edge and the mirrored pair of messages a real block would carry — outgoing on
    /// the funder's side, incoming on the child's.
    fn fund(&mut self, funder: u8, child: u8, amount: u64, ts: u32) {
        let hash = self.next_hash();
        self.graphs.add_first_funding(FundingEdge {
            src: owner(funder),
            dst: owner(child),
            amount: u128::from(amount),
            jetton_master: None,
            tx_hash: hash,
            first_ts: ts,
        });
        for direction in [Direction::Out, Direction::In] {
            self.messages.push(RawMessage {
                tx_hash: hash,
                direction,
                src: addr(funder),
                dst: addr(child),
                value_ton: amount,
                op_code: 0,
                body_boc: vec![],
                created_lt: u64::from(ts),
                mc_seqno: 1,
                finality: Finality::Finalized,
            });
        }
    }

    /// One transaction of an address's own.
    fn transact(&mut self, who: u8, lt: u64, ts: u32) {
        let hash = self.next_hash();
        self.txs.push(RawTx {
            account: addr(who),
            lt,
            tx_hash: hash,
            mc_seqno: 1,
            now: ts,
            finality: Finality::Finalized,
        });
    }
}

// Named so that a test can find a wallet by the part it plays rather than by a magic byte.

/// Who funds the hub.
pub const ROOT_HUB_FUNDER: u8 = 1;
/// The distributor.
pub const HUB: u8 = 2;
/// Who starts the chain.
pub const CHAIN_ROOT_FUNDER: u8 = 4;
/// The wallet that fires on a metronome.
pub const BOT: u8 = 10;
/// The wallet that fires irregularly.
pub const HUMAN: u8 = 11;
/// The wallet that was funded and then did nothing.
pub const ZERO_TX: u8 = 12;
/// The four addresses money is passed along.
pub const CHAIN: [u8; 4] = [60, 61, 62, 63];

/// The address for a byte.
#[must_use]
pub fn addr(byte: u8) -> TonAddress {
    TonAddress::new(0, [byte; 32])
}

/// The owner for a byte.
#[must_use]
pub fn owner(byte: u8) -> OwnerId {
    OwnerId(addr(byte))
}

/// The whole scenario: the graphs, the transactions and the messages.
#[must_use]
pub fn synthetic_owner_scenario() -> (FundingGraphs, Vec<RawTx>, Vec<RawMessage>) {
    let mut world = Scenario::new();

    world.fund(ROOT_HUB_FUNDER, HUB, 10_000_000, 1_000);

    // The hub funds twenty-five addresses, which is past any star threshold worth setting.
    for (i, child) in (10u8..=34).enumerate() {
        world.fund(
            HUB,
            child,
            5_000_000,
            2_000 + u32::try_from(i).unwrap_or(0) * 10,
        );
    }

    // The metronome: fifty transactions exactly a minute apart.
    for i in 0..50u64 {
        world.transact(BOT, i + 1, 2_010 + u32::try_from(i).unwrap_or(0) * 60);
    }

    // The contrast: seven transactions with wildly different gaps. What tells the two apart is
    // how anomalous the spacing is, not which direction it runs in.
    let human_gaps = [5u32, 340, 12, 890, 45, 1200];
    let mut at = 2_510;
    world.transact(HUMAN, 1, at);
    for (i, gap) in human_gaps.iter().enumerate() {
        at += gap;
        world.transact(HUMAN, u64::try_from(i + 2).unwrap_or(0), at);
    }

    // Funded by the hub in the loop above and given no transactions of its own on purpose: a
    // wallet with nothing to measure, put through the real extraction rather than a unit test.
    let _ = ZERO_TX;

    // A chain: the root funds the first address, which funds the next, and so on. The three in
    // the middle are pass-through, and the last one is where it stops.
    let mut previous = CHAIN_ROOT_FUNDER;
    for (i, &node) in CHAIN.iter().enumerate() {
        let ts = 3_000 + u32::try_from(i).unwrap_or(0) * 100;
        world.fund(previous, node, 1_000_000, ts);
        world.transact(node, 1, ts + 5);
        previous = node;
    }

    (world.graphs, world.txs, world.messages)
}
