# karman — Python SDK

Bindings over the same scenario contract the CLI speaks: a scenario file means the same thing on
every surface, and a `(scenario, seed)` pair reproduces the same run here as anywhere else.

The SDK is a thin PyO3 layer over [`sting-karman-app`](../../crates/sting-karman-app) — serialisation at the
boundary, no business logic of its own. Both surfaces call the same composition, so a notebook
and a command line asking the same question get the same answer by construction rather than by
two implementations agreeing today. [`tests/test_same_numbers.py`](./tests/test_same_numbers.py)
is what says so, across all four operations, and
[`notebooks/same-numbers.ipynb`](./notebooks/same-numbers.ipynb) shows it in the form a user
meets it in.

## Build and install

```bash
make sdk        # builds the wheel, installs it, and runs the equivalence tests
```

Or by hand, from this directory:

```bash
maturin build --release
pip install --force-reinstall ../../target/wheels/karman-*.whl
```

## Using it

```python
import json, karman

# One scenario, many independent runs. Returns the summary the terminal prints.
summary = json.loads(karman.run("testdata/scenarios/cpamm-smoke.json", runs=1000, seed=42))

# Graded against a claim registered before the sweep ran. Without one, no verdict.
karman.run(
    "testdata/scenarios/cpamm-smoke.json", runs=1000, seed=42,
    claim="findings/2026-08-19-cpamm-smoke-symmetric-flow-leaves-no-drift.json",
)

# One parameter across values, every point at the same seed.
karman.sweep("testdata/scenarios/cpamm-smoke.json", vary="group-count:0", values=[1, 5, 20])

# What this mechanism can claim on the verification ladder, and what it cannot.
karman.verify("testdata/scenarios/cpamm-mixed.json")
```

A scenario built in a notebook rather than read from a file goes through the same schema and the
same validation:

```python
karman.run(scenario_json=json.dumps(document), runs=100, seed=42)
```

Paths are resolved relative to the working directory, and reports are written where
`report_dir` says, exactly as the terminal writes them.
