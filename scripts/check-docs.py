#!/usr/bin/env python
"""The mechanical documentation rules.

Each rule counts its offenders against a baseline in `scripts/docs-baseline.json`, and the target is
always zero. A count above its baseline fails; `--update` lowers a baseline to what was measured and
never raises one, so the numbers ratchet down and a rule that has been paid for cannot be unpaid.
`--list` prints the offenders of every rule rather than only of the ones that grew, which is what a
milestone works from.

What the repository carries is checked: tracked files plus new ones that are not ignored. A document
that is ignored is no longer part of the repository, whether or not it is still on disk, so it is not
this script's business — which is what makes untracking the way to retire a document that a live one
contradicts.

Every rule prints its count, what it is allowed today and the target, and the offenders behind any
count that grew — so the output is a work list rather than a verdict.
"""

from __future__ import annotations

import functools
import json
import os
import re
import sys

from repository import REPO, tracked

if hasattr(sys.stdout, "reconfigure"):
    sys.stdout.reconfigure(encoding="utf-8")

BASELINE = os.path.join(REPO, "scripts", "docs-baseline.json")

# Files whose Cyrillic is test data rather than prose. Each entry needs a reason next to it, and an
# entry without one is a rule that has been quietly switched off.
CYRILLIC_ALLOWLIST: dict[str, str] = {
    "crates/sting-pipeline/src/controllers/report.rs": (
        "a Cyrillic line is the test input itself: a caret offset counted in characters against a "
        "string whose bytes outnumber them"
    ),
}

EMOJI = re.compile("[\U0001f000-\U0001faff☀-➿⬀-⯿️]")

# Development chronology: a named milestone, a phase number, or a promise about later. A reason is a
# property of the design and belongs in the documents; a milestone's name dates the text the moment it
# is written, while the common noun does not.
META = re.compile(
    r"\bФаза\s*\d|\bфазы?\s+\d|\bБлок\s*\d|\bблок(?:а|е|ов)?\s+\d\d"
    r"|\bphase\s+\d|\bblock\s+\d\d|\bM\d+(?:\.\d+)?\b"
    r"|\bпока что\b|\bв будущем\b|\bпозже добавим\b|\bfor now\b|\bTODO\b"
    r"|\bthis session\b|\bsame session\b|\bautonomous session\b|\bsession\s+\d"
    r"|\bpost-review\b|\brisk spike\b|\bDefinition of Done\b|\bDoD\b|\bthis phase\b",
    re.IGNORECASE,
)

# "this block" means a development block in a doc comment, which describes an item, and a basic block
# in a comment inside a body. Only the first is chronology, and only the first is matched.
BLOCK = re.compile(r"^\s*//[/!].*\b(?:this|that) block\b", re.IGNORECASE)

# Filler that carries no content, and marketing adjectives.
FILLER = re.compile(
    r"важно понимать|важно отметить|в современном мире|нельзя не отметить"
    r"|играет ключевую роль|стоит подчеркнуть|как известно"
    r"|мощн(ый|ая|ое|ые)|уникальн(ый|ая|ое|ые)|инновацион|передов(ой|ая|ое|ые)",
    re.IGNORECASE,
)

# A pointer into a document by path. The repository retires a document by untracking it, so a bare
# path that resolves to nothing is a dead pointer — while a reference naming the commit that still
# holds the file is a citation of history and stays.
DOC_PATH = re.compile(r"(?<![\w/-])((?:docs|security|abi-layer)/[\w./-]*\.md)")
# The same pointer written as a bare file name. `CONTROLLERS.md` resolves to nothing just as surely
# as `docs/critical/CONTROLLERS.md` does, and reads as if it resolves to something.
BARE_DOC = re.compile(r"(?<![\w/-])([A-Z][A-Z0-9_-]{2,}\.md)")
COMMIT_CITED = re.compile(r"git show [0-9a-f]{7,40}:")

# Every document closes by naming its own boundary. A document that only says what it covers leaves
# the reader to discover the edge by walking off it.
BOUNDARY = re.compile(
    r"does not|deliberately not|Чего\s.*\sне\s|намеренно не|сознательно не",
    re.IGNORECASE,
)

CYRILLIC = re.compile("[Ѐ-ӿ]")

# A decorative bar separates nothing the language does not already separate, and it survives every
# edit below it because nobody dares touch it.
SEPARATOR = re.compile(r"//[/!]?\s*[-=*~_]{6,}")

# Code that was commented out rather than deleted. Git holds the version that worked; a comment that
# holds a second one leaves the reader guessing which is current. A quotation of code in another
# language is caught by the same rule and belongs in prose for the same reason: a pasted snippet ages
# exactly like commented-out code and nothing recompiles it.
COMMENTED_CODE = re.compile(
    r"^\s*//\s*(?:let |fn |pub fn |if |for |while |match |return |use |impl |struct |enum |"
    r"self\.|assert|println!|dbg!).*[;{})]\s*$"
)

LINK = re.compile(r"\[[^\]]*\]\(([^)]+)\)")
FENCE = re.compile(r"^```.*?^```", re.MULTILINE | re.DOTALL)
HEADING = re.compile(r"^(#{1,6})\s+(.*)$")
SECTION_NUMBER = re.compile(r"^(\d+(?:\.\d+)*)\.?\s")

# A plan names its milestones; that is what makes it a plan. The meta rule exists to keep development
# chronology out of the documents that describe the system, and out of the code.
PLANNING = ("ROADMAP.md", "PLAN.md", "DECISIONS.md")

# A changelog is a record of what happened and is dated by construction. It names documents that were
# live when the entry was written, and rewriting it to match today would falsify the record.
HISTORICAL = ("CHANGELOG.md",)

# A document that states a prohibition has to be able to name what it prohibits.
RULES = ("AGENTS.md", "CLAUDE.md", "CONTRIBUTING.md")

# Names that look like a pointer into this repository and are not. Each needs its reason next to it,
# and an entry without one is a rule quietly switched off.
NOT_A_POINTER: dict[str, str] = {
    "GETTING_LIBEMULATOR.md": "the name a release archive gives a copied document, not a file here",
}


class Repository:
    """The files the repository carries, read once and shared by every rule."""

    def __init__(self) -> None:
        self._everything = set(tracked("*"))
        self._text: dict[str, str] = {}

    def text(self, path: str) -> str:
        if path not in self._text:
            with open(os.path.join(REPO, path), encoding="utf-8") as handle:
                self._text[path] = handle.read()
        return self._text[path]

    def carries(self, path: str) -> bool:
        return path in self._everything

    @functools.cached_property
    def docs(self) -> list[str]:
        return [p for p in tracked("docs/*.md", "docs/**/*.md") if p.endswith(".md")]

    @functools.cached_property
    def canonical(self) -> list[str]:
        return [p for p in self.docs if not p.startswith("docs/ru/")]

    @functools.cached_property
    def mirrors(self) -> dict[str, str]:
        return {p[len("docs/ru/") :]: p for p in self.docs if p.startswith("docs/ru/")}

    @functools.cached_property
    def markdown(self) -> list[str]:
        return self.docs + [p for p in tracked("*.md") if "/" not in p]

    @functools.cached_property
    def basenames(self) -> set[str]:
        return {p.rsplit("/", 1)[-1] for p in self._everything}

    @functools.cached_property
    def pointers(self) -> list[str]:
        """Everything that can point at a document: prose, workflows and manifests."""
        found = tracked("*.md", "*.toml", ".github/workflows/*.yml")
        return [
            p for p in found
            if not p.startswith(("karman/", "sybil/", "tools/", "vendor/"))
            and not p.endswith(HISTORICAL)
        ]

    @functools.cached_property
    def sources(self) -> list[str]:
        """This platform's own Rust. The projects that have not merged carry their own rules."""
        return tracked("crates/**/*.rs")

    @functools.cached_property
    def unmerged_sources(self) -> list[str]:
        return tracked("sybil/**/*.rs")


def heading_tree(text: str) -> list[tuple[int, str]]:
    """Levels and section numbers, without the prose — what a mirror must reproduce."""
    tree = []
    for line in text.splitlines():
        match = HEADING.match(line)
        if match:
            number = SECTION_NUMBER.match(match.group(2).strip())
            tree.append((len(match.group(1)), number.group(1) if number else ""))
    return tree


def matches(repo: Repository, paths: list[str], pattern: re.Pattern[str]) -> list[str]:
    found = []
    for path in paths:
        for number, line in enumerate(repo.text(path).splitlines(), 1):
            found.extend(f"{path}:{number}: {m.group(0)}" for m in pattern.finditer(line))
    return found


def missing_mirrors(repo: Repository) -> list[str]:
    mirrors = repo.mirrors
    return [p for p in repo.canonical if p[len("docs/") :] not in mirrors]


def diverged_mirrors(repo: Repository) -> list[str]:
    mirrors = repo.mirrors
    diverged = []
    for path in repo.canonical:
        mirror = mirrors.get(path[len("docs/") :])
        if mirror and heading_tree(repo.text(path)) != heading_tree(repo.text(mirror)):
            diverged.append(f"{path} vs {mirror}")
    return diverged


def link_targets(repo: Repository) -> tuple[list[str], list[str]]:
    """Links that resolve to nothing, and links into files the repository has retired."""
    broken, retired = [], []
    for path in repo.markdown:
        base = os.path.dirname(os.path.join(REPO, path))
        # A link inside a fenced block illustrates the link format and is not a link.
        for match in LINK.finditer(FENCE.sub("", repo.text(path))):
            target = match.group(1).split("#")[0].strip()
            if not target or target.startswith(("http://", "https://", "mailto:")):
                continue
            resolved = os.path.normpath(os.path.join(base, target))
            if not os.path.exists(resolved):
                broken.append(f"{path} -> {target}")
                continue
            relative = os.path.relpath(resolved, REPO).replace(os.sep, "/")
            if os.path.isfile(resolved) and not repo.carries(relative):
                retired.append(f"{path} -> {target}")
    return broken, retired


def dead_pointers(repo: Repository, paths: list[str] | None = None) -> list[str]:
    """Paths naming a document the repository no longer carries, without naming where it survives."""
    dead = []
    for path in paths if paths is not None else repo.pointers:
        for number, line in enumerate(repo.text(path).splitlines(), 1):
            if COMMIT_CITED.search(line):
                continue
            for match in DOC_PATH.finditer(line):
                target = match.group(1)
                if not repo.carries(target):
                    dead.append(f"{path}:{number}: {target}")
            if path.endswith(PLANNING):
                continue
            for match in BARE_DOC.finditer(line):
                name = match.group(1)
                if name not in repo.basenames and name not in NOT_A_POINTER:
                    dead.append(f"{path}:{number}: {name}")
    return dead


def unbounded_documents(repo: Repository) -> list[str]:
    """Documents whose last section does not say what they leave out."""
    unbounded = []
    for path in repo.docs:
        headings = [line for line in repo.text(path).splitlines() if line.startswith("## ")]
        if not headings or not BOUNDARY.search(headings[-1]):
            unbounded.append(f"{path}: {headings[-1] if headings else 'no sections'}")
    return unbounded


def russian_prose(repo: Repository, paths: list[str]) -> list[str]:
    prose = []
    for path in paths:
        if path in CYRILLIC_ALLOWLIST:
            continue
        count = len(CYRILLIC.findall(repo.text(path)))
        if count:
            prose.append(f"{path}: {count}")
    return prose


class Report:
    """Counts against a ratchet: a rule may improve or hold, and never worsen."""

    def __init__(self, verbose: bool) -> None:
        self.verbose = verbose
        self.rules: list[tuple[str, list[str]]] = []
        try:
            with open(BASELINE, encoding="utf-8") as handle:
                self.baseline: dict[str, int] = json.load(handle)
        except FileNotFoundError:
            self.baseline = {}

    def rule(self, name: str, offenders: list[str]) -> None:
        self.rules.append((name, offenders))

    def save(self) -> None:
        lowered = {
            name: min(len(offenders), self.baseline.get(name, len(offenders)))
            for name, offenders in self.rules
        }
        with open(BASELINE, "w", encoding="utf-8") as handle:
            json.dump(lowered, handle, indent=2, sort_keys=True)
            handle.write("\n")

    def emit(self) -> int:
        width = max(len(name) for name, _ in self.rules)
        failed = False
        for name, offenders in self.rules:
            allowed = self.baseline.get(name, 0)
            count = len(offenders)
            ok = count <= allowed
            failed = failed or not ok
            verdict = "ok" if count == 0 else ("held" if ok else "FAIL")
            print(f"{name.ljust(width)}  {count:>6}  (allowed {allowed}, target 0)  {verdict}")
            if not ok or self.verbose:
                for offender in offenders[:12]:
                    print(f"    {offender}")
                if len(offenders) > 12:
                    print(f"    … and {len(offenders) - 12} more")
        return 1 if failed else 0


# Every pattern with an example it must match. A pattern that matches nothing reports zero offenders
# and reads exactly like a rule that has been paid for — which is how a corrupted escape once turned
# this whole rule set green while the offenders were still there.
SAMPLES = [
    (EMOJI, "🚀"),
    (META, "see Фаза 5 for the plan"),
    (META, "not implemented for now"),
    (FILLER, "важно понимать, что"),
    (BOUNDARY, "## 9. What this document does not cover"),
    (BOUNDARY, "## 9. Чего этот документ не покрывает"),
    (CYRILLIC, "прозa"),
    (SEPARATOR, "// ---------------- section"),
    (COMMENTED_CODE, "    // let x = compute();"),
    (BLOCK, "/// found during this block's implementation"),
    (DOC_PATH, "see docs/blocks/03-benchmark-harness.md for it"),
    (COMMIT_CITED, "git show cf52463:docs/security/PRECISION_REPORT.md"),
]


def self_test() -> int:
    """Each pattern against an example it must match."""
    broken = [p.pattern for p, sample in SAMPLES if not p.search(sample)]
    print(f"patterns that match nothing  {len(broken):>4}  (target 0)")
    for pattern in broken:
        print(f"    {pattern}")
    return 1 if broken else 0


def main() -> int:
    if self_test():
        return 1
    repo = Repository()
    broken, retired = link_targets(repo)
    describing = [
        p for p in repo.markdown if not p.endswith(PLANNING + HISTORICAL + RULES)
    ]

    report = Report(verbose="--list" in sys.argv)
    report.rule("mirror missing", missing_mirrors(repo))
    report.rule("mirror heading tree", diverged_mirrors(repo))
    report.rule("broken links", broken)
    report.rule("links to retired files", retired)
    report.rule("dead document pointers", dead_pointers(repo))
    report.rule("emoji in documents", matches(repo, repo.markdown, EMOJI))
    report.rule("project meta in documents", matches(repo, describing, META))
    report.rule("documents without a boundary", unbounded_documents(repo))
    report.rule("filler in documents", matches(repo, repo.markdown, FILLER))
    report.rule("emoji in sources", matches(repo, repo.sources, EMOJI))
    report.rule("project meta in sources", matches(repo, repo.sources, META))
    report.rule("development blocks in doc comments", matches(repo, repo.sources, BLOCK))
    report.rule("separator bars in sources", matches(repo, repo.sources, SEPARATOR))
    report.rule("commented-out code", matches(repo, repo.sources, COMMENTED_CODE))
    report.rule("dead document pointers in sources", dead_pointers(repo, repo.sources))
    report.rule("cyrillic in sources", russian_prose(repo, repo.sources))
    report.rule("cyrillic in unmerged projects", russian_prose(repo, repo.unmerged_sources))
    if "--update" in sys.argv:
        report.save()
    return report.emit()


if __name__ == "__main__":
    sys.exit(main())
