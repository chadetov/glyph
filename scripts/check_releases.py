#!/usr/bin/env python3
"""Every released version has its notes, and no notes run ahead of a release.

The release notes are one file per version under site/releases/ (see
scripts/site_versions.py). Nothing used to read them, so a release could
publish with its own notes missing. This fails when:

  - a version from 0.1.0 up to the one in glyph-compiler/Cargo.toml has no
    site/releases/<version>.html, apart from the two that never had notes
    (0.1.1, never published, and 0.1.33, skipped);
  - a file exists for a version above Cargo's, which the site would announce
    as Latest before it is released;
  - a file does not parse, or the months go backwards as the versions go up.

Writing a release's notes: add site/releases/0.1.N.html, then run
`python3 scripts/build_site.py`.

Run from anywhere: python3 scripts/check_releases.py
"""
import pathlib
import sys

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))
import build_site  # noqa: E402
import site_versions  # noqa: E402


def main() -> int:
    try:
        releases = site_versions.load()
        cargo = build_site.workspace_version()
    except (site_versions.VersionsError, build_site.SiteError) as e:
        print(f"release notes check FAILED: {e}")
        return 1
    have = {r.version for r in releases}
    major, minor, patch = site_versions.vkey(cargo)
    want = {f"{major}.{minor}.{p}" for p in range(patch + 1)} - site_versions.KNOWN_GAPS
    missing = sorted(want - have, key=site_versions.vkey)
    ahead = sorted((v for v in have if site_versions.vkey(v) > site_versions.vkey(cargo)),
                   key=site_versions.vkey)
    stale_gaps = sorted(have & site_versions.KNOWN_GAPS, key=site_versions.vkey)
    problems = []
    for v in missing:
        problems.append(f"{v} has no notes: add site/releases/{v}.html")
    for v in ahead:
        problems.append(f"site/releases/{v}.html is for a version above Cargo's {cargo}; "
                        "the site would show it as Latest before it is released")
    for v in stale_gaps:
        problems.append(f"{v} now has notes; remove it from KNOWN_GAPS in scripts/site_versions.py")
    if problems:
        print("release notes check FAILED:")
        for p in problems:
            print(f"  - {p}")
        print("Then run: python3 scripts/build_site.py")
        return 1
    print(f"release notes OK: {len(have)} releases, 0.1.0 to {cargo}, "
          f"no gaps besides {', '.join(sorted(site_versions.KNOWN_GAPS, key=site_versions.vkey))}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
