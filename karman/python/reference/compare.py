"""Are the two engines running the same model?

Equivalence, not speed. The engine under test and the cadCAD reference draw from different
random streams by construction, so no run of one is a run of the other and asking for identical
numbers would be asking for the wrong thing. What can be asked is whether the two sets of final
reserves came from the same distribution, and that is a two-sample Kolmogorov-Smirnov test.

The test is written out rather than imported. It is fifteen lines, it is the whole basis of the
claim this file makes, and a reader who wants to check the claim should not have to trust a
dependency to do it. The critical value is the standard asymptotic one: reject when the largest
gap between the two empirical distributions exceeds `c(alpha) * sqrt((n + m) / (n * m))`, with
`c(0.05) = 1.358`.

Timings are printed alongside and are measurements, never gates: a number that varies by host
cannot decide anything (docs/PROFILER.md).
"""

from __future__ import annotations

import argparse
import math
import time
from pathlib import Path

from cpamm_cadcad import OPENING_QUOTE, OPENING_TOKEN, run_one

ROOT = Path(__file__).resolve().parents[2]
SCENARIO = "testdata/scenarios/cpamm-smoke.json"
# Two-sided, five percent. The one place a threshold appears, and it is the textbook value
# rather than one chosen after seeing the samples.
CRITICAL = 1.358


def largest_gap(left: list[int], right: list[int]) -> float:
    """The Kolmogorov-Smirnov statistic: the widest the two empirical distributions ever get."""
    left, right = sorted(left), sorted(right)
    n, m = len(left), len(right)
    i = j = 0
    gap = 0.0
    while i < n and j < m:
        if left[i] <= right[j]:
            i += 1
        else:
            j += 1
        gap = max(gap, abs(i / n - j / m))
    return gap


def indistinguishable(left: list[int], right: list[int]) -> tuple[bool, float, float]:
    """Whether the samples could have come from one distribution, at the registered level."""
    n, m = len(left), len(right)
    threshold = CRITICAL * math.sqrt((n + m) / (n * m))
    gap = largest_gap(left, right)
    return gap <= threshold, gap, threshold


def karman_finals(runs: int, seed: int) -> tuple[list[int], float]:
    """The engine's final token reserves, one per run, and how long it took.

    Through the bindings rather than the terminal, because the terminal publishes a summary and
    a distribution test needs the observations behind it. The bindings and the terminal are one
    implementation, so this is the sample either would have produced.
    """
    import karman

    started = time.perf_counter()
    finals = karman.finals(str(ROOT / SCENARIO), runs=runs, seed=seed)
    return finals, time.perf_counter() - started


def reference_finals(
    runs: int, seed: int, ticks: int, traders: int | None = None
) -> tuple[list[int], float]:
    """The reference's final token reserves, one per run, and how long it took."""
    started = time.perf_counter()
    finals = [run_one(seed * 1_000_003 + run, ticks, traders)[0] for run in range(runs)]
    return finals, time.perf_counter() - started


def percentiles(values: list[int]) -> dict[str, int]:
    """Nearest-rank percentiles, the same rule the report uses, so the two are comparable."""
    ordered = sorted(values)
    def at(num: int, den: int) -> int:
        index = max(1, -(-len(ordered) * num // den)) - 1
        return ordered[min(index, len(ordered) - 1)]
    return {
        "min": ordered[0], "p05": at(5, 100), "p25": at(25, 100), "median": at(50, 100),
        "p75": at(75, 100), "p95": at(95, 100), "max": ordered[-1],
    }


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--runs", type=int, default=1000)
    parser.add_argument("--ticks", type=int, default=500)
    parser.add_argument("--seed", type=int, default=42)
    args = parser.parse_args()

    engine, engine_seconds = karman_finals(args.runs, args.seed)
    reference, reference_seconds = reference_finals(args.runs, args.seed, args.ticks)
    engine_summary, reference_summary = percentiles(engine), percentiles(reference)

    print(f"opening reserves: {OPENING_TOKEN} token / {OPENING_QUOTE} quote")
    print(f"runs: {args.runs} · ticks: {args.ticks} · seed: {args.seed}\n")
    print(f"{'percentile':>10} {'engine':>14} {'cadCAD reference':>18} {'gap (bps)':>10}")
    for name in ("min", "p05", "p25", "median", "p75", "p95", "max"):
        ours, theirs = engine_summary[name], reference_summary[name]
        print(f"{name:>10} {ours:>14} {theirs:>18} {(theirs - ours) * 10_000 // ours:>10}")

    same, gap, threshold = indistinguishable(engine, reference)
    verdict = "INDISTINGUISHABLE" if same else "DIFFERENT"
    print(f"\nengine against reference: gap {gap:.4f} against threshold {threshold:.4f} "
          f"-> {verdict}")

    # A test that cannot fail proves nothing, so the same test runs against a sample neither
    # engine would have produced: the reference with a different population.
    altered, _ = reference_finals(args.runs, args.seed, args.ticks, traders=2)
    blind, gap_bad, threshold_bad = indistinguishable(engine, altered)
    told_apart = "indistinguishable — THE TEST IS BLIND" if blind else "different, as it must be"
    print(f"engine against a reference with a smaller population: gap {gap_bad:.4f} against "
          f"threshold {threshold_bad:.4f} -> {told_apart}")

    corridor_engine = engine_summary["p95"] - engine_summary["p05"]
    corridor_reference = reference_summary["p95"] - reference_summary["p05"]
    apart = abs(corridor_engine - corridor_reference) * 10_000 // corridor_engine
    print(f"corridor width: engine {corridor_engine}, reference {corridor_reference}, "
          f"apart by {apart} bps")

    print(f"\nwall clock: engine {engine_seconds:.2f}s, reference {reference_seconds:.2f}s "
          f"({reference_seconds / engine_seconds:.0f}x). A measurement, never a gate.")
    return 0 if same and not blind else 1


if __name__ == "__main__":
    raise SystemExit(main())
