# Glyph: a TypeScript-shaped language whose compiler refuses what `tsc --strict` lets through

Glyph is a statically typed language that compiles to TypeScript. It reads like
TypeScript, runs wherever TypeScript runs and imports npm packages directly. The
differences from TypeScript are few, and each one exists so that a program stays
correct when something other than its author edits it, which these days is
often a coding agent. The current release is 0.1.130. It is a preview: the
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
the case names, the build goes red. There are five cases, and they are all
below.

None of this means TypeScript has no way to get these checks. Some of them are
a compiler flag or a lint rule away, and I say which. The difference is that in
Glyph they are not something a project opts into, and they cannot be turned off.

### A key that may not be there

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

The typo makes the example easy to read, but it is not what Glyph objects to.
The correctly spelled `row.name` gets the same E0224, because a map says nothing
about which keys it holds. TypeScript's `noUncheckedIndexedAccess` catches this
too. It is not part of `--strict`, so a project has to know to turn it on.

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

This one survives `noUncheckedIndexedAccess`: with the flag on, `host` becomes
`string | undefined` and the template literal still accepts it.

### A case added to a union

```ts
// tsc --strict: no errors
export type Status = "todo" | "doing" | "done" | "archived";

export function announce(s: Status): void {
  switch (s) {
    case "todo":
      console.log("not started");
      break;
    case "doing":
      console.log("in progress");
      break;
    case "done":
      console.log("finished");
      break;
  }
}
```

`"archived"` was added to `Status` and this handler was not updated. For that
value it does nothing, and nothing says so. A `switch` that returns a value from
each case would be caught, because falling off the end of a function declared
to return `string` is TS2366. A handler that logs, or updates a map, or
dispatches, has no return value to miss, and that is the shape event handlers
and reducers take. In Glyph, `match` is the only conditional, and it has to
cover every case wherever it appears:

```glyph expect-error E0200
module m

pub type Status = "todo" | "doing" | "done" | "archived"

pub fn announce(s: Status) {
  match s {
    "todo" => print("not started"),
    "doing" => print("in progress"),
    "done" => print("finished"),
  }
}
```

```text
[E0200] Error: typecheck: non-exhaustive match on `"todo" | "doing" | "done" | "archived"`: missing variants "archived"
```

typescript-eslint's `switch-exhaustiveness-check` rule catches the TypeScript
version, and so does an `assertNever(s)` in a `default` branch, in a project
that uses them.

### A validator that never checked the socket

```ts
// tsc --strict: no errors
import type { Socket } from "net";

export type Conn = { id: number; sock: Socket };

export function isConn(value: unknown): value is Conn {
  return (
    typeof value === "object" &&
    value !== null &&
    typeof (value as Record<string, unknown>).id === "number" &&
    (value as Record<string, unknown>).sock !== undefined
  );
}
```

A type guard is a promise the compiler takes on trust. This one checks that
`sock` exists and says the value is a `Conn`, so a caller gets a `Socket` that
may be a string. In Glyph every record gets a generated `parse`, and the
compiler refuses to generate one it cannot honour:

```glyph expect-error E0304
module m

import net { Socket }

pub type Conn = {
  id: number,
  sock: Socket,
}

pub fn take(v: unknown) -> Result<Conn, Array<Issue>> {
  return Conn.parse(v)
}
```

```text
[E0304] Error: emit: cannot validate `Conn`: field `sock` has type `Socket`, which has no runtime check
   Help: Split the wire type from the domain type: parse a record whose fields
         are all checkable, then build this one from it.
```

Holding a socket in a record is fine. Claiming to have validated one is not.

### A parsed body nobody checked

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
this, and no flag changes it. The same line in Glyph:

```glyph expect-error E0204
module m

import std/json

pub type User = {
  id: number,
  email: string,
}

pub fn email_of(body: string) -> string {
  let user: User = json.parse(body)
  return user.email
}
```

```text
[E0204] Error: typecheck: type mismatch: expected `User`, found `Result<unknown, Array<Issue>>`
```

Glyph's `json.parse` returns a `Result`, so there is no way to get a `User` out
of it without matching on it, and with a type argument it validates the body
first. The way through looks like this:

```glyph
module m

import std/json

pub type User = {
  id: number,
  email: string,
}

pub fn email_of(body: string) -> string {
  return match json.parse<User>(body) {
    Ok(user) => user.email,
    Err(_) => "",
  }
}
```

Run all five with `python3 scripts/check_catches.py`. The TypeScript halves are
checked with `@types/node` installed, since one of them imports from `net`.

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

What exists is narrower. The five `catches/` programs above. A token count over
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

```glyph expect-error TS2322
module m

import path { join }

pub fn f() -> number {
  let n: number = join("a", "b")
  return n
}
```

React is the case people hit first: a state setter called with the wrong type is
a `tsc` error, mapped back to the argument, that the editor does not show. That
is G257 in the gap ledger.

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
- Releases are frequent, and every release from 0.1.120 to 0.1.130 is marked
  breaking, because each started rejecting programs the one before accepted. Pin the version
  (`glyph init` does) and read the release notes before upgrading.

The gap ledger, `docs/dogfooding-gaps.md`, lists every defect found by building
real programs, with a reproduction for each. At 0.1.130 it holds 301 entries, of
which 25 are open and 8 partly fixed.

## Trying it

```sh
npx @glyphlang/glyph init hello
cd hello && npm install && npx glyph run
```

That prints `hello from glyph`. From there, `docs/guide/tour.md` covers the
language in five minutes and `docs/guide/for-typescript-developers.md` maps what
you know onto it. There is also a playground at
https://glyphlang.io/playground/, which compiles one file in the browser. It
does not run `tsc`, so the last case above does not reproduce there.

The source is at github.com/chadetov/glyph, under MIT or Apache-2.0. If a
restriction gets in your way, or the compiler accepts something it should not,
open an issue with the program. A reproduction of something Glyph lets through
is the most useful report I can get.
