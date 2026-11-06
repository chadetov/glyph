#!/usr/bin/env python3
"""Every complete stdlib signature must agree with the runtime, both ways.

The checker types a call to `fs.read_text` or `array.map` from a row in
`glyph-typechecker/src/stdlib.rs`. The function that actually runs is the
TypeScript under `glyph-compiler/runtime/std/`. Nothing compared the two: a row
could declare a parameter the runtime does not take, or a return the runtime
does not produce, and every check the checker makes with it would be a check
against the wrong function. `tsc` on a full build sees the runtime and not the
row, so it cannot notice either.

This asks `tsc` directly. For every export whose signature in `glyph llms
--json` is complete (no `?`, so the row claims a type at every position), it

  1. writes the signature as a Glyph type alias, `pub type S3 = fn(string) ->
     Result<string, fs.FsError>`, and builds the module with the compiler under
     test, so the lowering to TypeScript is the emitter's own (`int`,
     `Nullable<T>`, `async fn` and the rest map exactly the way a program's do);
  2. quantifies the alias's type parameters onto the function, because a Glyph
     function type cannot yet be generic (generic `Ty::Fn` is a later release)
     and `tsc` compares two generic signatures by unifying them, which a generic
     alias of a plain function would not let it do;
  3. binds the runtime export to the declared type and a value of the declared
     type to the runtime export's type, one line each, and runs the pinned
     TypeScript over the result.

Runtime to declared fails when the runtime cannot be called the way the row
says it can. Declared to runtime fails when the row describes a function the
runtime is not, for example one with fewer parameters or a wider return.

A mismatch the ledger already records is listed in KNOWN with its gap number,
per row and direction. The check fails on a mismatch not in KNOWN, and on a
KNOWN entry that now passes, so a fix has to take its entry out with it.

`--llms-json FILE` reads the signatures from a saved `glyph llms --json`
instead of asking the binary. It exists to break the gate on purpose: edit one
signature in a copy and the check has to fail.

Exits 1 on any unexpected result, printing the row, the direction and `tsc`'s
own message.
"""

from __future__ import annotations

import argparse
import json
import os
import pathlib
import re
import shutil
import subprocess
import sys
import tempfile

import glyph_bin

ROOT = pathlib.Path(__file__).resolve().parent.parent
CI_TOOLS = ROOT / ".github/ci-tools"

RUNTIME_TO_DECLARED = "runtime -> declared"
DECLARED_TO_RUNTIME = "declared -> runtime"

# Mismatches the ledger records, as (module, export, direction) -> gap. Each one
# is a row in docs/dogfooding-gaps.md. Fixing the row or the runtime makes the
# entry pass, and the check then fails until the entry is removed here.
KNOWN: dict[tuple[str, str, str], str] = {
    # The row lets the two arrays differ in element type and returns the
    # first one's; the runtime takes one `T` for both.
    ("std/array", "concat", RUNTIME_TO_DECLARED): "G307",
    # A trailing optional parameter, which a Glyph function type cannot spell,
    # renders as required.
    ("std/string", "index_of", DECLARED_TO_RUNTIME): "G308",
    ("std/string", "pad_end", DECLARED_TO_RUNTIME): "G308",
    ("std/string", "pad_start", DECLARED_TO_RUNTIME): "G308",
    ("std/string", "slice", DECLARED_TO_RUNTIME): "G308",
    ("std/array", "slice", DECLARED_TO_RUNTIME): "G308, G309",
    # The runtime takes `ReadonlyArray<T>` where the row declares `Array<T>`.
    # Glyph has no readonly array, so no program can observe it.
    ("std/array", "any", DECLARED_TO_RUNTIME): "G309",
    ("std/array", "concat", DECLARED_TO_RUNTIME): "G309",
    ("std/array", "filter", DECLARED_TO_RUNTIME): "G309",
    ("std/array", "find", DECLARED_TO_RUNTIME): "G309",
    ("std/array", "flat_map", DECLARED_TO_RUNTIME): "G309",
    ("std/array", "fold", DECLARED_TO_RUNTIME): "G309",
    ("std/array", "fold_while", DECLARED_TO_RUNTIME): "G309",
    ("std/array", "get", DECLARED_TO_RUNTIME): "G309",
    ("std/array", "len", DECLARED_TO_RUNTIME): "G309",
    ("std/array", "map", DECLARED_TO_RUNTIME): "G309",
    ("std/array", "max", DECLARED_TO_RUNTIME): "G309",
    ("std/array", "max_by", DECLARED_TO_RUNTIME): "G309",
    ("std/array", "min", DECLARED_TO_RUNTIME): "G309",
    ("std/array", "min_by", DECLARED_TO_RUNTIME): "G309",
    ("std/array", "reverse", DECLARED_TO_RUNTIME): "G309",
    ("std/array", "sort", DECLARED_TO_RUNTIME): "G309",
    ("std/array", "sum", DECLARED_TO_RUNTIME): "G309",
    ("std/array", "try_fold", DECLARED_TO_RUNTIME): "G309",
    ("std/array", "zip", DECLARED_TO_RUNTIME): "G309",
}

# Names a rendered signature may carry unqualified that are types, not type
# parameters: the prelude's own, and the two the prelude declares ambiently for
# `std/json`. Anything else bare must be a single capital letter (a type
# parameter, as `display_ty` renders `Ty::Param`); a new bare name stops the
# check rather than being guessed at.
BARE_TYPES = {
    "Array", "Option", "Result", "Record", "Nullable", "Issue", "Schema",
    "string", "number", "int", "bool", "void", "unknown", "never", "fn", "async",
}

IDENT = re.compile(r"[A-Za-z_][A-Za-z0-9_]*(?:\.[A-Za-z_][A-Za-z0-9_]*)*")
FIELD_NAME = re.compile(r"\b[A-Za-z_][A-Za-z0-9_]*\??:")
STRING_LIT = re.compile(r'"(?:[^"\\]|\\.)*"')


def typescript_dir() -> pathlib.Path | None:
    """The pinned TypeScript: CI's locked tree, else NODE_PATH or the global prefix."""
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


def complete_rows(doc: dict) -> list[tuple[str, str, str]]:
    """(module, export, signature) for every value export with a complete signature."""
    rows = []
    for module in doc["stdlib"]["modules"]:
        for e in module["exports"]:
            sig = e.get("signature")
            if e["kind"] != "value" or sig is None or e.get("signature_partial"):
                continue
            if "?" in STRING_LIT.sub("", sig):
                # `signature_partial` is what says a row has holes; a `?` it
                # did not mention means the two disagree, and a row with a hole
                # has no declared type to compare at that position.
                raise SystemExit(f"{module['path']}::{e['name']}: `?` in a row llms calls complete: {sig}")
            rows.append((module["path"], e["name"], sig))
    return rows


def names_in(sig: str) -> tuple[list[str], set[str]]:
    """Type parameters in first-appearance order, and the modules qualified names use."""
    params: list[str] = []
    modules: set[str] = set()
    # Record field names (`{ ms: ... }`, `{ code?: ... }`) are not types.
    bare = FIELD_NAME.sub("", STRING_LIT.sub('""', sig))
    for tok in IDENT.findall(bare):
        if "." in tok:
            modules.add(tok.split(".", 1)[0])
        elif tok in BARE_TYPES:
            continue
        elif re.fullmatch(r"[A-Z]", tok):
            if tok not in params:
                params.append(tok)
        else:
            raise SystemExit(
                f"unrecognised bare name `{tok}` in `{sig}`: add it to BARE_TYPES if it is a "
                "type the prelude declares, or teach this script where it comes from"
            )
    return params, modules


def ns(module_path: str) -> str:
    return "rt_" + module_path.removeprefix("std/").replace("/", "_")


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.split("\n", 1)[0])
    ap.add_argument("--llms-json", type=pathlib.Path, help="read signatures from this file")
    args = ap.parse_args()

    if shutil.which("node") is None:
        print("node not found; this check runs TypeScript over the runtime.")
        return 1
    ts_dir = typescript_dir()
    if ts_dir is None:
        print("typescript not found (run `npm ci` in .github/ci-tools, or install it globally).")
        return 1
    ts_version = json.loads((ts_dir / "package.json").read_text())["version"]
    if ts_version != pinned_version():
        print(f"TypeScript at {ts_dir} is {ts_version}, but .github/ci-tools pins {pinned_version()}.")
        return 1

    binary = glyph_bin.resolve()
    if args.llms_json:
        doc = json.loads(args.llms_json.read_text())
    else:
        r = subprocess.run([str(binary), "llms", "--json"], capture_output=True, text=True, timeout=120)
        if r.returncode != 0:
            print("`glyph llms --json` failed:\n" + r.stderr)
            return 1
        doc = json.loads(r.stdout)

    rows = complete_rows(doc)
    if not rows:
        print("no complete stdlib rows found; the llms document changed shape.")
        return 1

    sig_lines = ["module sigs", ""]
    imports: set[str] = set()
    generics: dict[str, list[str]] = {}
    for i, (_, _, sig) in enumerate(rows):
        params, modules = names_in(sig)
        imports |= modules
        alias = f"S{i}"
        if params:
            generics[alias] = params
        head = f"pub type {alias}<{', '.join(params)}>" if params else f"pub type {alias}"
        sig_lines.append(f"{head} = {sig}")
    # The build copies only the runtime modules a program imports, so every
    # module under test is imported, not just the ones a signature names.
    imports |= {p.removeprefix("std/") for p, _, _ in rows}
    for m in sorted(imports, reverse=True):
        sig_lines.insert(2, f"import std/{m}")
    sig_lines.insert(2 + len(imports), "")

    with tempfile.TemporaryDirectory() as tmp:
        root = pathlib.Path(tmp)
        (root / "package.json").write_text(json.dumps(
            {"name": "stdlib-signature-check", "private": True, "type": "module", "glyph": {}}
        ) + "\n")
        (root / "src").mkdir()
        (root / "src/sigs.glyph").write_text("\n".join(sig_lines) + "\n")

        build = subprocess.run(
            [str(binary), "build", "src/", "--out", "dist/", "--no-tsc", "--no-test"],
            cwd=root, capture_output=True, text=True, timeout=300,
        )
        if build.returncode != 0:
            print("the declared signatures do not build as Glyph type aliases:\n")
            print((build.stdout + build.stderr).strip())
            return 1

        dist = root / "dist"
        emitted = (dist / "sigs.ts").read_text()
        # Move each alias's type parameters onto the function it names:
        # `type S0<T> = (a0: T) => T` becomes `type S0 = <T>(a0: T) => T`.
        for alias, params in generics.items():
            pat = re.compile(rf"^export type {alias}<{re.escape(', '.join(params))}> = ", re.M)
            emitted, n = pat.subn(f"export type {alias} = <{', '.join(params)}>", emitted)
            if n != 1:
                print(f"could not find the emitted alias `{alias}<{', '.join(params)}>` in sigs.ts.")
                return 1
        (dist / "sigs.ts").write_text(emitted)

        check = ['import type * as S from "./sigs";']
        for m in sorted({p for p, _, _ in rows}):
            check.append(f'import * as {ns(m)} from "./.glyph-runtime/{m}";')
        check.append("")
        line_of: dict[int, tuple[int, str]] = {}
        for i, (module, name, _) in enumerate(rows):
            value = f"{ns(module)}.{name}"
            check.append(f"// {module}::{name}")
            check.append(f"declare const d{i}: S.S{i};")
            check.append(f"export const f{i}: S.S{i} = {value};")
            line_of[len(check)] = (i, RUNTIME_TO_DECLARED)
            check.append(f"export const r{i}: typeof {value} = d{i};")
            line_of[len(check)] = (i, DECLARED_TO_RUNTIME)
        (dist / "stdlib_check.ts").write_text("\n".join(check) + "\n")

        tsc = subprocess.run(
            ["node", str(ts_dir / "bin/tsc"), "-p", str(dist), "--pretty", "false"],
            cwd=root, capture_output=True, text=True, timeout=600,
        )
        out = tsc.stdout + tsc.stderr

    # Group tsc's output into diagnostics: a located first line plus its
    # indented elaboration.
    failed: dict[tuple[int, str], list[str]] = {}
    stray: list[str] = []
    current: list[str] | None = None
    for line in out.splitlines():
        m = re.match(r"^(.*?)\((\d+),(\d+)\): error (TS\d+): (.*)$", line)
        if m:
            where = line_of.get(int(m.group(2))) if m.group(1).endswith("stdlib_check.ts") else None
            if where is None:
                current = stray
                stray.append(line)
            else:
                current = failed.setdefault(where, [])
                current.append(f"{m.group(4)}: {m.group(5)}")
        elif line.strip() and current is not None:
            current.append(line.strip())
        elif line.strip():
            stray.append(line)
    if stray:
        print("tsc reported errors outside the signature bindings, so the check proves nothing:\n")
        print("\n".join(stray))
        return 1
    if tsc.returncode != 0 and not failed:
        print("tsc failed without a located error:\n" + out)
        return 1

    mismatches = {(rows[i][0], rows[i][1], d): msg for (i, d), msg in failed.items()}
    sig_of = {(m, n): s for m, n, s in rows}

    unexpected = sorted(set(mismatches) - set(KNOWN))
    fixed = sorted(k for k in KNOWN if k not in mismatches and (k[0], k[1]) in sig_of)
    vanished = sorted(k for k in KNOWN if (k[0], k[1]) not in sig_of)

    for key in sorted(set(mismatches) & set(KNOWN)):
        print(f"known   {key[0]}::{key[1]} ({key[2]}), {KNOWN[key]}")
    for key in unexpected:
        module, name, direction = key
        print(f"\nMISMATCH {module}::{name} ({direction})")
        print(f"  declared: {sig_of[(module, name)]}")
        for msg in mismatches[key]:
            print(f"  {msg}")
    for key in fixed:
        print(f"\nNOW PASSES {key[0]}::{key[1]} ({key[2]}): remove its KNOWN entry ({KNOWN[key]}) and close the gap.")
    for key in vanished:
        print(f"\nNO COMPLETE ROW {key[0]}::{key[1]}: its KNOWN entry ({KNOWN[key]}) checks nothing now.")

    checked = len(rows) * 2
    if unexpected or fixed or vanished:
        print(f"\n{len(rows)} complete rows, {checked} directions: "
              f"{len(unexpected)} unexpected mismatch(es), {len(fixed)} known entry(ies) now passing, "
              f"{len(vanished)} known entry(ies) with no row.")
        return 1
    print(f"{len(rows)} complete stdlib rows agree with the runtime in both directions under "
          f"TypeScript {ts_version}, apart from {len(mismatches)} known mismatch(es).")
    return 0


if __name__ == "__main__":
    sys.exit(main())
