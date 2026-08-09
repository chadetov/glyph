#!/usr/bin/env python3
"""The walkthroughs show what `glyph init` actually writes.

A first-run page is the one piece of prose where being plausible is not good
enough. `web/start/` showed a `src/main.glyph` with a bare `print` and no
import, which compiles, so nothing caught it; what the scaffold writes is
`import std/io` and `io.println`. It also said three files where there are four.
A reader following it types something the tool did not produce and starts by
distrusting the tool.

Compiling the snippet cannot catch this, because the wrong snippet compiled
fine. The only check that works is running `glyph init` and comparing.

Hard-fails (exit 1) when a page that walks through the scaffold does not quote
the generated `main.glyph` verbatim, or does not name every file `glyph init`
created.
"""

from __future__ import annotations

import html
import os
import pathlib
import re
import shutil
import subprocess
import sys
import tempfile

import glyph_bin

ROOT = pathlib.Path(__file__).resolve().parent.parent

# Pages that walk a reader through the scaffold, and what each one claims.
#
#   source  it prints the generated `src/main.glyph`, so every line has to match
#   files   it enumerates what was created, so every created file has to be named
#   output  it shows what running the scaffold prints
#
# The two pages show different things: the site walks the file, the guide runs
# it and moves on. Asserting `source` against the guide would be asserting
# against a snippet it never shows. Dropping an assertion here is a visible diff
# on a reviewed file, which is the point.
WALKTHROUGHS = {
    pathlib.Path("web/start/index.html"): {"source", "files", "output"},
    pathlib.Path("docs/guide/start-here.md"): {"files", "output"},
}

TAG = re.compile(r"<[^>]+>")
LINE_SPAN = re.compile(r'<span class="ln">|<br\s*/?>')


def scaffold(glyph: pathlib.Path) -> tuple[str, list[str], str]:
    """-> (main.glyph source, created files, what running it prints)."""
    with tempfile.TemporaryDirectory() as tmp:
        d = pathlib.Path(tmp)
        r = subprocess.run(
            [str(glyph), "init", "probe"], capture_output=True, text=True, cwd=d
        )
        if r.returncode != 0:
            print("`glyph init` failed:")
            print(r.stdout + r.stderr)
            sys.exit(1)
        proj = d / "probe"
        files = sorted(
            str(p.relative_to(proj)) for p in proj.rglob("*") if p.is_file()
        )
        source = (proj / "src" / "main.glyph").read_text()
        # What the scaffold prints, taken from the scaffold rather than from a
        # string this script also hardcodes.
        printed = ""
        for line in source.splitlines():
            m = re.search(r'io\.e?print(?:ln)?\("([^"]*)"\)', line)
            if m:
                printed = m.group(1)
                break
        return source, files, printed


def page_text(path: pathlib.Path) -> str:
    raw = (ROOT / path).read_text()
    if path.suffix == ".html":
        return html.unescape(TAG.sub("", LINE_SPAN.sub("\n", raw)))
    return raw


def main() -> int:
    glyph = glyph_bin.resolve()

    source, files, printed = scaffold(glyph)
    body = [line for line in source.splitlines() if line.strip()]
    problems: list[str] = []

    for page, claims in WALKTHROUGHS.items():
        if not (ROOT / page).exists():
            problems.append(f"{page} — listed as a walkthrough but missing")
            continue
        text = page_text(page)

        if "source" in claims:
            missing = [line for line in body if line.strip() not in text]
            if missing:
                problems.append(
                    f"{page} — does not quote what `glyph init` writes. Missing:\n"
                    + "\n".join(f"      {line}" for line in missing)
                )

        if "files" in claims:
            unnamed = [f for f in files if pathlib.PurePosixPath(f).name not in text]
            if unnamed:
                problems.append(
                    f"{page} — `glyph init` creates {len(files)} files; "
                    f"these are never named:\n"
                    + "\n".join(f"      {f}" for f in unnamed)
                )

        if "output" in claims and printed and printed not in text:
            problems.append(
                f"{page} — the scaffold prints {printed!r}; the page never shows it"
            )

    for p in problems:
        print(p)
    if problems:
        print()
        print("run `glyph init` and make the walkthrough match it, or change the")
        print("template in glyph-cli and update both together.")
        return 1

    print(
        f"scaffold docs OK: {len(WALKTHROUGHS)} walkthrough(s) match "
        f"`glyph init` ({len(files)} files, {len(body)} source lines, "
        f"prints {printed!r})."
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
