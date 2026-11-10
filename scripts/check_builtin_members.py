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

It also generates the method rows (G312). `builtin_methods.rs` holds one row
per method of the three receivers, with the signature the same TypeScript
declares for it mapped to the shapes the checker can spell, by the rules
written in `builtin_members.rs` above `BuiltinMethod`. With `--write` this
script writes that file; without it, it renders the file again and fails when
the committed copy differs, so a TypeScript bump that changes a signature, or
a hand edit to the generated file, fails here.

Hard-fails (exit 1) on any difference, printing the names on each side.
"""

from __future__ import annotations

import difflib
import json
import os
import pathlib
import re
import shutil
import subprocess
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
TABLE = ROOT / "glyph-compiler/crates/glyph-typechecker/src/builtin_members.rs"
METHODS = ROOT / "glyph-compiler/crates/glyph-typechecker/src/builtin_methods.rs"
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

# The method rows (G312). Reads each method off the declared interface rather
# than off `string[]`, so `Array<T>`'s own `T` is still a type parameter and a
# position holding it maps to `Elem` instead of to `string`. `Object`'s methods
# are read the same way and the receiver's own declaration wins over them, which
# is the apparent type the member probe above reads names from.
#
# The rules are the ones `builtin_members.rs` documents above `BuiltinMethod`:
# overloads, a `this` type, a type predicate or a rest parameter leave the whole
# method untyped with the reason; any position outside the closed set below is
# `Unknown`. A `this` return reads as `T[]` once the checker has resolved it, so
# it is found in the declaration's syntax, not in the resolved type.
METHOD_PROBE = r"""
const ts = require(process.argv[1]);
const file = "probe.ts";
const options = { lib: ["lib.esnext.d.ts", "lib.dom.d.ts"], strict: true, noEmit: true };
const host = ts.createCompilerHost(options);
const read = host.getSourceFile.bind(host);
host.getSourceFile = (name, lang) =>
  name === file ? ts.createSourceFile(file, "export {};\n", ts.ScriptTarget.ESNext) : read(name, lang);
const program = ts.createProgram([file], options, host);
const checker = program.getTypeChecker();
const scope = checker.getSymbolsInScope(program.getSourceFile(file), ts.SymbolFlags.Interface);
const hasThisType = (node) => {
  let found = false;
  const walk = (n) => {
    if (found) return;
    if (n.kind === ts.SyntaxKind.ThisType) { found = true; return; }
    ts.forEachChild(n, walk);
  };
  walk(node);
  return found;
};
function methods(name) {
  const declared = checker.getDeclaredTypeOfSymbol(scope.find((s) => s.name === name));
  const elem = (declared.typeParameters || [])[0];
  const map = (t) => {
    if (t.flags & ts.TypeFlags.Number) return "Number";
    if (t.flags & ts.TypeFlags.String) return "String";
    if (t.flags & ts.TypeFlags.Boolean) return "Bool";
    if (t.flags & ts.TypeFlags.Void) return "Void";
    if (elem && t === elem) return "Elem";
    if (checker.isArrayType(t)) {
      const inner = map(checker.getTypeArguments(t)[0]);
      return inner === "Unknown" ? "Unknown" : `ArrayOf(${inner})`;
    }
    return "Unknown";
  };
  const out = {};
  for (const p of checker.getPropertiesOfType(declared)) {
    const n = p.getName();
    // Symbol-keyed members have no name a `.` read can spell, and a data
    // property is read, never called.
    if (n.startsWith("__@") || !(p.flags & ts.SymbolFlags.Method)) continue;
    const sigs = checker.getSignaturesOfType(checker.getTypeOfSymbol(p), ts.SignatureKind.Call);
    const sig = sigs[0];
    const decl = sig.declaration;
    let untyped = null;
    if (sigs.length > 1) untyped = "Overloads";
    else if (sig.thisParameter || hasThisType(decl)) untyped = "ThisType";
    else if (checker.getTypePredicateOfSignature(sig)) untyped = "Predicate";
    else if (decl.parameters.some((q) => q.dotDotDotToken)) untyped = "RestParameter";
    if (untyped) { out[n] = { untyped }; continue; }
    const params = sig.getParameters().map((q, i) => {
      const pd = decl.parameters[i];
      const optional = !!(pd.questionToken || pd.initializer);
      let t = checker.getTypeOfSymbol(q);
      // Under `strict` an optional parameter's type carries `undefined`; the
      // row says optional instead, so the `undefined` is dropped and what is
      // left is mapped. `boolean` is itself the union `true | false`.
      if (optional && t.isUnion()) {
        const rest = t.types.filter((u) => !(u.flags & ts.TypeFlags.Undefined));
        if (rest.length === 1) t = rest[0];
        else if (rest.length > 0 && rest.every((u) => u.flags & ts.TypeFlags.BooleanLiteral)) return { ty: "Bool", optional };
        else return { ty: "Unknown", optional };
      }
      return { ty: map(t), optional };
    });
    out[n] = { params, ret: map(checker.getReturnTypeOfSignature(sig)) };
  }
  return out;
}
const object = methods("Object");
const result = { version: ts.version };
for (const [key, name] of [["array", "Array"], ["string", "String"], ["number", "Number"]]) {
  result[key] = { ...object, ...methods(name) };
}
console.log(JSON.stringify(result));
"""


def rust_ty(ty: str) -> str:
    """`ArrayOf(Elem)` as the Rust expression the row holds."""
    if ty.startswith("ArrayOf(") and ty.endswith(")"):
        return f"T::ArrayOf(&{rust_ty(ty[len('ArrayOf('):-1])})"
    return f"T::{ty}"


def render_methods(got: dict) -> str:
    """The whole of `builtin_methods.rs`, from the method probe's answer."""
    lines = [
        "// @generated by `python3 scripts/check_builtin_members.py --write`. Do not edit:",
        "// CI renders this file again and fails when the committed copy differs.",
        "",
        "//! The methods of a JavaScript array, string and number, with the signature",
        f"//! TypeScript {got['version']}'s library (`lib: [\"esnext\", \"dom\"]`) declares for",
        "//! each, mapped by the rules `builtin_members.rs` documents above",
        "//! `BuiltinMethod` (G312). Sorted by name: `BuiltinReceiver::method`",
        "//! binary-searches.",
        "",
        "use crate::builtin_members::MethodSig::{Typed, Untyped};",
        "use crate::builtin_members::MethodTy as T;",
        "use crate::builtin_members::Untyped as U;",
        "use crate::builtin_members::{opt, req, BuiltinMethod};",
    ]
    for kind in ("array", "string", "number"):
        lines.append("")
        lines.append(f"pub(crate) const {kind.upper()}_METHODS: &[BuiltinMethod] = &[")
        for name in sorted(got[kind]):
            row = got[kind][name]
            if "untyped" in row:
                sig = f"Untyped(U::{row['untyped']})"
            else:
                params = ", ".join(
                    f"{'opt' if p['optional'] else 'req'}({rust_ty(p['ty'])})" for p in row["params"]
                )
                sig = f"Typed {{ params: &[{params}], ret: {rust_ty(row['ret'])} }}"
            lines.append(f'    BuiltinMethod {{ name: "{name}", sig: {sig} }},')
        lines.append("];")
    return "\n".join(lines) + "\n"


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
    write = "--write" in sys.argv[1:]
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

    r = subprocess.run(
        ["node", "-e", METHOD_PROBE, str(ts_dir)], capture_output=True, text=True, timeout=120
    )
    if r.returncode != 0:
        print("the TypeScript method probe failed:\n" + r.stderr)
        return 1
    rendered = render_methods(json.loads(r.stdout))
    rel = METHODS.relative_to(ROOT)
    if write:
        METHODS.write_text(rendered)
        print(f"wrote {rel}")
    elif not METHODS.is_file() or METHODS.read_text() != rendered:
        # A unified diff names the rows that moved, which is what a reviewer
        # of a TypeScript bump needs to read.
        committed = METHODS.read_text().splitlines(keepends=True) if METHODS.is_file() else []
        diff = "".join(
            difflib.unified_diff(committed, rendered.splitlines(keepends=True), str(rel), "regenerated")
        )
        failures.append(
            f"{rel} is not what TypeScript {got['version']} generates; "
            "run `python3 scripts/check_builtin_members.py --write` and review the diff:\n" + diff
        )

    if failures:
        print("builtin members: the checker's lists disagree with TypeScript")
        for f in failures:
            print("  " + f)
        return 1
    print(
        f"builtin members OK: {len(table['array'])} array, {len(table['string'])} string and "
        f"{len(table['number'])} number members match TypeScript {got['version']} "
        f"(esnext + dom, with Object's), and {rel} is what it generates."
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
