//! The Python surface over the composition.
//!
//! Bindings and nothing else: every operation here calls the same function the terminal calls,
//! so a notebook and a command line asking the same question get the same answer by
//! construction rather than by two implementations agreeing. Serialisation happens at this
//! boundary and business logic never crosses it.
//!
//! A scenario arrives as the path to the same JSON file the terminal reads, or as the JSON
//! itself. A second schema for notebooks would be a second contract, and the whole point of the
//! scenario contract is that there is one.

use std::path::{Path, PathBuf};

use pyo3::{exceptions::PyValueError, prelude::*};
use sting_karman_app::{
    finals as run_finals, read_scenario, run as run_sweep, sweep as sweep_knob,
    verify as verify_ladder,
};
use sting_karman_scenario::{Knob, ScenarioConfig};

/// Turns any error this workspace produces into one a notebook can read.
///
/// The chain is not walked: every error type below already renders its own cause, so walking
/// would print the same sentence once per level and bury the part a reader needs.
fn reported(error: impl std::fmt::Display) -> PyErr {
    PyValueError::new_err(error.to_string())
}

/// A scenario from a file, or from the JSON a notebook built in memory.
///
/// Both go through the same loader as the terminal, so a scenario a notebook accepts is one the
/// terminal would have accepted too.
fn scenario_of(path: Option<&str>, json: Option<&str>) -> PyResult<(PathBuf, ScenarioConfig)> {
    match (path, json) {
        (Some(path), None) => {
            let path = PathBuf::from(path);
            let scenario = read_scenario(&path).map_err(reported)?;
            Ok((path, scenario))
        }
        (None, Some(json)) => {
            let scenario: ScenarioConfig = serde_json::from_str(json).map_err(reported)?;
            scenario.validate().map_err(reported)?;
            // A scenario built in memory has no file to point at, and the reproduce line says
            // so rather than naming a path nobody could open.
            Ok((PathBuf::from("<scenario given inline>"), scenario))
        }
        _ => Err(PyValueError::new_err(
            "give exactly one of `scenario` (a path) or `scenario_json` (the document itself)",
        )),
    }
}

/// Runs a scenario as a Monte Carlo sweep and returns the summary the terminal prints.
///
/// The run report is written to `report_dir` exactly as the terminal writes it, and the summary
/// names where it went.
#[pyfunction]
#[pyo3(signature = (scenario=None, *, scenario_json=None, runs=100, seed=None, report_dir="out", claim=None))]
fn run(
    scenario: Option<&str>,
    scenario_json: Option<&str>,
    runs: u64,
    seed: Option<u64>,
    report_dir: &str,
    claim: Option<&str>,
) -> PyResult<String> {
    let (path, config) = scenario_of(scenario, scenario_json)?;
    let claim = claim.map(Path::new);
    run_sweep(&path, &config, runs, seed, Path::new(report_dir), claim).map_err(reported)
}

/// Varies one parameter across values and returns the summary the terminal prints.
#[pyfunction]
#[pyo3(signature = (scenario=None, *, vary, values, scenario_json=None, runs=100, seed=None, report_dir="out"))]
#[allow(
    clippy::too_many_arguments,
    clippy::needless_pass_by_value,
    reason = "the keyword arguments Python callers pass by name: the signature is the interface, and PyO3 extracts owned values"
)]
fn sweep(
    scenario: Option<&str>,
    vary: &str,
    values: Vec<u128>,
    scenario_json: Option<&str>,
    runs: u64,
    seed: Option<u64>,
    report_dir: &str,
) -> PyResult<String> {
    let (path, config) = scenario_of(scenario, scenario_json)?;
    let knob: Knob = vary.parse().map_err(reported)?;
    sweep_knob(
        &path,
        &config,
        knob,
        &values,
        runs,
        seed,
        Path::new(report_dir),
    )
    .map_err(reported)
}

/// Drives a scenario through the executable levels of the verification ladder.
#[pyfunction]
#[pyo3(signature = (scenario=None, *, scenario_json=None, seed=None, report_dir="out"))]
fn verify(
    scenario: Option<&str>,
    scenario_json: Option<&str>,
    seed: Option<u64>,
    report_dir: &str,
) -> PyResult<String> {
    let (path, config) = scenario_of(scenario, scenario_json)?;
    verify_ladder(&path, &config, seed, Path::new(report_dir)).map_err(reported)
}

/// The final token reserve of every completed run, in run order.
///
/// The sample a summary is computed from, for a comparison that needs the observations rather
/// than their percentiles.
#[pyfunction]
#[pyo3(signature = (scenario=None, *, scenario_json=None, runs=100, seed=None))]
fn finals(
    scenario: Option<&str>,
    scenario_json: Option<&str>,
    runs: u64,
    seed: Option<u64>,
) -> PyResult<Vec<i128>> {
    let (_, config) = scenario_of(scenario, scenario_json)?;
    run_finals(&config, runs, seed.unwrap_or(config.seed)).map_err(reported)
}

/// The module a notebook imports.
#[pymodule]
fn karman(module: &Bound<'_, PyModule>) -> PyResult<()> {
    module.add_function(wrap_pyfunction!(run, module)?)?;
    module.add_function(wrap_pyfunction!(sweep, module)?)?;
    module.add_function(wrap_pyfunction!(verify, module)?)?;
    module.add_function(wrap_pyfunction!(finals, module)?)?;
    Ok(())
}
