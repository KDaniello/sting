//! What the membership partition promises: the canonical columns, the half this side does not
//! compute left empty, and a partition that replaces itself rather than growing.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "AGENTS.md §5 scopes the no-unwrap/expect/panic rule to library code outside tests"
)]

use petgraph::graph::DiGraph;
use sting_sybil_cluster::materialize_hard_clusters;
use sting_sybil_cluster::schema::{
    ClusterMembershipRow, build_record_batch, cluster_membership_schema,
};
use sting_sybil_types::{FundingEdge, OwnerId, TonAddress};

fn owner(byte: u8) -> OwnerId {
    OwnerId(TonAddress::new(0, [byte; 32]))
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

fn synthetic_rows() -> Vec<ClusterMembershipRow> {
    let mut g: DiGraph<OwnerId, FundingEdge> = DiGraph::new();
    let a = g.add_node(owner(1));
    let b = g.add_node(owner(2));
    // The third owner is funded by nobody, which is a component of one rather than a gap.
    g.add_node(owner(3));
    g.add_edge(a, b, edge(1, 2));

    materialize_hard_clusters(&g, 10_000, 2)
        .into_iter()
        .map(|(owner, info)| ClusterMembershipRow {
            owner,
            wcc_id: info.wcc_id,
            wcc_size: info.wcc_size,
            snapshot_ts: 1_700_000_000,
        })
        .collect()
}

#[test]
fn the_batch_carries_the_canonical_schema() {
    let rows = synthetic_rows();
    let batch = build_record_batch(&rows).expect("the columns match the schema");

    assert_eq!(
        batch.schema().as_ref(),
        cluster_membership_schema().as_ref(),
        "the batch a run produces is the schema the vocabulary states, field for field"
    );
    assert_eq!(batch.num_rows(), rows.len());
    assert_eq!(batch.schema().fields().len(), 7);
}

#[test]
fn the_behavioural_half_is_left_empty_here() {
    use arrow::array::Array;

    let rows = synthetic_rows();
    let batch = build_record_batch(&rows).unwrap();
    for name in ["soft_cluster_id", "soft_cluster_size", "cluster_agreement"] {
        let col = batch.column_by_name(name).unwrap();
        assert_eq!(
            col.null_count(),
            batch.num_rows(),
            "{name} is empty on every row: this side does not compute it, and a zero would read as an answer"
        );
    }
}

#[test]
fn a_partition_reads_back_as_it_was_written_and_a_rerun_replaces_it() {
    use parquet::arrow::arrow_reader::ParquetRecordBatchReaderBuilder;
    use std::fs::File;

    let rows = synthetic_rows();
    let batch = build_record_batch(&rows).unwrap();

    let dir = std::env::temp_dir().join(format!("sting-sybil-cluster-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);

    let path = sting_sybil_cluster::persist::write_partition(&dir, &batch).unwrap();
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
        cluster_membership_schema().as_ref()
    );

    // A second write replaces the file rather than adding to it, which is what lets the other
    // half of the answer be filled in later by rewriting this same partition.
    let path_again = sting_sybil_cluster::persist::write_partition(&dir, &batch).unwrap();
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

/// The membership rows themselves, against a recorded snapshot.
///
/// The schema test says the columns are right; this says the values are. A regression in how a
/// component is identified, or in what its size counts, changes a number here and nothing else,
/// which is exactly the change an assertion written by hand would not have been looking for.
#[test]
fn the_membership_rows_match_their_recorded_snapshot() {
    let rows = synthetic_rows();
    // Sorted before it is recorded: the rows arrive from a hash map, and a snapshot that reordered
    // itself between runs would fail for a reason that has nothing to do with clustering.
    let mut readable: Vec<(String, i64, u32, u32)> = rows
        .iter()
        .map(|row| {
            (
                row.owner.0.to_raw_string(),
                row.wcc_id,
                row.wcc_size,
                row.snapshot_ts,
            )
        })
        .collect();
    readable.sort();
    insta::assert_json_snapshot!(readable);
}
