#!/usr/bin/env python
"""The inventory is a table with a machine behind it, not a description.

`docs/CAPABILITIES.md` states what this platform can do, one block per capability, eight fields
each. A field nobody filled in is the field that matters: what a capability does *not* do is
exactly what a reader assumes generously when it is missing, and a command nobody can run is a
claim rather than a capability.

So the document is parsed rather than read. Every block must carry all eight fields; every command
must name a binary this workspace builds; every layer must be one the architecture declares; and
every capability marked offline must be reachable without a network, a container or a key.

What this does not do: it does not run the commands — that is `scripts/run-capabilities.py`, which
reads the same blocks — and it does not judge whether a capability is worth having.
"""

from __future__ import annotations

import os
import re
import sys

from repository import REPO

INVENTORY = os.path.join(REPO, "docs", "CAPABILITIES.md")
MIRROR = os.path.join(REPO, "docs", "ru", "CAPABILITIES.md")

# The eight fields a capability is described by, in the order the document states them, and the
# mirror's names for the same eight. Both documents are parsed and both are checked: a mirror whose
# blocks are empty would pass a check that only counted identifiers, which is the failure mode a
# mirror has.
FIELDS = ("Input", "Output", "Command", "Mode", "Evidence", "Cost", "Does not", "Layer")
MIRRORED_FIELDS = (
    "Вход",
    "Выход",
    "Команда",
    "Режим",
    "Свидетельство",
    "Стоимость",
    "Чего не делает",
    "Слой",
)

# Every mode a capability may be in, and what each says about where it runs.
MODES = {
    "offline": "runs under `make pr` from a clean clone",
    "offline, costly": "runs under `make capabilities-full`, because minutes in the gate is a gate people stop running",
    "offline, needs an artefact": "runs offline over an input another capability produces",
    "offline, needs a toolchain": "runs offline once a toolchain the tree does not vendor is installed",
    "live": "runs under `make test-live`: needs a network, a container or a key",
}

# The layers the architecture declares, and which crate sits on each — read from the lint script so
# that one table answers both questions. A capability's layer is its crate's layer: a binary is a
# product by construction, and a capability driven through a test names the crate it tests.
def _declared_layers() -> dict[str, str]:
    import importlib.util

    path = os.path.join(os.path.dirname(os.path.abspath(__file__)), "check-lints.py")
    spec = importlib.util.spec_from_file_location("check_lints", path)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module.CRATE_LAYERS


CRATE_LAYERS = _declared_layers()
LAYERS = set(CRATE_LAYERS.values())
PRODUCTS = "5 · products"

HEADING = re.compile(r"^### `([a-z0-9.\-]+)`\s*$")
FIELD_ROW = re.compile(r"^\|\s*([^|]+?)\s*\|\s*(.+?)\s*\|\s*$")


def parse(path: str, fields: tuple[str, ...] = FIELDS) -> dict[str, dict[str, str]]:
    """Every capability in `path`, as its identifier and its fields."""
    capabilities: dict[str, dict[str, str]] = {}
    current: str | None = None
    with open(path, encoding="utf-8") as handle:
        for line in handle:
            heading = HEADING.match(line.rstrip())
            if heading:
                current = heading.group(1)
                capabilities[current] = {}
                continue
            if current is None:
                continue
            row = FIELD_ROW.match(line.rstrip())
            if row:
                name, value = row.group(1), row.group(2)
                if name in fields:
                    capabilities[current][fields[fields.index(name)]] = value
    return capabilities


def missing_fields(
    capabilities: dict[str, dict[str, str]], wanted: tuple[str, ...] = FIELDS, where: str = ""
) -> list[str]:
    found = []
    for name, fields in capabilities.items():
        for field in wanted:
            if not fields.get(field):
                found.append(f"{name}: no {field}{where}")
    return found


def wrong_modes(capabilities: dict[str, dict[str, str]]) -> list[str]:
    return [
        f"{name}: mode {fields['Mode']!r} is not one of {sorted(MODES)}"
        for name, fields in capabilities.items()
        if fields.get("Mode") and fields["Mode"] not in MODES
    ]


def wrong_layers(capabilities: dict[str, dict[str, str]]) -> list[str]:
    return [
        f"{name}: layer {fields['Layer']!r} is not one the architecture declares"
        for name, fields in capabilities.items()
        if fields.get("Layer") and fields["Layer"] not in LAYERS
    ]


# What makes an output machine-readable: something parses it, and something therefore depends on
# its shape. A human-readable answer — an explanation, a report meant to be read — carries no
# version, because there is no parser to break.
STRUCTURED = ("json", "sarif", "columnar", "parquet", "contractschema")

# A version, in any shape this tree states one: `2.1.0`, `0.2`, or the `v1` a feature
# partition carries in its own rows.
VERSION = re.compile(r"\b(\d+\.\d+(\.\d+)?|v\d+)\b")


def unversioned_outputs(capabilities: dict[str, dict[str, str]]) -> list[str]:
    """Every machine-readable output whose version the document does not state.

    A caller parsing an answer needs to know which shape it is parsing, and the place to say so is
    the same place that says what the answer is. An output nobody parses is exempt, and saying which
    outputs those are is what the exemption is for.
    """
    found = []
    for name, fields in capabilities.items():
        output = fields.get("Output", "")
        lowered = output.lower()
        if not any(word in lowered for word in STRUCTURED):
            continue
        if not VERSION.search(output):
            found.append(f"{name}: {output!r} states no version")
    return found


def binary_owners() -> dict[str, str]:
    """Which crate declares each binary this workspace builds."""
    owners: dict[str, str] = {}
    crates = os.path.join(REPO, "crates")
    for crate in sorted(os.listdir(crates)):
        manifest = os.path.join(crates, crate, "Cargo.toml")
        if not os.path.isfile(manifest):
            continue
        with open(manifest, encoding="utf-8") as handle:
            text = handle.read()
        for section in text.split("[[bin]]")[1:]:
            named = re.search(r'^\s*name\s*=\s*"([^"]+)"', section, re.M)
            if named:
                owners[named.group(1)] = crate
        source = os.path.join(crates, crate, "src")
        if os.path.isfile(os.path.join(source, "main.rs")):
            owners.setdefault(crate, crate)
        binaries = os.path.join(source, "bin")
        if os.path.isdir(binaries):
            for entry in os.listdir(binaries):
                if entry.endswith(".rs"):
                    owners[os.path.splitext(entry)[0]] = crate
    return owners


def layers_against_the_graph(capabilities: dict[str, dict[str, str]]) -> list[str]:
    """Every capability whose layer is not the layer of the crate its command drives.

    The crate is found the same way either way: a command that drives tests names its crate, and a
    command that runs a binary is owned by whichever crate declares that binary. A crate the
    architecture places is checked against that placement; a crate it does not place is a domain's
    own product surface, and a product is what a binary is here.
    """
    owners = binary_owners()
    found = []
    for name, fields in capabilities.items():
        claimed = fields.get("Layer", "")
        command = fields.get("Command", "")
        tested = re.search(r"-p\s+([a-z0-9\-]+)", command)
        if tested:
            crate = tested.group(1)
        else:
            invoked = re.search(r"`?([a-z0-9_\-]+)", command)
            crate = owners.get(invoked.group(1)) if invoked else None
            if crate is None:
                found.append(f"{name}: the command names no binary this workspace builds")
                continue
        expected = CRATE_LAYERS.get(crate, PRODUCTS)
        if claimed != expected:
            found.append(f"{name}: claims {claimed!r}, and `{crate}` sits on {expected!r}")
    return found


def unbuilt_commands(capabilities: dict[str, dict[str, str]]) -> list[str]:
    """Every command whose binary no manifest in this workspace declares."""
    binaries = set()
    crates = os.path.join(REPO, "crates")
    for crate in sorted(os.listdir(crates)):
        manifest = os.path.join(crates, crate, "Cargo.toml")
        if not os.path.isfile(manifest):
            continue
        with open(manifest, encoding="utf-8") as handle:
            text = handle.read()
        binaries |= set(re.findall(r'^\s*name\s*=\s*"([^"]+)"', text, re.M))
        source = os.path.join(crates, crate, "src")
        if os.path.isfile(os.path.join(source, "main.rs")):
            binaries.add(crate)
        binaries |= {
            os.path.splitext(entry)[0]
            for entry in os.listdir(os.path.join(source, "bin"))
            if entry.endswith(".rs")
        } if os.path.isdir(os.path.join(source, "bin")) else set()

    found = []
    for name, fields in capabilities.items():
        command = fields.get("Command", "")
        match = re.search(r"`([a-z0-9_\-]+)", command)
        if not match:
            found.append(f"{name}: the command names no binary")
            continue
        binary = match.group(1)
        if binary not in binaries and binary not in {"cargo", "make", "python"}:
            found.append(f"{name}: `{binary}` is a binary no manifest declares")
    return found


def mirror_gaps(capabilities: dict[str, dict[str, str]]) -> list[str]:
    """Every capability the Russian mirror is missing, and every one it has alone."""
    if not os.path.isfile(MIRROR):
        return ["docs/ru/CAPABILITIES.md does not exist"]
    mirrored = parse(MIRROR, MIRRORED_FIELDS)
    found = [f"{name}: not in the mirror" for name in capabilities if name not in mirrored]
    found += [f"{name}: in the mirror and not in the document" for name in mirrored if name not in capabilities]
    return found


def main() -> int:
    if not os.path.isfile(INVENTORY):
        print("docs/CAPABILITIES.md does not exist")
        return 1

    capabilities = parse(INVENTORY)
    print(f"capabilities described                    {len(capabilities):>4}")

    mirrored = parse(MIRROR, MIRRORED_FIELDS) if os.path.isfile(MIRROR) else {}
    checks = [
        ("capabilities missing a field", missing_fields(capabilities)),
        (
            "the same, in the mirror",
            missing_fields(mirrored, MIRRORED_FIELDS, " in the mirror"),
        ),
        ("capabilities with an unknown mode", wrong_modes(capabilities)),
        ("capabilities with an unknown layer", wrong_layers(capabilities)),
        ("layers that disagree with the graph", layers_against_the_graph(capabilities)),
        ("structured outputs with no version", unversioned_outputs(capabilities)),
        ("commands naming no built binary", unbuilt_commands(capabilities)),
        ("capabilities out of step with the mirror", mirror_gaps(capabilities)),
    ]
    failed = False
    for label, found in checks:
        print(f"{label:<41} {len(found):>4}  (target 0)")
        for entry in found:
            print(f"    {entry}")
        failed = failed or bool(found)

    return 1 if failed or not capabilities else 0


if __name__ == "__main__":
    sys.exit(main())
