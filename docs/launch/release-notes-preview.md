# Glyph preview release notes (0.1.130)

Glyph is a statically typed language that compiles to TypeScript. These notes
describe the compiler as of 0.1.130, published to npm as `@glyphlang/glyph` on
2026-11-02, for someone seeing the project for the first time. The per-release
history is at [glyphlang.io/versions](https://glyphlang.io/versions/) and in
[`docs/roadmap/releases.md`](../roadmap/releases.md).

It is a preview. The 0.1.x line can still reject programs an earlier 0.1.x
accepted, and every release from 0.1.120 to 0.1.130 did. Each release note says
whether the release is breaking, established by running the previous published
version and the new one on the same programs. `glyph init` pins the version in
`devDependencies`.

## Installing

```sh
npx @glyphlang/glyph init hello
cd hello && npm install && npx glyph run
```

`glyph init` writes `src/main.glyph`, a `package.json` that pins the compiler,
`typescript` and `tsx`, an `AGENTS.md` and an `.mcp.json`. `npx glyph run`
prints `hello from glyph`. Prebuilt binaries ship for macOS, Linux and Windows
through per-platform npm packages; `glyph doctor` checks that `node`, `tsx` and
`tsc` are present and new enough.

## The language

A Glyph file looks like this:

```glyph
module main

import std/array
import std/option { Some, None }
import std/result { Result, Ok, Err }

type User = {
  id: string,
  name: string,
}

pub fn find_user(users: Array<User>, id: string) -> Result<User, string> {
  let found = array.find(users, fn(u: User) -> bool {
    return u.id == id
  })
  return match found {
    Some(u) => Ok(u),
    None => Err("no user with id ${id}"),
  }
}
```

The differences from TypeScript, each with the reason it exists, are in
[`docs/guide/for-typescript-developers.md`](../guide/for-typescript-developers.md).
In short: there is no `any` and no cast; errors are values (`Result`, `Option`
and `?`); `match` is the only conditional and it must be exhaustive; each kind
of declaration has one syntactic form, so `grep "fn find_user"` finds the
definition; formatting has one layout and no options; and a type declared in
Glyph has a runtime validator (`User.parse`, `User.is`), so input from outside
is checked rather than cast.

## What the compiler refuses that `tsc --strict` accepts

[`catches/`](../../catches/) holds five pairs of programs. In each, `tsc --strict`
accepts the TypeScript and Glyph refuses the equivalent Glyph with the code
listed. `scripts/check_catches.py` runs both halves in CI, with `@types/node`
installed for the TypeScript.

| Case | Code |
|---|---|
| A `void` handler whose `match` misses a case of a union | E0200 |
| A read from a `Record<string, string>` typed as if the key were there | E0224 |
| `const [user, host] = s.split("@")` | E0230 |
| A validator claiming success for a `net.Socket` field it never checked | E0304 |
| `JSON.parse(body)` assigned to a `User` with nothing checked | E0204 |

All five are verifiability cases. None is evidence for greppability,
abstraction or diff stability.

Two of them have a TypeScript answer outside `--strict`. `noUncheckedIndexedAccess`
catches the map read (and not the split). typescript-eslint's
`switch-exhaustiveness-check`, or an `assertNever` default, catches the missing
union case. In Glyph those checks are always on.

The playground at https://glyphlang.io/playground/ compiles one file in the
browser and does not run `tsc`. All five are refused by Glyph's own checker, so
all five reproduce there.

## The toolchain

- `glyph check` type-checks a file or tree, including `tsc --strict` over the
  emitted TypeScript, and writes nothing. `--json` and `--agent` return the
  diagnostics as JSON; `--agent` adds the constraints a repair must keep and a
  description of each symbol a diagnostic names.
- `glyph build` and `glyph run` compile to TypeScript, type-check it, and run
  every `@example` test on the way.
- `glyph fmt` formats in place, `glyph fix` applies repairs the compiler can
  determine (removing unused imports, correcting an arm head or a field a
  character from the right name) and reports a non-exhaustive match without
  writing arms, and
  `glyph --explain <code>` prints the long form of any diagnostic.
- `glyph lsp` is a language server: diagnostics, hover, go-to-definition,
  completion, find-references and rename.
- `glyph mcp` serves the compiler's analysis to an agent over MCP, and
  `glyph query` asks the same questions from a shell: a symbol's full
  description, every `match` site over a union, the impact of a named change,
  references, dependencies, exports, and whether one type is assignable to
  another.
- `glyph gen` generates Glyph types from an OpenAPI spec, a `.d.ts` or a zod
  schema; `glyph publish` audits and builds a package for npm.

## What has been built with it

[`examples/apps/`](../../examples/apps/) holds 32 applications written in Glyph,
with no TypeScript in them, which `scripts/check_apps_are_glyph.py` enforces.
They include a chat server, a link checker, a spreadsheet, a job queue and a
terminal minesweeper. Each has a README saying what writing it found in the
compiler. The newest, `examples/apps/todo`, is a counter and a todo list on
React 18 using React's own hooks; CI bundles it with Vite, renders it in
headless Chromium and checks the text on the page.

The compiler's test suite is 1978 tests at 0.1.130.

## What has not been measured

Glyph is built on a hypothesis: that an agent given the same task produces
correct code more reliably in Glyph than in TypeScript, and that a reviewer
spends less time confirming it. No study has been run. There is no speedup
figure, and none should be quoted.

[`benchmarks/FINDINGS.md`](../../benchmarks/FINDINGS.md) has the structural
measurements that do exist. On three small functions counted with tiktoken's
`cl100k_base`, Glyph totals 436 tokens against TypeScript's 478, and on one of
the three (`slugify`, 67 against 47) Glyph is longer. A one-line edit in the
diff-stability fixture produces a one-line change in the emitted TypeScript.
Three functions are a small sample.

## Known limitations

- Values that come from npm packages, React included, are typed by `tsc` over
  the emitted TypeScript and not by Glyph's checker (G257). The build still
  fails on a wrong argument to a library function, with the error mapped to the
  argument in the Glyph source, but the editor, the MCP tools and `glyph check
  --no-tsc` do not report it. A React state setter called with the wrong type
  is the common case.
- An object literal with a field its declared type does not have is accepted;
  `tsc` refuses it (G273).
- A function value's parameter types are not compared, only its return type.
- A mismatch between two unnamed records reads
  ``expected `record`, found `record` `` without naming the field.
- Object destructuring in `let` is not in the language yet (E0005).
- `std/dom` has no element setters or event listeners; React owns the DOM it
  renders.
- The VS Code extension is built from source in `editors/vscode/`; it is not on
  the Marketplace.
- The gap ledger, [`docs/dogfooding-gaps.md`](../dogfooding-gaps.md), had 301
  entries at 0.1.130, of which 25 were open and 8 partly fixed. Each open entry has a reproduction
  and is scheduled in the release roadmap.

## Licence and source

[github.com/chadetov/glyph](https://github.com/chadetov/glyph), under MIT or
Apache-2.0.
