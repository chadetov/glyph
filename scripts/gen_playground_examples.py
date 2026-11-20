#!/usr/bin/env python3
"""Write the playground's example picker from the files it shows.

The picker offers the `catches/` cases and a few plain programs. Their source is
read from disk here, at build time, so the page cannot show a version of a case
that `scripts/check_catches.py` no longer checks: there is no second copy to
drift. `playground/build.sh` runs this before building the wasm, and the output,
`playground/examples.js`, is generated and gitignored.

Which cases go in is decided by a rule rather than a list. The playground runs
Glyph's own checker on one module and never runs `tsc`, so a case is shown when

  - it is a refusal: its `kind` is missing or `refused` (a warning, a boundary
    check, a run-time stop or a known gap is not something the page can show
    being refused), and the same rule `glyph-cli/build.rs` applies,
  - its `case.toml` names an `E....` code (a `TS....` code is a refusal from the
    `tsc` pass `glyph build` runs, which this page does not have), and
  - its directory holds exactly one `.glyph` file (the page compiles one module).

An unknown `kind` is an error, as it is in the compiler build.

Whether each included case really reproduces in the playground is checked by
a test in `crates/glyph-wasm`, which applies the same rule and compiles every
included case through the same `compile` the page calls.

Each case carries what the page shows beside the Glyph: its one-sentence
`why`, the `// show` excerpt of `ts.ts` (the same excerpt /catches/ shows, read
with the same function), the command and output `observed.json` recorded when
`scripts/check_catches.py` ran the TypeScript, and `ts_verdict`. These are
recorded facts about the case; the page runs neither `tsc` nor the program.
The Glyph in the editor is the whole `glyph.glyph`, with the `// show` and
`// end show` marker lines taken out (they are comments, so the program is the
one the test compiles).

The page's highlighter uses the keyword sets `scripts/site_highlight.py` reads
(the Glyph ones from the lexer's KEYWORDS table), written out here so the page
and the rest of the site colour code from one list.

The plain programs live in `playground/examples/`, one `.glyph` file each, and
`PROGRAMS` below gives each its label. A file with no entry, or an entry with no
file, is an error.

Usage: python3 scripts/gen_playground_examples.py
"""

from __future__ import annotations

import json
import pathlib
import sys
import tomllib

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))
import site_catches  # noqa: E402  (the /catches/ excerpt rule, shared)
import site_highlight  # noqa: E402  (the site's keyword sets, shared)

ROOT = pathlib.Path(__file__).resolve().parent.parent
CATCHES = ROOT / "catches"
PROGRAMS_DIR = ROOT / "playground" / "examples"
OUT = ROOT / "playground" / "examples.js"

# The program the page opens with when the URL names none. It has to be a case
# the picker includes, so the first thing a visitor sees is a refusal.
DEFAULT = "non-exhaustive-match"

# slug -> (picker label, one-line caption). Order is picker order.
PROGRAMS = {
    "ship": ("Ship an order", "Whether to ship an order, given where its payment stands."),
    "pricing": ("A tagged union", "A union with payloads and an exhaustive `match`."),
    "hello": ("Hello", "The program `glyph init` scaffolds."),
}


KINDS = ("refused", "warned", "boundary", "runtime", "gap")


def playground_case(d: pathlib.Path, meta: dict) -> bool:
    kind = meta.get("kind", "refused")
    if kind not in KINDS:
        raise SystemExit(f"catches/{d.name}/case.toml: unknown kind {kind!r}; it is one of {', '.join(KINDS)}")
    return (
        kind == "refused"
        and meta.get("code", "").startswith("E")
        and len(list(d.glob("*.glyph"))) == 1
    )


def editor_source(text: str) -> str:
    """A case's Glyph as the editor shows it: the `// show` marker lines taken
    out, and the blank lines they leave behind folded to one."""
    marks = {site_catches.MARK_OPEN, site_catches.MARK_CLOSE}
    out: list[str] = []
    for line in text.splitlines():
        if line.strip() in marks:
            continue
        if not line.strip() and (not out or not out[-1].strip()):
            continue
        out.append(line)
    while out and not out[-1].strip():
        out.pop()
    return "\n".join(out) + "\n"


def build() -> str:
    shown: list[dict] = []
    cases = 0
    for d in sorted(p for p in CATCHES.iterdir() if p.is_dir()):
        meta = tomllib.loads((d / "case.toml").read_text())
        if "section" in meta:
            cases += 1
        if not playground_case(d, meta):
            continue
        observed = json.loads((d / "observed.json").read_text())
        try:
            ts_code = site_catches.excerpt(d / "ts.ts")
        except site_catches.CatchesError as e:
            raise SystemExit(str(e)) from None
        shown.append(
            {
                "slug": d.name,
                "kind": "catch",
                "label": meta.get("title", d.name),
                "title": meta.get("title", ""),
                "code": meta["code"],
                "why": meta.get("why", ""),
                "ts_code": ts_code,
                "ts_command": observed["ts"]["command"],
                "ts_output": observed["ts"]["output"],
                "ts_verdict": meta.get("ts_verdict", ""),
                "source": editor_source((d / "glyph.glyph").read_text()),
            }
        )

    if DEFAULT not in {e["slug"] for e in shown}:
        raise SystemExit(
            f"the playground's default program `{DEFAULT}` is not a case the picker shows; "
            "it must be a catches/ case with an E code and one .glyph file"
        )
    # The default goes first, so the picker opens on it.
    shown.sort(key=lambda e: e["slug"] != DEFAULT)

    on_disk = {p.stem for p in PROGRAMS_DIR.glob("*.glyph")}
    if on_disk != set(PROGRAMS):
        raise SystemExit(
            f"playground/examples/ holds {sorted(on_disk)} but PROGRAMS lists {sorted(PROGRAMS)}"
        )
    for slug, (label, caption) in PROGRAMS.items():
        shown.append(
            {
                "slug": slug,
                "kind": "program",
                "label": label,
                "title": caption,
                "source": (PROGRAMS_DIR / f"{slug}.glyph").read_text(),
            }
        )

    words = {
        "glyph": sorted(site_highlight.GLYPH_KEYWORDS),
        "ts": sorted(site_highlight.TS_KEYWORDS),
        "primitives": sorted(site_highlight.PRIMITIVES),
    }
    return (
        "// Generated by scripts/gen_playground_examples.py from catches/ and\n"
        "// playground/examples/. Do not edit; run playground/build.sh.\n"
        f"export const DEFAULT_EXAMPLE = {json.dumps(DEFAULT)};\n"
        f"export const CASE_COUNT = {cases};\n"
        f"export const KEYWORDS = {json.dumps(words)};\n"
        f"export const EXAMPLES = {json.dumps(shown, indent=2)};\n"
    )


def main() -> int:
    OUT.write_text(build())
    print(f"wrote {OUT.relative_to(ROOT)}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
