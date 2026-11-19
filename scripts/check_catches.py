#!/usr/bin/env python3
"""The claims about what Glyph catches are checked, not asserted.

`catches/` holds one directory per case: a TypeScript program, the same program
in Glyph, and a `case.toml` saying what each of them does. Every claim the
`case.toml` makes is run here, so no side can rot into a claim that was true
once. The cases are the source the mistakes page is generated from, so a claim
the compiler stops honouring fails this check before it can reach the site.

What is checked for every case:

  - `ts.ts` (and any other `.ts` in the directory except `ts_ok.ts`) must
    type-check clean under `tsc --strict`. A case whose TypeScript stops
    compiling is not evidence of anything, it is a bug in the fixture.
  - `ts_output`, when given, is text the TypeScript program prints or throws
    when it runs (`tsx`). A list means every item.
  - `fullconfig` says whether a fully configured TypeScript project catches
    the mistake: `none`, `flag:<option>` (or `flag:<option>=false`), or
    `lint:<rule>`. "Fully configured" is every strictness flag tsc has plus
    eslint's recommended rules, typescript-eslint `strictTypeChecked`,
    `switch-exhaustiveness-check`, `strict-boolean-expressions`,
    `require-array-sort-compare` and `require-atomic-updates`.
      - `flag:X` means `tsc --strict` plus that one option reports an error.
      - `lint:R` means the lint run reports rule R.
      - `none` means neither the full flag set nor the lint run reports
        anything on the program. When something fires that has nothing to do
        with the mistake, the case carries `ts_ok.ts`, the corrected program,
        and `none` means nothing fires on `ts.ts` that does not also fire on
        `ts_ok.ts`.
    With a `ts_ok.ts`, a `flag:` or `lint:` case also has to stay quiet on the
    corrected program for that flag or rule, so the claim is about the mistake.

What is checked on the Glyph side depends on `kind`:

  - `refused` (the default): the build fails with `code`. A case whose Glyph
    starts compiling means the guarantee it documents is gone, which is the
    failure worth being loud about. A `TS....` code means the refusal comes
    from the tsc pass `glyph build` runs on the emitted program, not from
    Glyph's own checker; the case's README entry has to say so.
  - `warned`: the build passes and reports `code` as a warning.
  - `boundary`: the program builds and runs, and its output contains
    `glyph_output` (the check at the boundary turning bad data into an `Err`,
    or the program doing the right thing).
  - `runtime`: the program builds, then stops when it runs, with output
    containing `glyph_output`.
  - `gap`: a known hole, named by `gap_id`. Glyph accepts the program and it
    does the wrong thing: the run's output contains `glyph_output`. With a
    `TS....` `code`, the hole is that Glyph's own checker (`check --no-tsc`)
    accepts it and only the tsc pass refuses it. When the hole closes, this
    fails and says to move the case.

Cases with a `section` (`mistake`, `trap` or `gap`) are on the mistakes page
and also need its display fields, and `// show` ... `// end show` markers in
`ts.ts` and `glyph.glyph` around the lines the page shows.

The npm packages (TypeScript, the lint stack, `tsx` and the real `@types/node`)
are installed fresh on every run. No npm, or no network, is a failure rather
than a skip: a skipped case reads exactly like a verified one in a CI log.

This is the evidence base for the pillars, so a case that cannot be demonstrated
should be deleted rather than softened.

Hard-fails (exit 1) when anything stops behaving as its case claims.

Usage: python3 scripts/check_catches.py [case ...]
"""

from __future__ import annotations

import concurrent.futures
import json
import os
import pathlib
import re
import shutil
import subprocess
import sys
import tempfile
import time
import tomllib

import glyph_bin

ROOT = pathlib.Path(__file__).resolve().parent.parent
CASES = ROOT / "catches"

PILLAR_ORDER = ("verifiability", "greppability", "abstraction", "diff stability")
PILLARS = set(PILLAR_ORDER)
KIND_ORDER = ("refused", "warned", "boundary", "runtime", "gap")
SECTIONS = ("mistake", "trap", "gap")

# Major versions are pinned so a new major cannot change a verdict on its own;
# `@types/node` follows the latest, as the runtime check does.
NPM_PACKAGES = (
    "@types/node@latest",
    "typescript@6",
    "eslint@9",
    "@eslint/js@9",
    "typescript-eslint@8",
    "tsx@4",
)

STRICT_OPTIONS = {
    "strict": True,
    "target": "es2022",
    "module": "esnext",
    "moduleResolution": "bundler",
    # TypeScript 6 includes no `@types` package unless named.
    "types": ["node"],
    "noEmit": True,
    # Every case is checked in one program, so a file with no import or export
    # must still be its own module, or two cases declaring `User` collide.
    "moduleDetection": "force",
    "skipLibCheck": True,
}

# Every option tsc has that makes it stricter than `--strict`.
FULL_OPTIONS = {
    **STRICT_OPTIONS,
    "noUncheckedIndexedAccess": True,
    "noFallthroughCasesInSwitch": True,
    "noUnusedLocals": True,
    "noUnusedParameters": True,
    "noImplicitReturns": True,
    "noImplicitOverride": True,
    "allowUnreachableCode": False,
    "allowUnusedLabels": False,
    "exactOptionalPropertyTypes": True,
    "noPropertyAccessFromIndexSignature": True,
}

ESLINT_CONFIG = """\
import js from "@eslint/js";
import tseslint from "typescript-eslint";
export default tseslint.config(
  js.configs.recommended,
  ...tseslint.configs.strictTypeChecked,
  {
    languageOptions: { parserOptions: { projectService: true, tsconfigRootDir: import.meta.dirname } },
    rules: {
      "@typescript-eslint/switch-exhaustiveness-check": "error",
      "@typescript-eslint/strict-boolean-expressions": "error",
      "@typescript-eslint/require-array-sort-compare": "error",
      "require-atomic-updates": "error",
    },
  },
);
"""

MARK_OPEN = "// show"
MARK_CLOSE = "// end show"

MISTAKE_FIELDS = (
    "page_title", "why", "ts_verdict", "glyph_verdict", "ts_result", "glyph_result", "page_note",
)
LISTED_FIELDS = ("summary",)

TSC_LINE = re.compile(r"^(cases/([^/]+)/([^(]+))\((\d+),(\d+)\): error (TS\d+): (.*)$")


def read_case(d: pathlib.Path) -> dict:
    return tomllib.loads((d / "case.toml").read_text())


def as_list(v: object) -> list[str]:
    if v is None:
        return []
    return [v] if isinstance(v, str) else list(v)  # type: ignore[arg-type]


def kind_of(meta: dict) -> str:
    return meta.get("kind", "refused")


# ---------------------------------------------------------------- npm + tools


def install(root: pathlib.Path) -> tuple[dict[str, str] | None, str]:
    """Install the TypeScript tooling under `root`. Returns versions, or None and why."""
    if shutil.which("npm") is None:
        return None, "npm not found; this check installs TypeScript, the lint stack and @types/node."
    (root / "package.json").write_text('{"name":"catches-ts","private":true}\n')
    r = subprocess.run(
        ["npm", "install", "--no-audit", "--no-fund", "--save-dev", *NPM_PACKAGES],
        cwd=root, capture_output=True, text=True, timeout=900,
    )
    versions: dict[str, str] = {}
    for spec in NPM_PACKAGES:
        name = spec.rsplit("@", 1)[0]
        manifest = root / "node_modules" / name / "package.json"
        if not manifest.exists():
            why = r.stderr.strip() or r.stdout.strip()
            return None, f"could not install {name}; this check needs the network.\n{why}"
        versions[name] = json.loads(manifest.read_text()).get("version", "unknown")
    if r.returncode != 0:
        return None, f"npm install failed.\n{r.stderr.strip() or r.stdout.strip()}"
    return versions, ""


def stage(root: pathlib.Path, dirs: list[pathlib.Path]) -> None:
    """Copy every case's TypeScript under `root/cases/<slug>/`."""
    for d in dirs:
        out = root / "cases" / d.name
        out.mkdir(parents=True)
        for f in d.glob("*.ts"):
            (out / f.name).write_text(f.read_text())
    write_tsconfig(root / "tsconfig.json", FULL_OPTIONS)
    (root / "eslint.config.mjs").write_text(ESLINT_CONFIG)


def write_tsconfig(path: pathlib.Path, options: dict) -> None:
    path.write_text(json.dumps({"compilerOptions": options, "include": ["cases/**/*.ts"]}, indent=2))


def run_tsc(root: pathlib.Path, options: dict, name: str) -> tuple[dict[str, set[str]], str]:
    """Errors per case file (`<slug>/<file>` -> set of TS codes), and the raw output."""
    cfg = root / f"tsconfig.{name}.json"
    write_tsconfig(cfg, options)
    r = subprocess.run(
        [str(root / "node_modules/.bin/tsc"), "-p", cfg.name, "--pretty", "false"],
        cwd=root, capture_output=True, text=True, timeout=600,
    )
    out = (r.stdout + r.stderr).strip()
    errors: dict[str, set[str]] = {}
    for line in out.splitlines():
        m = TSC_LINE.match(line)
        if m:
            errors.setdefault(f"{m.group(2)}/{m.group(3)}", set()).add(m.group(6))
    if r.returncode != 0 and not errors:
        raise RuntimeError(f"tsc ({name}) failed without reporting a case file:\n{out}")
    return errors, out


def run_eslint(root: pathlib.Path) -> dict[str, set[str]]:
    """Rules reported per case file (`<slug>/<file>` -> set of rule ids)."""
    r = subprocess.run(
        [str(root / "node_modules/.bin/eslint"), "-f", "json", "cases"],
        cwd=root, capture_output=True, text=True, timeout=900,
    )
    try:
        report = json.loads(r.stdout)
    except json.JSONDecodeError as e:
        raise RuntimeError(f"eslint did not produce a report:\n{r.stdout}\n{r.stderr}") from e
    rules: dict[str, set[str]] = {}
    base = (root / "cases").resolve()
    for entry in report:
        rel = pathlib.Path(entry["filePath"]).resolve().relative_to(base)
        for m in entry["messages"]:
            # A message with no rule is a parse or config failure, which must
            # not pass for a clean file.
            rules.setdefault(str(rel), set()).add(m.get("ruleId") or f"fatal: {m.get('message')}")
    return rules


def run_ts(root: pathlib.Path, slug: str) -> str:
    r = subprocess.run(
        [str(root / "node_modules/.bin/tsx"), f"cases/{slug}/ts.ts"],
        cwd=root, capture_output=True, text=True, timeout=120,
    )
    return (r.stdout + r.stderr).strip()


# ---------------------------------------------------------------- glyph


def glyph_project(src: pathlib.Path, tmp: pathlib.Path) -> pathlib.Path:
    """Lay the case out as a project: some cases need a sibling module."""
    (tmp / "package.json").write_text('{"name":"catch","glyph":{}}\n')
    work = tmp / "src"
    work.mkdir()
    for f in src.glob("*.glyph"):
        (work / f.name).write_text(f.read_text())
    return tmp


def glyph(compiler: pathlib.Path, cwd: pathlib.Path, *args: str) -> tuple[int, str]:
    r = subprocess.run(
        [str(compiler), *args], capture_output=True, text=True, cwd=cwd, timeout=300,
    )
    return r.returncode, (r.stdout + r.stderr).strip()


def first_diagnostic(out: str) -> str:
    for line in out.splitlines():
        if re.match(r"\[(E|TS)\d+\] (Error|Warning)", line):
            return line
    return out.splitlines()[0] if out else "(no output)"


def check_glyph(compiler: pathlib.Path, d: pathlib.Path, meta: dict) -> list[str]:
    name = d.name
    kind = kind_of(meta)
    code = meta.get("code", "")
    expect = as_list(meta.get("glyph_output"))
    gap_id = meta.get("gap_id", "")
    with tempfile.TemporaryDirectory() as tmp:
        p = glyph_project(d, pathlib.Path(tmp))

        if kind == "gap" and code:
            rc, out = glyph(compiler, p, "check", "--no-tsc", "src")
            if rc != 0:
                return [
                    f"{name}: {gap_id} looks closed. Glyph's own checker now refuses this "
                    f"({first_diagnostic(out)}). Move the case to kind = \"refused\" with "
                    f"that code, take it off the page's \"does not catch yet\" list, and "
                    f"close {gap_id} in docs/dogfooding-gaps.md.\n{indent(out)}"
                ]

        rc, out = glyph(compiler, p, "build", "src", "--out", str(p / "out"))

        if kind == "refused":
            if rc == 0:
                return [
                    f"{name}: Glyph now accepts this. The guarantee the case documents "
                    f"is gone, or the case drifted.\n{indent(out)}"
                ]
            if f"[{code}] Error" not in out:
                return [f"{name}: expected {code}, got a different diagnostic.\n{indent(out)}"]
            return []

        if kind == "gap" and code:
            if rc == 0:
                return [
                    f"{name}: the build now passes, so the tsc pass no longer refuses this "
                    f"either. The case has drifted from {gap_id}.\n{indent(out)}"
                ]
            if f"[{code}] Error" not in out:
                return [f"{name}: expected {code} from the tsc pass, got a different diagnostic.\n{indent(out)}"]
            return []

        if rc != 0:
            if kind == "gap":
                return [
                    f"{name}: {gap_id} looks closed. Glyph now refuses this program "
                    f"({first_diagnostic(out)}). Move the case to kind = \"refused\" with "
                    f"that code, take it off the page's \"does not catch yet\" list, and "
                    f"close {gap_id} in docs/dogfooding-gaps.md.\n{indent(out)}"
                ]
            return [
                f"{name}: a `{kind}` case must build, and Glyph now refuses it.\n{indent(out)}"
            ]

        if kind == "warned":
            if f"[{code}] Warning" not in out:
                return [f"{name}: the warning {code} is gone from the build.\n{indent(out)}"]
            return []

        rc, run_out = glyph(compiler, p, "run", "--no-tsc", "src/glyph.glyph")
        missing = [e for e in expect if e not in run_out]
        if kind == "boundary":
            if rc != 0 or missing:
                return [
                    f"{name}: the program should run and print {missing or expect!r}; "
                    f"it exited {rc}.\n{indent(run_out)}"
                ]
        elif kind == "runtime":
            if rc == 0:
                return [f"{name}: the program should stop when it runs, and it exited 0.\n{indent(run_out)}"]
            if missing:
                return [f"{name}: the run stopped, but without {missing!r}.\n{indent(run_out)}"]
        elif kind == "gap":
            if missing:
                return [
                    f"{name}: {gap_id} may be closed: the run no longer shows the wrong "
                    f"behaviour {missing!r}. If the fix is real, move the case to the kind "
                    f"it now is, take it off the page's \"does not catch yet\" list, and "
                    f"close {gap_id} in docs/dogfooding-gaps.md.\n{indent(run_out)}"
                ]
    return []


# ---------------------------------------------------------------- case.toml


def check_meta(d: pathlib.Path, meta: dict) -> list[str]:
    name = d.name
    problems: list[str] = []
    kind = kind_of(meta)
    code = meta.get("code", "")
    if meta.get("pillar") not in PILLARS:
        problems.append(f"case.toml needs a `pillar` from {sorted(PILLARS)}")
    if not meta.get("title"):
        problems.append("case.toml needs a `title`")
    # `glyph-cli/build.rs` reads these three with a one-line `key = "value"`
    # rule and no unescaping, and ships them in `glyph llms --negative`.
    for f in ("code", "pillar", "title"):
        v = meta.get(f, "")
        if not isinstance(v, str) or any(c in v for c in '"\\\n<>'):
            problems.append(f"`{f}` must be plain one-line text with no quotes, backslashes or HTML")
    if kind not in KIND_ORDER:
        problems.append(f"`kind` must be one of {list(KIND_ORDER)}, not {kind!r}")
    if kind in ("refused", "warned") and not code:
        problems.append(f"a `{kind}` case needs a `code`")
    if kind in ("boundary", "runtime") and not as_list(meta.get("glyph_output")):
        problems.append(f"a `{kind}` case needs `glyph_output`")
    if kind == "gap":
        if not re.fullmatch(r"G\d+", meta.get("gap_id", "")):
            problems.append("a `gap` case needs a `gap_id` such as \"G386\"")
        if not code and not as_list(meta.get("glyph_output")):
            problems.append("a `gap` case needs `glyph_output` (the wrong behaviour), or a TS `code`")
        if code and not code.startswith("TS"):
            problems.append("a `gap` case's `code` is the tsc refusal Glyph's own checker misses, so it starts with TS")
    fc = meta.get("fullconfig")
    if fc is not None and not re.fullmatch(r"none|flag:\w+(=false)?|lint:[@\w/-]+", fc):
        problems.append(f"`fullconfig` must be none, flag:<option>[=false] or lint:<rule>, not {fc!r}")
    if (d / "ts_ok.ts").exists() and fc is None:
        problems.append("`ts_ok.ts` is only read by the `fullconfig` check, and the case has none")

    section = meta.get("section")
    if section is not None:
        if section not in SECTIONS:
            problems.append(f"`section` must be one of {list(SECTIONS)}")
        if meta.get("slug") != name:
            problems.append(f"`slug` must be the directory name, {name!r}")
        if not isinstance(meta.get("order"), int):
            problems.append("a case on the page needs an integer `order`")
        if fc is None:
            problems.append("a case on the page needs `fullconfig`")
        if section == "mistake":
            for f in MISTAKE_FIELDS:
                if not isinstance(meta.get(f, None), str):
                    problems.append(f"a mistake on the page needs `{f}`")
            for f in ("likelihood", "severity"):
                v = meta.get(f)
                if not (isinstance(v, int) and 1 <= v <= 5):
                    problems.append(f"`{f}` must be an integer from 1 to 5")
        else:
            for f in LISTED_FIELDS:
                if not meta.get(f):
                    problems.append(f"a {section} on the page needs `{f}`")
        if section == "gap" and kind != "gap":
            problems.append("a case in the page's gap list must be kind = \"gap\"")
        for f in ("ts.ts", "glyph.glyph"):
            if MARK_OPEN not in marks(d / f):
                problems.append(f"`{f}` needs `{MARK_OPEN}` ... `{MARK_CLOSE}` around the lines the page shows")
    for f in sorted([*d.glob("*.ts"), *d.glob("*.glyph")]):
        bad = unbalanced(f)
        if bad:
            problems.append(f"`{f.name}`: {bad}")
    return [f"{name}: {p}" for p in problems]


def marks(f: pathlib.Path) -> list[str]:
    return [line.strip() for line in f.read_text().splitlines() if line.strip() in (MARK_OPEN, MARK_CLOSE)]


def unbalanced(f: pathlib.Path) -> str:
    open_ = False
    for m in marks(f):
        if m == MARK_OPEN:
            if open_:
                return f"`{MARK_OPEN}` inside a shown block"
            open_ = True
        else:
            if not open_:
                return f"`{MARK_CLOSE}` with no `{MARK_OPEN}`"
            open_ = False
    return f"`{MARK_OPEN}` is never closed" if open_ else ""


# ---------------------------------------------------------------- TypeScript


def files_of(d: pathlib.Path) -> list[str]:
    """The case's program files, as `<slug>/<file>` keys: everything but the control."""
    return sorted(f"{d.name}/{f.name}" for f in d.glob("*.ts") if f.name != "ts_ok.ts")


def union(table: dict[str, set[str]], keys: list[str]) -> set[str]:
    out: set[str] = set()
    for k in keys:
        out |= table.get(k, set())
    return out


def rule_matches(fired: set[str], rule: str) -> bool:
    return rule in fired or f"@typescript-eslint/{rule}" in fired


def check_ts(
    d: pathlib.Path,
    meta: dict,
    strict: dict[str, set[str]],
    strict_out: str,
    flags: dict[str, dict[str, set[str]]],
    full: dict[str, set[str]],
    lint: dict[str, set[str]],
    ran: str | None,
) -> list[str]:
    name = d.name
    files = files_of(d)
    ok_key = [f"{name}/ts_ok.ts"] if (d / "ts_ok.ts").exists() else []

    broken = union(strict, files + ok_key)
    if broken:
        mine = "\n".join(line for line in strict_out.splitlines() if f"cases/{name}/" in line)
        return [
            f"{name}: the TypeScript no longer passes `tsc --strict`, so the case "
            f"proves nothing.\n{indent(mine)}"
        ]

    problems: list[str] = []
    expect = as_list(meta.get("ts_output"))
    if expect and ran is not None:
        missing = [e for e in expect if e not in ran]
        if missing:
            problems.append(
                f"{name}: the TypeScript no longer prints {missing!r} when it runs.\n{indent(ran)}"
            )

    fc = meta.get("fullconfig")
    if fc is None:
        return problems
    fired = {f"tsc {c}" for c in union(full, files)} | {f"lint {r}" for r in union(lint, files)}
    fired_ok = {f"tsc {c}" for c in union(full, ok_key)} | {f"lint {r}" for r in union(lint, ok_key)}
    if fc == "none":
        extra = sorted(fired - fired_ok)
        if extra:
            against = "what fires on `ts_ok.ts`" if ok_key else "a clean run"
            problems.append(
                f"{name}: fullconfig = \"none\", but the full flag set and lint run report "
                f"{extra} beyond {against}. Either a fully configured project catches this "
                f"(say which flag or rule), or the fixture needs a `ts_ok.ts` control."
            )
    elif fc.startswith("flag:"):
        option = flag_name(fc)
        caught = union(flags[option], files)
        if not caught:
            problems.append(
                f"{name}: fullconfig = \"{fc}\", but `tsc --strict` with that option "
                f"reports nothing on the program."
            )
        if ok_key and union(flags[option], ok_key):
            problems.append(
                f"{name}: fullconfig = \"{fc}\", but the option also fires on `ts_ok.ts`, "
                f"so it is not catching the mistake."
            )
    elif fc.startswith("lint:"):
        rule = fc[len("lint:"):]
        if not rule_matches(union(lint, files), rule):
            problems.append(
                f"{name}: fullconfig = \"{fc}\", but the lint run does not report {rule}; "
                f"it reports {sorted(union(lint, files)) or 'nothing'}."
            )
        if ok_key and rule_matches(union(lint, ok_key), rule):
            problems.append(
                f"{name}: fullconfig = \"{fc}\", but {rule} also fires on `ts_ok.ts`, "
                f"so it is not catching the mistake."
            )
    return problems


def flag_name(fc: str) -> str:
    return fc[len("flag:"):].split("=")[0]


def flag_options(fc: str) -> dict:
    body = fc[len("flag:"):]
    option, _, value = body.partition("=")
    return {**STRICT_OPTIONS, option: value != "false"}


# ---------------------------------------------------------------- main


def main() -> int:
    if not CASES.is_dir():
        print(f"missing {CASES.relative_to(ROOT)}/")
        return 1
    compiler = glyph_bin.resolve()

    dirs = sorted(p for p in CASES.iterdir() if p.is_dir())
    only = set(sys.argv[1:])
    if only:
        unknown = only - {d.name for d in dirs}
        if unknown:
            print(f"no such case: {sorted(unknown)}")
            return 1
        dirs = [d for d in dirs if d.name in only]
    with tempfile.TemporaryDirectory() as tmp:
        root = pathlib.Path(tmp)
        versions, why = install(root)
        if versions is None:
            print(why)
            return 1
        return check(compiler, dirs, root, versions)


def check(
    compiler: pathlib.Path, dirs: list[pathlib.Path], root: pathlib.Path, versions: dict[str, str]
) -> int:
    started = time.monotonic()
    metas: dict[str, dict] = {}
    problems: dict[str, list[str]] = {}
    for d in dirs:
        try:
            metas[d.name] = read_case(d)
        except (OSError, tomllib.TOMLDecodeError) as e:
            problems[d.name] = [f"{d.name}: case.toml does not parse: {e}"]
            continue
        problems[d.name] = check_meta(d, metas[d.name])
    good = [d for d in dirs if d.name in metas and not problems[d.name]]

    stage(root, good)
    flag_specs = sorted({metas[d.name]["fullconfig"] for d in good
                         if str(metas[d.name].get("fullconfig", "")).startswith("flag:")})

    workers = max(4, min(16, (os.cpu_count() or 4)))
    with concurrent.futures.ThreadPoolExecutor(workers) as pool:
        strict_f = pool.submit(run_tsc, root, STRICT_OPTIONS, "strict")
        full_f = pool.submit(run_tsc, root, FULL_OPTIONS, "full")
        flag_f = {flag_name(s): pool.submit(run_tsc, root, flag_options(s), "flag-" + flag_name(s))
                  for s in flag_specs}
        lint_f = pool.submit(run_eslint, root)
        ran_f = {d.name: pool.submit(run_ts, root, d.name) for d in good if metas[d.name].get("ts_output")}
        glyph_f = {d.name: pool.submit(check_glyph, compiler, d, metas[d.name]) for d in good}

        try:
            strict, strict_out = strict_f.result()
            full, _ = full_f.result()
            flags = {k: f.result()[0] for k, f in flag_f.items()}
            lint = lint_f.result()
        except RuntimeError as e:
            print(e)
            return 1
        for d in good:
            ran = ran_f[d.name].result() if d.name in ran_f else None
            problems[d.name] += check_ts(d, metas[d.name], strict, strict_out, flags, full, lint, ran)
            problems[d.name] += glyph_f[d.name].result()

    failed = [n for n in problems if problems[n]]
    for n in failed:
        for p in problems[n]:
            print(p)
            print()

    # Every pillar and kind is printed, the empty ones included. A pillar with no
    # case is a fact about the evidence, and leaving it off the line would hide it.
    by_pillar: dict[str, int] = {p: 0 for p in PILLAR_ORDER}
    by_kind: dict[str, int] = {k: 0 for k in KIND_ORDER}
    by_section: dict[str, int] = {s: 0 for s in SECTIONS}
    for meta in metas.values():
        by_pillar[meta.get("pillar", "?")] = by_pillar.get(meta.get("pillar", "?"), 0) + 1
        by_kind[kind_of(meta)] = by_kind.get(kind_of(meta), 0) + 1
        if "section" in meta:
            by_section[meta["section"]] = by_section.get(meta["section"], 0) + 1

    def spread(t: dict[str, int]) -> str:
        return ", ".join(f"{k}: {v}" for k, v in t.items())

    ok = len(dirs) - len(failed)
    print(f"catches: {ok}/{len(dirs)} cases hold every claim they make.")
    print(f"  by pillar: {spread(by_pillar)}")
    print(f"  by kind: {spread(by_kind)}")
    print(f"  on the mistakes page: {spread(by_section)}")
    print(
        "TypeScript checked with tsc --strict and the full flag set (typescript "
        f"{versions['typescript']}), typescript-eslint {versions['typescript-eslint']} "
        f"strictTypeChecked on eslint {versions['eslint']}, run with tsx {versions['tsx']}, "
        f"and @types/node {versions['@types/node']}."
    )
    print(f"Checked in {time.monotonic() - started:.0f}s after the install.")
    return 1 if failed else 0


def indent(s: str) -> str:
    return "\n".join("    " + line for line in s.splitlines()[:14])


if __name__ == "__main__":
    sys.exit(main())
