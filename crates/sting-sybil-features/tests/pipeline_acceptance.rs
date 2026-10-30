//! The whole extraction over one synthetic world, checked against expectations worked out by
//! hand.
//!
//! The groups of features each have their own unit tests. What is asked here is whether they hold
//! together: whether the world's metronome really does come out with tighter spacing than its
//! irregular wallet, whether the address that was funded and did nothing gets a row of zeros
//! rather than a row of NaNs, whether the chain is found, and whether the columns that come out
//! are the canonical ones in the canonical order.
//!
//! There is no independent oracle here, unlike the decoder's tests. The ground truth is arithmetic
//! done by hand over a scenario chosen to be realistic.
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

use std::collections::HashMap;

use sting_sybil_features::schema::wallet_features_schema;
use sting_sybil_features::{
    FeatureExtractionConfig, TopologyThresholds, WalletFeatureRow, build_record_batch,
    extract_features_for_range, fixtures,
};
use sting_sybil_types::OwnerId;

fn default_cfg() -> FeatureExtractionConfig {
    FeatureExtractionConfig {
        feature_version: "v-test".to_string(),
        snapshot_ts: 999_999,
        burst_factor: 0.5,
        topology: TopologyThresholds {
            star_fan_threshold: 20,
            tree_fan_threshold: 20,
        },
    }
}

fn extract_scenario() -> Vec<WalletFeatureRow> {
    let (graphs, txs, messages) = fixtures::synthetic_owner_scenario();
    extract_features_for_range(&txs, &messages, &[], &graphs, &default_cfg())
}

fn by_owner(rows: &[WalletFeatureRow]) -> HashMap<OwnerId, &WalletFeatureRow> {
    rows.iter().map(|row| (row.owner, row)).collect()
}

#[test]
fn every_owner_in_the_graphs_gets_exactly_one_row() {
    let (graphs, ..) = fixtures::synthetic_owner_scenario();
    let expected_owners: std::collections::HashSet<OwnerId> = graphs
        .gas
        .node_indices()
        .map(|index| graphs.gas[index])
        .chain(graphs.value.node_indices().map(|index| graphs.value[index]))
        .collect();

    let rows = extract_scenario();
    assert_eq!(rows.len(), expected_owners.len());
    let row_owners: std::collections::HashSet<OwnerId> = rows.iter().map(|row| row.owner).collect();
    assert_eq!(row_owners, expected_owners);
}

#[test]
fn the_rows_come_out_sorted_by_owner() {
    let rows = extract_scenario();
    let owners: Vec<sting_sybil_types::TonAddress> = rows.iter().map(|row| row.owner.0).collect();
    let mut sorted = owners.clone();
    sorted.sort();
    assert_eq!(owners, sorted);
}

#[test]
fn the_metronome_is_tighter_spaced_than_the_irregular_wallet() {
    let rows = extract_scenario();
    let map = by_owner(&rows);
    let bot = map[&fixtures::owner(fixtures::BOT)];
    let human = map[&fixtures::owner(fixtures::HUMAN)];

    // Both were funded once and by the same hub, so the event counts are equal. The contrast is
    // in the timestamps of their own transactions, which is a separate channel — fifty against
    // seven — and not in how many events either of them was part of.
    assert_eq!(bot.tx_count, 1);
    assert_eq!(human.tx_count, 1);
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
    // Exactly even spacing means no gap is below half the mean, so there is no burst at all.
    assert_eq!(bot.burst_count, 0);
}

#[test]
fn a_wallet_that_did_nothing_gets_zeros_rather_than_nans() {
    let rows = extract_scenario();
    let map = by_owner(&rows);
    let zero_tx = map[&fixtures::owner(fixtures::ZERO_TX)];

    // It was funded, which is one event; what it has none of is transactions of its own, and that
    // is what the temporal figures below are about.
    assert_eq!(zero_tx.tx_count, 1);
    assert_eq!(zero_tx.in_deg, 1);
    assert_eq!(zero_tx.out_deg, 0);
    assert_eq!(zero_tx.iet_mean, 0.0);
    assert_eq!(zero_tx.active_duration, 0.0);
    assert!(!zero_tx.iet_cv.is_nan());
    assert!(!zero_tx.iet_regularity.is_nan());
    assert!(!zero_tx.deg_ratio.is_nan() && zero_tx.deg_ratio.is_finite());
    assert_eq!(zero_tx.first_funder, Some(fixtures::owner(fixtures::HUB)));
}

#[test]
fn the_hub_reads_as_a_distributor_and_its_children_know_each_other() {
    let rows = extract_scenario();
    let map = by_owner(&rows);
    let hub = map[&fixtures::owner(fixtures::HUB)];

    assert!(hub.star_divergence, "twenty-five is past the threshold");
    assert!(!hub.star_convergence);

    let bot = map[&fixtures::owner(fixtures::BOT)];
    assert_eq!(bot.first_funder, Some(fixtures::owner(fixtures::HUB)));
    assert_eq!(
        bot.shared_funder_count, 24,
        "the other twenty-four addresses the same hub funded"
    );
}

#[test]
fn every_address_on_the_chain_is_flagged_and_the_hub_is_not() {
    let rows = extract_scenario();
    let map = by_owner(&rows);

    for byte in [fixtures::CHAIN_ROOT_FUNDER]
        .into_iter()
        .chain(fixtures::CHAIN)
    {
        let row = map[&fixtures::owner(byte)];
        assert!(row.chain_flag, "owner({byte}) is on the chain");
    }

    // The hub has twenty-five outgoing edges rather than one, so no chain runs through it.
    assert!(!map[&fixtures::owner(fixtures::HUB)].chain_flag);
}

#[test]
fn the_batch_flag_is_unset_rather_than_false_on_every_row() {
    let rows = extract_scenario();
    assert!(
        rows.iter().all(|row| row.bulk_flag.is_none()),
        "nothing can compute it yet, and false would be a claim nobody has established"
    );
}

#[test]
fn the_same_input_extracts_to_the_same_rows() {
    assert_eq!(extract_scenario(), extract_scenario());
}

/// The whole table against a recorded snapshot.
///
/// Individual assertions cover the features somebody thought to assert. A snapshot of every column
/// of every row catches a regression in the ones nobody did.
#[test]
fn the_feature_table_matches_its_recorded_snapshot() {
    insta::assert_json_snapshot!(extract_scenario());
}

#[test]
fn the_batch_carries_the_canonical_schema() {
    let rows = extract_scenario();
    let batch = build_record_batch(&rows).expect("the columns match the schema");

    assert_eq!(
        batch.schema().as_ref(),
        wallet_features_schema().as_ref(),
        "the batch a run produces is the schema the vocabulary states, field for field"
    );
    assert_eq!(batch.num_rows(), rows.len());
    assert_eq!(batch.schema().fields().len(), 31);
}

#[test]
fn a_partition_reads_back_as_it_was_written_and_a_rerun_replaces_it() {
    use parquet::arrow::arrow_reader::ParquetRecordBatchReaderBuilder;
    use std::fs::File;

    let rows = extract_scenario();
    let batch = build_record_batch(&rows).unwrap();

    let dir = std::env::temp_dir().join(format!("sting-sybil-features-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);

    let path = sting_sybil_features::persist::write_partition(&dir, &batch).unwrap();
    assert!(path.exists());

    let file = File::open(&path).unwrap();
    let reader = ParquetRecordBatchReaderBuilder::try_new(file)
        .unwrap()
        .build()
        .unwrap();
    let read_batches: Vec<_> = reader.map(std::result::Result::unwrap).collect();
    let total_rows: usize = read_batches
        .iter()
        .map(arrow::array::RecordBatch::num_rows)
        .sum();
    assert_eq!(total_rows, rows.len());
    assert_eq!(
        read_batches[0].schema().as_ref(),
        wallet_features_schema().as_ref()
    );

    // A second write of the same partition replaces the file rather than adding to it, which is
    // what makes a rerun over the same range idempotent.
    let path_again = sting_sybil_features::persist::write_partition(&dir, &batch).unwrap();
    assert_eq!(path, path_again);
    let file_again = File::open(&path_again).unwrap();
    let reader_again = ParquetRecordBatchReaderBuilder::try_new(file_again)
        .unwrap()
        .build()
        .unwrap();
    let total_rows_again: usize = reader_again.map(|batch| batch.unwrap().num_rows()).sum();
    assert_eq!(total_rows_again, rows.len(), "a rerun leaves one copy");

    let _ = std::fs::remove_dir_all(&dir);
}
