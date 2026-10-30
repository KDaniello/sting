//! Partitioned columnar files on disk, written the same way whatever is being written.
//!
//! Two things in this contour are stored as partitions — what a wallet looks like, and which
//! cluster it is in — and they differ in exactly one respect: how many levels the path has. The
//! rule about what a partition is does not differ at all, and stating it twice would be stating
//! it twice: a write that replaced a partition in one place and appended in the other would make
//! a rerun mean two different things depending on which output was being looked at.
//!
//! A partition is replaced rather than added to. That is what makes a rerun over the same range
//! idempotent, and it is what lets a second stage read a partition, fill in the columns it owns,
//! and write it back.
//!
//! # What this crate does not do
//!
//! It reads nothing back: a reader wants columns rather than files, and reading is done through
//! whatever query engine is at hand. It knows no schema — the batch carries its own — and it
//! decides no partition of its own: the batch says which day and which version it belongs to,
//! because that is what a reader will look under.

use std::fs::File;
use std::path::{Path, PathBuf};

use arrow::array::{Array, RecordBatch, StringArray, TimestampSecondArray};
use parquet::arrow::ArrowWriter;
use sting_sybil_types::epoch_seconds_to_ymd;

/// What a partition write refuses with.
#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    /// The file store refused.
    #[error("writing the partition failed: {0}")]
    Io(#[from] std::io::Error),
    /// The columnar file could not be written.
    #[error("parquet refused: {0}")]
    Parquet(#[from] parquet::errors::ParquetError),
    /// The batch spans more than one partition, so there is no one place to put it.
    #[error("{column} is not uniform across the batch: one write is one partition")]
    MixedPartition {
        /// Which column carried more than one value.
        column: &'static str,
    },
    /// The batch is missing a column the partition path is derived from.
    #[error("the batch carries no {column} column")]
    MissingColumn {
        /// Which column.
        column: &'static str,
    },
    /// A column held a type this writer does not expect.
    #[error("{column} is not of the type this schema states")]
    WrongColumnType {
        /// Which column.
        column: &'static str,
    },
    /// An empty batch names no partition.
    #[error("an empty batch names no partition")]
    EmptyBatch,
}

/// Whether the path carries a vocabulary version below the day.
///
/// Features do: two versions of the vocabulary describe the same wallets on the same day and must
/// not overwrite each other. Cluster membership does not: an address is in one cluster on a given
/// day, and there is no second answer to keep apart from the first.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Layout {
    /// `<root>/dt=<day>/`.
    ByDay,
    /// `<root>/dt=<day>/feature_version=<version>/`.
    ByDayAndVersion,
}

/// The column every batch is partitioned by.
const SNAPSHOT_COLUMN: &str = "snapshot_ts";
/// The column a versioned layout adds a level for.
const VERSION_COLUMN: &str = "feature_version";
/// One file per partition. A second would need a rule for naming it, and nothing here writes
/// enough rows at once to want one.
const PART_FILE: &str = "part-0.parquet";

/// Writes `batch` as one partition under `root`, replacing whatever was there.
///
/// # Errors
/// [`StoreError::EmptyBatch`] when there are no rows, [`StoreError::MixedPartition`] when the
/// batch spans more than one partition, and the underlying error when the file cannot be written.
pub fn write_partition(
    root: &Path,
    batch: &RecordBatch,
    layout: Layout,
) -> Result<PathBuf, StoreError> {
    if batch.num_rows() == 0 {
        return Err(StoreError::EmptyBatch);
    }

    let dir = match layout {
        Layout::ByDay => root.join(format!("dt={}", uniform_day(batch)?)),
        Layout::ByDayAndVersion => root
            .join(format!("dt={}", uniform_day(batch)?))
            .join(format!(
                "feature_version={}",
                uniform_text(batch, VERSION_COLUMN)?
            )),
    };
    std::fs::create_dir_all(&dir)?;
    let path = dir.join(PART_FILE);

    let file = File::create(&path)?;
    let mut writer = ArrowWriter::try_new(file, batch.schema(), None)?;
    writer.write(batch)?;
    writer.close()?;

    Ok(path)
}

/// The one value `column` holds across the whole batch.
fn uniform_text(batch: &RecordBatch, column: &'static str) -> Result<String, StoreError> {
    let values = batch
        .column_by_name(column)
        .ok_or(StoreError::MissingColumn { column })?
        .as_any()
        .downcast_ref::<StringArray>()
        .ok_or(StoreError::WrongColumnType { column })?;
    let first = values.value(0).to_string();
    if (0..values.len()).any(|row| values.is_null(row) || values.value(row) != first) {
        return Err(StoreError::MixedPartition { column });
    }
    Ok(first)
}

/// The one day the batch describes, as the partition names it.
fn uniform_day(batch: &RecordBatch) -> Result<String, StoreError> {
    let values = batch
        .column_by_name(SNAPSHOT_COLUMN)
        .ok_or(StoreError::MissingColumn {
            column: SNAPSHOT_COLUMN,
        })?
        .as_any()
        .downcast_ref::<TimestampSecondArray>()
        .ok_or(StoreError::WrongColumnType {
            column: SNAPSHOT_COLUMN,
        })?;
    let first = values.value(0);
    if (0..values.len()).any(|row| values.is_null(row) || values.value(row) != first) {
        return Err(StoreError::MixedPartition {
            column: SNAPSHOT_COLUMN,
        });
    }
    Ok(epoch_seconds_to_ymd(first))
}
