//! What the funding graph promises, one property at a time.
//!
//! The properties that matter are the ones about what must never happen: an edge between a jetton
//! wallet and anybody, an edge drawn from a contract that lied about its owner, an edge from a
//! transfer that bounced, a second funder recorded as an address's first, and a whale joined to a
//! farm by one unit of dust.
//!
//! Where a property is about the pipeline rather than about one function, the test drives real
//! bodies through the real decoder rather than building the decoded form by hand — otherwise it
//! would prove the assertion and not the code.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "AGENTS.md §5 scopes the no-unwrap/expect/panic rule to library code outside tests"
)]

use std::collections::{HashMap, HashSet};

use petgraph::graph::DiGraph;
use sting_sybil_graph::{
    ExclusionCfg, FundingGraphs, JettonEvent, JettonOp, JettonRpc, MasterAllowlist,
    apply_exclusion, correlate_traces, events_from_block, ingest_block_into_gas_graph,
    ingest_block_into_value_graph, is_excluded, largest_component_size, resolve_and_verify_owner,
    weakly_connected_components,
};
use sting_sybil_ingestion::{BlockBatch, Direction, RawMessage, RawTx};
use sting_sybil_types::{AccountKind, FundingEdge, JettonWalletAddr, OwnerId, TonAddress};

fn addr(byte: u8) -> TonAddress {
    TonAddress::new(0, [byte; 32])
}

/// A distinct address for every index, where `addr` only has 256 of them.
fn scaled_addr(i: usize) -> TonAddress {
    let mut hash = [0u8; 32];
    hash[0..8].copy_from_slice(&(i as u64).to_le_bytes());
    TonAddress::new(0, hash)
}

/// Encodes a real jetton body and serialises it to the bytes a message would carry.
///
/// All three opcodes share one layout — a query id, an amount as variable-width coins, and an
/// address — so one builder covers them. Tests that use it are driving the decoder inside the
/// pipeline rather than stepping around it.
fn synth_jetton_body(op_code: u32, query_id: u64, amount: u128, address: TonAddress) -> Vec<u8> {
    let tonlib_addr = tonlib_core::TonAddress::new(address.workchain, address.hash.into());
    let mut builder = tonlib_core::cell::CellBuilder::new();
    builder
        .store_u32(32, op_code)
        .unwrap()
        .store_u64(64, query_id)
        .unwrap()
        .store_coins(&num_bigint::BigUint::from(amount))
        .unwrap()
        .store_address(&tonlib_addr)
        .unwrap();
    let cell = builder.build().unwrap();
    tonlib_core::cell::BagOfCells::from_root(cell)
        .serialize(false)
        .unwrap()
}

#[test]
fn a_jetton_transfer_becomes_an_edge_between_owners() {
    let owner_a = addr(1);
    let owner_b = addr(2);
    let wallet_a = JettonWalletAddr(addr(10));
    let master = addr(99);

    let events = vec![
        JettonEvent {
            jetton_wallet: wallet_a,
            jetton_master: master,
            op: JettonOp::Transfer {
                query_id: 1,
                amount: 100,
                destination: owner_b,
            },
            lt: 1,
            bounced: false,
            tx_hash: [1; 32],
            now: 1000,
        },
        JettonEvent {
            jetton_wallet: wallet_a,
            jetton_master: master,
            op: JettonOp::InternalTransfer {
                query_id: 1,
                amount: 100,
                from: owner_a,
            },
            lt: 2,
            bounced: false,
            tx_hash: [2; 32],
            now: 1001,
        },
    ];

    let traces = correlate_traces(&events);
    assert_eq!(traces.len(), 1);
    assert_eq!(traces[0].from_owner, OwnerId(owner_a));
    assert_eq!(traces[0].to_owner, OwnerId(owner_b));
    assert_eq!(
        traces[0].tx_hash, [2; 32],
        "the hash comes from the message that closed the transfer"
    );
    assert_eq!(traces[0].completed_ts, 1001);
    assert_ne!(
        traces[0].from_owner.0, wallet_a.0,
        "an owner is never the wallet contract that moved the tokens"
    );
}

struct MockRpc {
    wallet_data: HashMap<JettonWalletAddr, (TonAddress, TonAddress)>,
    expected: HashMap<(TonAddress, TonAddress), JettonWalletAddr>,
}

impl JettonRpc for MockRpc {
    fn wallet_data(&self, jetton_wallet: JettonWalletAddr) -> Option<(TonAddress, TonAddress)> {
        self.wallet_data.get(&jetton_wallet).copied()
    }

    fn expected_wallet_address(
        &self,
        master: TonAddress,
        owner: TonAddress,
    ) -> Option<JettonWalletAddr> {
        self.expected.get(&(master, owner)).copied()
    }
}

#[test]
fn a_wallet_the_master_would_not_have_deployed_is_refused() {
    let owner = addr(1);
    let master = addr(99);
    let real_wallet = JettonWalletAddr(addr(10));
    // Anyone can deploy a contract that answers the way a jetton wallet does.
    let spoof_wallet = JettonWalletAddr(addr(66));

    let mut wallet_data = HashMap::new();
    wallet_data.insert(real_wallet, (owner, master));
    wallet_data.insert(spoof_wallet, (owner, master));
    let mut expected = HashMap::new();
    expected.insert((master, owner), real_wallet);

    let rpc = MockRpc {
        wallet_data,
        expected,
    };
    let allowlist = MasterAllowlist::new([master]);

    assert_eq!(
        resolve_and_verify_owner(&rpc, real_wallet, &allowlist),
        Some(OwnerId(owner))
    );
    assert_eq!(
        resolve_and_verify_owner(&rpc, spoof_wallet, &allowlist),
        None,
        "the master names one wallet for that owner, and it is not this one"
    );
}

#[test]
fn a_wallet_of_an_unlisted_master_is_refused() {
    let owner = addr(1);
    let unlisted_master = addr(199);
    let wallet = JettonWalletAddr(addr(10));

    let mut wallet_data = HashMap::new();
    wallet_data.insert(wallet, (owner, unlisted_master));
    let rpc = MockRpc {
        wallet_data,
        expected: HashMap::new(),
    };
    let allowlist = MasterAllowlist::new([addr(99)]);

    assert_eq!(resolve_and_verify_owner(&rpc, wallet, &allowlist), None);
}

#[test]
fn the_envelope_address_never_stands_in_for_an_owner() {
    // The wallet address is deliberately one that would be easy to mistake for an owner if
    // correlation read the envelope instead of the body.
    let confusable = JettonWalletAddr(addr(77));
    let owner_a = addr(1);
    let owner_b = addr(2);
    let master = addr(99);

    let events = vec![
        JettonEvent {
            jetton_wallet: confusable,
            jetton_master: master,
            op: JettonOp::Transfer {
                query_id: 5,
                amount: 50,
                destination: owner_b,
            },
            lt: 1,
            bounced: false,
            tx_hash: [1; 32],
            now: 1,
        },
        JettonEvent {
            jetton_wallet: confusable,
            jetton_master: master,
            op: JettonOp::InternalTransfer {
                query_id: 5,
                amount: 50,
                from: owner_a,
            },
            lt: 2,
            bounced: false,
            tx_hash: [2; 32],
            now: 2,
        },
    ];

    let traces = correlate_traces(&events);
    assert_eq!(traces.len(), 1);
    assert_ne!(traces[0].from_owner.0, confusable.0);
    assert_ne!(traces[0].to_owner.0, confusable.0);
}

/// The same invariant one level up, through the real decoder.
///
/// The envelope names an address the body does not: if the pipeline read the envelope where it
/// should read the body, the sender that comes out would be the envelope's.
#[test]
fn the_pipeline_reads_the_sender_out_of_the_body() {
    let envelope_wallet = addr(77);
    let real_sender_owner = addr(1);
    let master = addr(99);
    let wallet = JettonWalletAddr(envelope_wallet);

    let body = synth_jetton_body(
        sting_sybil_graph::OP_TRANSFER_NOTIFICATION,
        7,
        42,
        real_sender_owner,
    );
    let batch = BlockBatch {
        seqno: 1,
        finality: sting_sybil_types::Finality::Finalized,
        txs: vec![],
        messages: vec![RawMessage {
            tx_hash: [9; 32],
            direction: Direction::In,
            src: envelope_wallet,
            dst: addr(2),
            value_ton: 0,
            op_code: sting_sybil_graph::OP_TRANSFER_NOTIFICATION,
            body_boc: body,
            created_lt: 1,
            mc_seqno: 1,
            finality: sting_sybil_types::Finality::Finalized,
        }],
    };

    let mut wallet_data = HashMap::new();
    wallet_data.insert(wallet, (addr(1), master));
    let mut expected = HashMap::new();
    expected.insert((master, addr(1)), wallet);
    let rpc = MockRpc {
        wallet_data,
        expected,
    };
    let allowlist = MasterAllowlist::new([master]);

    let events = events_from_block(&batch, &rpc, &allowlist);
    assert_eq!(events.len(), 1);
    let JettonOp::TransferNotification { sender, .. } = events[0].op else {
        panic!("expected a transfer notification")
    };
    assert_eq!(sender, real_sender_owner);
    assert_ne!(sender, envelope_wallet);
}

#[test]
fn a_transfer_with_no_notification_still_becomes_an_edge() {
    // With no forward amount, no notification is sent at all — so an edge that depended on one
    // would silently miss every transfer of that shape.
    let events = vec![
        JettonEvent {
            jetton_wallet: JettonWalletAddr(addr(10)),
            jetton_master: addr(99),
            op: JettonOp::Transfer {
                query_id: 1,
                amount: 10,
                destination: addr(2),
            },
            lt: 1,
            bounced: false,
            tx_hash: [1; 32],
            now: 1,
        },
        JettonEvent {
            jetton_wallet: JettonWalletAddr(addr(10)),
            jetton_master: addr(99),
            op: JettonOp::InternalTransfer {
                query_id: 1,
                amount: 10,
                from: addr(1),
            },
            lt: 2,
            bounced: false,
            tx_hash: [2; 32],
            now: 2,
        },
    ];

    assert_eq!(correlate_traces(&events).len(), 1);
}

#[test]
fn a_bounced_transfer_becomes_no_edge() {
    let events = vec![
        JettonEvent {
            jetton_wallet: JettonWalletAddr(addr(10)),
            jetton_master: addr(99),
            op: JettonOp::Transfer {
                query_id: 1,
                amount: 10,
                destination: addr(2),
            },
            lt: 1,
            bounced: false,
            tx_hash: [1; 32],
            now: 1,
        },
        JettonEvent {
            jetton_wallet: JettonWalletAddr(addr(10)),
            jetton_master: addr(99),
            op: JettonOp::InternalTransfer {
                query_id: 1,
                amount: 10,
                from: addr(1),
            },
            lt: 2,
            bounced: true,
            tx_hash: [2; 32],
            now: 2,
        },
    ];

    assert!(
        correlate_traces(&events).is_empty(),
        "a transfer that came back moved nothing"
    );
}

#[test]
fn a_transfer_still_in_flight_becomes_no_edge() {
    let events = vec![JettonEvent {
        jetton_wallet: JettonWalletAddr(addr(10)),
        jetton_master: addr(99),
        op: JettonOp::Transfer {
            query_id: 1,
            amount: 10,
            destination: addr(2),
        },
        lt: 1,
        bounced: false,
        tx_hash: [1; 32],
        now: 1,
    }];

    assert!(correlate_traces(&events).is_empty());
}

#[test]
fn the_gas_graph_keeps_only_the_first_funding() {
    let mut graphs = FundingGraphs::default();
    let a = OwnerId(addr(1));
    let b = OwnerId(addr(2));

    let first = FundingEdge {
        src: a,
        dst: b,
        amount: 100,
        jetton_master: None,
        tx_hash: [1; 32],
        first_ts: 10,
    };
    let second = FundingEdge {
        src: a,
        dst: b,
        amount: 50,
        jetton_master: None,
        tx_hash: [2; 32],
        first_ts: 20,
    };

    graphs.add_value_edge(first);
    graphs.add_value_edge(second);

    graphs.add_first_funding(first);
    graphs.add_first_funding(second);

    assert_eq!(
        graphs.value.edge_count(),
        2,
        "the value graph counts every transfer"
    );
    assert_eq!(graphs.gas.edge_count(), 1, "an address is activated once");
}

/// Two different funders of one new address: only the first activated it.
///
/// Deduplicating by the `(funder, destination)` pair instead would record the second as a first
/// funder too, which is a claim about who put the address on the chain that is simply false.
#[test]
fn a_second_funder_of_one_address_is_not_its_first() {
    let mut graphs = FundingGraphs::default();
    let funder1 = OwnerId(addr(1));
    let funder2 = OwnerId(addr(2));
    let new_wallet = OwnerId(addr(3));

    graphs.add_first_funding(FundingEdge {
        src: funder1,
        dst: new_wallet,
        amount: 100,
        jetton_master: None,
        tx_hash: [1; 32],
        first_ts: 10,
    });
    graphs.add_first_funding(FundingEdge {
        src: funder2,
        dst: new_wallet,
        amount: 100,
        jetton_master: None,
        tx_hash: [2; 32],
        first_ts: 20,
    });

    assert_eq!(graphs.gas.edge_count(), 1);
    let edge = graphs.gas.edge_weights().next().unwrap();
    assert_eq!(edge.src, funder1, "the earlier funder is the one kept");
}

/// One logical transfer appears twice in a block, once from each side. The gas graph must count
/// it once.
#[test]
fn one_transfer_seen_from_both_sides_is_one_edge() {
    let funder = addr(1);
    let new_wallet = addr(2);
    let mut graphs = FundingGraphs::default();

    let batch = BlockBatch {
        seqno: 1,
        finality: sting_sybil_types::Finality::Finalized,
        txs: vec![],
        messages: vec![
            RawMessage {
                tx_hash: [1; 32],
                direction: Direction::Out,
                src: funder,
                dst: new_wallet,
                value_ton: 1_000_000_000,
                op_code: 0,
                body_boc: vec![],
                created_lt: 1,
                mc_seqno: 1,
                finality: sting_sybil_types::Finality::Finalized,
            },
            RawMessage {
                tx_hash: [1; 32],
                direction: Direction::In,
                src: funder,
                dst: new_wallet,
                value_ton: 1_000_000_000,
                op_code: 0,
                body_boc: vec![],
                created_lt: 1,
                mc_seqno: 1,
                finality: sting_sybil_types::Finality::Finalized,
            },
        ],
    };

    ingest_block_into_gas_graph(&batch, &mut graphs);
    assert_eq!(graphs.gas.edge_count(), 1);
}

#[test]
fn excluding_an_exchange_breaks_the_component_it_created() {
    let exchange = OwnerId(addr(255));
    let cfg = ExclusionCfg {
        cex_allowlist: HashSet::from([exchange]),
        bridge_allowlist: HashSet::new(),
        hub_degree_threshold: 100,
    };

    // One exchange and twenty wallets that have nothing to do with each other.
    let mut graph: DiGraph<OwnerId, FundingEdge> = DiGraph::new();
    let exchange_idx = graph.add_node(exchange);
    for i in 0..20u8 {
        let idx = graph.add_node(OwnerId(addr(i)));
        graph.add_edge(
            exchange_idx,
            idx,
            FundingEdge {
                src: exchange,
                dst: OwnerId(addr(i)),
                amount: 1,
                jetton_master: None,
                tx_hash: [0; 32],
                first_ts: 0,
            },
        );
    }
    let mut before = weakly_connected_components(&graph);
    assert_eq!(
        largest_component_size(&mut before, graph.node_count()),
        21,
        "one shared funder makes twenty strangers look like one cluster"
    );

    let filtered = apply_exclusion(&graph, &cfg, |owner| {
        if owner == exchange {
            AccountKind::Exchange
        } else {
            AccountKind::Wallet
        }
    });
    let mut after = weakly_connected_components(&filtered);
    assert_eq!(
        largest_component_size(&mut after, filtered.node_count()),
        1,
        "with the exchange gone, each wallet is its own component again"
    );
    assert_eq!(filtered.node_count(), 20);
}

#[test]
fn the_exclusion_predicate_covers_its_four_cases() {
    let cfg = ExclusionCfg {
        cex_allowlist: HashSet::from([OwnerId(addr(50))]),
        bridge_allowlist: HashSet::new(),
        hub_degree_threshold: 100,
    };
    assert!(is_excluded(OwnerId(addr(50)), AccountKind::Wallet, 0, &cfg));
    assert!(is_excluded(
        OwnerId(addr(1)),
        AccountKind::Contract,
        0,
        &cfg
    ));
    assert!(is_excluded(
        OwnerId(addr(1)),
        AccountKind::Wallet,
        200,
        &cfg
    ));
    assert!(!is_excluded(OwnerId(addr(1)), AccountKind::Wallet, 5, &cfg));
}

/// A stand-in for scale at a size a suite can afford, for the reason the clustering suite gives:
/// what it catches is a cost that is not linear, not a pace on a real network.
#[test]
fn ten_thousand_edges_are_added_in_seconds() {
    let n = 10_000usize;
    let mut graphs = FundingGraphs::default();
    let start = std::time::Instant::now();
    for i in 0..n {
        graphs.add_value_edge(FundingEdge {
            src: OwnerId(scaled_addr(i)),
            dst: OwnerId(scaled_addr(i + 1)),
            amount: 1,
            jetton_master: None,
            tx_hash: [0; 32],
            first_ts: u32::try_from(i).unwrap(),
        });
    }
    let elapsed = start.elapsed();

    assert_eq!(graphs.value.edge_count(), n);
    assert!(
        elapsed < std::time::Duration::from_secs(5),
        "elapsed={elapsed:?}, which is long enough to suspect the cost is not linear"
    );
}

/// The same edges in a different order build the same graph.
///
/// Building twice from one order would prove nothing — the same code over the same input. What
/// matters is rebuilding from storage, which promises no order at all.
#[test]
fn the_graph_does_not_depend_on_the_order_edges_arrive_in() {
    let edges = [
        FundingEdge {
            src: OwnerId(addr(1)),
            dst: OwnerId(addr(2)),
            amount: 10,
            jetton_master: None,
            tx_hash: [1; 32],
            first_ts: 1,
        },
        FundingEdge {
            src: OwnerId(addr(2)),
            dst: OwnerId(addr(3)),
            amount: 20,
            jetton_master: None,
            tx_hash: [2; 32],
            first_ts: 2,
        },
        FundingEdge {
            src: OwnerId(addr(1)),
            dst: OwnerId(addr(3)),
            amount: 30,
            jetton_master: None,
            tx_hash: [3; 32],
            first_ts: 3,
        },
    ];

    let build = |order: &[FundingEdge]| -> (usize, HashSet<(OwnerId, OwnerId)>) {
        let mut graphs = FundingGraphs::default();
        for edge in order {
            graphs.add_value_edge(*edge);
        }
        let pairs = graphs
            .value
            .edge_weights()
            .map(|edge| (edge.src, edge.dst))
            .collect::<HashSet<_>>();
        (graphs.value.node_count(), pairs)
    };

    let forward = build(&edges);
    let mut reversed = edges;
    reversed.reverse();
    let backward = build(&reversed);

    assert_eq!(forward, backward);
}

#[test]
fn a_dust_transfer_does_not_join_a_whale_to_a_farm() {
    let mut graphs = FundingGraphs::with_dust_threshold(1_000);

    // A farm: five addresses in a chain, funding each other in real amounts.
    let farm_edges: [(u8, u8, u128); 4] = [
        (0, 1, 1_000_000),
        (1, 2, 1_000_000),
        (2, 3, 1_000_000),
        (3, 4, 1_000_000),
    ];
    for (src, dst, amount) in farm_edges {
        graphs.add_value_edge(FundingEdge {
            src: OwnerId(addr(src)),
            dst: OwnerId(addr(dst)),
            amount,
            jetton_master: None,
            tx_hash: [0; 32],
            first_ts: 0,
        });
    }
    // The attack: one unit sent from the farm to a whale, which is the whale's only connection
    // to any of it.
    graphs.add_value_edge(FundingEdge {
        src: OwnerId(addr(0)),
        dst: OwnerId(addr(200)),
        amount: 1,
        jetton_master: None,
        tx_hash: [1; 32],
        first_ts: 0,
    });

    assert_eq!(graphs.value.node_count(), 5);
    let whale_present = graphs
        .value
        .node_weights()
        .any(|&owner| owner == OwnerId(addr(200)));
    assert!(!whale_present, "the dust floor keeps the whale out");

    let mut uf = weakly_connected_components(&graphs.value);
    assert_eq!(
        largest_component_size(&mut uf, graphs.value.node_count()),
        5,
        "the farm is still one component of five"
    );
}

#[test]
fn a_whole_transfer_through_the_pipeline_becomes_one_edge() {
    let owner_a = addr(1);
    let owner_b = addr(2);
    let wallet_a = addr(10);
    let master = addr(99);

    let batch = BlockBatch {
        seqno: 100,
        finality: sting_sybil_types::Finality::Finalized,
        txs: vec![RawTx {
            account: wallet_a,
            lt: 2,
            tx_hash: [2; 32],
            mc_seqno: 100,
            now: 12345,
            finality: sting_sybil_types::Finality::Finalized,
        }],
        messages: vec![
            RawMessage {
                tx_hash: [1; 32],
                direction: Direction::In,
                src: owner_a,
                dst: wallet_a,
                value_ton: 0,
                op_code: sting_sybil_graph::OP_TRANSFER,
                body_boc: synth_jetton_body(sting_sybil_graph::OP_TRANSFER, 1, 500, owner_b),
                created_lt: 1,
                mc_seqno: 100,
                finality: sting_sybil_types::Finality::Finalized,
            },
            RawMessage {
                tx_hash: [2; 32],
                direction: Direction::Out,
                src: wallet_a,
                dst: addr(20),
                value_ton: 0,
                op_code: sting_sybil_graph::OP_INTERNAL_TRANSFER,
                body_boc: synth_jetton_body(
                    sting_sybil_graph::OP_INTERNAL_TRANSFER,
                    1,
                    500,
                    owner_a,
                ),
                created_lt: 2,
                mc_seqno: 100,
                finality: sting_sybil_types::Finality::Finalized,
            },
        ],
    };

    let mut wallet_data = HashMap::new();
    wallet_data.insert(JettonWalletAddr(wallet_a), (owner_a, master));
    let mut expected = HashMap::new();
    expected.insert((master, owner_a), JettonWalletAddr(wallet_a));
    let rpc = MockRpc {
        wallet_data,
        expected,
    };
    let allowlist = MasterAllowlist::new([master]);

    let mut graphs = FundingGraphs::default();
    ingest_block_into_value_graph(&batch, &rpc, &allowlist, &mut graphs);

    assert_eq!(graphs.value.edge_count(), 1);
    let edge = graphs.value.edge_weights().next().unwrap();
    assert_eq!(edge.src, OwnerId(owner_a));
    assert_eq!(edge.dst, OwnerId(owner_b));
    assert_eq!(edge.amount, 500);
    assert_eq!(edge.jetton_master, Some(master));
    assert_eq!(
        edge.tx_hash, [2; 32],
        "the hash and the time both come from the closing message"
    );
    assert_eq!(edge.first_ts, 12345);
}

#[test]
fn an_unverified_wallet_produces_no_event_at_all() {
    let owner_a = addr(1);
    let wallet_a = addr(10);
    let unlisted_master = addr(199);

    let batch = BlockBatch {
        seqno: 100,
        finality: sting_sybil_types::Finality::Finalized,
        txs: vec![],
        messages: vec![RawMessage {
            tx_hash: [1; 32],
            direction: Direction::In,
            src: owner_a,
            dst: wallet_a,
            value_ton: 0,
            op_code: sting_sybil_graph::OP_TRANSFER,
            body_boc: synth_jetton_body(sting_sybil_graph::OP_TRANSFER, 1, 500, addr(2)),
            created_lt: 1,
            mc_seqno: 100,
            finality: sting_sybil_types::Finality::Finalized,
        }],
    };

    let mut wallet_data = HashMap::new();
    wallet_data.insert(JettonWalletAddr(wallet_a), (owner_a, unlisted_master));
    let rpc = MockRpc {
        wallet_data,
        expected: HashMap::new(),
    };
    let allowlist = MasterAllowlist::new([addr(99)]);

    assert!(events_from_block(&batch, &rpc, &allowlist).is_empty());
}

/// The full claimed scale, measured rather than gated: a million owners across ten million
/// edges, which is ten edges per owner rather than the one-to-one chain the smaller test builds.
///
/// It reports a rate and an estimate of the memory the edges themselves occupy, from `size_of` on
/// the real types rather than from a guess. Ignored by default: it costs seconds in release and
/// has no business in an ordinary run.
#[test]
#[ignore = "reports a figure at full scale; costs seconds in release"]
fn a_million_owners_and_ten_million_edges_are_measured() {
    let n_owners = 1_000_000usize;
    let n_edges = 10_000_000usize;
    let mut graphs = FundingGraphs::default();

    let start = std::time::Instant::now();
    for i in 0..n_edges {
        graphs.add_value_edge(FundingEdge {
            src: OwnerId(scaled_addr(i % n_owners)),
            dst: OwnerId(scaled_addr((i + 1) % n_owners)),
            amount: 1,
            jetton_master: None,
            tx_hash: [0; 32],
            first_ts: u32::try_from(i % usize::try_from(u32::MAX).unwrap()).unwrap(),
        });
    }
    let elapsed = start.elapsed();

    let bytes_per_edge = std::mem::size_of::<FundingEdge>();
    let bytes_per_node = std::mem::size_of::<OwnerId>();

    #[allow(
        clippy::cast_precision_loss,
        reason = "a count of edges and a byte total at this size are exact in a double, and both figures are rates"
    )]
    {
        let estimated_mb = (n_edges * bytes_per_edge + graphs.value_node_count() * bytes_per_node)
            as f64
            / 1_000_000.0;
        eprintln!(
            "add_value_edge: {n_edges} edges / {} owners in {elapsed:?} ({:.0} edges/s); \
             the edges and nodes themselves are about {estimated_mb:.1} MB, before what the \
             graph and its index add",
            graphs.value_node_count(),
            n_edges as f64 / elapsed.as_secs_f64(),
        );
    }

    assert!(
        elapsed < std::time::Duration::from_secs(300),
        "elapsed={elapsed:?}"
    );
}
