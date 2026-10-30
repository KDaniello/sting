//! Handing a feature table to Python without serialising it.
//!
//! The columnar layout is the same in both runtimes, so a table built here is read there through
//! the buffer protocol rather than encoded, sent and decoded. What that buys is not neatness: at
//! the width of this vocabulary and the size of a real batch, serialising twice per handoff is
//! most of the time the handoff costs.
//!
//! # What this crate does not do
//!
//! It reaches no chain and no storage: there is no runtime here, and the two ways features get to
//! Python — this one and the partition files — are alternatives rather than a chain. It computes
//! nothing of its own; every number it hands over comes from the extraction crate.

use arrow::array::{Float64Array, RecordBatch, StringArray};
use arrow::datatypes::{DataType, Field, Schema};
use pyo3::prelude::*;
use pyo3_arrow::PyRecordBatch;
use std::sync::Arc;

/// Two rows and two columns, to prove the handoff itself works.
///
/// It exists because the first thing to establish about a bridge is that a table crosses it at
/// all, separately from whether the table is the right one.
#[pyfunction]
fn demo_feature_batch() -> PyResult<PyRecordBatch> {
    let schema = Arc::new(Schema::new(vec![
        Field::new("owner", DataType::Utf8, false),
        Field::new("risk_stub", DataType::Float64, false),
    ]));
    let owners = StringArray::from(vec!["EQC...aaa", "EQC...bbb"]);
    let risk = Float64Array::from(vec![0.12_f64, 0.87]);
    let batch = RecordBatch::try_new(schema, vec![Arc::new(owners), Arc::new(risk)])
        .map_err(|err| pyo3::exceptions::PyValueError::new_err(err.to_string()))?;
    Ok(PyRecordBatch::new(batch))
}

/// The whole feature table, from the real extraction over the shared synthetic world.
///
/// The same scenario the Rust snapshot is taken over, so the two sides are looking at one world
/// rather than two invented ones. Not live data: there is no runtime in this crate, and the
/// partition files are the path for that.
#[pyfunction]
fn feature_batch(feature_version: String) -> PyResult<PyRecordBatch> {
    let (graphs, txs, messages) = sting_sybil_features::fixtures::synthetic_owner_scenario();
    let snapshot_ts = txs.iter().map(|tx| tx.now).max().unwrap_or(0);
    let cfg = sting_sybil_features::FeatureExtractionConfig {
        feature_version,
        snapshot_ts,
        burst_factor: 0.5,
        topology: sting_sybil_features::TopologyThresholds {
            star_fan_threshold: 20,
            tree_fan_threshold: 20,
        },
    };
    let rows =
        sting_sybil_features::extract_features_for_range(&txs, &messages, &[], &graphs, &cfg);
    let batch = sting_sybil_features::build_record_batch(&rows)
        .map_err(|err| pyo3::exceptions::PyValueError::new_err(err.to_string()))?;
    Ok(PyRecordBatch::new(batch))
}

/// The module Python imports.
#[pymodule]
fn sybil_native(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(demo_feature_batch, m)?)?;
    m.add_function(wrap_pyfunction!(feature_batch, m)?)?;
    Ok(())
}
