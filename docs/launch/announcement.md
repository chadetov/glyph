# Glyph: a TypeScript-shaped language whose compiler refuses what `tsc --strict` lets through

Glyph is a statically typed language that compiles to TypeScript. It reads like
TypeScript, runs wherever TypeScript runs and imports npm packages directly. The
differences from TypeScript are few, and each one exists so that a program stays
correct when something other than its author edits it, which these days is
often a coding agent. The current release is 0.1.128. It is a preview: the
language is usable, there are applications written in it, and 0.1.x releases
still reject code that compiled before.

This post shows what the compiler catches today, says what I have not
measured, and lists where it falls short.

## What it catches today

The repository has a directory called `catches/`. Each entry in it is one
program written twice: TypeScript that `tsc --strict` accepts, and the same
program in Glyph, which the compiler refuses. A script,
`scripts/check_catches.py`, runs both halves on every CI build. If the
TypeScript ever stops compiling, or the Glyph ever stops failing with the code
the case names, the build goes red. There are nine cases. Here are four of them.

### A misspelled key on a map

```ts
// tsc --strict: no errors
export function nameOf(row: Record<string, string>): string {
  return row.naem;
}
```

An index signature says every string key has a string value, so `row.naem` is
typed `string` and the function returns `undefined`. In Glyph:

```glyph expect-error E0224
module m

pub fn name_of(row: Record<string, string>) -> string {
  return row.naem
}
```

```text
[E0224] Error: typecheck: `Record<string, string>` is a map, so `naem` may not be there
   Help: Use `record.get(map, "key")`, which returns `Option<V>` so the absent
         case has somewhere to go. `record.has` tests for the key alone.
```

### Splitting a string into two names

```ts
// tsc --strict: no errors
export function describe(s: string): string {
  const [user, host] = s.split("@");
  return `${user} at ${host}`;
}
```

`host` is typed `string`. For the input `"nobody"` it is `undefined` and the
function returns `"nobody at undefined"`. Glyph's destructuring `let` has to
prove the array is long enough, and the result of `split` has no known length:

```glyph expect-error E0230
module m

import std/string

pub fn describe(s: string) -> string {
  let [user, host] = string.split(s, "@")
  return "${user} at ${host}"
}
```

```text
[E0230] Error: typecheck: the length of `Array<string>` is not known, so a name in the pattern could hold `undefined`
   Help: Its length is known only at run time, so match on it with a fallback
         arm: `match xs { [a, b] => ..., else => ... }`.
```

### A field read off a value nobody checked

```ts
// tsc --strict: no errors
export type User = { id: number; email: string };

export function emailOf(body: string): string {
  const user: User = JSON.parse(body);
  return user.email;
}
```

`JSON.parse` returns `any`, and the annotation on `user` turns it into a `User`
without checking anything. Most boundary code in a TypeScript service looks like
this. Glyph has no `any` and no cast, so an untrusted value stays `unknown` until
it is validated, and there is no way to write the read above:

```glyph
pub fn email_of(body: unknown) -> string {
  return body.email
}
```

This one is refused by `tsc` running over the emitted TypeScript
(`TS18046: 'body' is of type 'unknown'`), with the error mapped back to the
Glyph line. A type declared in Glyph gets a runtime validator, so the way
through is `User.parse(body)`, which returns a `Result` you have to match on:

```glyph
module m

pub type User = {
  id: number,
  email: string,
}

pub fn email_of(body: unknown) -> string {
  return match User.parse(body) {
    Ok(user) => user.email,
    Err(_) => "",
  }
}
```

### A case added to a union

Glyph's `match` has to cover every case of the union it matches on:

```glyph expect-error E0200
module m

pub type Status = "todo" | "doing" | "done" | "archived"

pub fn label(s: Status) -> string {
  return match s {
    "todo" => "not started",
    "doing" => "in progress",
    "done" => "finished",
  }
}
```

```text
[E0200] Error: typecheck: non-exhaustive match on `"todo" | "doing" | "done" | "archived"`: missing variants "archived"
```

TypeScript does catch the simplest version of this. A `switch` that falls off
the end of a function declared to return `string` is TS2366, and the TypeScript
half of this case in `catches/` needs a cast on its last line to get past that,
which is something I should fix in the case. The form `tsc` does not catch is a
switch that assigns or calls instead of returning, which is how a lot of code is
written:

```ts
// tsc --strict: no errors
export function label(s: Status): string {
  let out = "unknown";
  switch (s) {
    case "todo": out = "not started"; break;
    case "doing": out = "in progress"; break;
    case "done": out = "finished"; break;
  }
  return out;
}
```

In Glyph, `match` is the only conditional, and it is exhaustive wherever it
appears, so adding `"archived"` to `Status` stops the build at every site that
does not handle it.

The other five cases are a union imported from another module (E0200), a
misspelled field on a record imported from another module (E0210), a validator
that reports success for a field it never checked (E0304), a `Nullable<int>`
passed where an `int` is declared (E0211), and a local type named `Array`
(E0110). Run them yourself with `python3 scripts/check_catches.py`.

## What an agent gets from the compiler

The language is half of it. The other half is that the compiler answers
questions about a program, so an agent can ask rather than guess.

`glyph init` writes an `AGENTS.md` (the language reference, also printed by
`glyph llms`) and an `.mcp.json` into a new project, so a coding agent finds
both without being told. `glyph mcp` serves the compiler's analysis over MCP, and
`glyph query` asks the same questions from a shell: the full description of a
symbol, every `match` site over a union and which variants each one names, what
breaks if you change a declaration, whether one type is assignable to another.
`glyph check --agent` returns diagnostics as JSON with, for each one, the
constraints a repair has to keep and a description of every symbol it names.

## What I have not measured

The reason Glyph exists is a hypothesis: that an agent given the same task
produces correct code more reliably in Glyph than in TypeScript, and that a
reviewer spends less time confirming it. I have not run that study. There is no
speedup figure in this post because I do not have one. The task I intend to
measure (an agent adding a `Cancelled` state to an order type, ordinary
TypeScript agent against Glyph, same agent and prompt on both sides) is written
down in the roadmap with every result cell still a question mark, and the
losing run gets published along with the winning one.

What exists is narrower. The nine `catches/` programs above. A token count over
three small functions in `benchmarks/`, where Glyph comes out slightly shorter
than TypeScript in total and longer on one of the three; three functions are not
a corpus and I do not lean on it. And 32 applications under `examples/apps/`,
written in Glyph with no TypeScript in them (a CI check enforces that), each with
a README saying what writing it found wrong in the compiler. The most recent is
a todo list on React 18 that CI renders in headless Chromium and checks by
reading the text on the page.

## Where it falls short

Values that come from npm are typed by `tsc`, not by Glyph's own checker. Call a
function from a package with the wrong argument, or assign its result to the
wrong type, and the build fails, because `glyph check` runs `tsc --strict` over
the emitted TypeScript and maps the error back to your line. But the editor, the
MCP tools and `glyph check --no-tsc` do not see it. For example, this passes
`--no-tsc` and fails the full check with TS2322:

```glyph
import path { join }

pub fn f() -> number {
  let n: number = join("a", "b")
  return n
}
```

React is the case people hit first: a state setter called with the wrong type is
a `tsc` error underlined across the whole line, not a Glyph error at the
argument. That is G257 in the gap ledger.

Some other edges that are open today:

- An object literal with a field its declared type does not name is accepted
  (`tsc` refuses it as TS2353). This is G273.
- When two unnamed records do not match, the error says
  ``expected `record`, found `record` `` and does not name the field that
  differs.
- A function passed as a value has its return type compared and its parameter
  types not compared, so a callback with the wrong parameter type passes
  `--no-tsc`.
- The VS Code extension is not on the Marketplace. You build it from
  `editors/vscode/`. Any editor with an LSP client can run `glyph lsp`.
- Releases are frequent, and every release from 0.1.120 to 0.1.128 is marked
  breaking, because each started rejecting programs the one before accepted. Pin the version
  (`glyph init` does) and read the release notes before upgrading.

The gap ledger, `docs/dogfooding-gaps.md`, lists every defect found by building
real programs, with a reproduction for each. At 0.1.128 it holds 277 entries, of
which 18 are open.

## Trying it

```sh
npx @glyphlang/glyph init hello
cd hello && npm install && npx glyph run
```

That prints `hello from glyph`. From there, `docs/guide/tour.md` covers the
language in five minutes and `docs/guide/for-typescript-developers.md` maps what
you know onto it. There is also a playground at glyphlang.io/playground.

The source is at github.com/chadetov/glyph, under MIT or Apache-2.0. If a
restriction gets in your way, or the compiler accepts something it should not,
open an issue with the program. A reproduction of something Glyph lets through
is the most useful report I can get.
