#!/usr/bin/env python3
"""Move every release-carrying version string to a new version.

The release ceremony calls for sixteen edits across nine files, and doing them
by hand is the step most likely to go wrong when nobody is watching: a missed
`optionalDependencies` pin publishes a package whose platform binaries resolve
to the previous release. `check_versions.py` catches that afterwards; this
does it correctly in the first place.

It deliberately edits only the files below. A version number in prose, a
roadmap entry, a release note, or a benchmark README is a historical fact about
which release something shipped in, and rewriting those would turn the record
into a lie that reads as current.

    python3 scripts/bump_version.py 0.1.107
"""
import pathlib
import re
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent

# (path, how many occurrences of the old version we require on the lines we touch)
TARGETS = [
    ("glyph-compiler/Cargo.toml", 1),
    ("npm/glyph/package.json", 6),          # own version + five platform pins
    ("npm/platform/darwin-arm64/package.json", 1),
    ("npm/platform/darwin-x64/package.json", 1),
    ("npm/platform/linux-arm64/package.json", 1),
    ("npm/platform/linux-x64/package.json", 1),
    ("npm/platform/win32-x64/package.json", 1),
    ("README.md", 2),                        # one Socket badge, url twice
    ("npm/glyph/README.md", 2),
    ("web/index.html", 1),
    (".github/ISSUE_TEMPLATE/bug_report.yml", 1),   # the version placeholder in the bug form
]


def current_version() -> str:
    text = (ROOT / "glyph-compiler" / "Cargo.toml").read_text()
    m = re.search(r'^version = "(\d+\.\d+\.\d+)"', text, re.M)
    if not m:
        sys.exit("could not read the current version out of glyph-compiler/Cargo.toml")
    return m.group(1)


def main() -> int:
    if len(sys.argv) != 2 or not re.fullmatch(r"\d+\.\d+\.\d+", sys.argv[1]):
        sys.exit("usage: bump_version.py <major.minor.patch>")
    new = sys.argv[1]
    old = current_version()
    if old == new:
        sys.exit(f"already at {new}")

    problems, edited = [], 0
    for rel, expected in TARGETS:
        p = ROOT / rel
        if not p.exists():
            problems.append(f"{rel}: missing")
            continue
        text = p.read_text()
        found = text.count(old)
        if found != expected:
            problems.append(f"{rel}: expected {expected} occurrence(s) of {old}, found {found}")
            continue
        p.write_text(text.replace(old, new))
        edited += found

    if problems:
        print(f"bump {old} -> {new} FAILED, nothing further written:")
        for line in problems:
            print(f"  {line}")
        return 1

    print(f"bumped {old} -> {new}: {edited} strings across {len(TARGETS)} files.")
    print("next: refresh Cargo.lock, rebuild, and confirm `glyph --version`.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
