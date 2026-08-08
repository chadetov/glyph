#!/usr/bin/env python3
"""Backlog discipline: every gap in docs/dogfooding-gaps.md carries a status, and
the count in the header matches the entries.

The gap list grows one dogfooding trip at a time, and the status markers fell
behind the source once already: entries stayed unmarked long after the release
that closed them, so working out what was actually left meant re-reading all 59
against the compiler. This keeps that from recurring.

Hard-fails (exit 1) when:
  - a `G` entry carries a marker that is not one of the documented set,
  - the same `G` number is defined twice with different markers,
  - the header's reconciled counts disagree with the entries.

Numbering gaps (a missing G number) is fine: entries get retired.
"""

from __future__ import annotations

import pathlib
import re
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
GAPS = ROOT / "docs" / "dogfooding-gaps.md"

# The documented vocabulary, from the file's own "Reading the markers" section.
# A marker may carry a trailing note ("[DECIDED - documented v1 stance]"), so the
# check is on the first word.
FIXED = {"FIXED"}
PARTIAL = {"HALF", "IMPROVED"}
SETTLED = {"DECIDED", "RESOLVED"}
KNOWN = FIXED | PARTIAL | SETTLED

ENTRY = re.compile(r"\*\*G(\d+)\.\s*(\[[^\]]*\])?")

# A marker written inside backticks (``**G70. `[FIXED]` ...``) does not parse as
# one, so the entry silently counts as open while reading as closed. Two entries
# sat like that for releases, and the counts they were reconciled against were
# wrong the whole time. A marker is either a real marker or it is not written.
QUOTED_MARKER = re.compile(r"\*\*G(\d+)\.\s*`\[")
COUNTS = re.compile(
    r"of (\d+) entries, (\d+) are fixed, (\d+) are partly fixed, "
    r"(\d+) are decided or resolved, and (\d+) (?:are|is) open",
)


def marker_word(marker: str) -> str:
    """`[HALF FIXED]` -> `HALF`; `[]`/absent -> `` (open)."""
    if not marker:
        return ""
    return marker.strip("[]").split()[0].upper() if marker.strip("[]").split() else ""


def main() -> int:
    if not GAPS.exists():
        print(f"missing {GAPS.relative_to(ROOT)}")
        return 1
    text = GAPS.read_text()

    # First definition of a G number wins; a later bare mention is a reference,
    # not a redefinition. A genuine double-definition with a different marker is
    # the ambiguity worth failing on.
    status: dict[int, str] = {}
    bad: list[str] = []

    for num in QUOTED_MARKER.findall(text):
        bad.append(
            f"G{num}: its status marker is inside backticks, so it does not "
            f"count. Write it as `**G{num}. [FIXED] ...`"
        )
    for num, marker in ENTRY.findall(text):
        n = int(num)
        word = marker_word(marker or "")
        if word and word not in KNOWN:
            bad.append(f"G{n} has unknown marker {marker}")
            continue
        if n in status and status[n] != word:
            bad.append(f"G{n} defined twice with different status: {status[n] or 'open'} vs {word or 'open'}")
        status.setdefault(n, word)

    if bad:
        print("gap marker problems:")
        for b in bad:
            print(f"  {b}")
        print(f"allowed markers: {', '.join('[' + m + ' ...]' for m in sorted(KNOWN))}, or none for open.")
        return 1

    total = len(status)
    fixed = sum(1 for w in status.values() if w in FIXED)
    partly = sum(1 for w in status.values() if w in PARTIAL)
    settled = sum(1 for w in status.values() if w in SETTLED)
    openn = sum(1 for w in status.values() if not w)

    # The sentence wraps across lines in the markdown, so match on a
    # whitespace-collapsed copy.
    m = COUNTS.search(" ".join(text.split()))
    if not m:
        print("no reconciled-count sentence found in the header.")
        print("add one reading: of N entries, N are fixed, N are partly fixed, N are decided or resolved, and N are open")
        return 1

    claimed = tuple(int(g) for g in m.groups())
    actual = (total, fixed, partly, settled, openn)
    if claimed != actual:
        labels = ("entries", "fixed", "partly fixed", "decided/resolved", "open")
        print("the header's counts no longer match the entries:")
        for label, c, a in zip(labels, claimed, actual):
            flag = "" if c == a else "   <-- "
            print(f"  {label:18} header says {c:3}, entries say {a:3}{flag}")
        print("\nupdate the sentence in 'Reading the markers' to match, and mark any gap this")
        print("release closed with [FIXED] plus a note saying what closed it.")
        return 1

    print(
        f"gap list OK: {total} entries, {fixed} fixed, {partly} partly fixed, "
        f"{settled} decided/resolved, {openn} open."
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
