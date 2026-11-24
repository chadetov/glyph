#!/usr/bin/env python3
"""The checker's array, string, number, boolean and bigint member lists must be TypeScript's.

`glyph-typechecker/src/builtin_members.rs` holds every member a JavaScript
array, string, number and bigint have, and the checker refuses a member read
on `Array<T>`, `string`, `number` or `bigint` whose name is not in them
(G288, E0210; `bigint` since G500). The boolean list is read by the relation
alone (G502). The lists were
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

Each row also carries the text of the method's `@deprecated` tag, read off
the declarations (G389, E0244). The tag rarely names a replacement, so what
E0244 sends a reader to is written by hand in `DEPRECATED_REPLACEMENT`, and
generation fails when a deprecated method has no entry, an entry names a
method TypeScript does not deprecate, a replacement is absent or deprecated
itself, a tag's "Use `X`" disagrees with the entry, or node disagrees with it.

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
ARRAY_RE = re.compile(r"const (ARRAY|STRING|NUMBER|BOOLEAN|BIGINT)_MEMBERS: &\[&str\] = &\[(.*?)\];", re.S)

PROBE = r"""
const ts = require(process.argv[1]);
const file = "probe.ts";
const text = "declare const a: string[];\ndeclare const s: string;\ndeclare const n: number;\ndeclare const b: boolean;\ndeclare const g: bigint;\n";
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
  out[{ a: "array", s: "string", n: "number", b: "boolean", g: "bigint" }[decl.name.text]] = names;
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
# overloads, a `this` type, or a type predicate leave the whole method untyped
# with the reason; any position outside the closed set below is `Unknown`. A
# rest parameter is the row's `rest`, its element type mapped like any other
# position (G492). A set of overloads holding a rest parameter is merged into
# one row when the merge is exact, which `mergeSigs` states. An `Array<T>`
# method whose only `this` is its return type (`fill`, `sort`) returns `T[]`.
# A `this` return reads as `T[]` once the checker has resolved it, so it is
# found in the declaration's syntax, not in the resolved type.
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
// The text of a member's `@deprecated` tag, or null (G389, E0244). Read off
// the property's own declarations, so the tag is the library's words.
const deprecation = (p) => {
  const tag = p.getJsDocTags(checker).find((t) => t.name === "deprecated");
  return tag ? ts.displayPartsToString(tag.text || []).trim() : null;
};
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
const deprecatedProperties = [];
function methods(name) {
  const declared = checker.getDeclaredTypeOfSymbol(scope.find((s) => s.name === name));
  const elem = (declared.typeParameters || [])[0];
  const map = (t) => {
    if (t.flags & ts.TypeFlags.Number) return "Number";
    if (t.flags & ts.TypeFlags.BigInt) return "BigInt";
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
    // property is read, never called. A deprecated data property has no row
    // to carry its tag, so it is reported for the script to refuse.
    if (n.startsWith("__@")) continue;
    if (!(p.flags & ts.SymbolFlags.Method)) {
      if (deprecation(p) !== null) deprecatedProperties.push(`${name}.${n}`);
      continue;
    }
    const deprecated = deprecation(p);
    const sigs = checker.getSignaturesOfType(checker.getTypeOfSymbol(p), ts.SignatureKind.Call);
    const sig = sigs[0];
    // Whether any overload can answer `undefined` or `null` (G392, D56): the
    // call hands back a value Glyph's checker cannot see is missing.
    const absent = sigs.some((s) => {
      const r = checker.getReturnTypeOfSignature(s);
      return (r.isUnion() ? r.types : [r]).some((u) => u.flags & (ts.TypeFlags.Undefined | ts.TypeFlags.Null));
    });
    const read = readSig(sig);
    if (sigs.length > 1) {
      // Overloads are TypeScript's to resolve, with one exception: a set
      // holding a rest parameter (`splice`) is merged into one row when the
      // merge accepts exactly the calls the overloads accept between them.
      const merged = sigs.some((s) => s.declaration.parameters.some((q) => q.dotDotDotToken)) && mergeSigs(sigs);
      out[n] = merged ? { ...merged, absent, deprecated } : { untyped: "Overloads", absent, deprecated };
      continue;
    }
    out[n] = { ...read, absent, deprecated };
  }
  return out;
  // One signature as a row: its parameters, its rest parameter's element
  // type when it has one (G492), and its return, or the reason it is left
  // untyped. A `this` return is the receiver's own type, which for
  // `Array<T>` is `T[]` (`fill`, `sort`); a `this` anywhere else is untyped.
  function readSig(sig) {
    const decl = sig.declaration;
    const thisReturn = !!decl.type && decl.type.kind === ts.SyntaxKind.ThisType
      && !decl.parameters.some(hasThisType) && name === "Array";
    if (sig.thisParameter || (hasThisType(decl) && !thisReturn)) return { untyped: "ThisType" };
    if (checker.getTypePredicateOfSignature(sig)) return { untyped: "Predicate" };
    let rest = null;
    const params = [];
    sig.getParameters().forEach((q, i) => {
      const pd = decl.parameters[i];
      if (pd.dotDotDotToken) {
        const t = checker.getTypeOfSymbol(q);
        rest = checker.isArrayType(t) ? map(checker.getTypeArguments(t)[0]) : "Unknown";
        return;
      }
      const optional = !!(pd.questionToken || pd.initializer);
      let t = checker.getTypeOfSymbol(q);
      // Under `strict` an optional parameter's type carries `undefined`; the
      // row says optional instead, so the `undefined` is dropped and what is
      // left is mapped. `boolean` is itself the union `true | false`.
      if (optional && t.isUnion()) {
        const left = t.types.filter((u) => !(u.flags & ts.TypeFlags.Undefined));
        if (left.length === 1) t = left[0];
        else if (left.length > 0 && left.every((u) => u.flags & ts.TypeFlags.BooleanLiteral)) {
          params.push({ ty: "Bool", optional });
          return;
        } else {
          params.push({ ty: "Unknown", optional });
          return;
        }
      }
      params.push({ ty: map(t), optional });
    });
    const ret = thisReturn ? "ArrayOf(Elem)" : map(checker.getReturnTypeOfSignature(sig));
    return rest === null ? { params, ret } : { params, rest, ret };
  }
  // The overloads as one row, or null. Each must read as a row and declare
  // no type parameter of its own; the argument counts they accept must join
  // into one unbroken range, so a count none of them accepts is never let
  // through or refused by the merge alone; and a position, the rest element
  // and the return keep a type only where every overload that has one agrees
  // on it, and are `Unknown` otherwise. A call the merged row refuses is then
  // refused by every overload whose count it fits.
  function mergeSigs(sigs) {
    const rows = [];
    for (const s of sigs) {
      if (s.typeParameters && s.typeParameters.length) return null;
      const r = readSig(s);
      if (r.untyped) return null;
      rows.push(r);
    }
    const counts = rows
      .map((r) => [r.params.filter((q) => !q.optional).length, r.rest ? Infinity : r.params.length])
      .sort((a, b) => a[0] - b[0]);
    let reach = counts[0][1];
    for (const [lo, hi] of counts.slice(1)) {
      if (lo > reach + 1) return null;
      reach = Math.max(reach, hi);
    }
    const floor = counts[0][0];
    const agree = (tys) => (tys.every((t) => t === tys[0]) ? tys[0] : "Unknown");
    const width = Math.max(...rows.map((r) => r.params.length));
    const params = [];
    for (let i = 0; i < width; i++) {
      const at = rows.map((r) => (i < r.params.length ? r.params[i].ty : r.rest)).filter((t) => t);
      params.push({ ty: agree(at), optional: i >= floor });
    }
    const rests = rows.filter((r) => r.rest).map((r) => r.rest);
    const merged = { params, ret: agree(rows.map((r) => r.ret)) };
    if (rests.length) merged.rest = agree(rests);
    return merged;
  }
}
const object = methods("Object");
const result = { version: ts.version };
for (const [key, name] of [["array", "Array"], ["string", "String"], ["number", "Number"], ["boolean", "Boolean"], ["bigint", "BigInt"]]) {
  result[key] = { ...object, ...methods(name) };
}
result.deprecatedProperties = [...new Set(deprecatedProperties)].sort();
console.log(JSON.stringify(result));
"""


# Which methods change their receiver (G380, D5). Every method TypeScript
# declares on the receiver, `Object`'s included, is in exactly one of the two
# sets; `classification_failures` enforces it. A mutating method may stand
# only as the whole call of a `mut` statement (E0240), and `mut` before a
# non-mutating one is E0241, so a row in the wrong set is either a false
# refusal or a write the `^\s*mut ` audit misses.
# The five receivers. `boolean` is read for one rule only: a `bool` compared
# with an interface through its apparent members (G502, decision record 196).
# A member read on a `bool` is not checked against it (G507). `bigint` is a
# receiver of its own (G500, D38).
KINDS = ("array", "string", "number", "boolean", "bigint")

MUTATING = {
    "array": {"copyWithin", "fill", "pop", "push", "reverse", "shift", "sort", "splice", "unshift"},
    # A string and a number are immutable primitives: no method can change one.
    "string": set(),
    "number": set(),
    "boolean": set(),
    "bigint": set(),
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
    "boolean": OBJECT_METHODS,
    "bigint": OBJECT_METHODS,
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
    "boolean": {},
    "bigint": {},
}


def absent_failures(got: dict) -> list[str]:
    """Every absent row has a replacement, every replacement an absent row,
    and only a mutating method is left to E0240."""
    failures = []
    for kind in KINDS:
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


# What E0244 sends a reader to for every member TypeScript marks
# `@deprecated` (G389). The tag itself is read off `lib.d.ts` and written into
# the row; only 2 of the 16 tags TypeScript 6.0.3 carries name a replacement
# (`trimLeft`, `trimRight`), so the replacement is written here by hand and
# held to the tags and to node by `deprecated_failures`. A TypeScript that
# deprecates another member fails CI until someone writes its entry.
#
# - ("Rename", m): the same call under the name `m`, which the language
#   defines as the same function (`trimLeft` is `trimStart`).
# - ("Slice", "slice"): `substr(start, length)` is `slice(start, start +
#   length)` for a start and a length that are whole numbers of zero or
#   more; the help computes the end.
# - ("Markup", tag, attr): the HTML method builds `<tag attr="arg">s</tag>`,
#   which a template string writes directly.
DEPRECATED_REPLACEMENT = {
    "array": {},
    "string": {
        "anchor": ("Markup", "a", "name"),
        "big": ("Markup", "big", None),
        "blink": ("Markup", "blink", None),
        "bold": ("Markup", "b", None),
        "fixed": ("Markup", "tt", None),
        "fontcolor": ("Markup", "font", "color"),
        "fontsize": ("Markup", "font", "size"),
        "italics": ("Markup", "i", None),
        "link": ("Markup", "a", "href"),
        "small": ("Markup", "small", None),
        "strike": ("Markup", "strike", None),
        "sub": ("Markup", "sub", None),
        "substr": ("Slice", "slice"),
        "sup": ("Markup", "sup", None),
        "trimLeft": ("Rename", "trimStart"),
        "trimRight": ("Rename", "trimEnd"),
    },
    "number": {},
    "boolean": {},
    "bigint": {},
}

# A tag that names its replacement says so as "Use `X` instead".
TAG_USE = re.compile(r"Use `(\w+)`")

# The run-time check on the hand entries: a rename is the same function, a
# `Markup` method builds exactly the markup the entry names, and `substr` and
# `slice` read the same characters for every start and length that are whole
# numbers of zero or more. The condition is pinned from both sides: a pair
# outside it (a fractional start and length, a negative length) must come out
# different, so a help that drops "whole" or "zero or more" fails here. With
# one argument the two agree for every start, a fractional one included.
DEPRECATED_ORACLE = r"""
const table = JSON.parse(process.argv[1]);
const out = [];
const recv = 'a&"b';
for (const [name, entry] of Object.entries(table.string)) {
  const [kind] = entry;
  if (kind === "Rename") {
    if (String.prototype[name] !== String.prototype[entry[1]]) out.push(`string: \`${name}\` is not the same function as \`${entry[1]}\` in node`);
  } else if (kind === "Markup") {
    const [, tag, attr] = entry;
    const arg = 'q"r';
    const want = attr === null ? `<${tag}>${recv}</${tag}>` : `<${tag} ${attr}="q&quot;r">${recv}</${tag}>`;
    const got = attr === null ? recv[name]() : recv[name](arg);
    if (got !== want) out.push(`string: \`${name}\` builds ${JSON.stringify(got)} in node, and the entry says ${JSON.stringify(want)}`);
  } else if (kind === "Slice") {
    const s = "abcdefg";
    for (let a = 0; a <= 9; a++) for (let n = 0; n <= 9; n++) {
      if (s.substr(a, n) !== s.slice(a, a + n)) out.push(`string: \`substr(${a}, ${n})\` and \`slice(${a}, ${a + n})\` differ in node`);
    }
    for (let a = -9.5; a <= 9.5; a += 0.5) if (s.substr(a) !== s.slice(a)) out.push(`string: \`substr(${a})\` and \`slice(${a})\` differ in node`);
    for (const [a, n] of [[0.5, 0.6], [0, -1]]) {
      if (s.substr(a, n) === s.slice(a, a + n)) out.push(`string: \`substr(${a}, ${n})\` and \`slice(${a}, ${a + n})\` agree in node, and the help's condition says they need not`);
    }
  } else {
    out.push(`string: \`${name}\` has an entry of unknown kind ${kind}`);
  }
}
console.log(JSON.stringify(out));
"""


def deprecated_failures(got: dict) -> list[str]:
    """Every deprecated method has an entry and every entry a deprecated
    method; a replacement method exists and is not itself deprecated; a tag
    that names its replacement agrees with the entry; and node agrees with
    each entry."""
    failures = [
        f"`{p}` is a data property TypeScript marks @deprecated, and no row carries a data property's tag"
        for p in got.get("deprecatedProperties", [])
    ]
    for kind in KINDS:
        tags = {name: row["deprecated"] for name, row in got[kind].items() if row.get("deprecated") is not None}
        table = DEPRECATED_REPLACEMENT[kind]
        for name in sorted(set(tags) - set(table)):
            failures.append(
                f"{kind}: `{name}` is marked @deprecated ({tags[name]!r}) and has no entry in "
                "DEPRECATED_REPLACEMENT; write what E0244 should send a reader to"
            )
        for name in sorted(set(table) - set(tags)):
            failures.append(f"{kind}: `{name}` is in DEPRECATED_REPLACEMENT and TypeScript does not mark it @deprecated")
        for name, entry in sorted(table.items()):
            if entry[0] in ("Rename", "Slice"):
                target = entry[1]
                if target not in got[kind]:
                    failures.append(f"{kind}: `{name}`'s replacement `{target}` is not a method TypeScript declares")
                elif target in tags:
                    failures.append(f"{kind}: `{name}`'s replacement `{target}` is itself marked @deprecated")
            elif entry[0] != "Markup":
                failures.append(f"{kind}: `{name}` has an entry of unknown kind {entry[0]!r}")
            m = TAG_USE.search(tags.get(name) or "")
            if m and entry != ("Rename", m.group(1)):
                failures.append(
                    f"{kind}: `{name}`'s tag says to use `{m.group(1)}`, and DEPRECATED_REPLACEMENT says {entry!r}"
                )
    if failures:
        return failures
    r = subprocess.run(
        ["node", "-e", DEPRECATED_ORACLE, json.dumps(DEPRECATED_REPLACEMENT)],
        capture_output=True, text=True, timeout=60,
    )
    if r.returncode != 0:
        return ["the deprecated-member oracle failed to run:\n" + r.stderr]
    return json.loads(r.stdout)


def rust_str(text: str) -> str:
    """`text` as a Rust string literal."""
    out = []
    for ch in text:
        if ch in '"\\':
            out.append("\\" + ch)
        elif ch == "\n":
            out.append("\\n")
        elif ord(ch) < 0x20 or ord(ch) > 0x7E:
            out.append(f"\\u{{{ord(ch):x}}}")
        else:
            out.append(ch)
    return '"' + "".join(out) + '"'


def rust_deprecated(kind: str, name: str, row: dict) -> str:
    """The row's `deprecated` column: the tag and the hand replacement."""
    tag = row.get("deprecated")
    if tag is None:
        return "None"
    entry = DEPRECATED_REPLACEMENT[kind][name]
    if entry[0] == "Rename":
        repl = f"R::Rename({rust_str(entry[1])})"
    elif entry[0] == "Slice":
        repl = "R::Slice"
    else:
        attr = "None" if entry[2] is None else f"Some({rust_str(entry[2])})"
        repl = f"R::Markup {{ tag: {rust_str(entry[1])}, attr: {attr} }}"
    return f"Some(Deprecated {{ tag: {rust_str(tag)}, replacement: {repl} }})"


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
    for kind in KINDS:
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


# Which members of a modeled stdlib type change their receiver (G423 step 2,
# decision record 157, section 2b). The checker models the member tables of a
# handful of stdlib types (`stdlib.rs`, `stdlib_type_members`), and D5's audit
# reaches a write to one only if the member is known to make it. The runtime
# TypeScript carries no marker, so the split is written here by hand, in three
# classes keyed by (module, type, member):
#
# - Mutating: changes the value it is called on. Stands only as the whole call
#   of a `mut` statement, on a place (E0240), as an array's `push` does.
# - NonMutating: reads, or returns a new value. `mut` before it is E0241. A
#   `Decimal` is immutable, so `mut d.add(e)` throws its sum away.
# - Outside: writes state beyond the program's data (record 150 (e)): a
#   database. Never E0240 and never E0241; that is the effects questions'.
#
# `stdlib_failures` holds the sets to the runtime's declared types, and the
# oracle below runs each member of a fresh instance in node. The Rust unit test
# beside the generated table holds it to the checker's own member tables, so a
# modeled type added there fails CI until it is classified here.
STDLIB_MUTATING = {
    ("set", "Set"): {"add", "remove"},
    ("store", "Store"): {"set", "update"},
    ("collections", "Deque"): {"pop_back", "pop_front", "push_back", "push_front"},
    ("decimal", "Decimal"): set(),
    ("sqlite", "Db"): set(),
}
STDLIB_NON_MUTATING = {
    ("set", "Set"): {"has", "size", "values"},
    ("store", "Store"): {"get"},
    ("collections", "Deque"): {"len", "peek_back", "peek_front", "values"},
    ("decimal", "Decimal"): {
        "abs", "add", "cmp", "div", "eq", "is_negative", "is_zero", "mul", "neg", "round", "scale", "sub",
        "to_number", "to_string",
    },
    ("sqlite", "Db"): set(),
}
STDLIB_OUTSIDE = {
    ("set", "Set"): set(),
    ("store", "Store"): set(),
    ("collections", "Deque"): set(),
    ("decimal", "Decimal"): set(),
    ("sqlite", "Db"): {"close", "exec", "last_insert_id", "query", "query_one", "run"},
}
STDLIB_CLASSES = (("Mutating", STDLIB_MUTATING), ("NonMutating", STDLIB_NON_MUTATING), ("Outside", STDLIB_OUTSIDE))
RUNTIME_STD = ROOT / "glyph-compiler/runtime/std"
STDLIB_CLASS_TABLE = ROOT / "glyph-compiler/crates/glyph-typechecker/src/stdlib_member_classes.rs"

# The member names each classified type declares in the runtime TypeScript,
# read with the same checker the method rows use.
STDLIB_MEMBER_PROBE = r"""
const ts = require(process.argv[1]);
const dir = process.argv[2];
const wanted = JSON.parse(process.argv[3]);
const files = [...new Set(wanted.map(([m]) => `${dir}/${m}.ts`))];
const program = ts.createProgram(files, { strict: true, noEmit: true, target: ts.ScriptTarget.ESNext });
const checker = program.getTypeChecker();
const out = {};
for (const [m, name] of wanted) {
  const sf = program.getSourceFile(`${dir}/${m}.ts`);
  const sym = sf && checker.getSymbolAtLocation(sf);
  const exp = sym && checker.getExportsOfModule(sym).find((e) => e.name === name);
  out[`${m}.${name}`] = exp ? checker.getPropertiesOfType(checker.getDeclaredTypeOfSymbol(exp)).map((p) => p.getName()) : null;
}
console.log(JSON.stringify(out));
"""

# The run-time oracle for the stdlib split. The runtime modules are compiled
# with TypeScript's own transpiler and loaded in memory, so the oracle runs the
# code a Glyph program runs. Each member is called once, with samples chosen
# to cause a change if the member can make one (remove a present value, pop a
# non-empty deque, `update` with `n + 1`), on a fresh instance holding
# `[1, 2, 3]`, and the instance's state is snapshotted deeply before and after.
# A member with no sample here fails, so a member added to the runtime cannot
# slip past unclassified. `values()` must hand back a copy, and no `Decimal`
# member may change the value it is called on. `Db` is Outside and not run.
STDLIB_ORACLE = r"""
const ts = require(process.argv[1]);
const fs = require("fs");
const path = require("path");
const dir = process.argv[2];
const classes = JSON.parse(process.argv[3]);
const cache = {};
function load(name) {
  if (cache[name]) return cache[name].exports;
  const src = fs.readFileSync(path.join(dir, name + ".ts"), "utf8");
  const js = ts.transpileModule(src, { compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2022 } }).outputText;
  const mod = { exports: {} };
  cache[name] = mod;
  new Function("require", "module", "exports", js)((p) => load(p.replace(/^\.\//, "").replace(/\.(js|ts)$/, "")), mod, mod.exports);
  return mod.exports;
}
const set = load("set"), store = load("store"), collections = load("collections"), decimal = load("decimal");
const dec = (t) => { const r = decimal.decimal(t); if (r.tag !== "Ok") throw new Error("decimal " + t); return r.value; };
const types = {
  "set.Set": {
    make: () => set.create([1, 2, 3]),
    snap: (s) => JSON.stringify(s.values().slice().sort()),
    samples: { add: [4], has: [1], remove: [1], size: [], values: [] },
    copies: ["values"],
  },
  "store.Store": {
    make: () => store.create(1),
    snap: (s) => JSON.stringify(s.get()),
    samples: { get: [], set: [4], update: [(n) => n + 1] },
    copies: [],
  },
  "collections.Deque": {
    make: () => collections.deque([1, 2, 3]),
    snap: (q) => JSON.stringify(q.values()),
    samples: {
      push_back: [4], push_front: [0], pop_back: [], pop_front: [], peek_front: [], peek_back: [], len: [], values: [],
    },
    copies: ["values"],
  },
  "decimal.Decimal": {
    make: () => dec("10.50"),
    snap: (d) => JSON.stringify([d.to_string(), d.scale()]),
    samples: {
      add: [dec("1.25")], sub: [dec("1.25")], mul: [dec("2")], div: [dec("3"), 2], round: [1], neg: [], abs: [],
      cmp: [dec("1")], eq: [dec("1")], is_zero: [], is_negative: [], scale: [], to_string: [], to_number: [],
    },
    copies: [],
  },
};
const out = [];
for (const [key, t] of Object.entries(types)) {
  const cls = classes[key];
  if (!cls) { out.push(`${key}: the oracle runs it and the script classifies none of its members`); continue; }
  // Outside is for a type whose writes land outside the program's data (a
  // database). A type the oracle can build and snapshot holds its state in
  // the program, so none of its members can be Outside: a reader moved
  // there would pass the run below, which only sees "left it unchanged"
  // (record 171, F3).
  for (const [member, c] of Object.entries(cls)) {
    if (c === "Outside") out.push(`${key}: \`${member}\` is Outside, and the oracle holds this type's state in the program, so each member is Mutating or NonMutating`);
  }
  // Only the methods: a `Decimal` also carries its digits as data fields
  // (`_u`, `_s`) that its declared type does not name.
  const made = t.make();
  for (const member of Object.keys(made).filter((k) => typeof made[k] === "function")) {
    if (!(member in t.samples)) { out.push(`${key}: \`${member}\` has no sample arguments in STDLIB_ORACLE; add one`); continue; }
    const v = t.make();
    const before = t.snap(v);
    v[member](...t.samples[member]);
    const changed = t.snap(v) !== before;
    const c = cls[member];
    if (c === undefined) out.push(`${key}: \`${member}\` is a runtime member the script does not classify`);
    else if (changed && c !== "Mutating") out.push(`${key}: \`${member}\` changed its receiver in node and is ${c}, not Mutating`);
    else if (!changed && c === "Mutating") out.push(`${key}: \`${member}\` is Mutating and left its receiver unchanged in node`);
  }
  for (const member of t.copies) {
    const v = t.make();
    const before = t.snap(v);
    v[member]().push(99);
    if (t.snap(v) !== before) out.push(`${key}: changing the array \`${member}()\` returned changed the instance; it must return a copy`);
  }
}
for (const key of Object.keys(classes)) {
  if (!(key in types) && Object.values(classes[key]).some((c) => c !== "Outside")) {
    out.push(`${key}: a member is classified Mutating or NonMutating and STDLIB_ORACLE has no instance to run it on`);
  }
}
console.log(JSON.stringify(out));
"""


def stdlib_classes() -> dict[tuple[str, str], dict[str, str]]:
    """(module, type) -> {member: class}; a member in two classes is listed
    under both joined by `+`, for `stdlib_failures` to name."""
    out: dict[tuple[str, str], dict[str, str]] = {}
    for cls, table in STDLIB_CLASSES:
        for key, members in table.items():
            row = out.setdefault(key, {})
            for m in members:
                row[m] = cls if m not in row else row[m] + "+" + cls
    return out


def stdlib_failures(ts_dir: pathlib.Path) -> list[str]:
    """Each classified type's members in exactly one class, matching what the
    runtime TypeScript declares, and the split confirmed by running them."""
    failures = []
    keys = sorted(set(STDLIB_MUTATING) | set(STDLIB_NON_MUTATING) | set(STDLIB_OUTSIDE))
    for _, table in STDLIB_CLASSES:
        for key in sorted(set(keys) - set(table)):
            failures.append(f"{key[0]}.{key[1]}: missing from one of the three class tables; give it an empty set")
    classes = stdlib_classes()
    for key in keys:
        for m, c in sorted(classes.get(key, {}).items()):
            if "+" in c:
                failures.append(f"{key[0]}.{key[1]}: `{m}` is in more than one class ({c.replace('+', ', ')})")
    r = subprocess.run(
        ["node", "-e", STDLIB_MEMBER_PROBE, str(ts_dir), str(RUNTIME_STD), json.dumps(keys)],
        capture_output=True, text=True, timeout=120,
    )
    if r.returncode != 0:
        return failures + ["the stdlib member probe failed to run:\n" + r.stderr]
    declared = json.loads(r.stdout)
    for key in keys:
        names = declared.get(f"{key[0]}.{key[1]}")
        if names is None:
            failures.append(f"{key[0]}.{key[1]}: runtime/std/{key[0]}.ts exports no such type")
            continue
        have = set(classes.get(key, {}))
        for m in sorted(set(names) - have):
            failures.append(
                f"{key[0]}.{key[1]}: `{m}` is in none of Mutating, NonMutating and Outside; classify it "
                "(does calling it change the value it is called on?)"
            )
        for m in sorted(have - set(names)):
            failures.append(f"{key[0]}.{key[1]}: `{m}` is classified and the runtime type declares no such member")
    if failures:
        return failures
    by_key = {f"{k[0]}.{k[1]}": v for k, v in classes.items()}
    r = subprocess.run(
        ["node", "-e", STDLIB_ORACLE, str(ts_dir), str(RUNTIME_STD), json.dumps(by_key)],
        capture_output=True, text=True, timeout=120,
    )
    if r.returncode != 0:
        return ["the stdlib mutation oracle failed to run:\n" + r.stderr]
    return json.loads(r.stdout)


def render_stdlib_classes() -> str:
    """The whole of `stdlib_member_classes.rs`, from the three hand sets."""
    rows = sorted(
        (key[0], key[1], m, cls)
        for cls, table in STDLIB_CLASSES
        for key, members in table.items()
        for m in members
    )
    lines = [
        "// @generated by `python3 scripts/check_builtin_members.py --write`. Do not edit:",
        "// CI renders this file again and fails when the committed copy differs.",
        "",
        "//! Which members of a modeled stdlib type change their receiver (G423 step",
        "//! 2, D5), keyed by the type's module and name and the member. The split is",
        "//! the script's `STDLIB_MUTATING`, `STDLIB_NON_MUTATING` and `STDLIB_OUTSIDE`,",
        "//! held to the runtime TypeScript's declared members and confirmed in node",
        "//! by running each member on a fresh instance. Sorted: `stdlib_member_class`",
        "//! binary-searches.",
        "",
        "use crate::mutation::MemberClass::{self, Mutating, NonMutating, Outside};",
        "",
        "pub(crate) const STDLIB_MEMBER_CLASSES: &[(&str, &str, &str, MemberClass)] = &[",
    ]
    for module, ty, m, cls in rows:
        lines.append(f'    ("{module}", "{ty}", "{m}", {cls}),')
    lines.append("];")
    return "\n".join(lines) + "\n"


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
        "//! The methods of a JavaScript array, string, number, boolean and bigint, with the signature",
        f"//! TypeScript {got['version']}'s library (`lib: [\"esnext\", \"dom\"]`) declares for",
        "//! each, mapped by the rules `builtin_members.rs` documents above",
        "//! `BuiltinMethod` (G312). Sorted by name: `BuiltinReceiver::method`",
        "//! binary-searches. `mutates` is the script's `MUTATING` set (G380), which",
        "//! node confirms at generation by calling each array method. `absent` is",
        "//! read off the declarations: some overload's return holds `undefined` or",
        "//! `null` (G392, D56), and the script's `ABSENT_REPLACEMENT` names the",
        "//! `Option` spelling for every such row. `deprecated` is the text of the",
        "//! member's `@deprecated` tag, read off the declarations (G389, E0244), with",
        "//! the script's `DEPRECATED_REPLACEMENT` entry for what to write instead.",
        "",
        "use crate::builtin_members::MethodSig::{Rest, Typed, Untyped};",
        "use crate::builtin_members::MethodTy as T;",
        "use crate::builtin_members::Replacement as R;",
        "use crate::builtin_members::Untyped as U;",
        "use crate::builtin_members::{opt, req, BuiltinMethod, Deprecated};",
    ]
    for kind in KINDS:
        lines.append("")
        lines.append(f"pub(crate) const {kind.upper()}_METHODS: &[BuiltinMethod] = &[")
        for name in sorted(got[kind]):
            row = got[kind][name]
            if "untyped" in row:
                sig = f"Untyped(U::{row['untyped']})"
            elif "rest" in row:
                params = ", ".join(
                    f"{'opt' if p['optional'] else 'req'}({rust_ty(p['ty'])})" for p in row["params"]
                )
                sig = f"Rest {{ params: &[{params}], rest: {rust_ty(row['rest'])}, ret: {rust_ty(row['ret'])} }}"
            else:
                params = ", ".join(
                    f"{'opt' if p['optional'] else 'req'}({rust_ty(p['ty'])})" for p in row["params"]
                )
                sig = f"Typed {{ params: &[{params}], ret: {rust_ty(row['ret'])} }}"
            mutates = "true" if name in MUTATING[kind] else "false"
            absent = "true" if row["absent"] else "false"
            deprecated = rust_deprecated(kind, name, row)
            lines.append(
                f'    BuiltinMethod {{ name: "{name}", mutates: {mutates}, absent: {absent}, '
                f'deprecated: {deprecated}, sig: {sig} }},'
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
    if set(table) != set(KINDS):
        print(f"{TABLE.relative_to(ROOT)}: could not find ARRAY_MEMBERS, STRING_MEMBERS, NUMBER_MEMBERS, BOOLEAN_MEMBERS and BIGINT_MEMBERS.")
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
    for kind in KINDS:
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
    deprecated = deprecated_failures(methods)
    if deprecated:
        print("builtin members: a member TypeScript marks @deprecated and the replacement table disagree")
        for f in deprecated:
            print("  " + f)
        return 1
    stdlib = stdlib_failures(ts_dir)
    if stdlib:
        print("builtin members: the stdlib member split is incomplete or wrong")
        for f in stdlib:
            print("  " + f)
        return 1
    for path, rendered in ((METHODS, render_methods(methods)), (STDLIB_CLASS_TABLE, render_stdlib_classes())):
        rel = path.relative_to(ROOT)
        if write:
            path.write_text(rendered)
            print(f"wrote {rel}")
        elif not path.is_file() or path.read_text() != rendered:
            # A unified diff names the rows that moved, which is what a
            # reviewer of a TypeScript bump needs to read.
            committed = path.read_text().splitlines(keepends=True) if path.is_file() else []
            diff = "".join(
                difflib.unified_diff(committed, rendered.splitlines(keepends=True), str(rel), "regenerated")
            )
            failures.append(
                f"{rel} is not what TypeScript {got['version']} and the hand sets generate; "
                "run `python3 scripts/check_builtin_members.py --write` and review the diff:\n" + diff
            )

    if failures:
        print("builtin members: the checker's lists disagree with TypeScript")
        for f in failures:
            print("  " + f)
        return 1
    print(
        f"builtin members OK: {len(table['array'])} array, {len(table['string'])} string, "
        f"{len(table['number'])} number, {len(table['boolean'])} boolean and {len(table['bigint'])} bigint members match TypeScript {got['version']} "
        f"(esnext + dom, with Object's), every method is classified mutating or not and node "
        f"agrees on the array split, every absent-returning method has an `Option` spelling, "
        f"every deprecated method has a replacement that node confirms, "
        f"every member of a classified stdlib type is Mutating, NonMutating or Outside and node "
        f"agrees by running it, and {METHODS.relative_to(ROOT)} and "
        f"{STDLIB_CLASS_TABLE.relative_to(ROOT)} are what it generates."
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
