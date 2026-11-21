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

Each row also says whether the method changes its receiver (G380, D5). That
is the one fact the rows carry that TypeScript's library does not hold: no
declaration in `lib.d.ts` marks a method as mutating. So the split is written
here by hand, as `MUTATING` and `NON_MUTATING` per receiver, and kept honest
two ways. Generation fails when a method TypeScript declares is in neither set
or in both, so a TypeScript bump that adds a method fails CI until a person
classifies it. And node runs every array method with sample arguments on a
fresh array and checks the receiver changed exactly when the method is in
`MUTATING`, so the hand split is checked against the engine that runs the
code rather than against memory.

Each row also says whether the method can answer `undefined` or `null`
(G392, D56): `absent`, true when any overload's return type holds either.
Unlike `mutates` this is read off the declarations, not written by hand. What
is written by hand is `ABSENT_REPLACEMENT`, the `std/*` function E0242 sends
a reader to, and generation fails on an absent row without an entry.

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
    // Whether any overload can answer `undefined` or `null` (G392, D56): the
    // call hands back a value Glyph's checker cannot see is missing.
    const absent = sigs.some((s) => {
      const r = checker.getReturnTypeOfSignature(s);
      return (r.isUnion() ? r.types : [r]).some((u) => u.flags & (ts.TypeFlags.Undefined | ts.TypeFlags.Null));
    });
    const decl = sig.declaration;
    let untyped = null;
    if (sigs.length > 1) untyped = "Overloads";
    else if (sig.thisParameter || hasThisType(decl)) untyped = "ThisType";
    else if (checker.getTypePredicateOfSignature(sig)) untyped = "Predicate";
    else if (decl.parameters.some((q) => q.dotDotDotToken)) untyped = "RestParameter";
    if (untyped) { out[n] = { untyped, absent }; continue; }
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
    out[n] = { params, ret: map(checker.getReturnTypeOfSignature(sig)), absent };
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


# Which methods change their receiver (G380, D5). Every method TypeScript
# declares on the receiver, `Object`'s included, is in exactly one of the two
# sets; `classification_failures` enforces it. A mutating method may stand
# only as the whole call of a `mut` statement (E0240), and `mut` before a
# non-mutating one is E0241, so a row in the wrong set is either a false
# refusal or a write the `^\s*mut ` audit misses.
MUTATING = {
    "array": {"copyWithin", "fill", "pop", "push", "reverse", "shift", "sort", "splice", "unshift"},
    # A string and a number are immutable primitives: no method can change one.
    "string": set(),
    "number": set(),
}
OBJECT_METHODS = {
    "hasOwnProperty", "isPrototypeOf", "propertyIsEnumerable", "toLocaleString", "toString", "valueOf",
}
NON_MUTATING = {
    "array": OBJECT_METHODS | {
        "at", "concat", "entries", "every", "filter", "find", "findIndex", "findLast",
        "findLastIndex", "flat", "flatMap", "forEach", "includes", "indexOf", "join", "keys",
        "lastIndexOf", "map", "reduce", "reduceRight", "slice", "some", "toReversed", "toSorted",
        "toSpliced", "values", "with",
    },
    "string": OBJECT_METHODS | {
        "anchor", "at", "big", "blink", "bold", "charAt", "charCodeAt", "codePointAt", "concat",
        "endsWith", "fixed", "fontcolor", "fontsize", "includes", "indexOf", "isWellFormed",
        "italics", "lastIndexOf", "link", "localeCompare", "match", "matchAll", "normalize",
        "padEnd", "padStart", "repeat", "replace", "replaceAll", "search", "slice", "small",
        "split", "startsWith", "strike", "sub", "substr", "substring", "sup",
        "toLocaleLowerCase", "toLocaleUpperCase", "toLowerCase", "toUpperCase", "toWellFormed",
        "trim", "trimEnd", "trimLeft", "trimRight", "trimStart",
    },
    "number": OBJECT_METHODS | {"toExponential", "toFixed", "toPrecision"},
}

# The `Option` spelling of every method whose call can answer `undefined` or
# `null` (G392, D56). The `absent` column is read off `lib.d.ts`, so it is not
# kept by hand; what is kept by hand is where E0242 sends a reader, and
# `absent_failures` refuses an absent row with no entry here. A TypeScript
# that adds such a method then fails CI until someone names its replacement.
# `pop` and `shift` are "E0240": they change the array, E0240 already refuses
# their value, and nothing judges them a second time.
ABSENT_REPLACEMENT = {
    "array": {
        "at": "array.get",
        "find": "array.find",
        "findLast": "array.find_last",
        "pop": "E0240",
        "shift": "E0240",
    },
    "string": {
        "at": "string.char_at",
        "codePointAt": "string.code_point_at",
        "match": "regex.matches, or regex.captures for the groups",
    },
    "number": {},
}


def absent_failures(got: dict) -> list[str]:
    """Every absent row has a replacement, every replacement an absent row,
    and only a mutating method is left to E0240."""
    failures = []
    for kind in ("array", "string", "number"):
        absent = {name for name, row in got[kind].items() if row["absent"]}
        table = ABSENT_REPLACEMENT[kind]
        for name in sorted(absent - set(table)):
            failures.append(
                f"{kind}: `{name}` can return `undefined` or `null` and has no entry in "
                "ABSENT_REPLACEMENT; name the std function that returns its `Option` (E0242)"
            )
        for name in sorted(set(table) - absent):
            failures.append(f"{kind}: `{name}` is in ABSENT_REPLACEMENT and TypeScript declares no absent return for it")
        for name, repl in sorted(table.items()):
            if (repl == "E0240") != (name in MUTATING[kind]):
                failures.append(
                    f"{kind}: `{name}` is left to E0240 in ABSENT_REPLACEMENT, and only a mutating method can be"
                    if repl == "E0240"
                    else f"{kind}: `{name}` changes its receiver, so its replacement is E0240's"
                )
    return failures


# The run-time oracle for the array split. Each method is called once, with
# the sample arguments below, on a fresh `[3, 1, 2]` (unsorted, not a
# palindrome, so `sort` and `reverse` change it), and the receiver is compared
# before and after. A method with no sample here fails the oracle, so a new
# method cannot slip past it unclassified at run time either.
ARRAY_ORACLE = r"""
const samples = {
  at: [0], concat: [[4]], copyWithin: [0, 1], entries: [], every: [(x) => true],
  fill: [0], filter: [(x) => true], find: [(x) => false], findIndex: [(x) => false],
  findLast: [(x) => false], findLastIndex: [(x) => false], flat: [], flatMap: [(x) => [x]],
  forEach: [(x) => {}], hasOwnProperty: ["0"], includes: [1], indexOf: [1],
  isPrototypeOf: [{}], join: [","], keys: [], lastIndexOf: [1], map: [(x) => x], pop: [],
  propertyIsEnumerable: ["0"], push: [4], reduce: [(a, b) => a + b, 0],
  reduceRight: [(a, b) => a + b, 0], reverse: [], shift: [], slice: [1], some: [(x) => false],
  sort: [], splice: [0, 1], toLocaleString: [], toReversed: [], toSorted: [], toSpliced: [0, 1],
  toString: [], unshift: [0], valueOf: [], values: [], with: [0, 9],
};
const methods = JSON.parse(process.argv[1]);
const out = {};
for (const m of methods) {
  if (!(m in samples)) { out[m] = "no-sample"; continue; }
  const xs = [3, 1, 2];
  const before = JSON.stringify(xs);
  xs[m](...samples[m]);
  out[m] = JSON.stringify(xs) !== before;
}
console.log(JSON.stringify(out));
"""


def classification_failures(got: dict) -> list[str]:
    """Every method in exactly one of `MUTATING` and `NON_MUTATING`, and the
    array split confirmed by running each method in node."""
    failures = []
    for kind in ("array", "string", "number"):
        methods = set(got[kind])
        mut, pure = MUTATING[kind], NON_MUTATING[kind]
        for name in sorted(methods - mut - pure):
            failures.append(
                f"{kind}: `{name}` is in neither MUTATING nor NON_MUTATING; classify it "
                "(does calling it change the receiver?)"
            )
        for name in sorted(mut & pure):
            failures.append(f"{kind}: `{name}` is in both MUTATING and NON_MUTATING")
        for name in sorted((mut | pure) - methods):
            failures.append(f"{kind}: `{name}` is classified but TypeScript declares no such method")
    if failures:
        return failures
    r = subprocess.run(
        ["node", "-e", ARRAY_ORACLE, json.dumps(sorted(got["array"]))],
        capture_output=True, text=True, timeout=60,
    )
    if r.returncode != 0:
        return ["the array mutation oracle failed to run:\n" + r.stderr]
    for name, changed in sorted(json.loads(r.stdout).items()):
        if changed == "no-sample":
            failures.append(f"array: `{name}` has no sample arguments in ARRAY_ORACLE; add one")
        elif changed and name not in MUTATING["array"]:
            failures.append(f"array: `{name}` changed its receiver in node and is not in MUTATING")
        elif not changed and name in MUTATING["array"]:
            failures.append(f"array: `{name}` is in MUTATING and left its receiver unchanged in node")
    return failures


def rust_ty(ty: str) -> str:
    """`ArrayOf(Elem)` as the Rust expression the row holds."""
    if ty.startswith("ArrayOf(") and ty.endswith(")"):
        return f"T::ArrayOf(&{rust_ty(ty[len('ArrayOf('):-1])})"
    return f"T::{ty}"


def render_methods(got: dict) -> str:
    """The whole of `builtin_methods.rs`, from the method probe's answer and
    the hand split into `MUTATING` and `NON_MUTATING`."""
    lines = [
        "// @generated by `python3 scripts/check_builtin_members.py --write`. Do not edit:",
        "// CI renders this file again and fails when the committed copy differs.",
        "",
        "//! The methods of a JavaScript array, string and number, with the signature",
        f"//! TypeScript {got['version']}'s library (`lib: [\"esnext\", \"dom\"]`) declares for",
        "//! each, mapped by the rules `builtin_members.rs` documents above",
        "//! `BuiltinMethod` (G312). Sorted by name: `BuiltinReceiver::method`",
        "//! binary-searches. `mutates` is the script's `MUTATING` set (G380), which",
        "//! node confirms at generation by calling each array method. `absent` is",
        "//! read off the declarations: some overload's return holds `undefined` or",
        "//! `null` (G392, D56), and the script's `ABSENT_REPLACEMENT` names the",
        "//! `Option` spelling for every such row.",
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
            mutates = "true" if name in MUTATING[kind] else "false"
            absent = "true" if row["absent"] else "false"
            lines.append(
                f'    BuiltinMethod {{ name: "{name}", mutates: {mutates}, absent: {absent}, sig: {sig} }},'
            )
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
    methods = json.loads(r.stdout)
    classified = classification_failures(methods)
    if classified:
        print("builtin members: the mutation split is incomplete or wrong")
        for f in classified:
            print("  " + f)
        return 1
    missing = absent_failures(methods)
    if missing:
        print("builtin members: a method that can return `undefined` or `null` has no `Option` spelling")
        for f in missing:
            print("  " + f)
        return 1
    rendered = render_methods(methods)
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
        f"(esnext + dom, with Object's), every method is classified mutating or not and node "
        f"agrees on the array split, every absent-returning method has an `Option` spelling, "
        f"and {rel} is what it generates."
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
