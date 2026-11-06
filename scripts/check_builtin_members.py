#!/usr/bin/env python3
"""The checker's array, string and number member lists must be TypeScript's.

`glyph-typechecker/src/builtin_members.rs` holds every member a JavaScript
array, a JavaScript string and a JavaScript number have, and the checker
refuses a member read on `Array<T>`, `string` or `number` whose name is not in
them (G288, E0210). The lists were
read out of TypeScript's checker once, and a list read once is a snapshot: the
next TypeScript that adds a method to `Array.prototype` makes Glyph refuse a
program `tsc` accepts, and nothing in the Rust suite can notice, because the
suite has no TypeScript to ask.

So this asks it. It loads the TypeScript CI pins (`.github/ci-tools`), builds
the apparent type of `string[]`, `string` and `number` under `lib: ["esnext",
"dom"]`, which is the set the table's header says it holds, and compares the
property names with the three Rust arrays, both directions. A name TypeScript has and the
table lacks would be a false refusal; a name the table has and TypeScript lacks
would let a typo through to `tsc`.

It also checks the version the table's header names is the version it ran
against, so a TypeScript bump fails here first and the header cannot go on
naming a release nobody re-read.

Hard-fails (exit 1) on any difference, printing the names on each side.
"""

from __future__ import annotations

import json
import os
import pathlib
import re
import shutil
import subprocess
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
TABLE = ROOT / "glyph-compiler/crates/glyph-typechecker/src/builtin_members.rs"
CI_TOOLS = ROOT / ".github/ci-tools"

# The header sentence naming the TypeScript the lists were read with.
HEADER_VERSION = re.compile(r"Read with TypeScript (\d+\.\d+\.\d+)")
ARRAY_RE = re.compile(r"const (ARRAY|STRING|NUMBER)_MEMBERS: &\[&str\] = &\[(.*?)\];", re.S)

PROBE = r"""
const ts = require(process.argv[1]);
const file = "probe.ts";
const text = "declare const a: string[];\ndeclare const s: string;\ndeclare const n: number;\n";
const options = { lib: ["lib.esnext.d.ts", "lib.dom.d.ts"], strict: true, noEmit: true };
const host = ts.createCompilerHost(options);
const read = host.getSourceFile.bind(host);
host.getSourceFile = (name, lang) =>
  name === file ? ts.createSourceFile(file, text, ts.ScriptTarget.ESNext) : read(name, lang);
const program = ts.createProgram([file], options, host);
const checker = program.getTypeChecker();
const out = { version: ts.version };
for (const stmt of program.getSourceFile(file).statements) {
  const decl = stmt.declarationList.declarations[0];
  const t = checker.getApparentType(checker.getTypeAtLocation(decl.name));
  // Symbol-keyed members (`[Symbol.iterator]`) have no name a `.` read can spell.
  const names = checker.getPropertiesOfType(t).map((p) => p.getName()).filter((n) => !n.startsWith("__@"));
  out[{ a: "array", s: "string", n: "number" }[decl.name.text]] = names;
}
// `Object.prototype`'s own members, read the same way.
const objFile = "obj.ts";
const objText = "declare const o: Object;\n";
host.getSourceFile = (name, lang) =>
  name === objFile ? ts.createSourceFile(objFile, objText, ts.ScriptTarget.ESNext) : read(name, lang);
const p2 = ts.createProgram([objFile], options, host);
const c2 = p2.getTypeChecker();
const od = p2.getSourceFile(objFile).statements[0].declarationList.declarations[0];
out.object = c2.getPropertiesOfType(c2.getTypeAtLocation(od.name)).map((p) => p.getName());
console.log(JSON.stringify(out));
"""


def typescript_dir() -> pathlib.Path | None:
    """The pinned TypeScript: CI's locked tree, else what NODE_PATH or the
    global prefix holds (a developer machine without the CI tree installed)."""
    candidates = [CI_TOOLS / "node_modules/typescript"]
    for p in os.environ.get("NODE_PATH", "").split(os.pathsep):
        if p:
            candidates.append(pathlib.Path(p) / "typescript")
    if shutil.which("npm"):
        r = subprocess.run(["npm", "root", "-g"], capture_output=True, text=True)
        if r.returncode == 0:
            candidates.append(pathlib.Path(r.stdout.strip()) / "typescript")
    for c in candidates:
        if (c / "package.json").is_file():
            return c
    return None


def pinned_version() -> str:
    return json.loads((CI_TOOLS / "package.json").read_text())["dependencies"]["typescript"]


def main() -> int:
    if shutil.which("node") is None:
        print("node not found; this check asks TypeScript's checker for the member lists.")
        return 1
    ts_dir = typescript_dir()
    if ts_dir is None:
        print("typescript not found (run `npm ci` in .github/ci-tools, or install it globally).")
        return 1

    src = TABLE.read_text()
    header = HEADER_VERSION.search(src)
    if not header:
        print(f"{TABLE.relative_to(ROOT)}: no 'Read with TypeScript X.Y.Z' sentence in the header.")
        return 1
    table = {}
    for kind, body in ARRAY_RE.findall(src):
        table[kind.lower()] = re.findall(r'"([^"]+)"', body)
    if set(table) != {"array", "string", "number"}:
        print(f"{TABLE.relative_to(ROOT)}: could not find ARRAY_MEMBERS, STRING_MEMBERS and NUMBER_MEMBERS.")
        return 1

    r = subprocess.run(
        ["node", "-e", PROBE, str(ts_dir)], capture_output=True, text=True, timeout=120
    )
    if r.returncode != 0:
        print("the TypeScript probe failed:\n" + r.stderr)
        return 1
    got = json.loads(r.stdout)

    failures = []
    pin = pinned_version()
    if got["version"] != pin:
        failures.append(
            f"TypeScript at {ts_dir} is {got['version']}, but .github/ci-tools pins {pin}; "
            "check against the pinned version."
        )
    if header.group(1) != got["version"]:
        failures.append(
            f"the table's header says it was read with TypeScript {header.group(1)}, "
            f"and this is {got['version']}: re-read the lists and update the header."
        )
    for kind in ("array", "string", "number"):
        want = set(got[kind]) | set(got["object"])
        have = set(table[kind])
        missing = sorted(want - have)
        extra = sorted(have - want)
        if missing:
            failures.append(f"{kind}: TypeScript has, the table lacks (a false refusal): {', '.join(missing)}")
        if extra:
            failures.append(f"{kind}: the table has, TypeScript lacks (a typo let through): {', '.join(extra)}")
        if table[kind] != sorted(table[kind]):
            failures.append(f"{kind}: the table is not sorted, and `has` binary-searches it.")

    if failures:
        print("builtin members: the checker's lists disagree with TypeScript")
        for f in failures:
            print("  " + f)
        return 1
    print(
        f"builtin members OK: {len(table['array'])} array, {len(table['string'])} string and "
        f"{len(table['number'])} number members match TypeScript {got['version']} "
        "(esnext + dom, with Object's)."
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
