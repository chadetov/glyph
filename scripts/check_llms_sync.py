#!/usr/bin/env python3
"""The two diagnostic-code tables are written from the compiler, not by hand.

`docs/error-codes.md` and the `## Diagnostic codes` table in `AGENTS.md` used to
be two hand-maintained copies of a catalogue whose third copy was the compiler's
own `--explain` text. Nothing compared them, so they drifted: `E0304` had
`--explain` prose and no entry in one of the lists, and the wording of a dozen
rows differed between the two documents for no reason anybody chose.

The catalogue is one table in the compiler now (`CODES` in `glyph-cli`), and
`glyph llms --json` publishes it. This script renders the two markdown tables
from that JSON:

    scripts/check_llms_sync.py           # fail on any drift, printing it
    scripts/check_llms_sync.py --write   # write the tables, then re-mirror

Only the table rows are generated. Every paragraph around them is prose somebody
wrote and this leaves it alone.

`--write` also re-copies `AGENTS.md` over `llms.txt` and `web/llms.txt`, which
are byte-identical mirrors of it, so a table edit cannot leave the mirrors
behind. Run it in the release ceremony; run this without `--write` as a gate.
"""

from __future__ import annotations

import json
import pathlib
import re
import subprocess
import sys

import glyph_bin

ROOT = pathlib.Path(__file__).resolve().parent.parent
ERROR_CODES = ROOT / "docs" / "error-codes.md"
AGENTS = ROOT / "AGENTS.md"
MIRRORS = [ROOT / "llms.txt", ROOT / "web" / "llms.txt"]

# The phase sections of the catalogue, in the order the document lists them.
SECTIONS = [
    ("parser", "### Parser — `E000x`"),
    ("resolver", "### Resolver — `E01xx`"),
    ("typechecker", "### Typechecker — `E02xx`"),
    ("emitter", "### Emitter — `E03xx`"),
]


def cell(text: str) -> str:
    """A markdown table cell. The compiler holds the text with `|` written
    plainly, because two tables put it in two different columns."""
    return text.replace("|", r"\|")


def codes() -> list[dict]:
    glyph = glyph_bin.resolve()
    r = subprocess.run([str(glyph), "llms", "--json"], capture_output=True, text=True)
    if r.returncode != 0:
        sys.exit(f"`glyph llms --json` failed:\n{r.stderr.strip()}")
    return json.loads(r.stdout)["diagnostics"]["codes"]


def catalogue_table(rows: list[dict]) -> str:
    out = ["| Code | Meaning |", "|------|---------|"]
    out += [f"| `{c['code']}` | {cell(c['meaning'])} |" for c in rows]
    return "\n".join(out)


def bootstrap_table(rows: list[dict]) -> str:
    out = ["| Code | Meaning | Fix |", "|---|---|---|"]
    out += [f"| {c['code']} | {cell(c['meaning'])} | {cell(c['fix'])} |" for c in rows]
    return "\n".join(out)


TABLE = re.compile(r"^\|.*\|$\n(?:^\|.*\|$\n)*", re.M)


def replace_table_after(text: str, heading: str, table: str) -> str:
    """Replace the first markdown table that follows `heading`."""
    at = text.find(heading)
    if at == -1:
        sys.exit(f"{heading!r} is no longer in the document; the renderer needs updating")
    m = TABLE.search(text, at + len(heading))
    if m is None:
        sys.exit(f"no table follows {heading!r}")
    return text[: m.start()] + table + "\n" + text[m.end() :]


def rendered(rows: list[dict]) -> dict[pathlib.Path, str]:
    error_codes = ERROR_CODES.read_text()
    for phase, heading in SECTIONS:
        table = catalogue_table([c for c in rows if c["phase"] == phase])
        error_codes = replace_table_after(error_codes, heading, table)
    agents = replace_table_after(
        AGENTS.read_text(), "## Diagnostic codes", bootstrap_table(rows)
    )
    return {ERROR_CODES: error_codes, AGENTS: agents}


def main() -> int:
    write = "--write" in sys.argv[1:]
    rows = codes()
    want = rendered(rows)

    if write:
        for path, text in want.items():
            if path.read_text() != text:
                path.write_text(text)
                print(f"wrote {path.relative_to(ROOT)}")
        agents = AGENTS.read_text()
        for mirror in MIRRORS:
            if mirror.read_text() != agents:
                mirror.write_text(agents)
                print(f"mirrored {mirror.relative_to(ROOT)}")
        print("llms sync: tables written from `glyph llms --json`.")
        return 0

    drift: list[str] = []
    for path, text in want.items():
        if path.read_text() != text:
            drift.append(str(path.relative_to(ROOT)))
    agents = AGENTS.read_text()
    for mirror in MIRRORS:
        if mirror.read_text() != agents:
            drift.append(f"{mirror.relative_to(ROOT)} (not a byte-identical mirror of AGENTS.md)")

    if drift:
        print("the generated tables have drifted from the compiler's catalogue:")
        for d in drift:
            print(f"  {d}")
        print()
        print("run `python3 scripts/check_llms_sync.py --write` and commit the result.")
        print("if the change should go the other way, edit `CODES` in")
        print("`glyph-compiler/crates/glyph-cli/src/explain.rs`, rebuild, and run it again.")
        return 1

    print(f"llms sync: {len(rows)} codes, both tables and both mirrors match the compiler.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
