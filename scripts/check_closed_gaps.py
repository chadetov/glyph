#!/usr/bin/env python3
"""Catch an entry that was fixed in a shipped release and never closed.

This has happened four times. G174 through G179 were fixed by 0.1.107's
documentation work and stayed open for six releases. G180, G181, G183 and G184
shipped fixed in 0.1.108 and were still listed open at 0.1.114. G100 was closed
by 0.1.109's array reductions, G186 through G188 by 0.1.110, G190 through G195
and G198 and G199 by 0.1.110 to 0.1.112. Thirteen entries in one sweep.

The damage is not tidiness. `check_gaps.py` blocks a release when an open entry's
evidence goes stale, so a fixed-but-open entry costs a reproduction every five
releases, forever, for something that cannot fail. Worse, the open count is the
number this project reports about itself, and it was overstating by thirteen.

What this checks is narrow on purpose, because the general question is
undecidable. An open entry whose text names a release at or before the current
one as having fixed it is almost certainly closed and unmarked. That phrasing
appears when somebody writes the fix up in the entry and forgets the marker,
which is exactly the failure mode.

It cannot catch an entry that was fixed silently, and it says so rather than
implying the list is now trustworthy.
"""
import pathlib
import re
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
GAPS = ROOT / "docs" / "dogfooding-gaps.md"
CARGO = ROOT / "glyph-compiler" / "Cargo.toml"

ENTRY = re.compile(r"- \*\*G(\d+)\.\s*(\[[^\]]*\])?")
# "Closed by 0.1.109", "Fixed in 0.1.110", "closed in 0.1.112"
CLAIMS_FIX = re.compile(r"\b(?:closed|fixed)\s+(?:by|in)\s+(\d+)\.(\d+)\.(\d+)", re.I)


def current() -> tuple[int, int, int]:
    m = re.search(r'^version = "(\d+)\.(\d+)\.(\d+)"', CARGO.read_text(), re.M)
    if not m:
        sys.exit("could not read the workspace version")
    return tuple(int(g) for g in m.groups())


def blocks(text: str) -> dict[int, tuple[str, str]]:
    out, hits = {}, list(ENTRY.finditer(text))
    for i, m in enumerate(hits):
        end = hits[i + 1].start() if i + 1 < len(hits) else len(text)
        out[int(m.group(1))] = (m.group(2) or "", text[m.start():end])
    return out


def main() -> int:
    if not GAPS.exists():
        print(f"missing {GAPS.relative_to(ROOT)}")
        return 1
    cur = current()
    bad = []
    for n, (marker, body) in sorted(blocks(GAPS.read_text()).items()):
        if marker:                      # already carries [FIXED], [HALF FIXED], ...
            continue
        m = CLAIMS_FIX.search(body)
        if not m:
            continue
        ver = tuple(int(g) for g in m.groups())
        if ver <= cur:
            bad.append((n, ".".join(str(v) for v in ver), m.group(0)))

    if bad:
        print("entries that say they were fixed and carry no marker:")
        for n, ver, phrase in bad:
            print(f"  G{n} says \"{phrase}\" and {ver} has shipped, but the entry is still open")
        print()
        print("Mark it, with a note saying what closed it and how you confirmed it.")
        print("An entry that is fixed and open costs a reproduction every five")
        print("releases forever, and overstates the count this project reports")
        print("about itself.")
        return 1

    print("closed-gap check OK: no open entry claims a shipped release fixed it.")
    print("This cannot see an entry fixed without saying so; it catches the")
    print("common case, not every case.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
