#!/usr/bin/env python3
"""Every Glyph file under examples/ is what `glyph fmt` produces.

The diff-stability pillar claims one canonical form, and for a long time the
repo's own corpus was not in it: 80 of 175 files under `examples/` differed from
the formatter, some by a collapsed parameter list, some by a missing blank line,
and nothing noticed because nothing looked. A language whose examples do not
match its formatter is making a claim its own tree contradicts.

This runs `glyph fmt --check` over `examples/` and fails on any file that would
change. It is `examples/` only, on purpose. `tests/negative/` holds programs
that are deliberately unparseable, and the parser's snapshot fixtures carry
spans the snapshots depend on; reformatting either would be a different change
with its own verification.

Hard-fails (exit 1) when a file would be reformatted, when the formatter fails
on a file, or when the tree has no `.glyph` files at all (a gate over nothing
proves nothing).
"""

from __future__ import annotations

import pathlib
import subprocess
import sys

import glyph_bin

ROOT = pathlib.Path(__file__).resolve().parent.parent
TREE = ROOT / "examples"


def main() -> int:
    glyph = glyph_bin.resolve()

    if not any(TREE.rglob("*.glyph")):
        print(f"no .glyph files under {TREE.relative_to(ROOT)}; nothing was checked")
        return 1

    proc = subprocess.run(
        [str(glyph), "fmt", "--check", str(TREE.relative_to(ROOT))],
        capture_output=True,
        text=True,
        cwd=ROOT,
    )
    out = (proc.stdout + proc.stderr).strip()
    if proc.returncode != 0:
        print("examples/ is not formatted; run `glyph fmt examples`:")
        for line in out.splitlines():
            print(f"  {line}")
        return 1

    summary = out.splitlines()[-1] if out else "(no output)"
    print(f"examples/ is formatted: {summary}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
