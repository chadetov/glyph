#!/usr/bin/env python3
"""Every standard-library import written in AGENTS.md's prose resolves.

`check_docs_compile.py` compiles fenced blocks, so an import written inline, in
backticks inside a sentence or a recipe, was checked by nobody. "Recipes by
task" told readers to write `import std/http { serve, text, Request, Response }`
for months after `serve` was removed; the `std/net` section of the same file
said there was no `serve`, and the import was E0105 for anyone who copied it.

This reads every inline code span in `AGENTS.md` that holds an
`import std/...` line, writes it into a one-module project with an empty
`main`, and runs `glyph check --no-tsc` on it. A module the standard library
does not carry is E0104 and a name it does not export is E0105; either fails
the gate, with the compiler's diagnostic and the line the span sits on.

The `## Diagnostic codes` table is skipped. It is generated from the compiler's
catalogue (`check_llms_sync.py`), and its examples are wrong on purpose:
`import std/zzznope` is there to show what E0104 says.

`llms.txt` and `web/llms.txt` are byte-identical mirrors of `AGENTS.md`, held
to that by `check_llms_sync.py`, so checking `AGENTS.md` checks all three.

Hard-fails (exit 1) when any import does not resolve, or when the document has
no inline imports at all, which would mean this script stopped reading it.
"""

from __future__ import annotations

import pathlib
import re
import subprocess
import sys
import tempfile

import glyph_bin

ROOT = pathlib.Path(__file__).resolve().parent.parent
DOC = ROOT / "AGENTS.md"

# A backtick span on one line. Double-backtick spans do not occur around imports.
SPAN = re.compile(r"`([^`\n]+)`")
STD_IMPORT = re.compile(r"^import\s+std/[\w/]+(\s*\{[^}]*\})?$")
SKIPPED_SECTION = "## Diagnostic codes"


def inline_imports(text: str) -> list[tuple[int, str]]:
    """-> [(line, import statement)], outside fences and the skipped section."""
    out: list[tuple[int, str]] = []
    in_fence = False
    skipping = False
    for number, line in enumerate(text.splitlines(), start=1):
        if line.startswith("```"):
            in_fence = not in_fence
            continue
        if in_fence:
            continue
        if line.startswith("## "):
            skipping = line.startswith(SKIPPED_SECTION)
        if skipping:
            continue
        for m in SPAN.finditer(line):
            span = m.group(1).strip()
            if STD_IMPORT.match(span):
                out.append((number, span))
    return out


def check(glyph: pathlib.Path, statement: str) -> tuple[bool, str]:
    with tempfile.TemporaryDirectory(prefix="glyph-inline-import-") as tmp:
        d = pathlib.Path(tmp)
        (d / "package.json").write_text('{"name":"inlineimport","glyph":{}}\n')
        (d / "probe.glyph").write_text(
            "module probe\n\n"
            f"{statement}\n\n"
            "fn main(argv: Array<string>) -> number {\n"
            "  return 0\n"
            "}\n"
        )
        r = subprocess.run(
            [str(glyph), "check", "--no-tsc", "--no-test", "probe.glyph"],
            capture_output=True,
            text=True,
            cwd=d,
        )
        out = (r.stdout + r.stderr).strip()
        return r.returncode == 0, out


def main() -> int:
    glyph = glyph_bin.resolve()
    found = inline_imports(DOC.read_text())
    if not found:
        print(f"inline imports: found none in {DOC.relative_to(ROOT)}; the scan is broken.")
        return 1

    failed = 0
    for line, statement in found:
        ok, output = check(glyph, statement)
        if not ok:
            failed += 1
            print(f"{DOC.relative_to(ROOT)}:{line} — `{statement}` does not resolve")
            print("\n".join("    " + l for l in output.splitlines()))
            print()

    print(f"inline imports: {len(found)} checked in {DOC.relative_to(ROOT)}, {failed} failed.")
    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(main())
