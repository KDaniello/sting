#!/usr/bin/env python
"""What the repository carries, asked once and answered the same way everywhere.

A rule that reads the working directory judges whatever happens to be on the disk — a local sweep's
cache, a scratch file, a document that was retired and left behind. What the repository carries is a
different set: tracked files, plus new ones that are not ignored. Untracking is how a document is
retired here, so the two sets have to be told apart, and one implementation of the question keeps
every rule answering the same one.
"""

from __future__ import annotations

import os
import subprocess

REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))


def tracked(*patterns: str) -> list[str]:
    """Paths the repository carries, matching `patterns` — pathspecs, not globs of the disk.

    A file deleted but not yet staged is still in the index and no longer on the disk. It is
    dropped here rather than handed to a rule that would then fail on opening it: a check that
    stops with a traceback says nothing about the rule it was checking, and deleting a file is
    ordinary work rather than a broken repository.
    """
    out = subprocess.run(
        ["git", "ls-files", "--cached", "--others", "--exclude-standard", "--", *patterns],
        cwd=REPO,
        capture_output=True,
        text=True,
        check=True,
    )
    return [
        line
        for line in out.stdout.splitlines()
        if line and os.path.isfile(os.path.join(REPO, line))
    ]
