//! The canonical feature row, as code rather than as a document.
//!
//! Thirty-one columns in a fixed order. The Python side declares the same schema separately and
//! the two are kept in step by a test that compares them field for field — there is no code
//! generation in this stack, so nothing but that test would notice a divergence.
//!
//! Every field is nullable, and that is a compatibility fact rather than a preference: the Python
//! library makes fields nullable by default, and a single non-nullable column here would make the
//! two schemas unequal while looking identical to a reader.

use std::sync::Arc;

use arrow::array::{
    Array, BooleanArray, Float64Array, Int16Array, Int32Array, RecordBatch, StringArray,
    TimestampSecondArray,
};
use arrow::datatypes::{DataType, Field, Schema, TimeUnit};
use arrow::error::ArrowError;

use sting_sybil_types::OwnerId;

/// Whether a group acted as one batch, which nothing can answer yet.
///
/// The signal it would need is agreement across a cluster's whole feature set, and that measure
/// does not exist. `None` says "not computed"; `false` would say "definitely not a batch", which
/// is a claim nobody has established. It is a named constant rather than a bare `None` at the
/// call site so that the day it becomes computable is a diff on this line.
pub const BULK_FLAG_NOT_COMPUTED: Option<bool> = None;

/// The canonical schema, in the canonical order.
#[must_use]
pub fn wallet_features_schema() -> Arc<Schema> {
    Arc::new(Schema::new(vec![
        Field::new("owner", DataType::Utf8, true),
        Field::new(
            "snapshot_ts",
            DataType::Timestamp(TimeUnit::Second, None),
            true,
        ),
        Field::new("feature_version", DataType::Utf8, true),
        Field::new("iet_mean", DataType::Float64, true),
        Field::new("iet_std", DataType::Float64, true),
        Field::new("iet_cv", DataType::Float64, true),
        Field::new("iet_regularity", DataType::Float64, true),
        Field::new("burst_count", DataType::Int32, true),
        Field::new("longest_burst", DataType::Float64, true),
        Field::new("active_duration", DataType::Float64, true),
        Field::new("active_hours_entropy", DataType::Float64, true),
        Field::new("funding_sync_delta", DataType::Float64, true),
        Field::new("tx_count", DataType::Int32, true),
        Field::new("in_deg", DataType::Int32, true),
        Field::new("out_deg", DataType::Int32, true),
        Field::new("deg_ratio", DataType::Float64, true),
        Field::new("n_unique_in", DataType::Int32, true),
        Field::new("n_unique_out", DataType::Int32, true),
        Field::new("amount_mean_sent", DataType::Float64, true),
        Field::new("amount_var_sent", DataType::Float64, true),
        Field::new("amount_mean_recv", DataType::Float64, true),
        Field::new("amount_var_recv", DataType::Float64, true),
        Field::new("n_jetton_types", DataType::Int16, true),
        Field::new("novelty_ratio", DataType::Float64, true),
        Field::new("first_funder", DataType::Utf8, true),
        Field::new("shared_funder_count", DataType::Int32, true),
        Field::new("star_divergence", DataType::Boolean, true),
        Field::new("star_convergence", DataType::Boolean, true),
        Field::new("chain_flag", DataType::Boolean, true),
        Field::new("tree_flag", DataType::Boolean, true),
        Field::new("bulk_flag", DataType::Boolean, true),
    ]))
}

/// One wallet's features, typed, before they become columns.
///
/// An assembly type and not an API: each group of features is still computed by its own module.
/// What this exists for is to hold thirty-one typed fields together in one place so that the
/// column order is stated once.
///
/// `Serialize` is on it so a snapshot test can compare a whole row as text, the same way the raw
/// store's summary is compared.
#[derive(Clone, Debug, PartialEq, serde::Serialize)]
#[allow(
    clippy::struct_excessive_bools,
    reason = "the shape flags are what the vocabulary states; grouping them into a sub-struct would change the column layout the Python side is compared against"
)]
pub struct WalletFeatureRow {
    /// Whose row this is.
    pub owner: OwnerId,
    /// The point in time the features describe.
    pub snapshot_ts: u32,
    /// Which version of this vocabulary produced them.
    pub feature_version: String,
    /// Mean gap between the wallet's events, in seconds.
    pub iet_mean: f64,
    /// Standard deviation of those gaps.
    pub iet_std: f64,
    /// That deviation over the mean.
    pub iet_cv: f64,
    /// How metronome-like the spacing is.
    pub iet_regularity: f64,
    /// How many runs of unusually close events there were.
    pub burst_count: u32,
    /// How long the longest such run lasted, in seconds.
    pub longest_burst: f64,
    /// From first event to last, in seconds.
    pub active_duration: f64,
    /// How evenly the activity spreads across the hours of a day.
    pub active_hours_entropy: f64,
    /// How far this address's funding was from its siblings', in seconds.
    pub funding_sync_delta: f64,
    /// How many events in total.
    pub tx_count: u32,
    /// How many were incoming.
    pub in_deg: u32,
    /// How many were outgoing.
    pub out_deg: u32,
    /// Outgoing over incoming, smoothed by one.
    pub deg_ratio: f64,
    /// How many distinct senders.
    pub n_unique_in: u32,
    /// How many distinct recipients.
    pub n_unique_out: u32,
    /// Mean amount sent.
    pub amount_mean_sent: f64,
    /// Variance of amounts sent.
    pub amount_var_sent: f64,
    /// Mean amount received.
    pub amount_mean_recv: f64,
    /// Variance of amounts received.
    pub amount_var_recv: f64,
    /// How many distinct jettons were involved.
    pub n_jetton_types: u16,
    /// What share of events were with somebody new.
    pub novelty_ratio: f64,
    /// Who activated the address, if anybody in range did.
    pub first_funder: Option<OwnerId>,
    /// How many other addresses that funder activated.
    pub shared_funder_count: u32,
    /// Whether the address fans out.
    pub star_divergence: bool,
    /// Whether the address collects.
    pub star_convergence: bool,
    /// Whether it sits on a chain of pass-through addresses.
    pub chain_flag: bool,
    /// Whether it is the root of a hierarchy of distributors.
    pub tree_flag: bool,
    /// Whether the group acted as one batch, or nothing yet.
    pub bulk_flag: Option<bool>,
}

/// The rows as columns, sorted by owner.
///
/// Sorted because the rows arrive from hash-map traversals, whose order is not promised: a
/// snapshot taken from an unsorted batch would fail on a rerun for a reason that has nothing to
/// do with features.
///
/// # Errors
/// [`arrow::error::ArrowError`] when the columns do not match the schema, which can only happen
/// if this function and the schema above have drifted apart.
pub fn build_record_batch(rows: &[WalletFeatureRow]) -> Result<RecordBatch, ArrowError> {
    let mut sorted: Vec<&WalletFeatureRow> = rows.iter().collect();
    sorted.sort_by_key(|row| row.owner.0);

    let owner: Vec<String> = sorted.iter().map(|r| r.owner.0.to_raw_string()).collect();
    let snapshot_ts: Vec<i64> = sorted.iter().map(|r| i64::from(r.snapshot_ts)).collect();
    let feature_version: Vec<String> = sorted.iter().map(|r| r.feature_version.clone()).collect();
    let iet_mean: Vec<f64> = sorted.iter().map(|r| r.iet_mean).collect();
    let iet_std: Vec<f64> = sorted.iter().map(|r| r.iet_std).collect();
    let iet_cv: Vec<f64> = sorted.iter().map(|r| r.iet_cv).collect();
    let iet_regularity: Vec<f64> = sorted.iter().map(|r| r.iet_regularity).collect();
    let burst_count: Vec<i32> = sorted
        .iter()
        .map(|r| i32::try_from(r.burst_count).unwrap_or(i32::MAX))
        .collect();
    let longest_burst: Vec<f64> = sorted.iter().map(|r| r.longest_burst).collect();
    let active_duration: Vec<f64> = sorted.iter().map(|r| r.active_duration).collect();
    let active_hours_entropy: Vec<f64> = sorted.iter().map(|r| r.active_hours_entropy).collect();
    let funding_sync_delta: Vec<f64> = sorted.iter().map(|r| r.funding_sync_delta).collect();
    let tx_count: Vec<i32> = sorted
        .iter()
        .map(|r| i32::try_from(r.tx_count).unwrap_or(i32::MAX))
        .collect();
    let in_deg: Vec<i32> = sorted
        .iter()
        .map(|r| i32::try_from(r.in_deg).unwrap_or(i32::MAX))
        .collect();
    let out_deg: Vec<i32> = sorted
        .iter()
        .map(|r| i32::try_from(r.out_deg).unwrap_or(i32::MAX))
        .collect();
    let deg_ratio: Vec<f64> = sorted.iter().map(|r| r.deg_ratio).collect();
    let n_unique_in: Vec<i32> = sorted
        .iter()
        .map(|r| i32::try_from(r.n_unique_in).unwrap_or(i32::MAX))
        .collect();
    let n_unique_out: Vec<i32> = sorted
        .iter()
        .map(|r| i32::try_from(r.n_unique_out).unwrap_or(i32::MAX))
        .collect();
    let amount_mean_sent: Vec<f64> = sorted.iter().map(|r| r.amount_mean_sent).collect();
    let amount_var_sent: Vec<f64> = sorted.iter().map(|r| r.amount_var_sent).collect();
    let amount_mean_recv: Vec<f64> = sorted.iter().map(|r| r.amount_mean_recv).collect();
    let amount_var_recv: Vec<f64> = sorted.iter().map(|r| r.amount_var_recv).collect();
    let n_jetton_types: Vec<i16> = sorted
        .iter()
        .map(|r| i16::try_from(r.n_jetton_types).unwrap_or(i16::MAX))
        .collect();
    let novelty_ratio: Vec<f64> = sorted.iter().map(|r| r.novelty_ratio).collect();
    let first_funder: Vec<Option<String>> = sorted
        .iter()
        .map(|row| row.first_funder.map(|funder| funder.0.to_raw_string()))
        .collect();
    let shared_funder_count: Vec<i32> = sorted
        .iter()
        .map(|r| i32::try_from(r.shared_funder_count).unwrap_or(i32::MAX))
        .collect();
    let star_divergence: Vec<bool> = sorted.iter().map(|r| r.star_divergence).collect();
    let star_convergence: Vec<bool> = sorted.iter().map(|r| r.star_convergence).collect();
    let chain_flag: Vec<bool> = sorted.iter().map(|r| r.chain_flag).collect();
    let tree_flag: Vec<bool> = sorted.iter().map(|r| r.tree_flag).collect();
    let bulk_flag: Vec<Option<bool>> = sorted.iter().map(|r| r.bulk_flag).collect();

    let columns: Vec<Arc<dyn Array>> = vec![
        Arc::new(StringArray::from(owner)),
        Arc::new(TimestampSecondArray::from(snapshot_ts)),
        Arc::new(StringArray::from(feature_version)),
        Arc::new(Float64Array::from(iet_mean)),
        Arc::new(Float64Array::from(iet_std)),
        Arc::new(Float64Array::from(iet_cv)),
        Arc::new(Float64Array::from(iet_regularity)),
        Arc::new(Int32Array::from(burst_count)),
        Arc::new(Float64Array::from(longest_burst)),
        Arc::new(Float64Array::from(active_duration)),
        Arc::new(Float64Array::from(active_hours_entropy)),
        Arc::new(Float64Array::from(funding_sync_delta)),
        Arc::new(Int32Array::from(tx_count)),
        Arc::new(Int32Array::from(in_deg)),
        Arc::new(Int32Array::from(out_deg)),
        Arc::new(Float64Array::from(deg_ratio)),
        Arc::new(Int32Array::from(n_unique_in)),
        Arc::new(Int32Array::from(n_unique_out)),
        Arc::new(Float64Array::from(amount_mean_sent)),
        Arc::new(Float64Array::from(amount_var_sent)),
        Arc::new(Float64Array::from(amount_mean_recv)),
        Arc::new(Float64Array::from(amount_var_recv)),
        Arc::new(Int16Array::from(n_jetton_types)),
        Arc::new(Float64Array::from(novelty_ratio)),
        Arc::new(StringArray::from(first_funder)),
        Arc::new(Int32Array::from(shared_funder_count)),
        Arc::new(BooleanArray::from(star_divergence)),
        Arc::new(BooleanArray::from(star_convergence)),
        Arc::new(BooleanArray::from(chain_flag)),
        Arc::new(BooleanArray::from(tree_flag)),
        Arc::new(BooleanArray::from(bulk_flag)),
    ];

    RecordBatch::try_new(wallet_features_schema(), columns)
}
