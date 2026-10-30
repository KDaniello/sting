//! The canonical membership row, as code rather than as a document.
//!
//! Seven columns, of which this side fills four. The soft cluster, its size and whether the two
//! halves agree are left empty here and written by the side that computes them, which then
//! replaces the same partition — the two halves meet in the file rather than in a process.
//!
//! Every field is nullable, for the same compatibility reason the feature schema gives.

use std::sync::Arc;

use arrow::array::{
    Array, BooleanArray, Int32Array, Int64Array, RecordBatch, StringArray, TimestampSecondArray,
};
use arrow::datatypes::{DataType, Field, Schema, TimeUnit};
use arrow::error::ArrowError;
use sting_sybil_types::OwnerId;

/// The canonical schema, in the canonical order.
#[must_use]
pub fn cluster_membership_schema() -> Arc<Schema> {
    Arc::new(Schema::new(vec![
        Field::new("owner", DataType::Utf8, true),
        Field::new("wcc_id", DataType::Int64, true),
        Field::new("wcc_size", DataType::Int32, true),
        Field::new("soft_cluster_id", DataType::Int32, true),
        Field::new("soft_cluster_size", DataType::Int32, true),
        Field::new("cluster_agreement", DataType::Boolean, true),
        Field::new(
            "snapshot_ts",
            DataType::Timestamp(TimeUnit::Second, None),
            true,
        ),
    ]))
}

/// One owner's membership, by the structural half of the answer.
#[derive(Clone, Debug, PartialEq)]
pub struct ClusterMembershipRow {
    /// Whose membership this is.
    pub owner: OwnerId,
    /// Which component.
    pub wcc_id: i64,
    /// How many owners are in it.
    pub wcc_size: u32,
    /// The point in time it describes.
    pub snapshot_ts: u32,
}

/// The rows as columns, sorted by owner.
///
/// Sorted for the reason the feature builder gives: the rows arrive from a hash map, and a
/// partition that reordered itself between runs would not be reproducible.
///
/// # Errors
/// [`ArrowError`] when the columns do not match the schema, which can only happen if this
/// function and the schema above have drifted apart.
pub fn build_record_batch(rows: &[ClusterMembershipRow]) -> Result<RecordBatch, ArrowError> {
    let mut sorted: Vec<&ClusterMembershipRow> = rows.iter().collect();
    sorted.sort_by_key(|row| row.owner.0);

    let owner: Vec<String> = sorted
        .iter()
        .map(|row| row.owner.0.to_raw_string())
        .collect();
    let wcc_id: Vec<i64> = sorted.iter().map(|row| row.wcc_id).collect();
    let wcc_size: Vec<i32> = sorted
        .iter()
        .map(|row| i32::try_from(row.wcc_size).unwrap_or(i32::MAX))
        .collect();
    let soft_cluster_id: Vec<Option<i32>> = sorted.iter().map(|_| None).collect();
    let soft_cluster_size: Vec<Option<i32>> = sorted.iter().map(|_| None).collect();
    let cluster_agreement: Vec<Option<bool>> = sorted.iter().map(|_| None).collect();
    let snapshot_ts: Vec<i64> = sorted
        .iter()
        .map(|row| i64::from(row.snapshot_ts))
        .collect();

    let columns: Vec<Arc<dyn Array>> = vec![
        Arc::new(StringArray::from(owner)),
        Arc::new(Int64Array::from(wcc_id)),
        Arc::new(Int32Array::from(wcc_size)),
        Arc::new(Int32Array::from(soft_cluster_id)),
        Arc::new(Int32Array::from(soft_cluster_size)),
        Arc::new(BooleanArray::from(cluster_agreement)),
        Arc::new(TimestampSecondArray::from(snapshot_ts)),
    ];

    RecordBatch::try_new(cluster_membership_schema(), columns)
}
