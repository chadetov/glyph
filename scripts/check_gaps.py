#!/usr/bin/env python3
"""Backlog discipline: every gap in docs/dogfooding-gaps.md carries a status, and
the count in the header matches the entries.

The gap list grows one dogfooding trip at a time, and the status markers fell
behind the source once already: entries stayed unmarked long after the release
that closed them, so working out what was actually left meant re-reading all 59
against the compiler. This keeps that from recurring.

Open entries also carry evidence, not just a description. A gap is a claim about
a compiler that keeps moving, and the claims rot: one reconciliation pass found
five of ten entries already fixed, closable with no code change, or resting on a
premise that had stopped being true. A release spent implementing a fix for a gap
that closed two releases ago is a release spent on nothing. So every open entry
records the version it was last reproduced against, and the stamp has to be
recent:

    *Reproduced against 0.1.68.*

Hard-fails (exit 1) when:
  - a `G` entry carries a marker that is not one of the documented set,
  - the same `G` number is defined twice with different markers,
  - the header's reconciled counts disagree with the entries,
  - an open entry has no `Reproduced against <version>` stamp,
  - that stamp is more than STALE_AFTER patch releases behind the current one.

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

# An open entry says when it was last reproduced. Beyond this many patch
# releases the evidence is too old to act on without re-checking.
STALE_AFTER = 5
REPRODUCED = re.compile(r"Reproduced against (\d+)\.(\d+)\.(\d+)")
PACKAGE = ROOT / "npm" / "glyph" / "package.json"

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


def current_version() -> tuple[int, int, int]:
    import json

    v = json.loads(PACKAGE.read_text())["version"]
    a, b, c = v.split(".")
    return int(a), int(b), int(c)


def entry_blocks(text: str) -> dict[int, str]:
    """G number -> the prose of its first definition."""
    hits = [(m.start(), int(m.group(1))) for m in ENTRY.finditer(text)]
    blocks: dict[int, str] = {}
    for i, (pos, n) in enumerate(hits):
        if n in blocks:
            continue
        end = hits[i + 1][0] if i + 1 < len(hits) else len(text)
        blocks[n] = text[pos:end]
    return blocks


def check_evidence(text: str, open_numbers: list[int]) -> list[str]:
    cur = current_version()
    floor = (cur[0], cur[1], max(0, cur[2] - STALE_AFTER))
    blocks = entry_blocks(text)
    out: list[str] = []

    for n in sorted(open_numbers):
        m = REPRODUCED.search(blocks.get(n, ""))
        if not m:
            out.append(
                f"G{n} is open with no evidence. Reproduce it against the compiler "
                f"you just built and add: *Reproduced against "
                f"{cur[0]}.{cur[1]}.{cur[2]}.*"
            )
            continue
        stamp = tuple(int(g) for g in m.groups())
        if stamp < floor:
            out.append(
                f"G{n} was last reproduced against {stamp[0]}.{stamp[1]}.{stamp[2]}, "
                f"more than {STALE_AFTER} releases ago. Re-check it before acting on it."
            )
    return out


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

    stale = check_evidence(text, [n for n, w in status.items() if not w])
    if stale:
        print("open gaps whose evidence is missing or too old:")
        for s in stale:
            print(f"  {s}")
        print()
        print("A gap is a claim about a compiler that keeps moving. Reproducing it is")
        print("cheap; implementing a fix for one that already closed is not.")
        return 1

    print(
        f"gap list OK: {total} entries, {fixed} fixed, {partly} partly fixed, "
        f"{settled} decided/resolved, {openn} open (each reproduced recently)."
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
