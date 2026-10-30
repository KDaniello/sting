//! Where a feature partition goes, and what its path says about it.
//!
//! Two levels: the day the rows describe, then the vocabulary version that produced them. The
//! version is a level rather than a column to filter on because two versions describe the same
//! wallets on the same day, and a reader asking for one must not have to read the other.
//!
//! The writing itself belongs to [`sting_sybil_store`]: what a partition is does not differ
//! between the two things this contour stores, and stating the rule twice would let the two
//! answers drift.

use std::path::{Path, PathBuf};

use arrow::array::RecordBatch;
use sting_sybil_store::Layout;

use crate::Result;

/// Writes one partition, replacing whatever was there.
///
/// The day and the version come from the batch rather than from parameters: they are what a
/// reader will look under, and taking them from anywhere else would let rows be filed under a day
/// they do not claim.
///
/// # Errors
/// [`crate::FeatureError::Store`] when the batch names no single partition or the file cannot be
/// written.
pub fn write_partition(root: &Path, batch: &RecordBatch) -> Result<PathBuf> {
    Ok(sting_sybil_store::write_partition(
        root,
        batch,
        Layout::ByDayAndVersion,
    )?)
}
