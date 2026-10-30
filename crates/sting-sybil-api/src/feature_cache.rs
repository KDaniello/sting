//! The warm cache the request path is served from.
//!
//! Reads the partitions the pipeline has already written and flattens them into one map of
//! address to named values. Which columns are features is decided by what to leave out rather
//! than by a list to include: an address is bookkeeping, a snapshot is bookkeeping, a component
//! identifier is a label rather than a quantity. Anything else numeric is a feature, so a new
//! feature needs no change here.
//!
//! # The gap this cache documents
//!
//! The behavioural half of clustering never reaches disk — it is computed in memory while a
//! training frame is built and is not persisted anywhere — so `cluster_agreement` and the soft
//! cluster size are absent from every cached row. What is here is the feature partition in full
//! plus the component size from the structural half. Building that persistence is the clustering
//! side's work and not this crate's, and until it exists the counter in [`crate::metrics`] makes
//! the gap visible in production rather than only in this comment.

use std::collections::HashMap;
use std::fs::File;
use std::path::{Path, PathBuf};

use arrow::array::{
    Array, BooleanArray, Float64Array, Int16Array, Int32Array, RecordBatch, StringArray,
};
use parquet::arrow::arrow_reader::ParquetRecordBatchReaderBuilder;

const NON_FEATURE_COLUMNS: &[&str] = &[
    "owner",
    "snapshot_ts",
    "feature_version",
    "first_funder",
    "wcc_id",
];

fn list_subdirs_with_prefix(root: &Path, prefix: &str) -> std::io::Result<Vec<PathBuf>> {
    let mut dirs = Vec::new();
    if !root.exists() {
        return Ok(dirs);
    }
    for entry in std::fs::read_dir(root)? {
        let entry = entry?;
        if entry.file_type()?.is_dir() && entry.file_name().to_string_lossy().starts_with(prefix) {
            dirs.push(entry.path());
        }
    }
    Ok(dirs)
}

fn list_parquet_files(dir: &Path) -> std::io::Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    if !dir.exists() {
        return Ok(files);
    }
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path
            .extension()
            .is_some_and(|extension| extension == "parquet")
        {
            files.push(path);
        }
    }
    Ok(files)
}

fn read_batches(path: &Path) -> Result<Vec<RecordBatch>, CacheError> {
    let file = File::open(path)?;
    let reader = ParquetRecordBatchReaderBuilder::try_new(file)?.build()?;
    Ok(reader.collect::<Result<Vec<_>, _>>()?)
}

/// What loading the cache answers with when it cannot.
#[derive(Debug, thiserror::Error)]
pub enum CacheError {
    /// A directory or file could not be read.
    #[error("reading the feature store failed: {0}")]
    Io(#[from] std::io::Error),
    /// A partition file could not be read.
    #[error("parquet refused: {0}")]
    Parquet(#[from] parquet::errors::ParquetError),
    /// A partition held a batch this reader could not take apart.
    #[error("columnar layout refused: {0}")]
    Arrow(#[from] arrow::error::ArrowError),
}

/// The address column as written, which is already the raw form a caller will ask with.
fn owner_column(batch: &RecordBatch) -> Option<&StringArray> {
    batch
        .column_by_name("owner")?
        .as_any()
        .downcast_ref::<StringArray>()
}

/// One cell as a number, whatever width or type the column holds it in.
///
/// A null is `None` rather than zero: a feature that was not computed and a feature that came out
/// zero are different things, and the model treats an absent one as contributing nothing.
fn numeric_value(column: &dyn Array, row: usize) -> Option<f64> {
    if column.is_null(row) {
        return None;
    }
    if let Some(values) = column.as_any().downcast_ref::<Float64Array>() {
        return Some(values.value(row));
    }
    if let Some(values) = column.as_any().downcast_ref::<Int32Array>() {
        return Some(f64::from(values.value(row)));
    }
    if let Some(values) = column.as_any().downcast_ref::<Int16Array>() {
        return Some(f64::from(values.value(row)));
    }
    if let Some(values) = column.as_any().downcast_ref::<BooleanArray>() {
        return Some(if values.value(row) { 1.0 } else { 0.0 });
    }
    None
}

/// Folds one batch into the cache, one entry per address.
///
/// Where an address appears in more than one partition, the newest snapshot wins outright: the
/// request path wants the current state of an address, not its history.
fn merge_batch(
    batch: &RecordBatch,
    cache: &mut HashMap<String, HashMap<String, f64>>,
    latest_snapshot: &mut HashMap<String, i64>,
) {
    let Some(owners) = owner_column(batch) else {
        return;
    };
    let snapshot_col = batch.column_by_name("snapshot_ts").and_then(|column| {
        column
            .as_any()
            .downcast_ref::<arrow::array::TimestampSecondArray>()
    });

    let schema = batch.schema();
    let feature_cols: Vec<(&str, &std::sync::Arc<dyn Array>)> = schema
        .fields()
        .iter()
        .map(|field| field.name().as_str())
        .zip(batch.columns())
        .filter(|(name, _)| !NON_FEATURE_COLUMNS.contains(name))
        .collect();

    for row in 0..batch.num_rows() {
        if owners.is_null(row) {
            continue;
        }
        let owner = owners.value(row).to_string();
        let snapshot_ts = snapshot_col.map_or(0, |column| column.value(row));

        if let Some(&existing) = latest_snapshot.get(&owner)
            && snapshot_ts < existing
        {
            continue;
        }
        latest_snapshot.insert(owner.clone(), snapshot_ts);

        let entry = cache.entry(owner).or_default();
        entry.clear();
        for (name, column) in &feature_cols {
            if let Some(value) = numeric_value(column.as_ref(), row) {
                entry.insert((*name).to_string(), value);
            }
        }
    }
}

/// The whole cache: every feature partition of the version asked for, plus the component sizes.
///
/// The empty columns of the membership partition need no filtering by name — they are null, and a
/// null is not a value.
///
/// # Errors
/// [`CacheError`] when a directory cannot be listed or a partition cannot be read.
pub fn load_feature_cache(
    features_root: &Path,
    cluster_root: &Path,
    feature_version: &str,
) -> Result<HashMap<String, HashMap<String, f64>>, CacheError> {
    let mut cache = HashMap::new();
    let mut latest_snapshot = HashMap::new();

    for dt_dir in list_subdirs_with_prefix(features_root, "dt=")? {
        let version_dir = dt_dir.join(format!("feature_version={feature_version}"));
        for file in list_parquet_files(&version_dir)? {
            for batch in read_batches(&file)? {
                merge_batch(&batch, &mut cache, &mut latest_snapshot);
            }
        }
    }

    // The membership partitions have no version level: they are laid out by day alone.
    for dt_dir in list_subdirs_with_prefix(cluster_root, "dt=")? {
        for file in list_parquet_files(&dt_dir)? {
            for batch in read_batches(&file)? {
                merge_cluster_batch(&batch, &mut cache);
            }
        }
    }

    Ok(cache)
}

/// Folds membership into whatever is already cached for an address.
///
/// Additive rather than replacing: features and component size are two independent sources about
/// one address, and either one arriving second must not erase the other.
fn merge_cluster_batch(batch: &RecordBatch, cache: &mut HashMap<String, HashMap<String, f64>>) {
    let Some(owners) = owner_column(batch) else {
        return;
    };
    let schema = batch.schema();
    let feature_cols: Vec<(&str, &std::sync::Arc<dyn Array>)> = schema
        .fields()
        .iter()
        .map(|field| field.name().as_str())
        .zip(batch.columns())
        .filter(|(name, _)| !NON_FEATURE_COLUMNS.contains(name))
        .collect();

    for row in 0..batch.num_rows() {
        if owners.is_null(row) {
            continue;
        }
        let owner = owners.value(row).to_string();
        let entry = cache.entry(owner).or_default();
        for (name, column) in &feature_cols {
            if let Some(value) = numeric_value(column.as_ref(), row) {
                entry.insert((*name).to_string(), value);
            }
        }
    }
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::float_cmp,
    reason = "AGENTS.md §5 scopes the no-unwrap rule to library code outside tests, and these compare values read straight back out of a file"
)]
mod tests {
    use super::*;
    use arrow::array::{Float64Array as F64Arr, Int32Array as I32Arr, StringArray as StrArr};
    use arrow::datatypes::{DataType, Field, Schema, TimeUnit};
    use parquet::arrow::ArrowWriter;
    use std::sync::Arc;

    fn write_features_parquet(
        root: &Path,
        dt: &str,
        feature_version: &str,
        owners: &[&str],
        iet_cv: &[f64],
        snapshot_ts: i64,
    ) {
        let dir = root
            .join(format!("dt={dt}"))
            .join(format!("feature_version={feature_version}"));
        std::fs::create_dir_all(&dir).unwrap();
        let schema = Arc::new(Schema::new(vec![
            Field::new("owner", DataType::Utf8, true),
            Field::new(
                "snapshot_ts",
                DataType::Timestamp(TimeUnit::Second, None),
                true,
            ),
            Field::new("feature_version", DataType::Utf8, true),
            Field::new("iet_cv", DataType::Float64, true),
        ]));
        let n = owners.len();
        let batch = RecordBatch::try_new(
            schema.clone(),
            vec![
                Arc::new(StrArr::from(owners.to_vec())),
                Arc::new(arrow::array::TimestampSecondArray::from(vec![
                    snapshot_ts;
                    n
                ])),
                Arc::new(StrArr::from(vec![feature_version; n])),
                Arc::new(F64Arr::from(iet_cv.to_vec())),
            ],
        )
        .unwrap();
        let file = File::create(dir.join("part-0.parquet")).unwrap();
        let mut writer = ArrowWriter::try_new(file, schema, None).unwrap();
        writer.write(&batch).unwrap();
        writer.close().unwrap();
    }

    fn write_cluster_parquet(root: &Path, dt: &str, owners: &[&str], wcc_size: &[i32]) {
        let dir = root.join(format!("dt={dt}"));
        std::fs::create_dir_all(&dir).unwrap();
        let schema = Arc::new(Schema::new(vec![
            Field::new("owner", DataType::Utf8, true),
            Field::new("wcc_id", DataType::Int64, true),
            Field::new("wcc_size", DataType::Int32, true),
        ]));
        let n = owners.len();
        let batch = RecordBatch::try_new(
            schema.clone(),
            vec![
                Arc::new(StrArr::from(owners.to_vec())),
                Arc::new(arrow::array::Int64Array::from(vec![1i64; n])),
                Arc::new(I32Arr::from(wcc_size.to_vec())),
            ],
        )
        .unwrap();
        let file = File::create(dir.join("part-0.parquet")).unwrap();
        let mut writer = ArrowWriter::try_new(file, schema, None).unwrap();
        writer.write(&batch).unwrap();
        writer.close().unwrap();
    }

    #[test]
    fn a_written_partition_loads_into_the_cache_it_serves_from() {
        let dir =
            std::env::temp_dir().join(format!("sybil-rt-feature-cache-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let features_root = dir.join("features");
        let cluster_root = dir.join("cluster");

        write_features_parquet(
            &features_root,
            "2026-07-16",
            "v1",
            &["0:a", "0:b"],
            &[0.1, 0.9],
            1_000,
        );
        write_cluster_parquet(&cluster_root, "2026-07-16", &["0:a"], &[5]);

        let cache = load_feature_cache(&features_root, &cluster_root, "v1").unwrap();

        assert_eq!(cache["0:a"]["iet_cv"], 0.1);
        assert_eq!(cache["0:a"]["wcc_size"], 5.0);
        assert_eq!(cache["0:b"]["iet_cv"], 0.9);
        assert!(
            !cache["0:b"].contains_key("wcc_size"),
            "owner without a cluster row has no wcc_size"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_newest_snapshot_of_an_address_is_the_one_kept() {
        let dir = std::env::temp_dir().join(format!(
            "sybil-rt-feature-cache-latest-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        let features_root = dir.join("features");
        let cluster_root = dir.join("cluster");

        write_features_parquet(&features_root, "2026-07-10", "v1", &["0:a"], &[0.5], 1_000);
        write_features_parquet(&features_root, "2026-07-16", "v1", &["0:a"], &[0.05], 2_000);

        let cache = load_feature_cache(&features_root, &cluster_root, "v1").unwrap();
        assert_eq!(cache["0:a"]["iet_cv"], 0.05, "the later partition wins");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_partition_of_another_vocabulary_version_is_not_loaded() {
        let dir = std::env::temp_dir().join(format!(
            "sybil-rt-feature-cache-version-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        let features_root = dir.join("features");
        let cluster_root = dir.join("cluster");

        write_features_parquet(&features_root, "2026-07-16", "v1", &["0:a"], &[0.1], 1_000);
        write_features_parquet(&features_root, "2026-07-16", "v2", &["0:b"], &[0.2], 1_000);

        let cache = load_feature_cache(&features_root, &cluster_root, "v1").unwrap();
        assert!(cache.contains_key("0:a"));
        assert!(!cache.contains_key("0:b"));

        let _ = std::fs::remove_dir_all(&dir);
    }
}
