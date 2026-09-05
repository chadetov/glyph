#!/usr/bin/env python3
"""Every release gate, in order, as one command.

The gates were already written and already documented; what was missing was a
single way to run them. Thirteen commands typed by hand is not a control, it is
a list, and the failure mode is not that someone refuses to run one. It is that
someone runs twelve at two in the morning and does not notice which one they
skipped. That has happened here: a release failed CI on a lint no local check
had shown, because the lint gate is the one that runs separately from the build
and the test suite.

This runs all of them and reports every result rather than stopping at the first
failure, because knowing that three gates fail is worth more than discovering
them one release at a time.

It does NOT belong in `release.yml`. The gates that run the compiler need a
built `target/release/glyph`, and CI builds from the tag in a later job, so a
fresh checkout has nothing to check. Adding them there would fail every release
for a reason that has nothing to do with the release. The freshness this
protects is local: a binary on this machine older than the code on this machine.

Usage:
    python3 scripts/check_release.py            # every gate
    python3 scripts/check_release.py --suite    # and run the test suite first
"""

from __future__ import annotations

import pathlib
import subprocess
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
COMPILER = ROOT / "glyph-compiler"
SUITE_LOG = COMPILER / "target" / "suite.log"

# In ceremony order. Clippy first: it is the one no other local command runs, so
# it is the one that reaches CI unnoticed.
GATES = [
    ("check_clippy", "lint, exactly as CI runs it"),
    ("check_binary_fresh", "the binary is the code you built"),
    ("check_gaps", "markers, counts, and evidence freshness"),
    ("check_closed_gaps", "no entry is fixed and still listed open"),
    ("check_findings_scheduled", "every open finding is in the roadmap"),
    ("check_plans_fresh", "unshipped plans re-read, not re-stamped"),
    ("check_versions", "every version string agrees"),
    ("check_site", "links resolve, sub-nav and sitemap complete"),
    ("check_apps_are_glyph", "no TypeScript under examples/apps"),
    ("check_catches", "the paired demos still catch what they claim"),
    ("check_exact_or_absent", "no impact edge is manufactured under degeneracy"),
    ("check_scaffold_docs", "what `glyph init` writes still builds"),
    ("check_docs_compile", "every documented program compiles"),
    ("check_doc_claims", "test counts and promises match reality"),
    ("check_runtime_against_types_node", "the runtime matches its declarations"),
]


def run_suite() -> bool:
    """Run the workspace suite to the log `check_doc_claims` reads."""
    print("running the workspace suite (slow cold; this is the long one)")
    SUITE_LOG.parent.mkdir(parents=True, exist_ok=True)
    with SUITE_LOG.open("w") as log:
        code = subprocess.call(
            ["cargo", "test", "--workspace"],
            cwd=COMPILER,
            stdout=log,
            stderr=subprocess.STDOUT,
        )
    # `check_doc_claims` refuses a log with no completion marker, so a run that
    # died half way cannot be mistaken for a pass.
    with SUITE_LOG.open("a") as log:
        log.write(f"EXIT={code}\n")
    text = SUITE_LOG.read_text(errors="replace")
    passed = sum(
        int(line.split()[3])
        for line in text.splitlines()
        if line.startswith("test result: ok. ")
    )
    print(f"  suite: exit {code}, {passed} passed")
    return code == 0


def main() -> int:
    want_suite = "--suite" in sys.argv[1:]
    if want_suite and not run_suite():
        print("\nthe suite failed; the gates below are not worth reading yet.")
        return 1

    results: list[tuple[str, bool, str]] = []
    for name, blurb in GATES:
        proc = subprocess.run(
            [sys.executable, str(ROOT / "scripts" / f"{name}.py")],
            capture_output=True,
            text=True,
        )
        ok = proc.returncode == 0
        results.append((name, ok, (proc.stdout + proc.stderr).strip()))
        print(f"{'PASS' if ok else 'FAIL'}  {name:<34} {blurb}")

    failed = [(n, out) for n, ok, out in results if not ok]
    if not failed:
        print(f"\nall {len(GATES)} gates pass.")
        if not want_suite:
            print("the suite is not included; re-run with --suite before a cut.")
        return 0

    print(f"\n{len(failed)} of {len(GATES)} gates failed:\n")
    for name, out in failed:
        print(f"--- {name} ---")
        print(out or "(no output)")
        print()
    return 1


if __name__ == "__main__":
    raise SystemExit(main())
