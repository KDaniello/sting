"""The bindings and the terminal are one implementation, and this is what says so.

The SDK exists so a notebook can ask what the command line asks. If the two ever answered
differently there would be no way to tell which one a published number came from, so the claim
is checked rather than asserted: both surfaces drive the same scenario at the same seed, and
their summaries have to agree field for field.
"""

from __future__ import annotations

import json
import subprocess
import sys
from pathlib import Path

import karman
import pytest

ROOT = Path(__file__).resolve().parents[3]
BINARY = ROOT / "target" / "release" / "karman.exe"
if not BINARY.exists():
    BINARY = ROOT / "target" / "release" / "karman"
SCENARIO = "testdata/scenarios/cpamm-smoke.json"
MIXED = "testdata/scenarios/cpamm-mixed.json"
CLAIM = "findings/2026-08-19-cpamm-smoke-symmetric-flow-leaves-no-drift.json"


def terminal(*args: str) -> dict:
    """What the command line says, as the terminal itself would print it."""
    if not BINARY.exists():
        pytest.skip(f"{BINARY} is not built; run `cargo build --release` first")
    done = subprocess.run(
        [str(BINARY), *args], cwd=ROOT, capture_output=True, text=True, check=True
    )
    return json.loads(done.stdout.strip().splitlines()[-1])


def test_a_sweep_answers_the_same_on_both_surfaces() -> None:
    from_python = json.loads(
        karman.run(SCENARIO, runs=100, seed=42, report_dir="out/sdk")
    )
    from_terminal = terminal(
        "run", "--scenario", SCENARIO, "--runs", "100", "--seed", "42",
        "--report-dir", "out/cli",
    )
    # The report path differs because each was asked to write somewhere different; every
    # measured number must not.
    from_python.pop("report")
    from_terminal.pop("report")
    assert from_python == from_terminal


def test_a_graded_sweep_reaches_the_same_verdict_on_both_surfaces() -> None:
    from_python = json.loads(
        karman.run(SCENARIO, runs=1000, seed=42, report_dir="out/sdk", claim=CLAIM)
    )
    from_terminal = terminal(
        "run", "--scenario", SCENARIO, "--runs", "1000", "--seed", "42",
        "--claim", CLAIM, "--report-dir", "out/cli",
    )
    assert from_python["verdict"] == from_terminal["verdict"] == "confirmed"


def test_a_parameter_sweep_answers_the_same_on_both_surfaces() -> None:
    from_python = json.loads(
        karman.sweep(
            SCENARIO, vary="group-count:0", values=[1, 5], runs=50, seed=42,
            report_dir="out/sdk",
        )
    )
    from_terminal = terminal(
        "sweep", "--scenario", SCENARIO, "--vary", "group-count:0", "--values", "1,5",
        "--runs", "50", "--seed", "42", "--report-dir", "out/cli",
    )
    from_python.pop("report")
    from_terminal.pop("report")
    assert from_python == from_terminal


def test_the_ladder_reports_the_same_on_both_surfaces() -> None:
    from_python = json.loads(karman.verify(MIXED, seed=42, report_dir="out/sdk"))
    from_terminal = terminal(
        "verify", "--scenario", MIXED, "--seed", "42", "--report-dir", "out/cli"
    )
    from_python.pop("report")
    from_terminal.pop("report")
    assert from_python == from_terminal


def test_a_scenario_given_inline_runs_like_one_read_from_a_file() -> None:
    """A notebook that builds a scenario in memory speaks the same contract as a file."""
    document = json.loads((ROOT / SCENARIO).read_text(encoding="utf-8"))
    inline = json.loads(
        karman.run(scenario_json=json.dumps(document), runs=100, seed=42, report_dir="out/sdk")
    )
    from_file = json.loads(karman.run(SCENARIO, runs=100, seed=42, report_dir="out/sdk"))
    assert inline["token_reserve"] == from_file["token_reserve"]


def test_the_sample_behind_a_summary_is_the_summary() -> None:
    """The per-run finals and the percentiles the report publishes describe one sweep."""
    finals = karman.finals(SCENARIO, runs=100, seed=42)
    summary = json.loads(karman.run(SCENARIO, runs=100, seed=42, report_dir="out/sdk"))
    assert len(finals) == 100
    assert str(min(finals)) == summary["token_reserve"]["min"]
    assert str(max(finals)) == summary["token_reserve"]["max"]


def test_the_sample_is_a_function_of_the_seed_alone() -> None:
    assert karman.finals(SCENARIO, runs=50, seed=42) == karman.finals(SCENARIO, runs=50, seed=42)
    assert karman.finals(SCENARIO, runs=50, seed=42) != karman.finals(SCENARIO, runs=50, seed=43)


def test_an_unrunnable_scenario_is_refused_with_the_field_that_broke() -> None:
    document = json.loads((ROOT / SCENARIO).read_text(encoding="utf-8"))
    document["n_ticks"] = 0
    with pytest.raises(ValueError, match="n_ticks"):
        karman.run(scenario_json=json.dumps(document), runs=1, seed=42, report_dir="out/sdk")


def test_asking_for_neither_or_both_scenarios_is_refused() -> None:
    with pytest.raises(ValueError, match="exactly one"):
        karman.run()
    with pytest.raises(ValueError, match="exactly one"):
        karman.run(SCENARIO, scenario_json="{}")


if __name__ == "__main__":
    sys.exit(pytest.main([__file__, "-q"]))
