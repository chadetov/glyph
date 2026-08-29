#!/usr/bin/env python3
"""An unshipped release plan has to have been re-read recently.

A plan is a set of claims about a compiler that keeps moving, and the claims rot
the same way a gap's evidence does. `check_gaps.py` already forces an open gap to
be re-reproduced every few releases; this is the same rule pointed at the other
document, because that is where it actually bit.

The 0.1.79 plan is the worked example. When work started on it, it said the npm
round "cannot be committed at all yet" and named an owner's decision as the
blocker. All of it was already done: the apps gate skipped `node_modules`,
`.gitignore` covered it, CI installed per-app dependencies, and both `gen dts`
prerequisites were fixed. It also said wrapping node's `net` would close E0304,
which was never true, since E0304 is about a record holding a field with no
runtime check and the port does not change that. Five stale claims and one wrong
one, in the section a session reads before starting work.

So every unshipped `#### 0.1.NN` plan carries `*Reviewed against X.Y.Z.*` and
this fails when that stamp is missing or older than STALE_AFTER releases.
Reviewing means re-checking the section's claims against the compiler you just
built and correcting what has changed. **Re-stamping without re-reading is the
one thing this cannot catch and the only way to make it useless.**

Two things are deliberately out of scope. The rolling polish lane is a parking
lot rather than a commitment, and its items are re-read when one is picked up;
gating ~25 of them on a clock would be noise. The long-horizon sections (the 1.0
gate, `0.2.x`, the React track) are strategy, not plans a session executes from.

Run from the repo root: python3 scripts/check_plans_fresh.py
"""

from __future__ import annotations

import pathlib
import re
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
sys.path.insert(0, str(ROOT / "scripts"))

import check_gaps as cg  # noqa: E402  (path set above)

ROADMAP = ROOT / "docs" / "roadmap" / "releases.md"

# `#### 0.1.80 — finish what is half-built`, and later `### 0.1.94 — Next · ...`.
# A shipped entry needs no forward plan, so the marker in the title is what takes
# a section out of scope.
#
# Both heading levels are matched on purpose. Plans were `####` when this gate
# was written and the roadmap later moved them to `###`, at which point the
# pattern stopped matching anything at all and the gate passed by finding zero
# plans. It reported "0 unshipped plans reviewed within 5 releases" for weeks,
# which reads like a pass and is really a silence. A gate that cannot fail is
# worse than no gate, because it occupies the slot where a real check would go.
PLAN = re.compile(r"^#{3,4} (0\.\d+\.\d+) — (.+)$", re.M)
DONE = re.compile(r"shipped|landed on main", re.I)

# A plan whose release already shipped is history, not a forward plan, and the
# marker for that lives in the `### 0.1.NN — Shipped · ...` entry rather than in
# the `#### 0.1.NN` plan's own title. Reading only the plan title meant 0.1.81's
# plan kept demanding a review stamp for a release that had already gone out, and
# the only way to satisfy it was to hand-annotate a section that says nothing
# false. Cross-referencing removes the manual step instead of documenting it.
SHIPPED = re.compile(r"^### (0\.\d+\.\d+) — Shipped", re.M)
REVIEWED = re.compile(r"Reviewed against (\d+)\.(\d+)\.(\d+)")


def sections(text: str) -> list[tuple[str, str, str]]:
    """(version, title, body) for each `#### 0.x.y` heading."""
    hits = [(m.start(), m.end(), m.group(1), m.group(2)) for m in PLAN.finditer(text)]
    out = []
    for i, (start, end, version, title) in enumerate(hits):
        stop = hits[i + 1][0] if i + 1 < len(hits) else len(text)
        out.append((version, title, text[end:stop]))
    return out


def main() -> int:
    text = ROADMAP.read_text()
    cur = cg.current_version()
    floor = (cur[0], cur[1], max(0, cur[2] - cg.STALE_AFTER))

    problems: list[str] = []
    checked = 0
    shipped = set(SHIPPED.findall(text))
    for version, title, body in sections(text):
        # Already released, so the section is history rather than a forward plan,
        # whatever its heading says. This test is the reliable one: the two
        # markers below depend on title wording, and the oldest entries predate
        # both conventions (`### 0.1.72 — a typo answers in Glyph's own voice`
        # names no status at all, and 0.1.14's title says "ship the first slice",
        # which is a promise rather than a record). Comparing versions asks the
        # question directly instead of inferring it from prose.
        if tuple(int(n) for n in version.split(".")) <= cur:
            continue
        if DONE.search(title) or version in shipped:
            continue
        checked += 1
        m = REVIEWED.search(body)
        if not m:
            problems.append(
                f"{version} is an unshipped plan with no review stamp. Re-read it "
                f"against the compiler you just built, correct what has changed, and "
                f"add: *Reviewed against {cur[0]}.{cur[1]}.{cur[2]}.*"
            )
            continue
        stamp = tuple(int(g) for g in m.groups())
        if stamp < floor:
            problems.append(
                f"{version} was last reviewed against "
                f"{stamp[0]}.{stamp[1]}.{stamp[2]}, more than {cg.STALE_AFTER} "
                f"releases ago. Re-read it before working from it."
            )

    if problems:
        print("unshipped plans whose claims have not been re-checked:")
        for p in problems:
            print(f"  {p}")
        print()
        print(
            "A plan is a set of claims about a compiler that keeps moving. Re-read the\n"
            "section, fix what is no longer true, then move the stamp. Moving the stamp\n"
            "without reading is worse than having no stamp."
        )
        return 1

    print(
        f"plans fresh: {checked} unshipped plan"
        f"{'' if checked == 1 else 's'} reviewed within {cg.STALE_AFTER} releases."
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
