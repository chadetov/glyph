#!/usr/bin/env python3
"""The playground's wasm-bindgen version is written in one place and read
everywhere else.

`wasm-bindgen` the crate and `wasm-bindgen-cli` the tool must be the same
version exactly, and for a while they were not: the manifest pinned `=0.2.127`
under a comment saying they must match, while the README and `build.sh` both
told a reader to install 0.2.125. Following the instructions produced the very
mismatch the comment warned about, and the page could not be rebuilt from its
own documentation.

The manifest is authoritative. `build.sh` reads the pin out of it rather than
carrying a literal (`build.sh --pin` prints what it read), and this checks that
every other place the version is written down agrees:

  - `playground/README.md` states the version at least once, and every
    statement equals the pin;
  - `playground/build.sh` carries no version literal of its own, and what it
    extracts from the manifest is the pin;
  - `.github/workflows/playground.yml` installs the cli at the pin.

Hard-fails (exit 1) on any disagreement, and on a README that has stopped
stating the version at all, since a document with no claim cannot drift but
also cannot be followed.
"""

from __future__ import annotations

import pathlib
import re
import subprocess
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
MANIFEST = ROOT / "glyph-compiler" / "crates" / "glyph-wasm" / "Cargo.toml"
README = ROOT / "playground" / "README.md"
BUILD = ROOT / "playground" / "build.sh"
WORKFLOW = ROOT / ".github" / "workflows" / "playground.yml"

PIN = re.compile(r'^wasm-bindgen\s*=\s*"=(\d+\.\d+\.\d+)"', re.M)
# `wasm-bindgen-cli --version 0.2.127`, or the bare `0.2.127` the prose uses.
VERSION = re.compile(r"\b0\.2\.\d+\b")


def main() -> int:
    problems: list[str] = []

    m = PIN.search(MANIFEST.read_text())
    if not m:
        print(f"{MANIFEST.relative_to(ROOT)}: no exact `wasm-bindgen = \"=X.Y.Z\"` pin")
        return 1
    pin = m.group(1)

    stated = VERSION.findall(README.read_text())
    if not stated:
        problems.append(f"{README.relative_to(ROOT)} no longer states the wasm-bindgen version")
    for v in stated:
        if v != pin:
            problems.append(f"{README.relative_to(ROOT)} says {v}; the manifest pins {pin}")

    for v in VERSION.findall(BUILD.read_text()):
        problems.append(
            f"{BUILD.relative_to(ROOT)} carries the literal {v}; it must read the "
            f"version from the manifest"
        )
    r = subprocess.run(["bash", str(BUILD), "--pin"], capture_output=True, text=True)
    extracted = r.stdout.strip()
    if r.returncode != 0 or extracted != pin:
        problems.append(
            f"{BUILD.relative_to(ROOT)} --pin printed {extracted!r} "
            f"(exit {r.returncode}); the manifest pins {pin}"
        )

    installs = VERSION.findall(WORKFLOW.read_text())
    if not installs:
        problems.append(f"{WORKFLOW.relative_to(ROOT)} does not install wasm-bindgen-cli at a version")
    for v in installs:
        if v != pin:
            problems.append(f"{WORKFLOW.relative_to(ROOT)} installs {v}; the manifest pins {pin}")

    if problems:
        print("the playground's wasm-bindgen version disagrees with its manifest:")
        for p in problems:
            print(f"  {p}")
        return 1

    print(f"playground pin: wasm-bindgen {pin} in the manifest, README, build.sh and workflow")
    return 0


if __name__ == "__main__":
    sys.exit(main())
