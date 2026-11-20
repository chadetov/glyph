#!/usr/bin/env python3
"""The bundled runtime must compile on the oldest TypeScript the docs support.

`docs/guide/getting-started.md` says the floor the emitted code and its runtime
need, compiled in your own project, is TypeScript 4.9. The runtime under
`glyph-compiler/runtime/` is copied into a user's build whenever a program
reaches it, and it is type-checked there with the user's own `typescript`. So a
runtime file that uses a newer TypeScript feature breaks every project on an
older one, with no user code involved.

It nearly happened. The first cut of 0.1.153's `std/record.ts` used
`NoInfer<K>`, a TypeScript 5.4 intrinsic, which would have broken every
project on TypeScript 4.9 to 5.3 that imported `std/record` (record 126).
Nothing checked the supported-version claim, and CI's own TypeScript 6
compiled it clean; a review found it before the release.

The check models both configurations of the tsconfig `glyph build` generates,
and both must pass under `typescript@4.9.5`:

  1. A project without `@types/node`. The build writes the runtime's own
     `glyph-node-shims.d.ts` beside the runtime, so this pass compiles the
     shims, `glyph-prelude.d.ts`, `glyph-bootstrap.ts`, `std/*.ts` and
     `internal/*.ts` with `"types": []` and `"typeRoots"` at an empty
     directory.
  2. A project with `@types/node`. The build writes `"types": ["node"]` and no
     shims, so this pass compiles the same files without the shims against an
     `@types/node`, with `"typeRoots"` at that install. The version is the one
     DefinitelyTyped publishes for TypeScript 4.9 (its `ts4.9` tag), pinned in
     `.github/ci-tools/package-lock.json` under the alias `types-node-ts4.9`.
     The current `@types/node` needs TypeScript 5.6 and does not parse under
     4.9, so a project on the floor has to use that one.

Both passes set `types` and `typeRoots` explicitly. TypeScript before 6 loads
every `@types/*` it finds in an ancestor `node_modules`, so a check that left
them unset would pass on a machine with an `@types/node` in the home directory
and fail in CI (record 127). The gate writes its own tsconfig for each pass into
a cache directory outside the repo (`$XDG_CACHE_HOME/glyph`, else
`~/.cache/glyph`) and installs the two packages there. It needs the network the
first time.

Hard-fails (exit 1) when either pass reports an error, printing `tsc`'s output.
"""

from __future__ import annotations

import json
import os
import pathlib
import shutil
import subprocess
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
RUNTIME = ROOT / "glyph-compiler" / "runtime"
CI_LOCK = ROOT / ".github" / "ci-tools" / "package-lock.json"

# The floor `docs/guide/getting-started.md` states. Raising it is a decision
# about every downstream project, not a change to this number.
FLOOR = "4.9.5"

# The compiler flags a user's build checks the runtime with, apart from the
# two settings each pass decides.
OPTIONS = {
    "strict": True,
    "lib": ["es2022", "dom"],
    "skipLibCheck": True,
    "noEmit": True,
    "target": "es2022",
    "module": "es2022",
    "moduleResolution": "node",
}


def cache_root() -> pathlib.Path:
    base = os.environ.get("XDG_CACHE_HOME") or str(pathlib.Path.home() / ".cache")
    return pathlib.Path(base) / "glyph" / "runtime-ts-floor"


# The `@types/node` for TypeScript 4.9, pinned in the CI lockfile under an
# alias so it is not an `@types` package anything else in CI would load.
TYPES_NODE_ALIAS = "types-node-ts4.9"


def pinned_types_node() -> str | None:
    lock = json.loads(CI_LOCK.read_text())
    entry = lock.get("packages", {}).get(f"node_modules/{TYPES_NODE_ALIAS}")
    if not entry or entry.get("name") != "@types/node":
        return None
    return entry.get("version")


def install(prefix: pathlib.Path, spec: str, probe: pathlib.Path) -> bool:
    if probe.is_file():
        return True
    prefix.mkdir(parents=True, exist_ok=True)
    r = subprocess.run(
        ["npm", "install", "--no-audit", "--no-fund", "--no-save", "--prefix", str(prefix), spec],
        capture_output=True, text=True, timeout=600,
    )
    if r.returncode != 0 or not probe.is_file():
        print(f"could not install {spec}; this check needs the network.")
        print(r.stderr.strip() or r.stdout.strip())
        return False
    return True


def runtime_sources() -> list[pathlib.Path]:
    files = [RUNTIME / "glyph-prelude.d.ts", RUNTIME / "glyph-bootstrap.ts"]
    files += sorted(f for f in (RUNTIME / "std").glob("*.ts") if not f.name.endswith(".d.ts"))
    files += sorted(f for f in (RUNTIME / "internal").glob("*.ts") if not f.name.endswith(".d.ts"))
    return files


def run_pass(tsc: pathlib.Path, where: pathlib.Path, files: list[pathlib.Path],
             types: list[str], type_roots: pathlib.Path) -> tuple[bool, str]:
    where.mkdir(parents=True, exist_ok=True)
    config = {
        "compilerOptions": {**OPTIONS, "types": types, "typeRoots": [str(type_roots)]},
        "files": [str(f) for f in files],
    }
    (where / "tsconfig.json").write_text(json.dumps(config, indent=2) + "\n")
    r = subprocess.run(
        ["node", str(tsc), "-p", str(where / "tsconfig.json"), "--pretty", "false"],
        cwd=RUNTIME, capture_output=True, text=True, timeout=600,
    )
    out = (r.stdout + r.stderr).strip()
    return r.returncode == 0 and not out, out


def main() -> int:
    if shutil.which("npm") is None or shutil.which("node") is None:
        print("npm and node are needed; this check installs and runs TypeScript.")
        return 1
    types_node = pinned_types_node()
    if types_node is None:
        print(f"no `{TYPES_NODE_ALIAS}` (@types/node) pinned in {CI_LOCK.relative_to(ROOT)}; pass 2 needs one.")
        return 1

    cache = cache_root()
    ts_prefix = cache / f"typescript-{FLOOR}"
    tsc = ts_prefix / "node_modules" / "typescript" / "bin" / "tsc"
    node_prefix = cache / f"types-node-{types_node}"
    node_roots = node_prefix / "node_modules" / "@types"
    if not install(ts_prefix, f"typescript@{FLOOR}", tsc):
        return 1
    if not install(node_prefix, f"@types/node@{types_node}", node_roots / "node" / "package.json"):
        return 1
    empty = cache / "no-types"
    if empty.exists():
        shutil.rmtree(empty)
    empty.mkdir(parents=True)

    sources = runtime_sources()
    shims = RUNTIME / "glyph-node-shims.d.ts"
    passes = [
        ("without @types/node", cache / "pass-shims", [shims, *sources], [], empty),
        (f"with @types/node {types_node}", cache / "pass-types-node", sources, ["node"], node_roots),
    ]
    failed = False
    for label, where, files, types, roots in passes:
        ok, out = run_pass(tsc, where, files, types, roots)
        if ok:
            print(f"pass {label}: OK")
        else:
            failed = True
            print(f"pass {label}: the runtime does not compile on TypeScript {FLOOR}, the documented floor:\n")
            print(out)
            print()
    if failed:
        return 1
    print(f"runtime floor OK: {len(sources)} runtime files compile under TypeScript {FLOOR} "
          f"(--strict), with and without @types/node.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
