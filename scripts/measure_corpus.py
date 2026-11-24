#!/usr/bin/env python3
"""Run two glyph binaries over one fixed set of corpus units and diff the results.

Usage:
    python3 scripts/measure_corpus.py --old OLD_GLYPH --new NEW_GLYPH [--out DIR] [--root REPO] [--list]

A unit is one of:
  * an app directory under examples/apps/ (checked with its own directory as root);
  * a case under catches/;
  * a case under tests/exact-or-absent/ (its src/);
  * an entry under tests/negative/ (a directory, or a single .glyph file);
  * every other tracked .glyph file, on its own.

Each unit is copied into a scratch directory before it is checked, so nothing is
written into the tree and a single file never sees its neighbours. Both binaries
run `check --no-tsc --no-test --json` and `build --no-tsc --no-test --json` on
the copy. The report lists, per unit whose output differs, the exit codes and
the diagnostics each binary added or dropped, with scratch paths normalised.
The unit list is written to the output directory so two runs can be compared.
Exit status is 0 whether or not units differ; read the report.
"""

import argparse
import json
import os
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent


def tracked_glyph_files():
    out = subprocess.run(
        ["git", "ls-files", "*.glyph"], cwd=ROOT, capture_output=True, text=True, check=True
    ).stdout.split()
    return sorted(out)


def units():
    found = []
    claimed_prefixes = []

    def add(kind, rel, src):
        found.append({"id": rel, "kind": kind, "copy": rel, "src": src})

    apps = ROOT / "examples" / "apps"
    for d in sorted(p for p in apps.iterdir() if p.is_dir() and any(p.glob("*.glyph"))):
        rel = str(d.relative_to(ROOT))
        add("app", rel, ".")
        claimed_prefixes.append(rel + "/")
    for d in sorted(p for p in (ROOT / "catches").iterdir() if p.is_dir()):
        rel = str(d.relative_to(ROOT))
        add("catch", rel, ".")
        claimed_prefixes.append(rel + "/")
    for d in sorted(p for p in (ROOT / "tests" / "exact-or-absent").iterdir() if p.is_dir()):
        rel = str(d.relative_to(ROOT))
        add("exact-or-absent", rel, "src" if (d / "src").is_dir() else ".")
        claimed_prefixes.append(rel + "/")
    neg = ROOT / "tests" / "negative"
    for p in sorted(neg.iterdir()):
        rel = str(p.relative_to(ROOT))
        if p.is_dir():
            add("negative", rel, ".")
            claimed_prefixes.append(rel + "/")
        elif p.suffix == ".glyph":
            add("negative-file", rel, ".")
            claimed_prefixes.append(rel)
    for f in tracked_glyph_files():
        if any(f == c or f.startswith(c) for c in claimed_prefixes):
            continue
        add("file", f, ".")
    return found


def stage(unit, scratch):
    """Copy a unit into `scratch` and return the directory to run in and the path to check."""
    src = ROOT / unit["copy"]
    dest = scratch / "u"
    if src.is_dir():
        shutil.copytree(src, dest, ignore=shutil.ignore_patterns("node_modules", "dist", "gen", ".glyph-runtime"))
        nm = src / "node_modules"
        if nm.is_dir():
            os.symlink(nm, dest / "node_modules")
    else:
        dest.mkdir()
        shutil.copy2(src, dest / src.name)
    return dest, unit["src"]


def run(binary, cwd, args):
    p = subprocess.run([binary] + args, cwd=cwd, capture_output=True, text=True, timeout=300)
    return p.returncode, p.stdout


def diagnostics(stdout, scratch):
    try:
        data = json.loads(stdout)
    except (json.JSONDecodeError, ValueError):
        text = stdout.replace(str(scratch), "<unit>")
        return ["unparsed: " + line for line in text.splitlines() if line.strip()]
    diags = data.get("diagnostics", data if isinstance(data, list) else [])
    out = []
    for d in diags:
        if not isinstance(d, dict):
            continue
        path = str(d.get("file") or d.get("path") or "").replace(str(scratch), "<unit>")
        line = d.get("line") or (d.get("range") or {}).get("start", {}).get("line", "")
        out.append(f'{d.get("code", "?")} {d.get("severity", "")} {path}:{line} {d.get("message", "")}'.strip())
    return sorted(out)


def measure(binary, unit):
    with tempfile.TemporaryDirectory(prefix="measure-") as tmp:
        scratch = Path(tmp)
        cwd, target = stage(unit, scratch)
        c_code, c_out = run(binary, cwd, ["check", "--no-tsc", "--no-test", "--json", target])
        b_code, b_out = run(binary, cwd, ["build", target, "--out", str(scratch / "out"), "--no-tsc", "--no-test", "--json"])
        return {
            "check": c_code,
            "build": b_code,
            "check_diags": diagnostics(c_out, scratch),
            "build_diags": diagnostics(b_out, scratch),
        }


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--old", help="the earlier glyph binary")
    ap.add_argument("--new", help="the later glyph binary")
    ap.add_argument("--out", default=None, help="directory for units.txt and report.json")
    ap.add_argument("--list", action="store_true", help="print the unit list and stop")
    ap.add_argument("--root", default=None, help="repository to measure (default: the one holding this script)")
    a = ap.parse_args()
    global ROOT
    if a.root:
        ROOT = Path(a.root).resolve()

    us = units()
    if a.list:
        for u in us:
            print(u["kind"], u["id"])
        print(f"{len(us)} units", file=sys.stderr)
        return 0
    if not (a.old and a.new):
        ap.error("--old and --new are required unless --list is given")

    out = Path(a.out or tempfile.mkdtemp(prefix="measure-corpus-"))
    out.mkdir(parents=True, exist_ok=True)
    (out / "units.txt").write_text("".join(f'{u["kind"]} {u["id"]}\n' for u in us))

    report = []
    pairs = {}
    for i, u in enumerate(us, 1):
        old = measure(a.old, u)
        new = measure(a.new, u)
        pairs[(old["check"], new["check"])] = pairs.get((old["check"], new["check"]), 0) + 1
        if old != new:
            entry = {"unit": u["id"], "kind": u["kind"],
                     "check_exit": [old["check"], new["check"]],
                     "build_exit": [old["build"], new["build"]]}
            for key in ("check_diags", "build_diags"):
                o, n = set(old[key]), set(new[key])
                entry[key] = {"added": sorted(n - o), "dropped": sorted(o - n)}
            report.append(entry)
        print(f"\r{i}/{len(us)}", end="", file=sys.stderr)
    print(file=sys.stderr)

    (out / "report.json").write_text(json.dumps(report, indent=2))
    print(f"{len(us)} units; {len(report)} differ. Unit list and report in {out}")
    print("check exit pairs (old, new): " + ", ".join(f"{k}: {v}" for k, v in sorted(pairs.items())))
    for e in report:
        print(f'\n{e["unit"]} ({e["kind"]}) check {e["check_exit"][0]} -> {e["check_exit"][1]}, build {e["build_exit"][0]} -> {e["build_exit"][1]}')
        for key in ("check_diags", "build_diags"):
            for d in e[key]["added"]:
                print(f"  + [{key[:5]}] {d}")
            for d in e[key]["dropped"]:
                print(f"  - [{key[:5]}] {d}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
