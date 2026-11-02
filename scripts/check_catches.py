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
    the failure worth being loud about. A `TS....` code means the refusal comes
    from the tsc pass `glyph build` runs on the emitted program, not from
    Glyph's own checker; the case's README entry has to say so.

The TypeScript is checked with the real `@types/node` installed, because a case
about a host handle has to import it the way a real project does
(`import type { Socket } from "net"`). The package is installed fresh on every
run. No npm, or no network, is a failure rather than a skip: a skipped case
reads exactly like a verified one in a CI log.

This is the evidence base for the pillars, so a case that cannot be demonstrated
should be deleted rather than softened.

Hard-fails (exit 1) when either half stops behaving as its case claims.
"""

from __future__ import annotations

import json
import pathlib
import re
import shutil
import subprocess
import sys
import tempfile

import glyph_bin

ROOT = pathlib.Path(__file__).resolve().parent.parent
CASES = ROOT / "catches"

FIELD = re.compile(r'^\s*(\w+)\s*=\s*"([^"]*)"\s*$', re.M)
PILLAR_ORDER = ("verifiability", "greppability", "abstraction", "diff stability")
PILLARS = set(PILLAR_ORDER)


def read_case(d: pathlib.Path) -> dict[str, str]:
    meta = dict(FIELD.findall((d / "case.toml").read_text()))
    return meta


def install_types_node(root: pathlib.Path) -> tuple[str | None, str]:
    """Install `@types/node` under `root`. Returns its version, or None and why."""
    if shutil.which("npm") is None:
        return None, "npm not found; this check installs the real @types/node."
    (root / "package.json").write_text('{"name":"catches-ts","private":true}\n')
    r = subprocess.run(
        ["npm", "install", "--no-audit", "--no-fund", "--save-dev", "@types/node@latest"],
        cwd=root, capture_output=True, text=True, timeout=600,
    )
    manifest = root / "node_modules/@types/node/package.json"
    if r.returncode != 0 or not manifest.exists():
        why = (r.stderr.strip() or r.stdout.strip())
        return None, f"could not install @types/node; this check needs the network.\n{why}"
    return json.loads(manifest.read_text()).get("version", "unknown"), ""


def tsc_accepts(ts: pathlib.Path, types_root: pathlib.Path) -> tuple[bool, str]:
    # Each case gets its own directory under the one that holds `node_modules`,
    # so the installed `@types/node` resolves without a reinstall per case.
    with tempfile.TemporaryDirectory(dir=types_root) as tmp:
        out = pathlib.Path(tmp)
        (out / "in.ts").write_text(ts.read_text())
        # A `tsc` on PATH, or the package named explicitly. Bare `npx tsc`
        # resolves to the unrelated `tsc` package, a shim deprecated since 2016,
        # which prints "This is not the tsc command you are looking for" and
        # exits non-zero. Every case then reads as "the TypeScript no longer
        # passes tsc --strict", which is a false report about the evidence this
        # script exists to check.
        tsc = [shutil.which("tsc")] if shutil.which("tsc") else ["npx", "--yes", "--package", "typescript@6", "tsc"]
        r = subprocess.run(
            tsc + [
                "--strict", "--noEmit", "--target", "es2022",
                "--moduleResolution", "bundler", "--module", "esnext",
                # TypeScript 6 includes no `@types` package unless named.
                "--types", "node",
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
    with tempfile.TemporaryDirectory() as tmp:
        types_root = pathlib.Path(tmp)
        version, why = install_types_node(types_root)
        if version is None:
            print(why)
            return 1
        return check(glyph, dirs, types_root, version)


def check(
    glyph: pathlib.Path, dirs: list[pathlib.Path], types_root: pathlib.Path, version: str
) -> int:
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

        accepted, ts_out = tsc_accepts(d / "ts.ts", types_root)
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

    # Every pillar is printed, the empty ones included. A pillar with no case is
    # a fact about the evidence, and leaving it off the line would hide it.
    by_pillar: dict[str, int] = {p: 0 for p in PILLAR_ORDER}
    for d in dirs:
        pillar = read_case(d).get("pillar", "?")
        by_pillar[pillar] = by_pillar.get(pillar, 0) + 1
    spread = ", ".join(f"{k}: {v}" for k, v in by_pillar.items())

    print(f"catches: {ok}/{len(dirs)} verified both ways ({spread}).")
    print(f"TypeScript checked with tsc --strict and @types/node {version}.")
    return 1 if problems else 0


def indent(s: str) -> str:
    return "\n".join("    " + line for line in s.splitlines()[:14])


if __name__ == "__main__":
    sys.exit(main())
