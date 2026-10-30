# The comparison, and how to run it again

Two analysers over one dataset, scored by one rule. What is in this directory is the output of a run
on 2026-08-23; what is below is enough to run it again and get the same table, or to substitute
another tool for the participant.

| Piece | What it was |
|---|---|
| dataset | 508 units: 43 vendored real projects, 368 injected defects, 97 hand-written fixtures |
| participant | `acton` 1.1.0 (`9cf4d1f`, released 2026-05-22), the toolchain's own linter |
| participant's image | `ubuntu:24.04`, the release tarball for `x86_64-unknown-linux-gnu` |
| scoring | unit level, the same function for both participants |
| units both could be scored on | 415 of 508 |

## Running it

```sh
# 1. the dataset, one project per unit
cargo run -q -p sting-benchmark -- export --dir bench-run

# 2. the participant, once per unit — in a container, because acton publishes no Windows build
docker run --rm -v "$PWD/bench-run:/run" -v "<acton>:/acton" ubuntu:24.04 bash -c '
  cd /run
  for dir in */; do
    cd "/run/$dir"
    /acton/acton init >/dev/null 2>&1
    /acton/acton check --output-format json --color never > out.json 2>err.txt
  done'

# 3. the numbers, from what the participant printed
cargo run -q -p sting-benchmark -- score --dir bench-run \
  --configuration "acton 1.1.0 as shipped" --report corpus/benchmark/acton-as-shipped.md
```

The scoring step never runs the participant. It reads the file the participant left beside each unit,
which is what makes the run reproducible on a machine with no container runtime, and lets anybody put
their own tool's output under the same name and be scored by the same code.

The release tarball's checksum, as published beside it:
`c2e640eacbb5b6ece1c343cab2ab6d2db74643d0706777aad181ed7e6e1bfc16`.

## The two configurations, and why both

Their `unauthorized-access` rule is a preview and is **off** in a default install. A number taken
with it silently enabled would not be a measurement of the tool anybody runs; a number taken without
it would not say what the rule can do. So both runs are here, named, and neither replaces the other:

| File | Configuration |
|---|---|
| [`acton-as-shipped.md`](./acton-as-shipped.md) | the defaults, nothing added |
| [`acton-preview-rule-enabled.md`](./acton-preview-rule-enabled.md) | `unauthorized-access = "warn"` added to each unit's `Acton.toml` |

## What the numbers are not

They are not a ranking. The dataset's defect classes are the ones this repository's detectors were
written for, and a participant with rules for four of the thirteen is measured on four — the
aggregate row says over how many classes, and a class the participant has no rule for is printed as
"no rule" rather than as a zero. "There is no rule" and "the rule is wrong" are different claims, and
conflating them flatters whoever has fewer rules.

They are not a measurement over the whole dataset either. 93 of 508 units are excluded because the
participant could not compile them, counted by why: 40 fragments rather than whole contracts, 26
importing generated code this corpus does not carry, 15 carrying syntax their compiler refuses, and
12 mutations a type system catches and ours does not. A fifth cause was ours until this run — a
mutated unit published as one merged text, which a resolver reads as two definitions of every symbol
— and the exporter now writes it back out as the files it came from, which moved the comparison from
337 units to 415.
