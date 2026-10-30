#!/usr/bin/env python
"""Runs the commands the inventory claims, so that a claim cannot outlive the thing it claims.

`docs/CAPABILITIES.md` names a command per capability. A command that no longer runs is the most
expensive kind of documentation: it is believed, tried, and found broken by whoever needed it most.

So the commands are run rather than read. Which ones depends on the mode: `offline` under the
ordinary gate, and the costly and artefact-dependent ones only when asked for, because minutes in a
gate is a gate people stop running. Live capabilities are never run here — they need a network, a
container or a key, and `make test-live` is where that belongs.

A command is run exactly as the document states it, from the repository root. Placeholders are the
one exception: an angle-bracketed word is a value the caller supplies, and a capability whose
command carries one is skipped with that fact printed rather than run with something invented.

What this does not do: it does not check a command's output. Whether the answer is right is what
the test suites are for; what this establishes is narrower and is the thing documentation gets
wrong — that the command exists, is spelled correctly, and returns zero.
"""

from __future__ import annotations

import os
import re
import shlex
import subprocess
import sys
import time

import importlib.util

from repository import REPO


def _load_checker():
    """The checker's own parser, imported by path: a hyphen is not a module name."""
    path = os.path.join(os.path.dirname(os.path.abspath(__file__)), "check-capabilities.py")
    spec = importlib.util.spec_from_file_location("check_capabilities", path)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


_CHECKER = _load_checker()
parse, INVENTORY = _CHECKER.parse, _CHECKER.INVENTORY

# Which modes this script runs, and under which flag.
ORDINARY = {"offline"}
FULL = ORDINARY | {"offline, costly", "offline, needs an artefact"}
LIVE = {"live"}

# A command that holds a port is started, watched for a moment and then stopped. Waiting for it to
# finish would never return, and skipping it would leave the two services as the only capabilities
# nothing checks — which is where a broken command hides longest.
SERVING = ("serve",)

# A sweep is the same shape as a service: it does not finish, it is stopped. One of them walks a
# chain's whole transaction stream and the other a whole market, and their own Cost fields say so —
# what running them here establishes is that they start, reach their service and keep going.
SWEEPING = ("sweep-ton-census",)

# How long such a command is given to fail on its own. One that has not exited by then has loaded
# its configuration, bound its port or reached its service — which is what starting it proves.
SERVE_WATCH_SECS = 6

PLACEHOLDER = re.compile(r"<[a-z][a-z\-]*>")


def command_of(fields: dict[str, str]) -> str | None:
    """The command a block states, unwrapped from its backticks."""
    match = re.search(r"`([^`]+)`", fields.get("Command", ""))
    return match.group(1) if match else None


def runnable(command: str) -> tuple[bool, str]:
    """Whether this command can be run unattended, and why not when it cannot."""
    if PLACEHOLDER.search(command):
        return False, "carries a value the caller supplies"
    return True, ""


def serves(command: str) -> bool:
    """Whether this command runs until it is stopped rather than finishing on its own."""
    return any(f" {word}" in command for word in SERVING + SWEEPING)


def start_and_stop(command: str) -> tuple[int, str, float]:
    """Starts a service, watches it for a moment, and stops it.

    Exiting early is the failure this catches: a missing model file, an unbindable port, a key file
    that does not parse. Still running after the watch is the answer, because a service that has
    bound its port has done everything starting can prove.
    """
    parts = resolve(shlex.split(command, posix=False))
    started = time.monotonic()
    try:
        process = subprocess.Popen(parts, cwd=REPO, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
    except OSError as error:
        return 127, str(error), time.monotonic() - started
    try:
        code = process.wait(timeout=SERVE_WATCH_SECS)
    except subprocess.TimeoutExpired:
        process.terminate()
        try:
            process.wait(timeout=10)
        except subprocess.TimeoutExpired:
            process.kill()
        return 0, "still running when stopped", time.monotonic() - started
    output = (process.stderr.read() if process.stderr else "") or ""
    tail = output.strip().splitlines()
    # Finishing early is not a failure by itself: a sweep given a small target really does finish.
    # What is a failure is finishing with something to say about why.
    return code, (tail[-1] if tail else "finished before the watch was over"), time.monotonic() - started


def resolve(parts: list[str]) -> list[str]:
    """A command's own binary, taken from this workspace's build rather than from the path."""
    if parts and os.path.sep not in parts[0]:
        local = os.path.join(REPO, "target", "debug", parts[0])
        for candidate in (local, local + ".exe"):
            if os.path.isfile(candidate):
                parts[0] = candidate
                break
    return parts


def run(command: str, timeout_secs: int) -> tuple[int, str, float]:
    """Runs `command` from the repository root, and returns its code, its tail and its seconds."""
    parts = resolve(shlex.split(command, posix=False))
    started = time.monotonic()
    try:
        finished = subprocess.run(
            parts, cwd=REPO, capture_output=True, text=True, timeout=timeout_secs
        )
    except subprocess.TimeoutExpired:
        return 124, f"did not finish within {timeout_secs}s", time.monotonic() - started
    except OSError as error:
        return 127, str(error), time.monotonic() - started
    tail = (finished.stderr or finished.stdout or "").strip().splitlines()
    return finished.returncode, (tail[-1] if tail else ""), time.monotonic() - started


def main() -> int:
    full = "--full" in sys.argv
    live = "--live" in sys.argv
    wanted = LIVE if live else FULL if full else ORDINARY
    timeout_secs = 900 if full or live else 120

    capabilities = parse(INVENTORY)
    ran = failed = skipped = 0

    # In the order the document states them rather than alphabetically: the clustering capabilities
    # are a chain — a schema, then rows, then features over those rows — and a document read top to
    # bottom is where that order is already written down.
    for name, fields in capabilities.items():
        mode = fields.get("Mode", "")
        if mode not in wanted:
            continue
        command = command_of(fields)
        if command is None:
            print(f"    {name}: states no command")
            failed += 1
            continue
        can_run, why = runnable(command)
        if not can_run:
            print(f"    {name}: skipped, {why}")
            skipped += 1
            continue

        code, tail, seconds = (
            start_and_stop(command) if serves(command) else run(command, timeout_secs)
        )
        ran += 1
        if code != 0:
            failed += 1
            print(f"    {name}: exit {code} after {seconds:.1f}s — {tail}")
        else:
            print(f"    {name}: ok in {seconds:.1f}s")

    scope = (
        "every live capability"
        if live
        else "every offline capability, costly ones included"
        if full
        else "every offline capability"
    )
    print(f"{scope}: {ran} run, {failed} failed, {skipped} skipped  (target 0 failed)")
    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(main())
