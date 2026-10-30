#!/usr/bin/env python
"""One lint table and one HTTP client, each checked rather than trusted.

`unsafe_code = "forbid"` cannot be relaxed by an inner attribute, which is the point of `forbid` and
also its cost: the two crates that hold the FFI boundary cannot inherit the workspace table at all,
because Cargo refuses to mix `[lints] workspace = true` with a local override. They spell the table
out instead, and a hand-copied table is a table that drifts.

So the copies are checked rather than trusted. Every crate either inherits, or reproduces the
workspace's lints exactly apart from one key it is named here as allowed to change.

The second rule is the same shape: reaching a public service belongs to `sting-chain`, and a crate
naming an HTTP client of its own is a second retry policy waiting to disagree with the first. Chain
access existed six times in Rust before that crate; the count is what this checks.

The third rule is the layering: a crate may depend on the layer below it and never on the layer
above. Dependencies point down and only down, which is what keeps a foundation crate from being
unbuildable without the thing built on top of it, and the manifests are where an upward edge would
first appear.

What this does not do: it does not judge which lints the workspace sets, and it does not read code
for what a client is used for. A manifest naming a transport is the signal, because a dependency
that is declared is a dependency that can be used. It says nothing about crates outside a declared
domain, because a layer nobody has stated is a layer nobody can check.
"""

from __future__ import annotations

import os
import sys
import tomllib

from repository import REPO

# The exception and its price, per crate: the lint it is allowed to change, and to what.
# An entry here is a decision recorded in docs/ARCHITECTURE.md §1.3; a crate that is not here
# inherits, and nothing else is a third state.
EXCEPTIONS: dict[str, dict[str, str]] = {
    "sting-emulator-sys": {"unsafe_code": "allow"},
    "sting-emulator": {"unsafe_code": "allow"},
    # A counting global allocator: a run's allocation pressure and live peak, on every platform,
    # and a global allocator implements an `unsafe` trait.
    "sting-karman-cli": {"unsafe_code": "allow"},
}

# Every crate that may name an HTTP transport, and the one reason each may.
TRANSPORT_ALLOWED = {
    "sting-chain": "the one client every part of this platform reaches a service through",
    # A load generator, and nothing else — which is why it is a crate of its own rather than a
    # module of the CLI. What it talks to is this platform's own service over a loopback, and a
    # paced, retrying chain client pointed at it would measure the pacer rather than the service.
    "sting-load": "the load generator, which measures this platform's own surfaces",
}

# What counts as naming a transport. Not a list of every HTTP crate that exists — a list of the ones
# this tree has actually used, which is what a check can be honest about.
TRANSPORTS = ("ureq", "reqwest", "hyper", "isahc", "curl", "attohttpc")

# Workspaces waiting for the merge, counted rather than exempted: their copies are real and their
# number only ever goes down.
UNMERGED = ("sybil/crates", "karman/crates")


def load(path: str) -> dict:
    with open(path, "rb") as handle:
        return tomllib.load(handle)


def expected(workspace: dict, allowed: dict[str, str]) -> dict[str, dict[str, object]]:
    """The workspace's lint tables with this crate's named exception applied."""
    tables = {group: dict(table) for group, table in workspace.items()}
    for lint, level in allowed.items():
        for table in tables.values():
            if lint in table:
                table[lint] = level
    return tables


def offenders() -> list[str]:
    workspace = load(os.path.join(REPO, "Cargo.toml"))["workspace"]["lints"]
    found = []
    for name in sorted(os.listdir(os.path.join(REPO, "crates"))):
        manifest = os.path.join(REPO, "crates", name, "Cargo.toml")
        if not os.path.isfile(manifest):
            continue
        lints = load(manifest).get("lints", {})
        if name not in EXCEPTIONS:
            if lints.get("workspace") is not True:
                found.append(f"{name}: does not inherit [workspace.lints]")
            continue
        if lints.get("workspace") is True:
            found.append(f"{name}: inherits, and its exception is then unreachable")
            continue
        want = expected(workspace, EXCEPTIONS[name])
        for group, table in want.items():
            have = lints.get(group, {})
            for lint, level in table.items():
                if have.get(lint) != level:
                    found.append(f"{name}: {group}.{lint} is {have.get(lint)!r}, workspace says {level!r}")
            for lint in have.keys() - table.keys():
                found.append(f"{name}: {group}.{lint} is set here and nowhere in the workspace")
    return found


def transports_in(root: str) -> list[str]:
    """Every crate under `root` naming an HTTP transport it is not allowed to name."""
    found = []
    directory = os.path.join(REPO, root)
    if not os.path.isdir(directory):
        return found
    for name in sorted(os.listdir(directory)):
        manifest = os.path.join(directory, name, "Cargo.toml")
        if not os.path.isfile(manifest):
            continue
        crate = load(manifest)
        declared = set()
        for table in ("dependencies", "dev-dependencies", "build-dependencies"):
            declared |= set(crate.get(table, {}))
        named = sorted(declared & set(TRANSPORTS))
        if named and crate.get("package", {}).get("name") not in TRANSPORT_ALLOWED:
            found.append(f"{root}/{name}: names {', '.join(named)}")
    return found


# The layers of each domain, lowest first. A crate may name anything below it and nothing above.
# Only domains whose layering is a decision are listed: a crate outside these is not checked here,
# because a layer nobody has stated is a layer nobody can check.
# The five layers `docs/ARCHITECTURE.md` §1.1 states, by the name that document gives each. Read
# by `check-capabilities.py`, so that a capability claiming a layer is claiming the layer its own
# crate sits on rather than one its author liked the sound of.
CRATE_LAYERS: dict[str, str] = {
    "sting-core": "1 · foundation",
    "sting-corpus": "1 · foundation",
    "sting-emulator": "1 · foundation",
    "sting-emulator-sys": "1 · foundation",
    "sting-tolkc": "1 · foundation",
    "sting-fift": "1 · foundation",
    "sting-chain": "1 · foundation",
    "sting-address": "1 · foundation",
    "sting-tep": "1 · foundation",
    "sting-ast": "2 · parsing",
    "sting-abi-core": "2 · parsing",
    "sting-karman-tvm": "2 · parsing",
    "sting-semantic": "3 · analysis",
    "sting-dataflow": "3 · analysis",
    "sting-detectors": "4 · subject",
    "sting-scamcheck": "4 · subject",
    "sting-cli": "5 · products",
    "sting-abi-cli": "5 · products",
    "sting-abi-service": "5 · products",
}


LAYERS: dict[str, list[list[str]]] = {
    "sybil": [
        ["sting-sybil-types"],
        ["sting-sybil-store"],
        ["sting-sybil-ingestion"],
        ["sting-sybil-graph"],
        ["sting-sybil-features", "sting-sybil-scorer"],
        ["sting-sybil-cluster", "sting-sybil-api"],
        ["sting-sybil-python"],
    ],
    "karman": [
        ["sting-karman-types"],
        ["sting-karman-core"],
        ["sting-karman-scenario"],
        [
            "sting-karman-mech-curve",
            "sting-karman-mech-lending",
            "sting-karman-mech-perp",
            "sting-karman-mech-showcase",
            "sting-karman-mech-vouch",
            "sting-karman-data",
            "sting-karman-report",
            "sting-karman-tvm",
        ],
        ["sting-karman-agents"],
        ["sting-karman-verify"],
        ["sting-karman-app"],
        ["sting-karman-cli", "sting-karman-python"],
    ],
}


# A test may reach for something built above it where the thing it reaches for is a harness: the
# alternative is a measurement proven against a series written to satisfy it. Named with the
# reason, one line per edge, the same way a lint exception is.
TEST_EDGES_ALLOWED: dict[tuple[str, str], str] = {
    ("sting-karman-agents", "sting-karman-verify"): (
        "the verification harnesses, so a stylised-fact measurement is proven against a real "
        "mechanism and a real population"
    ),
}


def layered_edges() -> list[str]:
    """Every dependency that points sideways or upwards through the layers.

    A library dependency may only point down: a crate that names its own layer is a crate two
    things at the same level now know about each other, which is how a level stops being one. A
    test dependency may point sideways — a test is allowed to drive a sibling — and upwards only
    where it is named above.
    """
    level = {
        crate: depth
        for layers in LAYERS.values()
        for depth, names in enumerate(layers)
        for crate in names
    }
    found = []
    for name in sorted(level):
        manifest = os.path.join(REPO, "crates", name, "Cargo.toml")
        if not os.path.isfile(manifest):
            found.append(f"{name}: named as a layer and not in the tree")
            continue
        crate = load(manifest)
        for table in ("dependencies", "build-dependencies", "dev-dependencies"):
            for dependency in sorted(crate.get(table, {})):
                above = level.get(dependency)
                if above is None or dependency == name:
                    continue
                if table == "dev-dependencies":
                    if above > level[name] and (name, dependency) not in TEST_EDGES_ALLOWED:
                        found.append(f"{name} -> {dependency} (the layer above, in {table})")
                    continue
                if above >= level[name]:
                    relation = "its own layer" if above == level[name] else "the layer above"
                    found.append(f"{name} -> {dependency} ({relation}, in {table})")
    return found


def main() -> int:
    found = offenders()
    print(f"crates out of step with [workspace.lints]  {len(found):>4}  (target 0)")
    for entry in found:
        print(f"    {entry}")

    transports = transports_in("crates")
    print(f"crates naming an HTTP client of their own  {len(transports):>4}  (target 0)")
    for entry in transports:
        print(f"    {entry}")

    unmerged = [entry for root in UNMERGED for entry in transports_in(root)]
    print(f"the same, in workspaces awaiting the merge  {len(unmerged):>4}  (target 0)  held")
    for entry in unmerged:
        print(f"    {entry}")

    upward = layered_edges()
    print(f"dependencies that do not point down       {len(upward):>4}  (target 0)")
    for entry in upward:
        print(f"    {entry}")

    return 1 if found or transports or upward else 0


if __name__ == "__main__":
    sys.exit(main())
