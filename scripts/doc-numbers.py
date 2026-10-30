#!/usr/bin/env python
"""Derive the numbers the documentation is allowed to state, and check that it states those.

A number typed by hand drifts from the code the day after it is typed, and the reader cannot tell a
current number from a stale one. Seven facts about this tree are computed here — six from the tree
itself, one from a recorded run — and printed, so that a document copies them rather than counting.

A claim about the whole tree is written in one canonical form, and that form is what this checks:
"N crates in the workspace", "N tests green", "N detectors in sting", "N method ids in the
dictionary", "N Tolk files in the corpus", "N% aggregate precision over the default set" and
"N% aggregate precision over every measured detector", each with its Russian mirror. The last two
exist because one aggregate has been quoted as both 96.4% and 82.0% in the same tree, and both were
true of different sets. Any other number — a subset, another project's detector count, a measurement
of one crate — is not a claim about this tree and is not this script's business.

The precision `sting explain` prints per detector is checked against `corpus/baseline.json` the
same way, because a figure typed into that prose drifts the first time the corpus grows.

`--update` re-measures the facts that need a run and rewrites `scripts/facts.json`.
"""

from __future__ import annotations

import json
import os
import re
import subprocess
import sys

import tomllib

from repository import REPO, tracked

if hasattr(sys.stdout, "reconfigure"):
    sys.stdout.reconfigure(encoding="utf-8")

FACTS = os.path.join(REPO, "scripts", "facts.json")
BASELINE = os.path.join(REPO, "benches", "baseline.toml")

# A number in prose is written with thin spaces between the thousands, so the pattern accepts them
# and the comparison strips them.
NUMBER = r"(\d[\d   ]*)"

CLAIMS = {
    "crates": re.compile(
        NUMBER + r"\s+(?:crates in the workspace|крейтов в workspace)", re.IGNORECASE),
    "tests": re.compile(
        NUMBER + r"\s+(?:tests green|тестов зелёных)", re.IGNORECASE),
    "detectors": re.compile(
        NUMBER + r"\s+(?:detectors in sting|детекторов в sting)", re.IGNORECASE),
    "method_ids": re.compile(
        NUMBER + r"\s+(?:method ids in the dictionary|method id в словаре)", re.IGNORECASE),
    "corpus_files": re.compile(
        NUMBER + r"\s+(?:Tolk files in the corpus|файлов Tolk в корпусе)", re.IGNORECASE),
    "precision_default": re.compile(
        r"(\d+\.\d)%\s+(?:aggregate precision over the default set"
        r"|агрегатной precision по набору по умолчанию)", re.IGNORECASE),
    "precision_measured": re.compile(
        r"(\d+\.\d)%\s+(?:aggregate precision over every measured detector"
        r"|агрегатной precision по всем измеренным детекторам)", re.IGNORECASE),
}


# `sting explain <ID>` prints a precision per detector, and a figure typed into that prose drifts from
# `corpus/baseline.json` the first time the corpus grows. The entries are keyed by detector code, so
# each claim is checked against the detector it belongs to.
EXPLAIN = os.path.join(REPO, "crates", "sting-detectors", "src", "explain.rs")
EXPLAIN_ENTRY = re.compile(r'^\s{8}"([A-Z][A-Z0-9]*)",$', re.M)
EXPLAIN_PERCENT = re.compile(r"(\d+\.\d)%\s+(?:precision|on (?:our|sting's) own corpus)")


def detector_precisions() -> dict[str, float]:
    """Precision per detector, from the same baseline `sting bench` writes."""
    path = os.path.join(REPO, "corpus", "baseline.json")
    with open(path, encoding="utf-8") as handle:
        measured = json.load(handle)["detectors"]
    out = {}
    for code, entry in measured.items():
        hits, misses = entry["true_positives"], entry["false_positives"]
        if hits + misses:
            out[code] = round(100 * hits / (hits + misses), 1)
    return out


def stale_explanations() -> list[str]:
    """Percentages in `sting explain`'s prose that disagree with the baseline."""
    measured = detector_precisions()
    with open(EXPLAIN, encoding="utf-8") as handle:
        text = handle.read()
    marks = [(m.group(1), m.start()) for m in EXPLAIN_ENTRY.finditer(text)]
    stale = []
    for index, (code, start) in enumerate(marks):
        end = marks[index + 1][1] if index + 1 < len(marks) else len(text)
        for match in EXPLAIN_PERCENT.finditer(text[start:end]):
            claimed = float(match.group(1))
            if code in measured and abs(claimed - measured[code]) > 0.05:
                line = text.count("\n", 0, start + match.start()) + 1
                stale.append(
                    f"crates/sting-detectors/src/explain.rs:{line}: "
                    f"{code} claimed {claimed}%, measured {measured[code]}%"
                )
    return stale


def tracked_docs() -> list[str]:
    # The tree also carries the documents of projects that are not merged yet; their numbers are
    # theirs, and this checks this platform's own.
    return [
        path for path in tracked("docs/*.md", "docs/**/*.md", "*.md")
        if path.endswith(".md") and (path.startswith("docs/") or "/" not in path)
    ]


def count_workspace_members() -> int:
    with open(os.path.join(REPO, "Cargo.toml"), encoding="utf-8") as handle:
        members = re.search(r"members = \[(.*?)\]", handle.read(), re.S)
    return len([entry for entry in members.group(1).split(",") if entry.strip()]) if members else 0


def count_detectors() -> int:
    """One `impl Detector for` per detector — the trait is what the pipeline dispatches on."""
    out = subprocess.run(
        ["git", "grep", "-c", "impl Detector for", "--", "crates/sting-detectors/src"],
        cwd=REPO, capture_output=True, text=True,
    )
    return sum(int(line.rsplit(":", 1)[1]) for line in out.stdout.splitlines() if ":" in line)


def count_corpus_files() -> int:
    return sum(1 for path in tracked("corpus") if path.endswith(".tolk"))


def count_method_ids() -> int:
    path = os.path.join(REPO, "corpus-abi", "method-dictionary.json")
    with open(path, encoding="utf-8") as handle:
        return len(json.load(handle).get("entries", {}))


def precision() -> tuple[float, float]:
    """Aggregate precision over the default set, and over every detector the baseline measures.

    Two detectors are off by default with the reason stated in their own source, and leaving them in
    or out moves the aggregate by fourteen points — so both numbers are computed and neither is
    allowed to stand alone.
    """
    path = os.path.join(REPO, "corpus", "baseline.json")
    with open(path, encoding="utf-8") as handle:
        measured = json.load(handle)["detectors"]

    def aggregate(entries: dict[str, dict[str, int]]) -> float:
        hits = sum(entry["true_positives"] for entry in entries.values())
        misses = sum(entry["false_positives"] for entry in entries.values())
        return round(100 * hits / (hits + misses), 1) if hits + misses else 0.0

    off_by_default = {"AC", "LEP"}
    default_set = {name: entry for name, entry in measured.items() if name not in off_by_default}
    return aggregate(default_set), aggregate(measured)


def measure_tests() -> int:
    """Tests the suite actually runs, which no static count can stand in for.

    A failing run is refused rather than counted: "N tests green" is a claim about a suite that
    passed, and recording the number a red run reached would make the claim false while it still
    reads as measured.
    """
    out = subprocess.run(
        ["cargo", "nextest", "run", "--workspace"],
        cwd=REPO, capture_output=True, text=True,
    )
    if out.returncode != 0:
        raise SystemExit("the suite is red; `N tests green` is not a number this tree can state")
    match = re.search(r"(\d+) tests run", out.stderr + out.stdout)
    return int(match.group(1)) if match else 0


def facts(update: bool) -> dict[str, int]:
    stored: dict[str, int] = {}
    if os.path.exists(FACTS):
        with open(FACTS, encoding="utf-8") as handle:
            stored = json.load(handle)
    default_precision, measured_precision = precision()
    computed = {
        "crates": count_workspace_members(),
        "precision_default": default_precision,
        "precision_measured": measured_precision,
        "detectors": count_detectors(),
        "corpus_files": count_corpus_files(),
        "method_ids": count_method_ids(),
        "tests": measure_tests() if update else stored.get("tests", 0),
    }
    if update:
        with open(FACTS, "w", encoding="utf-8") as handle:
            json.dump(computed, handle, indent=2, sort_keys=True)
            handle.write("\n")
    return computed


def stale_claims(known: dict[str, int]) -> list[str]:
    """Every canonical claim in the documents that disagrees with the measured fact behind it."""
    stale: list[str] = []
    for path in tracked_docs():
        with open(os.path.join(REPO, path), encoding="utf-8") as handle:
            lines = handle.read().splitlines()
        for number, line in enumerate(lines, 1):
            for name, pattern in CLAIMS.items():
                for match in pattern.finditer(line):
                    raw = match.group(1)
                    claimed: float | int = (
                        float(raw) if "." in raw else int(re.sub(r"[^\d]", "", raw))
                    )
                    if claimed != known[name]:
                        stale.append(
                            f"{path}:{number}: {name} claimed {claimed}, measured {known[name]}"
                        )
    return stale


# What a baseline entry promises about the fact it names. A count that may grow is a floor and
# not a mirror: rewriting it on every new test would make the file a copy of the tree rather than
# the record of what a gate was passed with.
DIRECTIONS = {
    "at_least": lambda recorded, measured: measured >= recorded,
    "at_most": lambda recorded, measured: measured <= recorded,
    "exact": lambda recorded, measured: measured == recorded,
}


def stale_baselines(known: dict[str, int]) -> list[str]:
    """Every recorded baseline the tree no longer satisfies.

    A baseline is the number a gate was passed with, and it stays that number. What is checked is
    the promise beside it: a floor the tree must not fall below, a ceiling it must not exceed, or
    an exact value. A recorded number nobody checks is a recorded number nobody can trust.
    """
    if not os.path.exists(BASELINE):
        return [
            f"{os.path.relpath(BASELINE, REPO)}: missing, and ROADMAP.md §14 says every gate "
            f"number lives there"
        ]
    with open(BASELINE, "rb") as handle:
        recorded = tomllib.load(handle)
    stale: list[str] = []
    for milestone, entries in recorded.items():
        if not isinstance(entries, dict):
            continue
        for name, entry in entries.items():
            if not isinstance(entry, dict) or "fact" not in entry:
                continue
            where = f"[{milestone}.{name}]"
            fact = entry["fact"]
            if fact not in known:
                stale.append(f"{where}: names fact {fact!r}, which nothing derives")
                continue
            holds = DIRECTIONS.get(entry.get("direction", ""))
            if holds is None:
                stale.append(
                    f"{where}: names fact {fact!r} without a direction "
                    f"({', '.join(sorted(DIRECTIONS))})"
                )
            elif not holds(entry["value"], known[fact]):
                stale.append(
                    f"{where}: recorded {entry['value']} as {entry['direction']}, "
                    f"measured {known[fact]}"
                )
    return stale


def main() -> int:
    update = "--update" in sys.argv
    known = facts(update)

    print("facts")
    for name, value in sorted(known.items()):
        print(f"  {name.ljust(13)} {value}")

    stale = stale_claims(known) + stale_explanations() + stale_baselines(known)
    print(f"\nclaims that disagree with the tree: {len(stale)}")
    for entry in stale[:20]:
        print(f"  {entry}")
    if len(stale) > 20:
        print(f"  … and {len(stale) - 20} more")
    return 1 if stale else 0


if __name__ == "__main__":
    sys.exit(main())
