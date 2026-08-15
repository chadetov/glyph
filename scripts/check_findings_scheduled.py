#!/usr/bin/env python3
"""Every unfinished finding has to be scheduled somewhere, not just recorded.

`docs/dogfooding-gaps.md` is where a finding is written down. `docs/roadmap/releases.md`
is where work is decided. Those are different documents, and a gap can sit in the
first for months without anyone deciding what to do about it: three entries
(G105, G106, G107) had been reproduced repeatedly and appeared nowhere in the
roadmap at all, and two more (G19, G20) were carrying an `[IMPROVED]` marker that
made them look handled while the underlying limitation was untouched.

So this fails when an entry that is not finished is not mentioned by number in
the roadmap. Mentioning it is a low bar on purpose: the point is that somebody
decided where it goes, not that it is scheduled for the next release. Parking it
in the rolling lane with a sentence about why counts.

"Not finished" means the marker is absent (open) or partial (`[HALF FIXED]`,
`[IMPROVED]`). `[FIXED]`, `[DECIDED]` and `[RESOLVED]` are done and need no
forward plan. The marker vocabulary and the entry regex are imported from
`check_gaps.py` rather than restated, so the two gates cannot disagree about what
an entry's status is.

Run from the repo root: python3 scripts/check_findings_scheduled.py
"""

from __future__ import annotations

import pathlib
import re
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
sys.path.insert(0, str(ROOT / "scripts"))

import check_gaps as cg  # noqa: E402  (path set above)

GAPS = ROOT / "docs" / "dogfooding-gaps.md"
ROADMAP = ROOT / "docs" / "roadmap" / "releases.md"


def statuses(text: str) -> dict[int, str]:
    """G number -> marker word, taking the first definition of each."""
    out: dict[int, str] = {}
    for m in cg.ENTRY.finditer(text):
        n = int(m.group(1))
        if n not in out:
            out[n] = cg.marker_word(m.group(2) or "")
    return out


def main() -> int:
    gaps = GAPS.read_text()
    roadmap = ROADMAP.read_text()

    done = cg.FIXED | cg.SETTLED
    unfinished = sorted(n for n, word in statuses(gaps).items() if word not in done)
    missing = [n for n in unfinished if not re.search(rf"\bG{n}\b", roadmap)]

    if missing:
        print("findings recorded but never scheduled:")
        for n in missing:
            print(f"  G{n} is open or partly fixed and is not mentioned in the roadmap.")
        print()
        print(
            "A gap nobody decided about is a gap that gets rediscovered. Add each to\n"
            "docs/roadmap/releases.md: a planned release if it is next, the rolling\n"
            "polish lane if it is not, or parked with the reason. Naming the number is\n"
            "what this checks."
        )
        return 1

    print(
        f"findings scheduled: {len(unfinished)} unfinished entr"
        f"{'y' if len(unfinished) == 1 else 'ies'}, all named in the roadmap."
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
