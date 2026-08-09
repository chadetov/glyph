#!/usr/bin/env python3
"""The claim "tsc --strict passes this, Glyph does not" is checked, not asserted.

`catches/` holds one directory per case: the TypeScript a strict project would
accept, the Glyph that says no, and a `case.toml` naming the diagnostic and the
pillar. Both halves are run here, so neither side can rot into a claim that was
true once:

  - `ts.ts` must type-check clean under `tsc --strict`. A case whose TypeScript
    stops compiling is not evidence of anything, it is a bug in the fixture.
  - `glyph.glyph` must fail, with the exact code the case names. A case whose
    Glyph starts compiling means the guarantee it documents is gone, which is
    the failure worth being loud about.

This is the evidence base for the pillars, so a case that cannot be demonstrated
should be deleted rather than softened.

Hard-fails (exit 1) when either half stops behaving as its case claims.
"""

from __future__ import annotations

import pathlib
import re
import subprocess
import sys
import tempfile

import glyph_bin

ROOT = pathlib.Path(__file__).resolve().parent.parent
CASES = ROOT / "catches"

FIELD = re.compile(r'^\s*(\w+)\s*=\s*"([^"]*)"\s*$', re.M)
PILLARS = {"verifiability", "greppability", "abstraction", "diff stability"}


def read_case(d: pathlib.Path) -> dict[str, str]:
    meta = dict(FIELD.findall((d / "case.toml").read_text()))
    return meta


def tsc_accepts(ts: pathlib.Path) -> tuple[bool, str]:
    with tempfile.TemporaryDirectory() as tmp:
        out = pathlib.Path(tmp)
        (out / "in.ts").write_text(ts.read_text())
        r = subprocess.run(
            [
                "npx", "tsc", "--strict", "--noEmit", "--target", "es2022",
                "--moduleResolution", "bundler", "--module", "esnext",
                str(out / "in.ts"),
            ],
            capture_output=True,
            text=True,
            cwd=out,
        )
        return r.returncode == 0, (r.stdout + r.stderr).strip()


def glyph_rejects(compiler: pathlib.Path, src: pathlib.Path) -> tuple[bool, str]:
    """Build the case's whole directory: some cases need a sibling module."""
    with tempfile.TemporaryDirectory() as tmp:
        d = pathlib.Path(tmp)
        (d / "package.json").write_text('{"name":"catch","glyph":{}}\n')
        work = d / "src"
        work.mkdir()
        for f in src.glob("*.glyph"):
            (work / f.name).write_text(f.read_text())
        r = subprocess.run(
            [str(compiler), "build", "src", "--out", str(d / "out")],
            capture_output=True,
            text=True,
            cwd=d,
        )
        return r.returncode != 0, (r.stdout + r.stderr).strip()


def main() -> int:
    if not CASES.is_dir():
        print(f"missing {CASES.relative_to(ROOT)}/")
        return 1
    glyph = glyph_bin.resolve()

    dirs = sorted(p for p in CASES.iterdir() if p.is_dir())
    problems: list[str] = []
    ok = 0

    for d in dirs:
        name = d.name
        meta = read_case(d)
        code = meta.get("code", "")
        pillar = meta.get("pillar", "")
        if not code or pillar not in PILLARS:
            problems.append(
                f"{name}: case.toml needs `code` and a `pillar` from {sorted(PILLARS)}"
            )
            continue

        accepted, ts_out = tsc_accepts(d / "ts.ts")
        if not accepted:
            problems.append(
                f"{name}: the TypeScript no longer passes `tsc --strict`, so the case "
                f"proves nothing.\n{indent(ts_out)}"
            )
            continue

        rejected, g_out = glyph_rejects(glyph, d)
        if not rejected:
            problems.append(
                f"{name}: Glyph now accepts this. The guarantee the case documents "
                f"is gone, or the case drifted.\n{indent(g_out)}"
            )
            continue
        if code not in g_out:
            problems.append(
                f"{name}: expected {code}, got a different diagnostic.\n{indent(g_out)}"
            )
            continue
        ok += 1

    for p in problems:
        print(p)
        print()

    by_pillar: dict[str, int] = {}
    for d in dirs:
        meta = read_case(d)
        by_pillar[meta.get("pillar", "?")] = by_pillar.get(meta.get("pillar", "?"), 0) + 1
    spread = ", ".join(f"{k}: {v}" for k, v in sorted(by_pillar.items()))

    print(f"catches: {ok}/{len(dirs)} verified both ways ({spread}).")
    return 1 if problems else 0


def indent(s: str) -> str:
    return "\n".join("    " + line for line in s.splitlines()[:14])


if __name__ == "__main__":
    sys.exit(main())
