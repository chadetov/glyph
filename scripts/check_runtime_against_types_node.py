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
import re
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

        # Same tree, same installed package: ask the narrower question too.
        parity = check_name_parity(root, version)
        if parity != 0:
            return parity

    print(f"runtime compiles against @types/node {version}")
    return 0


# ---------------------------------------------------------------- name parity
#
# The runtime check above only exercises declarations the compiler's own runtime
# happens to touch. A declaration only *user* code reaches is invisible to it,
# and that is not hypothetical: the `Signals` type sat exported from
# `child_process` through two review rounds, building green with nothing
# installed and failing TS2305 the moment someone installed the package. The
# guard written for it hardcoded the one name, so adding a second invented name
# passed the whole suite.
#
# This asks a narrower question that needs no build: for every name the shim
# exports from a node module, does `@types/node` export it too? It cannot catch a
# declaration whose *shape* is wider than node's, which is a harder problem and
# is tracked separately. It does catch a name node has never had.

MODULE = re.compile(r'^declare module "(?!node:)([^"]+)" \{', re.M)
EXPORTED = re.compile(r"^\s*export (?:declare )?(?:function|const|let|var|class|interface|type|enum)\s+([A-Za-z_$][\w$]*)", re.M)


def shim_exports(shim: pathlib.Path) -> dict[str, list[str]]:
    """Every name each bare (non `node:`) module block exports, by module."""
    text = shim.read_text()
    out: dict[str, list[str]] = {}
    for m in MODULE.finditer(text):
        start = m.end()
        depth = 1
        i = start
        while i < len(text) and depth:
            if text[i] == "{":
                depth += 1
            elif text[i] == "}":
                depth -= 1
            i += 1
        names = EXPORTED.findall(text[start:i])
        if names:
            out[m.group(1)] = sorted(set(names))
    return out


def check_name_parity(root: pathlib.Path, version: str) -> int:
    """Every name the shim exports must exist in the real @types/node."""
    shim = glyph_bin.ROOT / "glyph-compiler" / "runtime" / "glyph-node-shims.d.ts"
    exports = shim_exports(shim)
    if not exports:
        print("could not read any module exports out of the shim; the parser above")
        print("no longer matches the file, which makes this check silently vacuous.")
        return 1

    probe = root / "parity.ts"
    lines = []
    for module, names in sorted(exports.items()):
        for name in names:
            lines.append(f'import type {{ {name} as _{module.replace("/", "_").replace("-", "_")}_{name} }} from "{module}";')
    probe.write_text("\n".join(lines) + "\n")

    tsc = subprocess.run(
        # `bundler` resolution rather than the classic `node10`, which TypeScript
        # 6 reports as deprecated and fails on. Nothing here depends on the
        # resolution mode: every import names a bare node module.
        ["npx", "--yes", "tsc", "--noEmit", "--strict", "--skipLibCheck",
         "--module", "esnext", "--moduleResolution", "bundler",
         "--types", "node", str(probe)],
        cwd=root, capture_output=True, text=True, timeout=900,
    )
    if tsc.returncode != 0:
        bad = [l for l in (tsc.stdout + tsc.stderr).splitlines() if "TS2305" in l or "TS2724" in l]
        print(f"the shim exports names @types/node {version} does not have:")
        print()
        for line in bad or (tsc.stdout + tsc.stderr).splitlines()[:20]:
            print(f"  {line}")
        print()
        print("A name only user code reaches never appears in the runtime build, so")
        print("it builds green with nothing installed and fails the moment someone")
        print("follows the guide and installs the package. Declare it globally")
        print("(a `declare namespace NodeJS` block) rather than inside the module,")
        print("or give the module the name node actually has.")
        return 1

    total = sum(len(v) for v in exports.values())
    print(f"name parity OK: {total} exported names across {len(exports)} modules exist in @types/node {version}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
