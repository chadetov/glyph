#!/usr/bin/env python3
"""Numbers and version references in live docs must match reality.

This gate exists because the release audit kept finding the same thing. Across
the 0.1.89 cut, five separate review agents spent seventy-seven minutes between
them, and every blocking finding they returned was a stale number or a stale
version reference in a markdown file: a test count in the transpiler roadmap
that no longer matched the suite, a sentence promising a feature "in 0.1.88"
after 0.1.88 had shipped without it. That is a two-second script, not an hour
of review, and review time spent on it is review time not spent on the diff.

Three checks:

1. **Test counts.** Any live status doc that states a number of tests has to
   state the number the suite actually reports. The count comes from a suite
   log, and the log has to be newer than every crate source, so this cannot
   pass on a stale number the way a hand-check could.

2. **Forward references.** A sentence promising something "in 0.1.N" is a
   promise with a deadline. Once 0.1.N ships, the sentence is either true and
   should be past tense, or false. Either way it cannot stay as written.

3. **The Next marker.** Exactly one release section carries it, and it names a
   version ahead of what has shipped.

4. **Density figures.** The token totals the site's benchmarks page draws, and
   any "N% fewer tokens than TypeScript" sentence in a live doc or page, have
   to agree with the newest results file under benchmarks/results/. The
   density claim was re-measured on 2026-09-06 when the fixtures were made to
   compile (341 became 436), FINDINGS.md and the release notes were corrected,
   and the page on glyphlang.io kept saying 341 and 29% for two weeks until an
   outside reviewer quoted it back. A number that is published in two places
   with one gate is a number that will disagree.

Frozen history is exempt by design: archive/, the implementation plan, the open
questions, and the per-release entries in releases.md and web/versions/ are
records of what was true when written. Editing those to match the present is
the one thing this repo treats as worse than leaving them alone.
"""

from __future__ import annotations

import argparse
import pathlib
import re
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent

# Docs that describe the present. A wrong number in one of these is a live lie.
LIVE_DOCS = [
    "README.md",
    "glyph-compiler/README.md",
    "docs/README.md",
    "docs/manifesto.md",
    "docs/error-codes.md",
    "docs/stability.md",
    "docs/language/spec.md",
]
LIVE_GLOBS = ["docs/roadmap/*.md", "docs/guide/*.md", "docs/language/*.md"]

# Records of what was true when written. Never reconciled to the present.
FROZEN = {
    "docs/implementation-plan.md",
    "docs/open-questions.md",
    # Per-release entries; "690 tests green" under a shipped heading is a fact
    # about that release, not a claim about now.
    "docs/roadmap/releases.md",
}

SUITE_DEFAULT = ROOT / "glyph-compiler" / "target" / "suite.log"

# "1057 tests pass", "1057 workspace tests", "1,057 tests green". Deliberately
# narrow: a doc saying "199 integration tests" is counting one file, and holding
# that against the workspace total would be a false failure every release.
TEST_CLAIM = re.compile(
    r"\b(\d[\d,]{2,})\s+(?:workspace\s+tests?|tests?\s+(?:pass|passing|green))\b",
    re.I,
)

# A promise, not a record. "shipped in 0.1.76" and "added in 0.1.45" are history
# and correct; "lands in 0.1.90" is a debt that comes due. The past-tense forms
# have to stay out of this pattern or the gate cries wolf on five true sentences
# and gets ignored, which is worse than not having it.
PROMISE = re.compile(
    r"\b(?:will\s+(?:\w+\s+){0,3}(?:in|for)"
    r"|(?:lands?|ships?|arrives?|comes?)\s+in"
    r"|(?:planned|scheduled|targeted|slated|due)\s+(?:for|in|at)"
    r"|coming\s+in)"
    r"\s+(0\.\d+\.\d+)\b",
    re.I,
)
PASSED = re.compile(r"^test result: ok\. (\d+) passed", re.M)
# Entries were headed `### 0.1.N` up to 0.1.113 and `**0.1.N` since; the
# gate reads both, or it goes blind to every live entry, which is how it stayed
# quiet from 0.1.113 to 0.1.123.
SHIPPED = re.compile(r"^(?:### |\*\*)(0\.\d+\.\d+) — Shipped", re.M)
NEXT = re.compile(r"^(?:### |\*\*)(0\.\d+\.\d+) — Next", re.M)


def vkey(v: str) -> tuple:
    return tuple(int(x) for x in v.split("."))


def live_files() -> list[pathlib.Path]:
    seen: list[pathlib.Path] = []
    for rel in LIVE_DOCS:
        p = ROOT / rel
        if p.exists() and rel not in FROZEN:
            seen.append(p)
    for glob in LIVE_GLOBS:
        for p in sorted(ROOT.glob(glob)):
            rel = p.relative_to(ROOT).as_posix()
            if rel not in FROZEN and p not in seen:
                seen.append(p)
    return seen


def newest_source() -> float:
    newest = 0.0
    # The runtime `.ts` files count as sources: they are materialized by the
    # compiler and a change to one breaks emit tests exactly the way a change to
    # a crate does. Leaving them out meant a stale log could pass this gate after
    # a runtime edit, which is the one thing the gate exists to prevent.
    for pat in (
        "glyph-compiler/crates/**/*.rs",
        "glyph-compiler/**/Cargo.toml",
        "glyph-compiler/runtime/**/*.ts",
    ):
        for p in ROOT.glob(pat):
            if "/target/" in p.as_posix():
                continue
            newest = max(newest, p.stat().st_mtime)
    return newest


def suite_complete(log: pathlib.Path) -> bool:
    """Did the run that wrote this log finish?

    The recipes append an ``EXIT=`` marker as the last line. Without it the log
    belongs to a run that is still going or that died partway, and its partial
    totals are not a count of anything. Reading one anyway reported "the suite
    reports 11" against a tree with over a thousand tests, which is a gate
    inventing a number; a gate that does that once gets ignored afterwards.
    """
    if not log.exists():
        return False
    return "EXIT=" in log.read_text(errors="replace")


def suite_total(log: pathlib.Path) -> int | None:
    """Sum the per-binary totals cargo prints. One line per test binary."""
    if not log.exists() or not suite_complete(log):
        return None
    counts = [int(m) for m in PASSED.findall(log.read_text(errors="replace"))]
    return sum(counts) if counts else None


def check_test_counts(total: int, fails: list[str]) -> None:
    for p in live_files():
        rel = p.relative_to(ROOT).as_posix()
        for i, line in enumerate(p.read_text(errors="replace").splitlines(), 1):
            for m in TEST_CLAIM.finditer(line):
                claimed = int(m.group(1).replace(",", ""))
                # Only three- and four-digit numbers reach here, so this is not
                # catching "2 tests" in a sentence about a specific pair.
                if claimed != total:
                    fails.append(
                        f"{rel}:{i} claims {claimed} tests; the suite reports "
                        f"{total}\n      {line.strip()[:100]}"
                    )


def check_forward_refs(shipped_max: str, fails: list[str]) -> None:
    for p in live_files():
        rel = p.relative_to(ROOT).as_posix()
        for i, line in enumerate(p.read_text(errors="replace").splitlines(), 1):
            for m in PROMISE.finditer(line):
                ref = m.group(1)
                if vkey(ref) <= vkey(shipped_max):
                    fails.append(
                        f"{rel}:{i} promises something in {ref}, but {ref} has "
                        f"shipped. Say what it does now, or name a later version.\n"
                        f"      {line.strip()[:100]}"
                    )


RESULT_FILE = re.compile(r"^\d{4}-\d{2}-\d{2}T\d{2}-\d{2}-\d{2}Z\.json$")
BAR = re.compile(
    r'<span class="bar-lang">([A-Za-z]+)</span>\s*<div class="bar-track">'
    r'\s*<div class="bar-fill[^"]*"[^>]*>(\d+)</div>'
)
FEWER = re.compile(
    r"(\d+)%\s+fewer(?:\s+tokens)?\s+than\s+(?:the\s+equivalent\s+)?"
    r"(TypeScript|Go|Rust|Python)\b",
    re.I,
)
DENSITY_PAGE = "web/benchmarks/index.html"


def density_totals() -> dict[str, int] | None:
    """Per-language token totals from the newest density results file."""
    results = ROOT / "benchmarks" / "results"
    files = sorted(p for p in results.glob("*.json") if RESULT_FILE.match(p.name))
    if not files:
        return None
    import json

    data = json.loads(files[-1].read_text(errors="replace"))
    totals: dict[str, int] = {}
    for m in data.get("measurements", []):
        totals[m["language"].lower()] = totals.get(m["language"].lower(), 0) + int(m["tokens"])
    return totals or None


def density_files() -> list[pathlib.Path]:
    files = live_files()
    for rel in ("benchmarks/FINDINGS.md", "benchmarks/README.md"):
        p = ROOT / rel
        if p.exists():
            files.append(p)
    for p in sorted(ROOT.glob("web/**/index.html")):
        rel = p.relative_to(ROOT).as_posix()
        # web/versions/ is the release-notes record and stays as written.
        if not rel.startswith("web/versions/"):
            files.append(p)
    return files


def check_density_claims(fails: list[str]) -> dict[str, int] | None:
    totals = density_totals()
    if totals is None:
        fails.append("no density results file under benchmarks/results/")
        return None
    page = ROOT / DENSITY_PAGE
    if page.exists():
        text = page.read_text(errors="replace")
        drawn = {lang.lower(): int(n) for lang, n in BAR.findall(text)}
        for lang, n in drawn.items():
            if lang in totals and totals[lang] != n:
                fails.append(
                    f"{DENSITY_PAGE} draws {lang} at {n} tokens; the newest results "
                    f"file totals {totals[lang]}"
                )
        if "glyph" not in drawn:
            fails.append(f"{DENSITY_PAGE} draws no Glyph bar the gate can read")
    glyph = totals.get("glyph")
    if glyph is None:
        return totals
    for p in density_files():
        rel = p.relative_to(ROOT).as_posix()
        for i, line in enumerate(p.read_text(errors="replace").splitlines(), 1):
            for m in FEWER.finditer(line):
                claimed = int(m.group(1))
                other = totals.get(m.group(2).lower())
                if other is None:
                    continue
                actual = round((other - glyph) / other * 100)
                if abs(claimed - actual) > 1:
                    fails.append(
                        f"{rel}:{i} claims {claimed}% fewer tokens than "
                        f"{m.group(2)}; the newest results file says {actual}% "
                        f"({glyph} to {other})\n      {line.strip()[:100]}"
                    )
    return totals


def check_next_marker(fails: list[str]) -> str | None:
    rel = "docs/roadmap/releases.md"
    p = ROOT / rel
    if not p.exists():
        fails.append(f"{rel} is missing")
        return None
    text = p.read_text(errors="replace")
    nexts = NEXT.findall(text)
    shipped = SHIPPED.findall(text)
    if len(nexts) != 1:
        fails.append(
            f"{rel} carries {len(nexts)} Next markers; exactly one release is "
            f"the committed target ({', '.join(nexts) or 'none found'})"
        )
    if not shipped:
        fails.append(f"{rel} has no shipped entries to compare against")
        return None
    top = max(shipped, key=vkey)
    if nexts and vkey(nexts[0]) <= vkey(top):
        fails.append(
            f"{rel} marks {nexts[0]} as Next, but {top} has already shipped"
        )
    return top


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument(
        "--suite-log",
        default=str(SUITE_DEFAULT),
        help="cargo test output to read the real test count from",
    )
    ap.add_argument(
        "--tests",
        type=int,
        help="the count, when the suite ran somewhere this cannot see it",
    )
    args = ap.parse_args()

    fails: list[str] = []
    top = check_next_marker(fails)
    if top:
        check_forward_refs(top, fails)
    totals = check_density_claims(fails)

    total = args.tests
    if total is None:
        log = pathlib.Path(args.suite_log)
        total = suite_total(log)
        if total is None and log.exists() and not suite_complete(log):
            fails.append(
                f"{log} has no EXIT= marker, so the run that wrote it is still "
                f"going or it died partway. Wait for it to finish, or pass "
                f"--tests N. A partial log's totals are not a test count."
            )
        elif total is None:
            fails.append(
                f"no suite log at {log}. Run the suite and tee it there, or "
                f"pass --tests N. A doc's test count cannot be checked against "
                f"a number nobody measured."
            )
        elif log.stat().st_mtime < newest_source():
            fails.append(
                f"{log} is older than the crate sources. It reports {total} "
                f"tests for a compiler that has changed since. Re-run the "
                f"suite; do not re-stamp the log."
            )
            total = None
    if total is not None:
        check_test_counts(total, fails)

    if fails:
        print("doc claims FAIL:")
        for f in fails:
            print(f"  - {f}")
        return 1
    density = f", density figures match {totals['glyph']} tokens" if totals else ""
    print(f"doc claims OK: test counts match {total}, no stale version promises{density}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
