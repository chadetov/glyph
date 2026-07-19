# Glyph for agents

This file takes you from zero to writing correct, runnable Glyph in one read.
No source-diving required. If you only fetch one file about Glyph, fetch this one.

## What Glyph is

Glyph is a statically typed language that **transpiles to TypeScript**. It looks
almost like TypeScript, but it is deliberately stricter on a few axes so that
code is verifiable, greppable, and safe for an agent to edit without breaking it.
Every Glyph module compiles to a readable `.ts` file, runs anywhere TypeScript
runs, and can import any npm package. You adopt it one file at a time.

## Install and run

```sh
npm install -g @glyphlang/glyph     # the compiler (CLI is `glyph`)
npm install -g tsx typescript       # needed for `glyph run` and `--check`
```

```sh
glyph init [dir]                    # scaffold a runnable starter (src/, .types/, package.json)
glyph run path.glyph [args...]      # type-check, compile, and run main(argv)
glyph build src/ --out dist/        # compile a tree to TypeScript (tsc --strict by default)
glyph build src/ --out dist/ --test # also run @example / @doc @run / property tests
glyph fmt [path]                    # format in place (one canonical layout)
glyph gen openapi spec.yaml --out src/  # generate committed Glyph types from an OpenAPI/JSON Schema spec (--client also emits a typed std/http client)
glyph gen dts types.d.ts --out src/     # generate committed Glyph types from a TypeScript .d.ts (needs node + typescript)
glyph gen zod schemas.ts --out src/     # generate committed Glyph types from zod schemas (needs tsx + zod)
glyph llms                          # reprint this bootstrap offline (alias: glyph docs)
glyph --explain E0204               # long-form explanation + fix for any error code
```

## The canonical program shape

```glyph
module hello

import std/io
import std/process

fn main(argv: Array<string>) -> number {
  io.println("hello from glyph")
  return 0
}
```

- Every file starts with `module <name>`.
- `main(argv: Array<string>)` is the entrypoint. Its `number` return is the
  process exit code.
- `main` **may be `async`**: `async fn main(argv: Array<string>) -> number`. The
  runtime `await`s it.
- Imports are either **named** (`import std/result { Ok, Err }` brings names into
  scope) or **namespaced** (`import std/array` then `array.map(...)`).

## Syntax cheat-sheet

### Bindings (`let` / `const`) and mutation (`mut`)

`mut` is a **statement prefix on a mutation**, not a kind of binding. You
introduce a binding with `let` (function-level) or `const` (module-level), and to
*change* it later you write a `mut` statement:

```glyph
let total = 0          // binding (immutable by default; prefer let)
mut total = total + 5  // reassignment: `mut` PREFIXES the assignment
mut user.role = Admin  // field assignment
mut grid[key] = value  // index assignment
mut xs.push(item)      // mutating method call
```

`mut` is legal in exactly four forms — `mut x = e`, `mut x.field = e`,
`mut x[k] = e`, `mut x.method(args)`. A bare assignment without `mut`
(`total = ...`) is a parse error, and `mut foo()` (a free-function call) is
illegal. Because every mutation is marked, `grep -n "^\s*mut "` is a complete
audit of all mutation in a file.

### `match` is the only conditional

There is **no `if`/`else`**. Every branch is a `match`, and **every arm ends with
a trailing comma** (including the last).

```glyph
let label = match user.role {
  Admin => "admin",
  Member => "member",
  Guest => "guest",
}

let sign = match n > 0 {     // match on a bool
  true => "positive",
  false => "non-positive",
}

let kind = match argv {                 // string-literal + array-destructuring patterns
  [] => "empty",
  ["add", text] => "add",
  [head, ..._] => "other",
  else => "fallback",                   // `else` is the catch-all, only as a whole arm
}
```

### Closures

```glyph
let double = fn(n: number) -> number { n * 2 }   // tail expression is the return value
let log = fn(s: string) -> void { io.println(s) }
```

### `Result` / `Option` and the `?` operator

```glyph
import std/result { Result, Ok, Err }

fn parse_age(s: string) -> Result<number, string> {
  let n = number.parse(s)?    // `?` unwraps Ok, or returns the Err from this fn
  return Ok(n)
}
```

Rules for `?`: it may appear **only inside a function whose return type is a
`Result`**; the operand must be a `Result`; on `Ok` it unwraps to the success
value, on `Err` it returns that error from the enclosing function. The error type
`E` must match the enclosing function's `E` **exactly** (there is no `From`
conversion in v1).

### Record literals and sum types

```glyph
type Role =
  | Admin
  | Member
  | Guest

type User = {
  email: string,
  role: Role,
}

let u: User = {                // bare object literal; type comes from the annotation
  email: "a@b.com",            // every field, trailing comma, no `TypeName {}` prefix
  role: Admin,
}
```

A union **variant that carries a payload** is constructed
`Variant({ field: value })`:

```glyph
type Shape =
  | Circle({ radius: number })
  | Square({ side: number })

let c: Shape = Circle({ radius: 2 })
```

There is **no object-literal shorthand** (`{ email }` is rejected; write
`{ email: email }`).

### JSX (components)

`component` declarations emit React function components. JSX control flow uses a
**restricted set of directives** — `<if>`, `<else>`, `<for>`, `<match>`,
`<case>` — not arbitrary `{cond && ...}` expressions.

```glyph
component Greeting(name: string) {
  return <div>
    <if cond={name != ""}>
      <span>Hello, {name}</span>
    </if>
    <else>
      <span>Hello, stranger</span>
    </else>
  </div>
}
```

### Template strings

`"Hello, ${user.email}"` interpolates expressions. (v1 limitation: a literal
`${` cannot be escaped; concatenate strings if you need one.)

## The standard library (full surface)

Call namespaced functions as `module.fn(...)`. Types and constructors come in via
named imports. Signatures below are in Glyph terms.

### Prelude — available with no import

```
number.to_string(n: number) -> string
number.parse(s: string) -> Result<number, string>
par.all<T>(xs: Array<T>) -> Array<T>                 // async; awaits all
par.all_ok<T, E>(xs: Array<Result<T, E>>) -> Result<Array<T>, E>
print(message: string) -> void                       // quick stdout line
assert(condition: bool) -> void                      // throws if false
```

Ambient types (usable with no import): `number`, `string`, `bool`, `void`,
`Array<T>`, `Record<K, V>`, `Schema<T>`, `Issue`.

### std/result

```
type Result<T, E>            // constructors: Ok(value), Err(error)
result.map(f)               // method: transform the Ok value
result.map_err(f)           // method: transform the Err value
```

### std/option

```
type Option<T>              // constructors: Some(value), None
```

### std/array

```
array.find<T>(xs, predicate) -> Option<T>
array.filter<T>(xs, predicate) -> Array<T>
array.map<T, U>(xs, f) -> Array<U>
array.zip<A, B, C>(a, b, f) -> Array<C>
array.len<T>(xs) -> number
array.push<T>(xs, x) -> Array<T>            // returns a new array
array.concat<T>(a, b) -> Array<T>
array.reverse<T>(xs) -> Array<T>
array.slice<T>(xs, start, end?) -> Array<T>
array.any<T>(xs, predicate) -> bool
array.contains<T>(xs, value) -> bool
array.sort<T>(xs, compare) -> Array<T>
```

### std/string

```
string.from(value) -> string
string.join(parts, separator) -> string
string.split(s, separator) -> Array<string>
string.len(s) -> number
string.trim(s) -> string
string.lower(s) -> string
string.upper(s) -> string
string.contains(s, substring) -> bool
string.starts_with(s, prefix) -> bool
string.ends_with(s, suffix) -> bool
```

### std/io

```
io.println(message) -> void
io.eprintln(message) -> void                // to stderr
io.read_line() -> Option<string>
io.read_to_string() -> string
```

### std/json

```
json.parse<T>(text) -> Result<T, Array<Issue>>            // casts; use parse_with to validate
json.parse_with<T>(text, schema) -> Result<T, Array<Issue>>
json.stringify(value, options?) -> string                 // options: { indent: number }
```

For a record/union type `T`, `json.parse<T>(text)` is auto-rewritten to validate
against `T.schema`. Use the `json.parse<T>` namespace form (not the named-import
form) to get validation.

### std/fs

```
type FsError = { kind: ErrorKind, message: string }
fs.read_text(path) -> Result<string, FsError>
fs.write_text(path, contents) -> Result<void, FsError>
fs.exists(path) -> bool
fs.remove(path) -> Result<void, FsError>     // ErrorKind.NotFound for a missing file
```

### std/process

```
process.args() -> Array<string>
process.exit(code) -> never
process.env(name) -> Option<string>
process.cwd() -> string
```

### std/record

```
record.get<V>(r, key) -> Option<V>           // absence-aware read
record.has<V>(r, key) -> bool
record.keys<V>(r) -> Array<string>
record.values<V>(r) -> Array<V>
record.set<V>(r, key, value) -> Record<string, V>   // returns a new record
record.remove<V>(r, key) -> Record<string, V>
```

### std/time

```
type Duration                                 // time.Duration.ms(n) constructs one (namespaced)
time.now() -> number                          // epoch milliseconds
time.sleep(duration) -> void                  // async; await it
time.debounce(delay, f) -> fn                  // returns a debounced function
```

### std/stream and std/test (property testing)

```
type Stream<T>
stream.ints() -> Stream<number>               // 0, -1, 1, -2, 2, ...
stream.bools() -> Stream<bool>
stream.from<T>(values) -> Stream<T>
test.property<T>(predicate, gen, count?) -> Result<void, string>
```

Property tests are deterministic (sampled by index, no RNG). Run them with
`@example` (see Testing below).

### std/http (client + server)

```
type Request  = { url: string, method: string, headers: Record<string, string>, body: unknown }
type Response = { status: number, body: unknown }
type HttpError = { status: number, message: string }
type Handler  = fn(Request) -> Result<Response, string>   // may be async

http.get(url) -> Result<Response, HttpError>          // client; async, await it
http.post(url, body) -> Result<Response, HttpError>   // client; async
http.serve(port, handler) -> Result<void, string>     // server; async, await keeps process alive
http.json(status, body) -> Response                   // application/json response
http.text(status, body) -> Response                   // text/plain response
http.query(req) -> Record<string, string>             // parse the URL query string
http.path(req) -> string                              // URL path without the query
```

A `Handler` returns `Ok(response)` for any status (a 404 is a normal `Ok`) or
`Err(message)` (sent as a 500). `await http.serve(port, handler)` starts the
server and suspends `main`, which keeps the process alive (see the execution
model below).

## Importing external code (npm packages and Node builtins)

A Glyph import path is emitted **verbatim** as the TypeScript module specifier:

```glyph
import react { useState }        // emits: import { useState } from "react";
import http { createServer }     // emits: import { createServer } from "http";
```

So you import an npm package by its package name, and a **Node builtin by its
bare name** (`http`, `fs`, `path`) — **not** `node:http` (the `:` is not a legal
path character in a Glyph import; Node resolves the bare name to the builtin
anyway).

To give the type-checker types for an external module, drop an ambient
declaration file under `<src>/.types/`. Anything matching
`<src>/.types/**/*.d.ts` is auto-discovered and type-checked with your build.
(Full guide with a worked example: `docs/guide/external-imports.md`.)

Worked example:

```
src/
  main.glyph
  .types/
    http.d.ts        // declare module "http" { export function createServer(...): ... }
```

```glyph
module main
import http { createServer }
// ... createServer is now typed from .types/http.d.ts
```

## The execution model

`glyph run` (and a built `main`) does, in effect:

```ts
const code = await main(process.argv.slice(2));
process.exit(typeof code === "number" ? code : 0);
```

That is: it **awaits `main`, then calls `process.exit`**. For a normal CLI this
is exactly right. For a **long-running process** (a server, a watcher), `main`
must not return until you want to exit. `http.serve` is built for this: it stays
pending while the server listens, so `await http.serve(port, handler)` suspends
`main` and the process stays alive until the server closes — no sleep hack. Any
other long-running task follows the same shape: `await` a promise that resolves
only on shutdown.

## Testing

Tests live next to the code and run on build:

```glyph
@example double(21) == 42
fn double(n: number) -> number {
  n * 2
}
```

```glyph
import std/stream
import std/test
import std/result { Ok }

@example test.property(fn(n: number) -> bool { n + 0 == n }, stream.ints()) == Ok(void)
fn identity_holds() -> bool { true }
```

Run them with `glyph build src/ --out dist/ --test`. An `@example expr == expr`
passes when both sides are structurally equal; a bare `@example expr` asserts the
expression is `true`. `@doc """..."""` blocks with a ` ```glyph @run ``` ` fence
also execute. **Limitation:** an `@example` that compares against a prelude
constructor (e.g. `Ok`) must import it (`import std/result { Ok }`).

## Gotchas (read these once, save an hour)

- **`bool`, not `boolean`.** The boolean type is spelled `bool`.
- **`void` is a value and a type.** `-> void` is a valid return type, and `void`
  is a usable value (`Ok(void)`).
- **The tail expression is the return value.** A non-`void` function or block
  returns its last expression; an explicit `return` is optional (both `{ n * 2 }`
  and `{ return n * 2 }` work). `return` is **not** mandatory.
- **Object-literal shorthand is rejected.** Write `{ email: email }`, never
  `{ email }`.
- **Every `match` arm needs a trailing comma**, including the last.
- **Object keys may be quoted strings.** Use `{"Content-Type": x}` for keys that
  are not identifiers; an identifier key stays bareword (`{ plain: x }`).
  Object-literal *shorthand* is still rejected — always write the value. An
  interpolated key (`{"${e}": x}`) is not allowed (no computed keys).
- **`mut` is narrow.** It only enables reassignment and mutating method calls;
  there is no `mut` parameter, field, or other position.
- **No `node:` import prefix.** Import Node builtins by bare name (`import http`).

## Where to go deeper

- Five-minute tour: `docs/guide/tour.md`
- For TypeScript developers (deltas + gotchas): `docs/guide/for-typescript-developers.md`
- Tutorial (a todo CLI): `docs/guide/tutorial.md`
- Full standard-library reference: `docs/reference/stdlib.md`
- Language spec: `docs/language/spec.md`
- Error codes and fixes: `docs/error-codes.md`
- Editor setup: `docs/guide/editor-setup.md`
