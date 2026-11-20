#!/usr/bin/env python3
"""The bundled runtime must compile on the oldest TypeScript the docs support.

`docs/guide/getting-started.md` says the floor the emitted code needs is
TypeScript 4.9, and `glyph doctor` accepts any TypeScript 5. The runtime under
`glyph-compiler/runtime/` is copied into a user's build whenever a program
reaches it, and it is type-checked there with the user's own `typescript`. So a
runtime file that uses a newer TypeScript feature breaks every project on an
older one, with no user code involved.

It nearly happened. The first cut of 0.1.153's `std/record.ts` used
`NoInfer<K>`, a TypeScript 5.4 intrinsic, which would have broken every
project on TypeScript 4.9 to 5.3 that imported `std/record` (record 126).
Nothing checked the supported-version claim, and CI's own TypeScript 6
compiled it clean; a review found it before the release.

This installs `typescript@4.9.5` into a cache directory outside the repo and
compiles every runtime file (`glyph-bootstrap.ts`, `std/*.ts`, `internal/*.ts`)
with `--strict --lib es2022,dom --skipLibCheck --noEmit`, the flags a user's
build checks them with. It needs the network the first time, like the
`@types/node` check next to it in CI.

Hard-fails (exit 1) on any error, printing `tsc`'s output.
"""

from __future__ import annotations

import os
import pathlib
import shutil
import subprocess
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
RUNTIME = ROOT / "glyph-compiler" / "runtime"

# The floor `docs/guide/getting-started.md` states. Raising it is a decision
# about every downstream project, not a change to this number.
FLOOR = "4.9.5"


def cache_dir() -> pathlib.Path:
    base = os.environ.get("XDG_CACHE_HOME") or str(pathlib.Path.home() / ".cache")
    return pathlib.Path(base) / "glyph" / f"typescript-{FLOOR}"


def install(where: pathlib.Path) -> pathlib.Path | None:
    tsc = where / "node_modules" / "typescript" / "bin" / "tsc"
    if tsc.is_file():
        return tsc
    where.mkdir(parents=True, exist_ok=True)
    r = subprocess.run(
        ["npm", "install", "--no-audit", "--no-fund", "--no-save", "--prefix", str(where),
         f"typescript@{FLOOR}"],
        capture_output=True, text=True, timeout=600,
    )
    if r.returncode != 0 or not tsc.is_file():
        print(f"could not install typescript@{FLOOR}; this check needs the network.")
        print(r.stderr.strip() or r.stdout.strip())
        return None
    return tsc


def runtime_files() -> list[pathlib.Path]:
    files = [RUNTIME / "glyph-bootstrap.ts"]
    files += sorted((RUNTIME / "std").glob("*.ts"))
    files += sorted((RUNTIME / "internal").glob("*.ts"))
    return [f for f in files if not f.name.endswith(".d.ts")]


def main() -> int:
    if shutil.which("npm") is None or shutil.which("node") is None:
        print("npm and node are needed; this check installs and runs TypeScript.")
        return 1
    tsc = install(cache_dir())
    if tsc is None:
        return 1
    files = runtime_files()
    r = subprocess.run(
        ["node", str(tsc), "--strict", "--lib", "es2022,dom", "--skipLibCheck", "--noEmit",
         "--target", "es2022", "--module", "es2022", "--moduleResolution", "node",
         *[str(f) for f in files]],
        cwd=RUNTIME, capture_output=True, text=True, timeout=600,
    )
    out = (r.stdout + r.stderr).strip()
    if r.returncode != 0 or out:
        print(f"the bundled runtime does not compile on TypeScript {FLOOR}, the documented floor:\n")
        print(out)
        return 1
    print(f"runtime floor OK: {len(files)} runtime files compile under TypeScript {FLOOR} (--strict).")
    return 0


if __name__ == "__main__":
    sys.exit(main())
