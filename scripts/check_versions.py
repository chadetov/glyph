#!/usr/bin/env python3
"""Publish discipline: every version in the repo must agree, and the published
npm package should not fall behind.

Hard-fails (exit 1) when the workspace Cargo version, the six npm package.json
versions, and the launcher's five optionalDependencies pins are not all equal.
A mismatch there is how a broken or half-published release happens.

Also hard-fails when a version written outside a manifest points at something
other than the current release: the Socket badge URLs in the two READMEs, the
home page's hero pill, and the version the bug-report template suggests. Each of
those is a string a release has to bump and nothing else reads, so it goes stale
quietly and stays stale until someone happens to look.

Best-effort notice (never fails the build) when the published npm `latest` is
behind the repo version, so a stale package like the one a reviewer once hit two
versions behind is at least visible in CI.

Hard-fails when the release-notes entry for a version npm has not served yet
already says it was published and smoke-tested. That sentence is a verification
record, and at cut time the verification has not run.

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

# Files that write the version into prose or a form, where a regex is the only
# way to find it. Each entry is (path, label, pattern) and the pattern's first
# group is the version.
#
# The home page's hero pill hard-codes the version it advertises. Nothing checked
# it, and it sat at v0.1.72 for thirteen releases while the site told every
# visitor that was current. Same failure as the Socket badges, different file.
#
# The bug-report template suggests a version to the reporter. 0.1.93 rewrote it
# from the "0.1.9" it had carried since the template was written into the shape
# `glyph --version` prints, which made it a ceremony string, and 0.1.94 went out
# without it. Nothing on either list a person or a gate works from named it.
PINNED_PATTERNS = (
    ("web/index.html", "hero pill", r'class="pill"[^>]*>.*?v([0-9]+\.[0-9]+\.[0-9]+)'),
    (
        ".github/ISSUE_TEMPLATE/bug_report.yml",
        "version placeholder",
        r'placeholder:\s*"glyph ([0-9]+\.[0-9]+\.[0-9]+)"',
    ),
)


def pattern_versions() -> dict[str, str]:
    out: dict[str, str] = {}
    for rel, label, pattern in PINNED_PATTERNS:
        path = ROOT / rel
        if not path.exists():
            continue
        for i, m in enumerate(re.finditer(pattern, path.read_text(), re.S)):
            out[f"{rel}:{label} #{i + 1}"] = m.group(1)
    return out


def badge_versions() -> dict[str, str]:
    out: dict[str, str] = {}
    for rel, prefix in PINNED_BADGES:
        path = ROOT / rel
        if not path.exists():
            continue
        found = re.finditer(re.escape(prefix) + r"([0-9]+\.[0-9]+\.[0-9]+)", path.read_text())
        for i, m in enumerate(found):
            out[f"{rel}:badge URL #{i + 1}"] = m.group(1)
    return out


def lock_versions() -> dict[str, str]:
    """Every workspace crate's version as Cargo.lock records it.

    `cargo` writes each workspace member's version into the lockfile, so a bump
    that edits `[workspace.package]` and stops there leaves the lock behind. CI
    builds with `--locked`, which refuses to update it, so the whole job fails on
    a line that says nothing about versions: "cannot update the lock file ...
    because --locked was passed". The fix is `cargo update --workspace
    --offline`, and the point of checking here is that the message says so.
    """
    lock = ROOT / "glyph-compiler" / "Cargo.lock"
    if not lock.exists():
        return {}
    out: dict[str, str] = {}
    for block in lock.read_text().split("[[package]]"):
        name = re.search(r'(?m)^name = "(glyph[a-z-]*)"', block)
        ver = re.search(r'(?m)^version = "([^"]+)"', block)
        # Only workspace members carry a `path`-less local source: a registry
        # crate named `glyph-*` would have a `source =` line.
        if name and ver and "source =" not in block:
            out[f"Cargo.lock:{name.group(1)}"] = ver.group(1)
    return out


# The publish-verification sentence in a releases.md entry, in the past tense.
# The placeholder that stands in its place until the publish has run says "are
# recorded here once they have run", which matches neither pattern, so the gate
# fires on a claim and stays quiet on a promise.
PUBLISH_CLAIM = re.compile(r"\bPublished \d{4}-\d{2}-\d{2}\b|\bsmoke-tested\b")


def premature_publish_claim(repo: str) -> str | None:
    """The entry for an unpublished version claiming it was already verified.

    The release commit is written before the publish, so at cut time nothing
    about `npm install` or the execute bit has been checked. The convention the
    0.1.91 pair of commits set is that the entry carries a placeholder saying the
    smoke test is recorded once it has run, and a follow-up commit replaces it
    with the record. 0.1.92 was cut carrying the finished sentence instead: a
    verification claim for work nobody had done, inside the commit that was about
    to be tagged. No gate caught it, because the two gates nearby check other
    things: doc claims looks at test counts and forward references, and the npm
    lag here was only ever a notice.

    Only the section for the current repo version is read. Every older entry is
    frozen history, where the same sentence is a true statement about a release
    that did ship.
    """
    notes = ROOT / "docs" / "roadmap" / "releases.md"
    if not notes.exists():
        return None
    body = notes.read_text()
    m = re.search(rf"(?m)^### {re.escape(repo)}\b.*$", body)
    if not m:
        return None
    rest = body[m.end():]
    nxt = re.search(r"(?m)^### ", rest)
    section = rest[: nxt.start()] if nxt else rest
    hit = PUBLISH_CLAIM.search(section)
    return hit.group(0) if hit else None


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

    lock = lock_versions()
    stale_lock = {k: v for k, v in lock.items() if v != repo}
    if stale_lock:
        print(f"Cargo.lock is behind the workspace version ({repo}):")
        for k, v in stale_lock.items():
            print(f"  {k.replace(':', ': ')} = {v}")
        print("run: cd glyph-compiler && cargo update --workspace --offline")
        return 1

    mismatched = {k: v for k, v in versions.items() if v != repo}
    if mismatched:
        print(f"version mismatch: workspace Cargo is {repo}, but:")
        for k, v in mismatched.items():
            print(f"  {k} = {v}")
        print("bump every package.json (version + optionalDependencies) to match Cargo.")
        return 1

    pinned = {**badge_versions(), **pattern_versions()}
    stale = {k: v for k, v in pinned.items() if v != repo}
    if stale:
        print("a version-pinned URL or badge is out of date:")
        for k, v in stale.items():
            print(f"  {k} points at {v}, repo is {repo}")
        print("bump the version where it is written, or stop pinning it there.")
        return 1

    # Count what was compared, not one slice of it. The line used to report
    # len(versions), the eleven npm strings, while the Cargo version, the
    # lockfile entries, the badge URLs and the hero pill were all checked and
    # none of them counted. A reader reconciling the number against the release
    # ceremony's list of strings to bump got a smaller answer than the work.
    checked = 1 + len(versions) + len(lock) + len(pinned)
    tagged = " (matches the requested tag)" if expected is not None else ""
    print(f"version consistency OK: all {checked} version strings are {repo}{tagged}")
    print(f"  Cargo.toml 1, npm {len(versions)}, Cargo.lock {len(lock)}, pinned URLs {len(pinned)}")

    latest = published_latest()
    if latest and latest != repo:
        # A notice, not a failure: the repo is expected to be ahead between a
        # bump and its publish. Flag only so staleness is visible.
        print(f"::notice::npm latest is {latest}, repo is {repo}. Publish when ready so npm does not fall behind.")
        claim = premature_publish_claim(repo)
        if claim:
            print()
            print(f"but the {repo} release-notes entry already says it was published:")
            print(f'  docs/roadmap/releases.md: "{claim}"')
            print()
            print(f"npm serves {latest}, so that publish and its smoke test have not run.")
            print("Until they have, the entry carries the placeholder:")
            print()
            print("  The publish and the clean-npx smoke test (`--version`, the execute bit,")
            print("  `glyph init`, `npm install`, `glyph run`, and the headline feature itself)")
            print("  are recorded here once they have run.")
            print()
            print("Replace it with the record afterwards, in its own commit, the way")
            print("3745199 did for 0.1.91.")
            return 1
    elif latest:
        print(f"npm latest matches the repo ({latest}).")
    return 0


if __name__ == "__main__":
    sys.exit(main())
