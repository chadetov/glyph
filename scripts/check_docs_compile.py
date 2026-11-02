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

Some snippets legitimately cannot be compiled here, and each marker's claim is
checked, so none of them becomes a blanket way out:

  expect-error E0228   the snippet is meant to be broken, and it names the code
                       it draws. It has to fail, and it has to fail with that
                       code; one that compiles, or draws another code, is
                       reported. The code is required: "this is wrong somehow"
                       is not a fact a reader can act on.
  needs-react          the snippet is a whole module that needs React and
                       nothing else outside `std/`: a `component`, which emits
                       a React import of its own, or an import of `react` or
                       `react-dom`. It is compiled in a project whose
                       `node_modules` is the todo app's (React 18 and
                       `@types/react`, from that app's lockfile) and has to
                       pass `tsc --strict` there. A missing `node_modules` is
                       a failure, never a skip. It may also import a sibling
                       module that an earlier `needs-react` fence in the same
                       document declares (`module app`, then a fence with
                       `import app { App }`): that fence is written into the
                       same project, so a page's `app` and `main` modules are
                       compiled together, as the page builds them. A sibling
                       no earlier fence declares is still `needs-deps`.
  needs-deps           the snippet needs something no gate project supplies: an
                       npm package other than React, a sibling module no
                       earlier `needs-react` fence declares, or an ambient
                       `.d.ts`. It is skipped, so it has to actually
                       need one: a snippet whose only outside need is React is
                       told to use `needs-react` instead.
  example=NAME         a complete program, compiled like any other, and named so
                       fragments can point at it.
  fragment-of=NAME     an excerpt of the `example=NAME` in the same document.
                       Every line of it has to appear, in order, in that
                       example, so the excerpt cannot drift away from the
                       program that proves it compiles.

  markdown   ```glyph expect-error E0228
  html       <pre data-check="expect-error E0228">

Four documents are held to a stricter rule: `AGENTS.md`, its two mirrors, and
`docs/reference/stdlib.md`. These are what `glyph llms` prints and what an agent
reads before writing Glyph, so an unverified line in them is a claim nothing
checks. Every fence in them is compiled or carries a marker; a bare fragment is
an error rather than a skip. The rest of the docs still skip fragments.

Hard-fails (exit 1) when a checked snippet does not compile, when a marker's
claim is false, when a strict document carries an unmarked fragment, and prints
the compiler's own diagnostic against the source file and line the snippet came
from.
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
EXPECT_ERROR = re.compile(r"\bexpect-error(?:[=\s]+(E\d{4}))?")
NEEDS_DEPS = re.compile(r"\bneeds-deps\b")
NEEDS_REACT = re.compile(r"\bneeds-react\b")
EXAMPLE = re.compile(r"\bexample=([\w-]+)")
FRAGMENT_OF = re.compile(r"\bfragment-of=([\w-]+)")
IMPORT_PATH = re.compile(r"^\s*import\s+([\w/.@-]+)", re.M)

# The documents an agent reads before writing Glyph. An unmarked fragment in one
# of these is an error, not a skip.
STRICT = ("AGENTS.md", "llms.txt", "web/llms.txt", "docs/reference/stdlib.md")


def marker(info: str) -> tuple[str, str]:
    """-> (kind, argument). The argument is the code for `expect-error` and the
    example name for `example=` / `fragment-of=`."""
    m = EXPECT_ERROR.search(info)
    if m:
        return "expect-error", m.group(1) or ""
    if NEEDS_REACT.search(info):
        return "needs-react", ""
    if NEEDS_DEPS.search(info):
        return "needs-deps", ""
    m = EXAMPLE.search(info)
    if m:
        return "example", m.group(1)
    m = FRAGMENT_OF.search(info)
    if m:
        return "fragment-of", m.group(1)
    return "", ""


def excerpt_lines(body: str) -> list[str]:
    """The lines of a fragment that have to appear in its example: everything
    but blanks and lines that are only a comment, which are the elision the
    excerpt is allowed to write for itself."""
    out = []
    for line in body.splitlines():
        s = line.strip()
        if s and not s.startswith("//"):
            out.append(s)
    return out


def is_excerpt_of(fragment: str, example: str) -> str:
    """`""` when every line of the fragment appears in the example in order,
    else the first line that does not."""
    have = [l.strip() for l in example.splitlines()]
    at = 0
    for want in excerpt_lines(fragment):
        while at < len(have) and have[at] != want:
            at += 1
        if at == len(have):
            return want
        at += 1
    return ""


COMPONENT = re.compile(r"^\s*(pub\s+)?component\s+\w", re.M)


def needs_outside_deps(body: str) -> bool:
    """Whether an empty project really cannot supply what this snippet needs.

    An import of anything but `std/` is the obvious case. A `component` is the
    other one: it emits a React import the snippet never wrote, so a JSX example
    needs `react` installed however few imports it has.
    """
    if COMPONENT.search(body):
        return True
    return any(not m.startswith("std/") for m in IMPORT_PATH.findall(body))


# The packages the `needs-react` project has installed, as Glyph import paths.
REACT_PACKAGES = re.compile(r"^(react|react-dom)(/[\w/.-]+)?$")
REACT_MODULES = ROOT / "examples" / "apps" / "todo" / "node_modules"


def needs_only_react(body: str, siblings: dict[str, str] | None = None) -> bool:
    """Whether React, plus sibling modules earlier `needs-react` fences in the
    same document declare, is all this snippet needs from outside `std/`. That
    is what the `needs-react` project supplies."""
    siblings = siblings or {}
    outside = [m for m in IMPORT_PATH.findall(body) if not m.startswith("std/")]
    if not outside and not COMPONENT.search(body):
        return False
    return all(REACT_PACKAGES.match(m) or m in siblings for m in outside)


def sibling_closure(body: str, siblings: dict[str, str]) -> dict[str, str]:
    """The sibling modules a fence imports, and the ones those import in turn,
    from the `needs-react` fences declared earlier in its document."""
    out: dict[str, str] = {}
    todo = [m for m in IMPORT_PATH.findall(body) if m in siblings]
    while todo:
        name = todo.pop()
        if name in out:
            continue
        out[name] = siblings[name]
        todo += [m for m in IMPORT_PATH.findall(siblings[name]) if m in siblings]
    return out

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


def latest_release_entry(page: pathlib.Path) -> pathlib.Path | None:
    """Write the newest release entry to a temp file so it is checked like a doc.

    The entry carrying the `latest` badge is the one describing the compiler in
    this repo. Everything below it describes an older one.
    """
    if not page.exists():
        return None
    text = page.read_text()
    start = text.find('<span class="rel-tag latest">')
    if start == -1:
        return None
    open_tag = text.rfind("<section>", 0, start)
    end = text.find("</section>", start)
    if open_tag == -1 or end == -1:
        return None
    # Inside the repo, because the reporting below prints paths relative to it.
    # target/ is gitignored, so this leaves nothing behind in the tree.
    out = ROOT / "glyph-compiler" / "target" / "glyph-latest-release-entry.html"
    out.parent.mkdir(parents=True, exist_ok=True)
    out.write_text(text[open_tag : end + len("</section>")])
    return out


def sources() -> list[pathlib.Path]:
    out = [p for p in (ROOT / "docs").rglob("*.md")]
    out += [ROOT / "README.md", ROOT / "AGENTS.md", ROOT / "llms.txt"]
    # The whole release history is excluded on purpose: a 0.1.3 entry documents
    # 0.1.3, and holding old notes to today's syntax would force us to rewrite
    # history every time the language moves.
    #
    # The newest entry is different. It documents what just shipped, and nothing
    # was checking it: the 0.1.85 notes published `tls.connect(host, 443, {
    # timeout_ms: 3000 })` when the signature takes a scalar, which is also the
    # shape that release had explicitly argued against. It went out because this
    # exclusion swallowed the one entry that describes the current compiler.
    out += [p for p in (ROOT / "web").rglob("*.html") if "versions" not in p.parts]
    latest = latest_release_entry(ROOT / "web" / "versions" / "index.html")
    if latest is not None:
        out.append(latest)
    out += [ROOT / "web" / "llms.txt"]
    return [p for p in out if p.exists()]


def snippets(path: pathlib.Path) -> tuple[list[tuple[int, str, str, str]], list[tuple[int, str]]]:
    """-> ([(line, body, marker kind, marker argument)], unmarked fragments).

    Every fence is returned. A fence with no marker that does not stand on its
    own comes back in the second list, which the caller skips or reports
    depending on the document.
    """
    text = path.read_text()
    found: list[tuple[int, str, str, str]] = []
    fragments: list[tuple[int, str]] = []

    pattern = MD_BLOCK if path.suffix in (".md", ".txt") else HTML_BLOCK
    for m in pattern.finditer(text):
        info, raw = m.group(1), m.group(2)
        body = raw if path.suffix in (".md", ".txt") else html_text(raw)
        kind, arg = marker(info)
        if not kind and not self_contained(body):
            fragments.append((line_of(text, m.start()), body))
            continue
        found.append((line_of(text, m.start()), body, kind, arg))

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


def check_react(
    glyph: pathlib.Path, body: str, siblings: dict[str, str] | None = None
) -> tuple[bool, str]:
    """Compile a whole module in a project with React and its types installed,
    with `tsc` in the loop, next to the sibling modules it imports. Passing
    means `tsc --strict passed`, not only a zero exit."""
    module = MODULE_NAME.match(body)
    name = module.group(1) if module else "snippet"
    with tempfile.TemporaryDirectory(prefix="glyph-docs-react-") as tmp:
        d = pathlib.Path(tmp)
        (d / "package.json").write_text(
            '{ "name": "docsnippet-react", "private": true, "type": "module", "glyph": {} }\n'
        )
        (d / "node_modules").symlink_to(REACT_MODULES, target_is_directory=True)
        for sib_name, sib_body in (siblings or {}).items():
            (d / f"{sib_name}.glyph").write_text(
                sib_body if sib_body.endswith("\n") else sib_body + "\n"
            )
        (d / f"{name}.glyph").write_text(body if body.endswith("\n") else body + "\n")
        r = subprocess.run([str(glyph), "check", "."], capture_output=True, text=True, cwd=d)
        out = (r.stdout + r.stderr).strip()
        return r.returncode == 0 and "tsc --strict passed" in out, out


MODULE_NAME = re.compile(r"\s*module\s+([\w/]+)")


def main() -> int:
    glyph = glyph_bin.resolve()
    react_ready = (REACT_MODULES / "@types" / "react").is_dir() and (
        REACT_MODULES / "react"
    ).is_dir()
    reacted = 0

    checked = failed = fragments = opted = 0
    problems: list[str] = []
    coverage: list[str] = []

    for path in sorted(sources()):
        rel = str(path.relative_to(ROOT))
        strict = rel in STRICT
        blocks, unmarked = snippets(path)
        fragments += len(unmarked)
        examples = {arg: body for _, body, kind, arg in blocks if kind == "example"}
        # Modules declared by this document's `needs-react` fences so far, which
        # a later `needs-react` fence may import as siblings.
        react_siblings: dict[str, str] = {}

        for line, _ in unmarked:
            if not strict:
                continue
            failed += 1
            problems.append(
                f"{rel}:{line} — a fragment in a document an agent reads before writing Glyph.\n"
                f"    Make it a whole module, or mark it `fragment-of=NAME` and point it at an\n"
                f"    `example=NAME` fence in this same file."
            )

        for line, body, mark, arg in blocks:
            where = f"{rel}:{line}"

            if mark == "needs-deps":
                opted += 1
                if not needs_outside_deps(body):
                    failed += 1
                    problems.append(
                        f"{where} — marked `needs-deps`, but it imports nothing outside `std/`\n"
                        f"    and declares no `component`. Drop the marker: this snippet compiles here."
                    )
                elif needs_only_react(body, react_siblings):
                    failed += 1
                    problems.append(
                        f"{where} — marked `needs-deps`, but React, and siblings earlier\n"
                        f"    `needs-react` fences in this file declare, are all it needs from outside\n"
                        f"    `std/`, and the gate has React installed. Make it a whole module and\n"
                        f"    mark it `needs-react`, so it is compiled rather than skipped."
                    )
                continue

            if mark == "needs-react":
                own = MODULE_NAME.match(body)
                siblings = sibling_closure(body, react_siblings)
                if own:
                    react_siblings[own.group(1)] = body
                if not self_contained(body):
                    failed += 1
                    problems.append(
                        f"{where} — marked `needs-react`, but it is not a whole module, so there\n"
                        f"    is nothing to compile. Give it a `module` line."
                    )
                    continue
                if not needs_only_react(body, siblings):
                    failed += 1
                    problems.append(
                        f"{where} — marked `needs-react`, but "
                        + (
                            "it needs nothing outside `std/`. Drop the marker."
                            if not needs_outside_deps(body)
                            else "it needs more than React from outside `std/`, and\n"
                            "    no earlier `needs-react` fence in this file declares the rest.\n"
                            "    Declare the sibling in a fence above it, or mark it `needs-deps`."
                        )
                    )
                    continue
                if not react_ready:
                    failed += 1
                    problems.append(
                        f"{where} — marked `needs-react`, and {REACT_MODULES.relative_to(ROOT)}\n"
                        f"    has no react or @types/react to compile it against. Run `npm ci` in\n"
                        f"    examples/apps/todo first; this is a failure, not a skip."
                    )
                    continue
                checked += 1
                reacted += 1
                ok, output = check_react(glyph, body, siblings)
                if not ok:
                    failed += 1
                    problems.append(
                        f"{where} — snippet does not pass `tsc --strict` against React\n{indent(output)}"
                    )
                continue

            if mark == "fragment-of":
                opted += 1
                if arg not in examples:
                    failed += 1
                    problems.append(
                        f"{where} — marked `fragment-of={arg}`, and this file has no\n"
                        f"    ```glyph example={arg} fence for it to be a fragment of."
                    )
                    continue
                stray = is_excerpt_of(body, examples[arg])
                if stray:
                    failed += 1
                    problems.append(
                        f"{where} — marked `fragment-of={arg}`, but this line is not in that\n"
                        f"    example, so nothing compiles it:\n        {stray}"
                    )
                continue

            checked += 1
            ok, output = check(glyph, body)

            if mark == "expect-error":
                if not arg:
                    failed += 1
                    problems.append(
                        f"{where} — marked `expect-error` with no code.\n"
                        f"    Name the code it draws (```glyph expect-error E0228)."
                    )
                elif ok:
                    failed += 1
                    problems.append(
                        f"{where} — marked `expect-error {arg}`, but it compiles.\n"
                        f"    Drop the marker, or make the snippet show the error it claims."
                    )
                elif arg not in output:
                    failed += 1
                    problems.append(
                        f"{where} — marked `expect-error {arg}`, and it draws something else:\n"
                        f"{indent(output)}"
                    )
                continue

            if not ok:
                failed += 1
                problems.append(f"{where} — snippet does not compile\n{indent(output)}")

        if strict:
            marked = sum(1 for _, _, k, _ in blocks if k in ("needs-deps", "fragment-of"))
            coverage.append(
                f"  {rel}: {len(blocks) + len(unmarked)} fences, "
                f"{len(blocks) - marked} compiled, {marked} marked, {len(unmarked)} skipped"
            )

    for p in problems:
        print(p)
        print()

    for line in coverage:
        print(line)

    total = checked + fragments + opted
    print(
        f"docs snippets: {checked} checked ({reacted} against React), {failed} failed, "
        f"{fragments} fragments skipped, {opted} need deps or are excerpts "
        f"({total} blocks seen)."
    )
    if failed:
        print()
        print("mark a deliberately broken snippet `expect-error <CODE>`, a whole module that")
        print("needs only React (and siblings declared by `needs-react` fences above it)")
        print("`needs-react`, one that imports another npm package or an undeclared sibling")
        print("`needs-deps`, and an excerpt of a whole program `fragment-of=NAME`.")
        print("Every claim is checked.")
        return 1
    return 0


def indent(s: str) -> str:
    return "\n".join("    " + line for line in s.splitlines())


if __name__ == "__main__":
    sys.exit(main())
