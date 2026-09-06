#!/usr/bin/env python3
"""Every Glyph benchmark fixture is a program the compiler accepts.

`benchmarks/measure.sh` counts lines and tokens per language for the same task,
and the comparison only means something if every side is a working program.
Two of the three Glyph fixtures were not: one used a regex literal the language
does not have, the other `?`-propagated an `http.HttpError` out of a function
returning a different error type. Both were shorter than the working Glyph would
be, so the number the benchmark reported was flattering and wrong, and nothing
under `scripts/` or `.github/` built them, which is how it stayed that way from
the first measurement onward.

Each fixture is copied into its own empty project and checked with `glyph
check`, tsc included, exactly the way the docs snippets are. Its own project,
because checking `benchmarks/glyph/` as one directory pulls every fixture into
one module graph and reports one file's error three times.

Hard-fails (exit 1) when a fixture does not compile, printing the compiler's own
diagnostic, or when there are no fixtures to check.
"""

from __future__ import annotations

import pathlib
import shutil
import subprocess
import sys
import tempfile

import glyph_bin

ROOT = pathlib.Path(__file__).resolve().parent.parent
FIXTURES = ROOT / "benchmarks" / "glyph"


def check(glyph: pathlib.Path, fixture: pathlib.Path) -> tuple[bool, str]:
    with tempfile.TemporaryDirectory() as tmp:
        d = pathlib.Path(tmp)
        (d / "package.json").write_text('{"name":"benchmark-fixture","glyph":{}}\n')
        src = d / "src"
        src.mkdir()
        shutil.copy(fixture, src / fixture.name)
        r = subprocess.run(
            [str(glyph), "check", str(src / fixture.name)],
            capture_output=True,
            text=True,
            cwd=d,
        )
        return r.returncode == 0, (r.stdout + r.stderr).strip()


def main() -> int:
    glyph = glyph_bin.resolve()

    fixtures = sorted(FIXTURES.glob("*.glyph"))
    if not fixtures:
        print(f"no fixtures under {FIXTURES.relative_to(ROOT)}; nothing was checked")
        return 1

    failed = 0
    for fixture in fixtures:
        ok, output = check(glyph, fixture)
        rel = fixture.relative_to(ROOT)
        if ok:
            continue
        failed += 1
        print(f"{rel} does not compile:")
        for line in output.splitlines():
            print(f"  {line}")
        print()

    if failed:
        print(f"benchmark fixtures: {failed} of {len(fixtures)} do not compile")
        return 1
    print(f"benchmark fixtures: {len(fixtures)} checked, all compile")
    return 0


if __name__ == "__main__":
    sys.exit(main())
