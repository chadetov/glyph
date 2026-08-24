#!/usr/bin/env python3
"""Publish discipline: every version in the repo must agree, and the published
npm package should not fall behind.

Hard-fails (exit 1) when the workspace Cargo version, the six npm package.json
versions, and the launcher's five optionalDependencies pins are not all equal.
A mismatch there is how a broken or half-published release happens.

Also hard-fails when a version-pinned badge URL in a README points at a version
that is not the current one. Socket's badge URL carries the version, so it is a
string that silently goes stale on the next release and shows a reader a report
for a package they are not about to install.

Best-effort notice (never fails the build) when the published npm `latest` is
behind the repo version, so a stale package like the one a reviewer once hit two
versions behind is at least visible in CI.

With `--expect <version>` it also hard-fails when the repo is not at that
version. The release workflow passes the pushed tag, which is the only thing
that ties the tag to what the manifests actually say.
"""

from __future__ import annotations

import argparse
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


# A README badge whose URL carries the package version. One entry per URL shape;
# the version is whatever the last path segment is.
PINNED_BADGES = (
    ("README.md", "https://badge.socket.dev/npm/package/@glyphlang/glyph/"),
    ("npm/glyph/README.md", "https://badge.socket.dev/npm/package/@glyphlang/glyph/"),
)


def stale_badges(repo: str) -> list[str]:
    bad: list[str] = []
    for rel, prefix in PINNED_BADGES:
        path = ROOT / rel
        if not path.exists():
            continue
        for m in re.finditer(re.escape(prefix) + r"([0-9]+\.[0-9]+\.[0-9]+)", path.read_text()):
            if m.group(1) != repo:
                bad.append(f"{rel}: badge points at {m.group(1)}, repo is {repo}")
    return bad


def stale_lockfile(repo: str) -> list[str]:
    """Workspace crates in Cargo.lock still carrying an older version.

    `cargo` writes each workspace member's version into the lockfile, so a bump
    that edits `[workspace.package]` and stops there leaves the lock behind. CI
    builds with `--locked`, which refuses to update it, so the whole job fails on
    a line that says nothing about versions: "cannot update the lock file ...
    because --locked was passed". The fix is `cargo update --workspace
    --offline`, and the point of checking here is that the message says so.
    """
    lock = ROOT / "glyph-compiler" / "Cargo.lock"
    if not lock.exists():
        return []
    bad: list[str] = []
    for block in lock.read_text().split("[[package]]"):
        name = re.search(r'(?m)^name = "(glyph[a-z-]*)"', block)
        ver = re.search(r'(?m)^version = "([^"]+)"', block)
        # Only workspace members carry a `path`-less local source: a registry
        # crate named `glyph-*` would have a `source =` line.
        if name and ver and "source =" not in block and ver.group(1) != repo:
            bad.append(f"Cargo.lock: {name.group(1)} = {ver.group(1)}")
    return bad


def expected_from_argv(argv: list[str]) -> str | None:
    """The version the caller says this must be, from `--expect <version>`.

    The release workflow passes the pushed tag here. Without it nothing ever
    compares the tag against the manifests: `git tag v0.1.82` on the 0.1.81
    commit builds 0.1.81 binaries, and because the GitHub Release job did not
    depend on the npm publish job, a Release named v0.1.82 carrying 0.1.81
    binaries was created even though npm rejected the duplicate version. The
    ceremony said "confirm the tagged commit carries the bumped version first",
    which is exactly the kind of step a gate has to hold instead of a person.

    Parsed with argparse rather than by scanning argv, because a gate that
    quietly turns itself off is worse than no gate. A hand-rolled `"--expect" in
    argv` check treats `--expect=0.1.82`, `--expected 0.1.82`, and any typo as
    "no expectation given" and then prints "version consistency OK" and exits 0,
    so the one check standing between a mis-tagged commit and the registry would
    report success while doing nothing. argparse rejects all three.
    """
    parser = argparse.ArgumentParser(
        prog="check_versions.py",
        description="Every version string in the repo must agree, and optionally match --expect.",
    )
    parser.add_argument(
        "--expect",
        metavar="VERSION",
        help="fail unless the repo is at this version; a leading 'v' is stripped, so a tag works",
    )
    # parse_args exits 2 on an unknown flag, which is the behaviour that matters:
    # an unrecognized argument must never be read as "check nothing".
    args = parser.parse_args(argv)
    return args.expect.lstrip("v") if args.expect else None


def main() -> int:
    repo = cargo_version()
    versions = npm_versions()

    expected = expected_from_argv(sys.argv[1:])
    if expected is not None and expected != repo:
        print(f"tag/version mismatch: asked to release {expected}, repo is {repo}.")
        print()
        print("The tag names a version no manifest carries. Either the bump commit")
        print("was never merged, or the tag landed on the wrong commit. Delete the")
        print("tag, put it on the commit that carries the bump, and push it again:")
        print(f"  git tag -d v{expected} && git push origin :refs/tags/v{expected}")
        return 1

    stale_lock = stale_lockfile(repo)
    if stale_lock:
        print(f"Cargo.lock is behind the workspace version ({repo}):")
        for s in stale_lock:
            print(f"  {s}")
        print("run: cd glyph-compiler && cargo update --workspace --offline")
        return 1

    mismatched = {k: v for k, v in versions.items() if v != repo}
    if mismatched:
        print(f"version mismatch: workspace Cargo is {repo}, but:")
        for k, v in mismatched.items():
            print(f"  {k} = {v}")
        print("bump every package.json (version + optionalDependencies) to match Cargo.")
        return 1

    stale = stale_badges(repo)
    if stale:
        print("a version-pinned badge URL is out of date:")
        for s in stale:
            print(f"  {s}")
        print("bump the version in the badge URL, or switch it to an unpinned one.")
        return 1

    tagged = " (matches the requested tag)" if expected is not None else ""
    print(f"version consistency OK: all {len(versions)} version strings are {repo}{tagged}")

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
