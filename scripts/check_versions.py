#!/usr/bin/env python3
"""Publish discipline: every version in the repo must agree, and the published
npm package should not fall behind.

Hard-fails (exit 1) when the workspace Cargo version, the six npm package.json
versions, and the launcher's five optionalDependencies pins are not all equal.
A mismatch there is how a broken or half-published release happens.

Best-effort notice (never fails the build) when the published npm `latest` is
behind the repo version, so a stale package like the one a reviewer once hit two
versions behind is at least visible in CI.
"""

from __future__ import annotations

import json
import pathlib
import re
import subprocess
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent


def cargo_version() -> str:
    text = (ROOT / "glyph-compiler" / "Cargo.toml").read_text()
    m = re.search(r"\[workspace\.package\][^\[]*?version\s*=\s*\"([^\"]+)\"", text, re.S)
    if not m:
        sys.exit("could not read [workspace.package] version from Cargo.toml")
    return m.group(1)


def npm_versions() -> dict[str, str]:
    """Every version string across the six package.json files, labeled by source."""
    out: dict[str, str] = {}
    for p in sorted((ROOT / "npm").rglob("package.json")):
        data = json.loads(p.read_text())
        rel = p.relative_to(ROOT)
        out[f"{rel}:version"] = data["version"]
        for dep, ver in (data.get("optionalDependencies") or {}).items():
            out[f"{rel}:optionalDependencies.{dep}"] = ver
    return out


def published_latest() -> str | None:
    try:
        r = subprocess.run(
            ["npm", "view", "@glyphlang/glyph", "version"],
            capture_output=True, text=True, timeout=30,
        )
        return r.stdout.strip() or None
    except Exception:
        return None


def main() -> int:
    repo = cargo_version()
    versions = npm_versions()

    mismatched = {k: v for k, v in versions.items() if v != repo}
    if mismatched:
        print(f"version mismatch: workspace Cargo is {repo}, but:")
        for k, v in mismatched.items():
            print(f"  {k} = {v}")
        print("bump every package.json (version + optionalDependencies) to match Cargo.")
        return 1

    print(f"version consistency OK: all {len(versions)} version strings are {repo}")

    latest = published_latest()
    if latest and latest != repo:
        # A notice, not a failure: the repo is expected to be ahead between a
        # bump and its publish. Flag only so staleness is visible.
        print(f"::notice::npm latest is {latest}, repo is {repo}. Publish when ready so npm does not fall behind.")
    elif latest:
        print(f"npm latest matches the repo ({latest}).")
    return 0


if __name__ == "__main__":
    sys.exit(main())
