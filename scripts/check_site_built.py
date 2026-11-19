#!/usr/bin/env python3
"""web/ is exactly what scripts/build_site.py renders from site/.

The site's pages are generated from site/, but web/ is what the Pages workflow
deploys, so the two can drift two ways: someone edits a page in web/ directly
(and the next build silently throws the edit away), or edits site/ and forgets
to rebuild (and the change never ships). Either way the source and the served
site disagree about what the site says.

This renders site/ in memory and compares every file the
generator owns with web/, byte for byte. It fails naming each file that
differs, each generated file web/ is missing, and each page in web/ that no
source renders. The fix is always the same: edit site/, then run
`python3 scripts/build_site.py`.

Run from anywhere: python3 scripts/check_site_built.py
"""
import difflib
import pathlib
import sys

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))
import build_site  # noqa: E402

WEB = build_site.ROOT / "web"


def main() -> int:
    try:
        files = build_site.build()
    except build_site.SiteError as e:
        print(f"site build check FAILED: {e}")
        return 1
    problems = []
    for rel, want in sorted(files.items()):
        have_path = WEB / rel
        if not have_path.exists():
            problems.append((f"web/{rel}: missing (site/ renders it)", None))
            continue
        have = have_path.read_text()
        if have != want:
            diff = list(difflib.unified_diff(have.splitlines(), want.splitlines(),
                                             f"web/{rel}", f"rendered/{rel}", n=1, lineterm=""))
            problems.append((f"web/{rel}: differs from what site/ renders", diff[:12]))
    for rel in build_site.stale_pages(WEB, files):
        problems.append((f"web/{rel}: no page in site/pages/ renders it", None))
    if problems:
        print("site build check FAILED: web/ is not what scripts/build_site.py renders.")
        for msg, diff in problems:
            print(f"  - {msg}")
            for line in diff or []:
                print(f"      {line}")
        print("Edit the source under site/, then run: python3 scripts/build_site.py")
        return 1
    pages = sum(1 for k in files if k.endswith(".html"))
    print(f"site build OK: web/ matches site/ ({pages} pages, sitemap.xml, style.css)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
