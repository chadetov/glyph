#!/usr/bin/env python3
"""The bundled runtime must compile against the real `@types/node`.

`docs/guide/external-imports.md` tells you to install `@types/node` for the full
node surface, because the bundled ambient shim declares only what the runtime
itself calls. The build then prefers the real typings and skips the shim, which
means every file under `runtime/std/` is suddenly checked against declarations
nobody in this repo wrote.

Twice now that has failed on a program containing nothing but `pub fn main`, on
a line of the compiler's own runtime, with no user code involved:

    std/process.ts: Type 'string | number' is not assignable to type 'number'
    std/net.ts:     Property 'buffer' does not exist on type 'string | NonSharedBuffer'

Both came from the same place. The shim declared a node API more narrowly than
node actually has it (`process.exitCode` as `number | undefined`, a socket's
`data` chunk as a buffer), runtime code was written against the narrow type, and
the narrowing held right up until the real typings arrived.

A fixture cannot catch that. The first attempt at this check used a stand-in
`@types/node` built by copying the shim, which is a check that the shim agrees
with itself: it passed against the very version of the package that was failing.
So this installs the real thing, at `latest`, and builds a bare `main` against
it. It needs the network, like the codec check next to it in CI.

Hard-fails (exit 1) when the runtime does not type-check against the published
package. Prints the version it resolved, so a failure says which release of
`@types/node` moved.
"""

from __future__ import annotations

import json
import pathlib
import shutil
import subprocess
import sys
import tempfile

import glyph_bin

# The bare program. It imports nothing: the build copies the whole runtime into
# `.glyph-runtime/` and type-checks it either way, which is the point. A user
# who writes this and installs the package we recommend must get a clean build.
MAIN = """module main

pub fn main() -> number {
  return 0
}
"""

MANIFEST = {
    "name": "glyph-types-node-check",
    "private": True,
    "type": "module",
    # Marks the resolution root (D41), so `node_modules` here is the one the
    # build wires into the generated tsconfig.
    "glyph": {},
}


def npm(args: list[str], cwd: pathlib.Path) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        ["npm", *args], cwd=cwd, capture_output=True, text=True, timeout=600
    )


def main() -> int:
    if shutil.which("npm") is None:
        print("npm not found; this check installs the real @types/node.")
        return 1
    if shutil.which("tsc") is None and shutil.which("npx") is None:
        print("no tsc available; the build cannot type-check the runtime.")
        return 1

    binary = glyph_bin.resolve()

    with tempfile.TemporaryDirectory() as tmp:
        root = pathlib.Path(tmp)
        (root / "package.json").write_text(json.dumps(MANIFEST, indent=2) + "\n")
        src = root / "src"
        src.mkdir()
        (src / "main.glyph").write_text(MAIN)

        install = npm(
            ["install", "--no-audit", "--no-fund", "--save-dev", "@types/node@latest"],
            root,
        )
        if install.returncode != 0:
            print("could not install @types/node; this check needs the network.")
            print(install.stderr.strip() or install.stdout.strip())
            return 1

        installed = json.loads(
            (root / "node_modules/@types/node/package.json").read_text()
        )
        version = installed.get("version", "unknown")

        build = subprocess.run(
            [str(binary), "build", "src/", "--out", "dist/"],
            cwd=root,
            capture_output=True,
            text=True,
            timeout=900,
        )
        output = (build.stdout + build.stderr).strip()

        # The shim must actually have been skipped, or a green result here says
        # nothing about the real typings.
        if (root / "dist/.glyph-runtime/glyph-node-shims.d.ts").exists():
            print(f"@types/node {version} installed, but the build still wrote the")
            print("bundled shim, so the runtime was not checked against the real")
            print("typings. This check proves nothing until that is fixed.")
            return 1

        if build.returncode != 0:
            print(f"the bundled runtime does not compile against @types/node {version}:")
            print()
            print(output)
            print()
            print("This is the compiler's own runtime, not user code: a program")
            print("containing nothing but `pub fn main` fails this way for anyone who")
            print("follows docs/guide/external-imports.md. Fix the runtime, and fix")
            print("the shim declaration that let it be written that way.")
            return 1

    print(f"runtime compiles against @types/node {version}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
