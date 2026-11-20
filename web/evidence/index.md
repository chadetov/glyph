<!-- Markdown copy of https://glyphlang.io/evidence/, generated from the same source as the page. Last changed 2026-11-20. -->

Evidence

# How each claim on this site is checked

A claim that Glyph catches something is a pair of programs in the repository, one in TypeScript and one in Glyph, and a CI gate runs both on every change. If the TypeScript stops compiling or the Glyph stops being refused, the build fails. You can run any case yourself in two commands.

## The fixtures

Each directory under [catches/](https://github.com/chadetov/glyph/tree/main/catches) holds `ts.ts`, `glyph.glyph` and a `case.toml` saying what each one does. `scripts/check_catches.py` runs in CI. It requires every `ts.ts` to pass `tsc --strict`, since a TypeScript program that doesn't compile proves nothing, and checks the Glyph half according to the case's kind. There are 52 checked cases: 37 TypeScript cases, 5 JavaScript traps that do not exist in Glyph, and 10 holes Glyph does not close yet. Glyph refuses 28 of them when you build (23 TypeScript cases and 5 traps). By the kind the gate checks:

| Kind | Cases | What the gate requires of the Glyph |

|---|---|---|

| `refused` | 28 | The build fails with the code in `case.toml` |
| `warned` | 3 | The build passes and reports that code as a warning |
| `boundary` | 9 | The program runs, and the check at the boundary turns the bad data into an `Err` |
| `runtime` | 2 | The program builds, then stops when it runs, with the stated output |
| `gap` | 10 | Glyph accepts the program and it does the wrong thing. When the hole closes, the gate fails and says to move the case |

A case that stops being demonstrable is deleted rather than softened, and the [catches README](https://github.com/chadetov/glyph/blob/main/catches/README.md) lists the six that were deleted and why. One of them turned out to be caught by `tsc` too; another only worked because its TypeScript took a value as `any`.

## The TypeScript it is compared against

Each case also says whether a fully configured TypeScript project would have caught the mistake, which is where the "would TypeScript catch it" tag comes from. "Fully configured" is `tsc --strict` plus every option that makes it stricter, and a typescript-eslint run. The gate installs the packages fresh on each run (TypeScript 6, eslint 9, typescript-eslint 8, tsx 4 and the latest `@types/node`) and fails, rather than skips, without the network.

tsconfig options on top of strict

```
noUncheckedIndexedAccess: true
noFallthroughCasesInSwitch: true
noUnusedLocals: true
noUnusedParameters: true
noImplicitReturns: true
noImplicitOverride: true
allowUnreachableCode: false
allowUnusedLabels: false
exactOptionalPropertyTypes: true
noPropertyAccessFromIndexSignature: true
```

eslint.config.mjs

```
export default tseslint.config(
  js.configs.recommended,
  ...tseslint.configs.strictTypeChecked,
  { rules: {
    "@typescript-eslint/switch-exhaustiveness-check": "error",
    "@typescript-eslint/strict-boolean-expressions": "error",
    "@typescript-eslint/require-array-sort-compare": "error",
    "require-atomic-updates": "error",
  } },
);
```

A case's tag is `none`, `flag:` plus the one option that reports it, or `lint:` plus the rule. When the corrected program also trips a rule for an unrelated reason, the case carries `ts_ok.ts` and the tag only counts what fires on the mistake. For example, `non-exhaustive-match` is `lint:switch-exhaustiveness-check`: typescript-eslint catches it if you turn that rule on, and the page says so.

## Reproduce one case

Run the gate on a single case from a clone of the repository, or do it by hand. Both halves of `non-exhaustive-match`, as they ran for this page:

the gate, one case

```
$ python3 scripts/check_catches.py non-exhaustive-match
catches: 1/1 cases hold every claim they make.
TypeScript checked with tsc --strict and the full flag set (typescript 6.0.3),
typescript-eslint 8.73.0 strictTypeChecked on eslint 9.39.5, run with
tsx 4.23.15, and @types/node 26.6.6.
```

the TypeScript half, by hand

```
$ npm i -D typescript@6 tsx@4
$ npx tsc --strict --noEmit ts.ts
$ echo $?
0
$ npx tsx ts.ts
finished
archived logged nothing
```

the Glyph half, by hand

```
$ glyph check --no-tsc catches/non-exhaustive-match
[E0200] Error: typecheck: non-exhaustive match
  on `"todo" | "doing" | "done" | "archived"`:
  missing variants "archived"
glyph check: 1 error(s) across 1 module(s)
```

## The snippets on this site compile

`scripts/check_docs_compile.py` runs in CI and compiles every Glyph snippet on the site that stands on its own (it opens with `module` and declares something), plus the ones in the docs and `llms.txt`. A snippet that is meant to fail has to name the error it draws, and it has to draw exactly that one. Snippets that need React or an npm package are compiled against those packages' real types. Fragments are counted and skipped, and the count is printed so the skipped share stays visible.

## What it does not catch yet

The 10 `gap` cases are on [the catches page](https://glyphlang.io/catches/#not-yet), each with its gap number. Every gap found by building apps in Glyph is in the ledger, [docs/dogfooding-gaps.md](https://github.com/chadetov/glyph/blob/main/docs/dogfooding-gaps.md), where a bracketed marker is the only live status (`[FIXED]`, `[HALF FIXED]`, or none for open) and `scripts/check_gaps.py` fails CI when an open entry was last reproduced more than five releases ago.

## How a release is marked Breaking

A release is marked Breaking when it rejects a program the previous version accepted. That is decided by writing the smallest affected program and running it under the published previous release and the new binary, then comparing exit codes. It is not inferred from the wording of the notes. Releases before 0.1.99 were never tested this way, so an unmarked one is untested rather than safe. The record is on [the versions page](https://glyphlang.io/versions/).

## The rest of the record

- [Recommended Catches Every case: the TypeScript, the Glyph, the error, and whether tooled TypeScript would catch it. See every case](https://glyphlang.io/catches/)
- [Hardening How agents playing skeptical engineers attack the compiler, and the bugs that found.](https://glyphlang.io/hardening/)
- [Benchmarks What is measured (token density, grep, diffs) and the reliability study that isn't done yet.](https://glyphlang.io/benchmarks/)
- [Versions Every release, what it changed, and which ones are Breaking.](https://glyphlang.io/versions/)
