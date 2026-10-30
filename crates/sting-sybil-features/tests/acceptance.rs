//! What each group of features promises on its own.
//!
//! Two of these are worth naming. Every edge case produces a number rather than a NaN — a single
//! NaN in a feature vector destroys whatever scaler reads it and takes the whole batch with it.
//! And nothing here is a gas-price feature: on this chain fees say almost nothing about who is
//! behind an address, and a feature built on them would be measuring the network's pricing rather
//! than anybody's behaviour.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "AGENTS.md §5 scopes the no-unwrap/expect/panic rule to library code outside tests"
)]
#![allow(
    clippy::float_cmp,
    reason = "the values compared here are exact by construction — a zero a branch returns, or an amount read straight back out"
)]

use std::collections::HashSet;

use petgraph::graph::DiGraph;
use sting_sybil_features::{
    WalletEvent, detect_star_convergence, detect_star_divergence, extract_aggregate,
    extract_temporal, is_chain, is_tree_root, jaccard, pairwise_jaccard_within_groups,
};
use sting_sybil_types::{FundingEdge, OwnerId, TonAddress};

fn addr(byte: u8) -> TonAddress {
    TonAddress::new(0, [byte; 32])
}

fn owner(byte: u8) -> OwnerId {
    OwnerId(addr(byte))
}

fn edge(src: u8, dst: u8) -> FundingEdge {
    FundingEdge {
        src: owner(src),
        dst: owner(dst),
        amount: 1,
        jetton_master: None,
        tx_hash: [0; 32],
        first_ts: 0,
    }
}

#[test]
fn one_transaction_leaves_no_gap_to_measure() {
    let temporal = extract_temporal(&[1000]);
    assert_eq!(temporal.iet_mean, 0.0);
    assert_eq!(temporal.active_duration, 0.0);
    assert!(!temporal.iet_cv.is_nan());
    assert!(!temporal.iet_regularity.is_nan());
}

#[test]
fn no_transactions_at_all_is_a_row_of_zeros() {
    let temporal = extract_temporal(&[]);
    assert_eq!(temporal, extract_temporal(&[]));

    let aggregate = extract_aggregate(&[]);
    assert_eq!(aggregate.tx_count, 0);
    assert_eq!(
        aggregate.deg_ratio, 0.0,
        "smoothed by one, so nothing divides by nothing"
    );
}

#[test]
fn a_wallet_that_only_receives_still_has_a_finite_ratio() {
    let events = vec![
        WalletEvent::native(owner(1), 100, false),
        WalletEvent::native(owner(2), 200, false),
    ];
    let agg = extract_aggregate(&events);
    assert_eq!(agg.out_deg, 0);
    assert_eq!(agg.amount_mean_sent, 0.0);
    assert!(!agg.deg_ratio.is_nan() && agg.deg_ratio.is_finite());
}

#[test]
fn even_spacing_reads_as_tighter_than_wildly_varying_spacing() {
    let bot_timestamps: Vec<u32> = (0..50).map(|i| i * 60).collect();
    let bot = extract_temporal(&bot_timestamps);

    // Not "a person acts rarely" — what separates them is the spread of the gaps, and pinning a
    // direction is exactly the mistake this feature exists to avoid.
    let human_intervals = [
        5u32, 340, 12, 890, 45, 1200, 3, 670, 88, 2, 990, 15, 430, 60, 1500,
    ];
    let mut t = 0u32;
    let mut human_timestamps = vec![0u32];
    for gap in human_intervals {
        t += gap;
        human_timestamps.push(t);
    }
    let human = extract_temporal(&human_timestamps);

    assert!(
        bot.iet_cv < human.iet_cv,
        "bot={}, human={}",
        bot.iet_cv,
        human.iet_cv
    );
    assert!(
        bot.iet_regularity > human.iet_regularity,
        "bot={}, human={}",
        bot.iet_regularity,
        human.iet_regularity
    );
}

#[test]
fn similarity_reads_off_known_sets() {
    let a: HashSet<_> = [1, 2, 3].into_iter().collect();
    let b: HashSet<_> = [2, 3, 4].into_iter().collect();
    assert!((jaccard(&a, &b) - 0.5).abs() < 1e-9);

    let identical: HashSet<_> = [1, 2, 3].into_iter().collect();
    assert!((jaccard(&a, &identical) - 1.0).abs() < 1e-9);

    let disjoint: HashSet<_> = [10, 20].into_iter().collect();
    assert_eq!(jaccard(&a, &disjoint), 0.0);

    let empty: HashSet<i32> = HashSet::new();
    assert_eq!(
        jaccard(&empty, &empty),
        0.0,
        "knowing nobody is not something two wallets have in common"
    );
}

#[test]
fn comparing_within_groups_is_not_comparing_everybody() {
    // A hundred groups of ten is 4 500 comparisons; the same thousand wallets compared globally
    // would be 499 500, and at a real size that difference is the whole feasibility of it.
    let groups: Vec<Vec<HashSet<u32>>> = (0..100)
        .map(|group| (0..10).map(|i| HashSet::from([group * 10 + i])).collect())
        .collect();
    let results = pairwise_jaccard_within_groups(&groups);
    let total_pairs: usize = results.iter().map(Vec::len).sum();
    assert_eq!(total_pairs, 100 * (10 * 9 / 2));
}

#[test]
fn a_fan_out_reads_as_a_distributor_and_a_single_edge_does_not() {
    let mut graph: DiGraph<OwnerId, FundingEdge> = DiGraph::new();
    let hub = graph.add_node(owner(0));
    for i in 1..=25u8 {
        let leaf = graph.add_node(owner(i));
        graph.add_edge(hub, leaf, edge(0, i));
    }
    assert!(detect_star_divergence(&graph, hub, 20));

    let normal = graph.add_node(owner(200));
    let another = graph.add_node(owner(201));
    graph.add_edge(normal, another, edge(200, 201));
    assert!(!detect_star_divergence(&graph, normal, 20));
}

#[test]
fn a_fan_in_reads_as_a_collector() {
    let mut graph: DiGraph<OwnerId, FundingEdge> = DiGraph::new();
    let collector = graph.add_node(owner(0));
    for i in 1..=25u8 {
        let source = graph.add_node(owner(i));
        graph.add_edge(source, collector, edge(i, 0));
    }
    assert!(detect_star_convergence(&graph, collector, 20));
}

#[test]
fn a_path_is_a_chain_until_something_else_joins_it() {
    let mut graph: DiGraph<OwnerId, FundingEdge> = DiGraph::new();
    let nodes: Vec<_> = (0..5u8).map(|i| graph.add_node(owner(i))).collect();
    for pair in nodes.windows(2) {
        graph.add_edge(pair[0], pair[1], edge(0, 0));
    }
    assert!(is_chain(&graph, &nodes));

    // A second edge into the middle of it: the path is still a path, but the node it runs
    // through is doing something else as well.
    let extra = graph.add_node(owner(99));
    graph.add_edge(extra, nodes[2], edge(99, 2));
    assert!(!is_chain(&graph, &nodes));
}

#[test]
fn a_distributor_whose_child_distributes_is_a_tree_root() {
    let mut graph: DiGraph<OwnerId, FundingEdge> = DiGraph::new();
    let root = graph.add_node(owner(0));
    let mut children = vec![];
    for i in 1..=25u8 {
        let child = graph.add_node(owner(i));
        graph.add_edge(root, child, edge(0, i));
        children.push(child);
    }
    for i in 26..=50u8 {
        let grandchild = graph.add_node(owner(i));
        graph.add_edge(children[0], grandchild, edge(1, i));
    }
    assert!(is_tree_root(&graph, root, 20));
}

#[test]
fn the_same_input_extracts_to_the_same_features() {
    let timestamps = [100u32, 160, 250, 400, 900];
    assert_eq!(extract_temporal(&timestamps), extract_temporal(&timestamps));

    let events = vec![
        WalletEvent::native(owner(1), 10, true),
        WalletEvent::native(owner(2), 20, false),
    ];
    let same_again = vec![
        WalletEvent::native(owner(1), 10, true),
        WalletEvent::native(owner(2), 20, false),
    ];
    assert_eq!(extract_aggregate(&events), extract_aggregate(&same_again));
}

/// A pace floor rather than a benchmark: twenty thousand wallets of twenty events each, which
/// must run at ten thousand wallets a second or better.
///
/// What it catches is a change that makes the pass superlinear, which at chain size is the
/// difference between a run and a run that never ends.
#[test]
fn the_temporal_pass_holds_ten_thousand_wallets_a_second() {
    let n = 20_000usize;
    let start = std::time::Instant::now();
    for i in 0..n {
        let timestamps: Vec<u32> = (0..20)
            .map(|j| u32::try_from(i).unwrap_or(0) * 1000 + j * 60)
            .collect();
        let _ = extract_temporal(&timestamps);
    }
    let elapsed = start.elapsed();
    #[allow(
        clippy::cast_precision_loss,
        reason = "a wallet count at this size is exact in a double, and the figure is a rate"
    )]
    let per_sec = n as f64 / elapsed.as_secs_f64();
    assert!(
        per_sec >= 10_000.0,
        "{per_sec:.0} wallets a second, over {elapsed:?}"
    );
}

/// A guard against a feature nobody should add.
///
/// Gas price and gas used are not signals on this chain, and a field carrying one would look
/// perfectly ordinary in a review. Reading the sources for the names is crude and catches it.
#[test]
fn no_gas_price_feature_has_crept_in() {
    let src_files = [
        include_str!("../src/temporal.rs"),
        include_str!("../src/aggregate.rs"),
        include_str!("../src/topology.rs"),
    ];
    for src in src_files {
        let lowered = src.to_lowercase();
        assert!(
            !lowered.contains("gas_price") && !lowered.contains("gas_used"),
            "a gas-price feature measures the network, not the wallet"
        );
    }
}

#[test]
fn the_largest_amount_and_repeated_timestamps_stay_finite() {
    let events = vec![
        WalletEvent::native(owner(1), u128::MAX, true),
        WalletEvent::native(owner(1), 0, false),
    ];
    let agg = extract_aggregate(&events);
    assert!(agg.amount_mean_sent.is_finite());

    // Several transactions inside one block share a timestamp, so every gap is zero: the mean is
    // zero and the coefficient of variation is defined as zero rather than left as a NaN.
    let timestamps = [500u32, 500, 500, 500];
    let temporal = extract_temporal(&timestamps);
    assert!(!temporal.iet_cv.is_nan());
    assert!(!temporal.iet_regularity.is_nan());
}

proptest::proptest! {
    /// For any timestamps and any amount, nothing panics and nothing comes out as a NaN.
    #[test]
    fn prop_no_input_produces_a_nan(
        mut timestamps in proptest::collection::vec(0u32..1_000_000, 0..50),
        amount in 0u128..=u128::MAX,
    ) {
        timestamps.sort_unstable();
        let temporal = extract_temporal(&timestamps);
        proptest::prop_assert!(!temporal.iet_cv.is_nan());
        proptest::prop_assert!(!temporal.iet_regularity.is_nan());
        proptest::prop_assert!(!temporal.iet_mean.is_nan());

        let events = vec![WalletEvent::native(owner(1), amount, true)];
        let agg = extract_aggregate(&events);
        proptest::prop_assert!(agg.amount_mean_sent.is_finite());
        proptest::prop_assert!(!agg.deg_ratio.is_nan());
    }
}
