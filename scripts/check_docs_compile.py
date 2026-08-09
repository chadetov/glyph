#!/usr/bin/env python3
"""Every self-contained Glyph snippet in the docs and on the site compiles.

Prose rots in two directions and only one of them is obvious. The visible
direction is a snippet that stops compiling because the language moved under it.
The quiet direction cost more: a snippet that looks wrong to a reader who then
"fixes" correct documentation. Both are answered by compiling the thing instead
of reading it.

A snippet is checked when it stands on its own: it opens with a `module` line and
declares something. Fragments (a lone `fn`, a type, three lines of a `match`, a
`module` header shown with nothing but its imports) are counted and skipped,
because wrapping them in a synthetic module would check a program nobody wrote.
An imports-only module is a fragment by the language's own rule, D15: it is
E0102, not a program. Coverage is printed so the skipped share stays visible.

Two snippets legitimately cannot be compiled here, and each has a marker whose
claim this script then checks, so neither becomes a blanket way out:

  expect-error   the snippet is meant to be broken (the E0200 example). It has
                 to actually fail; one that compiles is reported.
  needs-deps     the snippet imports something an empty project cannot supply,
                 an npm package or a sibling module. It has to actually import
                 a non-`std/` module; one that does not is reported.

  markdown   ```glyph expect-error
  html       <pre data-check="expect-error">

Hard-fails (exit 1) when a checked snippet does not compile, when a marker's
claim is false, and prints the compiler's own diagnostic against the source file
and line the snippet came from.
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

# ```glyph, ```glyph expect-error, ```glyph @run, ...
MD_BLOCK = re.compile(r"```glyph([^\n]*)\n(.*?)```", re.S)
HTML_BLOCK = re.compile(r"<pre([^>]*)>(.*?)</pre>", re.S)
TAG = re.compile(r"<[^>]+>")
# The site renders one source line per `<span class="ln">` with no newline in the
# markup, so stripping tags naively collapses a whole module onto line 1.
LINE_SPAN = re.compile(r'<span class="ln">|<br\s*/?>')
EXPECT_ERROR = re.compile(r"\bexpect-error\b")
NEEDS_DEPS = re.compile(r"\bneeds-deps\b")
IMPORT_PATH = re.compile(r"^\s*import\s+([\w/.@-]+)", re.M)


def marker(info: str) -> str:
    if EXPECT_ERROR.search(info):
        return "expect-error"
    if NEEDS_DEPS.search(info):
        return "needs-deps"
    return ""


def imports_outside_std(body: str) -> bool:
    return any(not m.startswith("std/") for m in IMPORT_PATH.findall(body))

MODULE_HEAD = re.compile(r"\s*module\s+\w")
IMPORT_OR_NOISE = re.compile(r"\s*(import\b|//|$)")


def self_contained(body: str) -> bool:
    """A module header plus at least one declaration. Imports alone are E0102."""
    if not MODULE_HEAD.match(body):
        return False
    lines = body.splitlines()
    return any(
        not IMPORT_OR_NOISE.match(line)
        for line in lines[1:]
        if not MODULE_HEAD.match(line)
    )


def html_text(raw: str) -> str:
    return html.unescape(TAG.sub("", LINE_SPAN.sub("\n", raw))).lstrip("\n")


def line_of(text: str, index: int) -> int:
    return text.count("\n", 0, index) + 1


def sources() -> list[pathlib.Path]:
    out = [p for p in (ROOT / "docs").rglob("*.md")]
    out += [ROOT / "README.md", ROOT / "AGENTS.md", ROOT / "llms.txt"]
    out += [p for p in (ROOT / "web").rglob("*.html") if "versions" not in p.parts]
    out += [ROOT / "web" / "llms.txt"]
    return [p for p in out if p.exists()]


def snippets(path: pathlib.Path) -> tuple[list[tuple[int, str, str]], int]:
    """-> ([(line, body, marker)], fragments_skipped)."""
    text = path.read_text()
    found: list[tuple[int, str, str]] = []
    fragments = 0

    pattern = MD_BLOCK if path.suffix in (".md", ".txt") else HTML_BLOCK
    for m in pattern.finditer(text):
        info, raw = m.group(1), m.group(2)
        body = raw if path.suffix in (".md", ".txt") else html_text(raw)
        if not self_contained(body):
            fragments += 1
            continue
        found.append((line_of(text, m.start()), body, marker(info)))

    return found, fragments


def check(glyph: pathlib.Path, body: str) -> tuple[bool, str]:
    with tempfile.TemporaryDirectory() as tmp:
        d = pathlib.Path(tmp)
        (d / "package.json").write_text('{"name":"docsnippet","glyph":{}}\n')
        src = d / "src"
        src.mkdir()
        (src / "snippet.glyph").write_text(body if body.endswith("\n") else body + "\n")
        r = subprocess.run(
            [str(glyph), "check", str(src / "snippet.glyph")],
            capture_output=True,
            text=True,
            cwd=d,
        )
        return r.returncode == 0, (r.stdout + r.stderr).strip()


def main() -> int:
    glyph = glyph_bin.resolve()

    checked = failed = fragments = opted = 0
    problems: list[str] = []

    for path in sorted(sources()):
        blocks, frag = snippets(path)
        fragments += frag
        for line, body, mark in blocks:
            rel = path.relative_to(ROOT)
            where = f"{rel}:{line}"

            if mark == "needs-deps":
                opted += 1
                if not imports_outside_std(body):
                    failed += 1
                    problems.append(
                        f"{where} — marked `needs-deps`, but every import is `std/`.\n"
                        f"    Drop the marker: this snippet compiles here."
                    )
                continue

            checked += 1
            ok, output = check(glyph, body)

            if mark == "expect-error":
                if ok:
                    failed += 1
                    problems.append(
                        f"{where} — marked `expect-error`, but it compiles.\n"
                        f"    Drop the marker, or make the snippet show the error it claims."
                    )
                continue

            if not ok:
                failed += 1
                problems.append(f"{where} — snippet does not compile\n{indent(output)}")

    for p in problems:
        print(p)
        print()

    total = checked + fragments + opted
    print(
        f"docs snippets: {checked} checked, {failed} failed, "
        f"{fragments} fragments skipped, {opted} need deps ({total} blocks seen)."
    )
    if failed:
        print()
        print("mark a deliberately broken snippet `expect-error`, and one that imports an")
        print("npm package or a sibling module `needs-deps`. Both claims are checked.")
        return 1
    return 0


def indent(s: str) -> str:
    return "\n".join("    " + line for line in s.splitlines())


if __name__ == "__main__":
    sys.exit(main())
