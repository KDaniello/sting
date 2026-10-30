//! Where a membership partition goes: one level, the day.
//!
//! No version level, and that is a fact about the answer rather than an omission — an address is
//! in one cluster on a given day, so there is no second answer to keep apart from the first.
//!
//! The writing itself belongs to [`sting_sybil_store`], which is also what makes the replacement
//! rule the same here as it is for features: this partition is written twice, once by the
//! structural half and once by the behavioural half filling in the columns it owns.

use std::path::{Path, PathBuf};

use arrow::array::RecordBatch;
use sting_sybil_store::Layout;

use crate::Result;

/// Writes one partition, replacing whatever was there.
///
/// # Errors
/// [`crate::ClusterError::Store`] when the batch names no single partition or the file cannot be
/// written.
pub fn write_partition(root: &Path, batch: &RecordBatch) -> Result<PathBuf> {
    Ok(sting_sybil_store::write_partition(
        root,
        batch,
        Layout::ByDay,
    )?)
}
