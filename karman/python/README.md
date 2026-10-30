# python

The independent check on the engine, and the notebooks that drive it.

| Directory | What it holds |
|---|---|
| [`reference/`](./reference) | the constant-product scenario written again in cadCAD, and the test that asks whether the two engines are running the same model |

## Why a second implementation

A simulator that agrees with itself has shown nothing. The reference is the same model on a
different engine, written from the same description rather than from this workspace's code, and
the question it answers is whether the numbers are the model's or the implementation's.

The two cannot produce identical runs and are not asked to: one draws from a seeded ChaCha
stream and the other from Python's Mersenne Twister, so a run here is not a run there. What must
agree is the distribution.

## Running it

```bash
make reference
```

Or by hand, once the engine's Python bindings are installed with `make sdk`:

```bash
cd python/reference && python compare.py --runs 1000 --ticks 500 --seed 42
```

It exits non-zero when the two distributions differ, and also when they cannot be told apart
from a sample neither engine would have produced — a test that cannot fail is not evidence, so
the same test is run against a reference with a different population and has to reject it.

## What it is not

It is not a benchmark that decides anything. The timing it prints is a measurement of one host
and gates nothing ([`../docs/PROFILER.md`](../docs/PROFILER.md)); the numbers it produced are
recorded in [`../benches/baseline.toml`](../benches/baseline.toml) beside the method that took
them.
