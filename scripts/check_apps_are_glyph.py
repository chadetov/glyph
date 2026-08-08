#!/usr/bin/env python3
"""No application under examples/apps/ may contain TypeScript.

Glyph's whole claim is that you write Glyph. An app that needs a hand-written
`.d.ts` to reach a socket, or an `extern_ts` string to reach a timer, is an app
that needs the language Glyph is supposed to replace, and every such line is a
line the Glyph type checker does not see.

Two apps did need both, and both were fixed by making the stdlib cover the
gap (`std/timers`, `std/websocket`) and by shipping declarations for the Node
builtins a program actually imports. This check keeps that from quietly
reverting: the next time an app reaches for TypeScript, the honest response is
to extend the stdlib, not to add a declaration file.

Hard-fails (exit 1) when an app contains:
  - a `.d.ts` file, or any `.ts`/`.js` source, or
  - an `extern_ts(...)` escape in a `.glyph` file.

`examples/.types/` is deliberately out of scope. It stubs `react` and a fake
`api/users` for the numbered examples, standing in for npm packages a real
project installs and gets types from. That is a fixture, not a language gap.
"""

from __future__ import annotations

import pathlib
import re
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
APPS = ROOT / "examples" / "apps"

EXTERN = re.compile(r"\bextern_ts\s*\(")


def main() -> int:
    if not APPS.is_dir():
        print(f"missing {APPS.relative_to(ROOT)}")
        return 1

    problems: list[str] = []

    for path in sorted(APPS.rglob("*")):
        if not path.is_file():
            continue
        rel = path.relative_to(ROOT)
        if path.suffix in {".ts", ".js", ".mjs", ".cjs"} or path.name.endswith(".d.ts"):
            problems.append(
                f"{rel}: an app must not carry TypeScript. If this is reaching for a "
                f"host capability, add it to the stdlib instead."
            )

    for path in sorted(APPS.rglob("*.glyph")):
        rel = path.relative_to(ROOT)
        for n, line in enumerate(path.read_text().splitlines(), 1):
            # The word appears in prose in a couple of file headers explaining
            # why it is *not* used; only a call is a real escape.
            if EXTERN.search(line):
                problems.append(
                    f"{rel}:{n}: `extern_ts` escape. Whatever this reaches for "
                    f"belongs in the stdlib, where the type checker can see it."
                )

    if problems:
        print("apps must be written in Glyph alone:")
        for p in problems:
            print(f"  {p}")
        return 1

    apps = sorted(d.name for d in APPS.iterdir() if d.is_dir())
    print(f"apps are pure Glyph: {len(apps)} checked ({', '.join(apps)})")
    return 0


if __name__ == "__main__":
    sys.exit(main())
