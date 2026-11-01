#!/usr/bin/env python3
"""Check the React example against React's own declarations.

`examples/03_react_component.glyph` is the example a reader opens to see how
Glyph writes a React component. It used to be checked only against
`examples/.types/glyph-externals.d.ts`, a stub that declared `use_state`,
`use_effect` and `use_memo` with a `{ value, set }` shape React has never had,
so the example compiled in CI and drew TS2724 three times in a real project
(G271). The stub is still what lets the examples tree build without React
installed, which means it can drift from React again without anything failing.

This gate puts the example in a throwaway project whose `node_modules` is the
todo app's, which CI installs from that app's committed lockfile with React 18
and `@types/react`, and runs `glyph check` with `tsc` in the loop. A hook the
example calls that React does not have, or a call shaped the way React does not
accept, fails here.

It needs `examples/apps/todo/node_modules`; run `npm ci` in that directory
first. A missing tree is a failure, never a skip, because a gate that passes
when it cannot look has proved nothing.
"""

from __future__ import annotations

import pathlib
import shutil
import subprocess
import sys
import tempfile

import glyph_bin

ROOT = pathlib.Path(__file__).resolve().parent.parent
EXAMPLE = ROOT / "examples" / "03_react_component.glyph"
MODULES = ROOT / "examples" / "apps" / "todo" / "node_modules"


def main() -> int:
    if not (MODULES / "@types" / "react").is_dir():
        print(
            f"react example FAIL: {MODULES.relative_to(ROOT)} has no @types/react; "
            "run `npm ci` in examples/apps/todo first"
        )
        return 1
    glyph = glyph_bin.resolve()
    source = EXAMPLE.read_text()
    module = next(
        (line.split()[1] for line in source.splitlines() if line.startswith("module ")),
        None,
    )
    if module is None:
        print(f"react example FAIL: {EXAMPLE.relative_to(ROOT)} declares no module")
        return 1
    with tempfile.TemporaryDirectory(prefix="glyph-react-example-") as tmp:
        project = pathlib.Path(tmp)
        (project / "package.json").write_text(
            '{ "name": "react-example", "private": true, "type": "module", "glyph": {} }\n'
        )
        (project / "node_modules").symlink_to(MODULES, target_is_directory=True)
        target = project / f"{module}.glyph"
        target.parent.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(EXAMPLE, target)
        run = subprocess.run(
            [str(glyph), "check", "."],
            cwd=project,
            capture_output=True,
            text=True,
        )
    out = run.stdout + run.stderr
    if run.returncode != 0 or "tsc --strict passed" not in out:
        print(f"react example FAIL: {EXAMPLE.relative_to(ROOT)} against @types/react:")
        print(out)
        return 1
    print(f"react example OK: {EXAMPLE.relative_to(ROOT)} passes tsc --strict against @types/react")
    return 0


if __name__ == "__main__":
    sys.exit(main())
