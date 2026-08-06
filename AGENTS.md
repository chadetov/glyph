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
glyph run path.glyph [args...]      # type-check, compile, and run main(argv); reports every diagnostic `glyph build` reports
glyph build src/ --out dist/        # compile a tree to TypeScript (tsc --strict and @example/@doc @run by default)
glyph build src/ --out dist/ --json # emit diagnostics as JSON (code, severity, file, line/col, help) for tools/agents
glyph build src/ --out dist/ --no-test # skip the @example / @doc @run / property tests
glyph fmt [path]                    # format in place (one canonical layout)
glyph fmt --check [path]            # exit non-zero if anything is unformatted (CI), writes nothing
glyph gen openapi spec.yaml --out src/  # generate committed Glyph types from an OpenAPI/JSON Schema spec (--client: a typed std/http client; --handlers: server stubs + a router)
glyph gen dts types.d.ts --out src/     # generate committed Glyph types from a TypeScript .d.ts (needs node + typescript)
glyph gen zod schemas.ts --out src/     # generate committed Glyph types from zod schemas (needs tsx + zod)
glyph llms                          # reprint this bootstrap offline (alias: glyph docs)
glyph --explain E0204               # long-form explanation + fix for any error code
glyph mcp [root]                    # run an MCP server (stdio) exposing analysis to an agent as tools
```

`glyph run` builds the whole directory the file sits in, so it reports exactly
the diagnostics `glyph build` would report on that tree, warnings included. They
go to stderr after the program's output, followed by a `glyph run: N error(s), M
warning(s) in the source tree` line. A sibling module that fails to compile is
not importable but does not stop the run, and today does not change the exit
code: `glyph run` exits with whatever `main` returned.

If you drive Glyph through the Model Context Protocol, `glyph mcp [root]` speaks
MCP over stdio and exposes five tools over the project: `glyph_diagnostics`
(type-check one file → coded diagnostics with ranges), `glyph_hover` (the
inferred type at a position), `glyph_definition` (where a name is defined,
following imports), `glyph_references` (every reference to a symbol across the
whole project — declaration, uses, and each importing module's import binding),
and `glyph_symbols` (search declarations by name). Positions are 0-based
`line`/`character` (UTF-16). This is the interactive complement to
`glyph build --json`, which remains the batch path for coded diagnostics.

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
- **`await` on a method chain.** A fluent chain of value methods awaits the whole
  chain: `await cursor.find({}).to_array()` awaits `to_array` (the async
  terminal), as in JavaScript. The Result idiom awaits the head call: `await
  load(p).map_err(f)` runs `map_err` on the awaited `Result`. A bare or namespaced
  function head (`load(...)`, `http.get(...)`) is the Result idiom; a value-method
  head (`x.method(...)`) is fluent. You write the natural thing either way.
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

A `match` that is the whole value of a `let` or a `mut` accepts everything a
statement accepts: a block-bodied arm, an `await` in an arm, a `break` or
`continue` that leaves the surrounding loop, and an arm that reads the binding it
is assigning (the accumulator form).

```glyph
mut in_fence = match is_fence(line) {   // reads the binding it assigns
  true => !in_fence,
  false => in_fence,
}

let cache = match offline {
  true => no_cache(),                   // `{}` here is an empty BLOCK, not a record
  false => await fetch_all(urls),
}
```

A `match` nested **inside a larger expression** (an argument, an operand, a field
of a literal) compiles to a closure, so its arms must be single expressions and a
`return` inside one is illegal. Hoist it into its own `let` and the restriction
goes away.

### Loops (`for` / `loop`)

There is no `while`. `for` iterates a bounded collection; `loop` is the
unbounded form and needs a `break`.

```glyph
for item in items {                  // one binding: the element
  io.println(item.name)
}

for i, item in items {               // two bindings: index (0-based number), element
  io.println("${i}: ${item.name}")
}

for key, value in scores {           // over a Record: key (string), value
  io.println("${key} = ${value}")
}

// The array form is picked from the iterand's declared type. Iterating a call's
// result directly gives you a STRING index and nothing complains, so bind it:
let rows: Array<string> = array.slice(lines, 1)

for i, row in rows {
  io.println("${i}: ${row}")
}

loop {                               // unbounded; `break`/`continue` are legal
  match done() {
    true => break,
    false => step(),
  }
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

`"Hello, ${user.email}"` interpolates expressions, calls included:
`"total: ${format_money(sum)}"`. Write `\${` for a literal dollar-brace. The one
thing the interior cannot hold is another string literal (`"${f("x")}"`) — bind
it to a `let` first.

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
array.fold<T, A>(xs, init, f) -> A          // f is (acc, x); no index
array.index_of<T>(xs, value) -> Option<number>
array.flat_map<T, U>(xs, f) -> Array<U>     // flattens one level
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
string.repeat(s, count) -> string           // a negative count yields "" (TS throws)
string.pad_start(s, width, pad?) -> string  // pad defaults to a space
string.pad_end(s, width, pad?) -> string
string.slice(s, start, end?) -> string
string.index_of(s, needle, from?) -> Option<number>   // None, not -1
string.replace_all(s, from, to) -> string   // every occurrence
string.trim_start(s) -> string
string.trim_end(s) -> string
```

Argument order: every module except `std/regex` takes the subject first;
`std/regex` takes the pattern first. So `string.replace_all(s, from, to)` and
`regex.replace_all(pattern, text, replacement)` are opposite orders, as are
`string.split(s, separator)` and `regex.split(pattern, text)`, and every
parameter of all four is a `string`, so a swap compiles and prints the wrong
thing.

Both `index_of` functions return `Option`, which `tsc` enforces, but Glyph does
not model their return type yet: a `match` on `index_of` with no `None` arm is
not an E0200 and throws at run time. Write the `None` arm.

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

### Runtime validators (`T.parse` / `T.is` / `T.schema`)

Every record (and non-generic union) type `T` you declare also generates a
runtime descriptor with three members. This is the mechanism behind
`json.parse<T>`, and it is how a boundary value becomes typed:

```
T.is(value: unknown) -> bool                          // shape guard for declared fields
T.parse(value: unknown) -> Result<T, Array<Issue>>    // validate an unknown into a Result
T.schema                                              // a Schema<T> (e.g. T.schema.array())
```

Use `T.parse` on an already-decoded `unknown` (a request body, a config object);
use `json.parse<T>(text)` when you have a raw JSON *string*. There is no `as`
cast in Glyph, so `T.parse` (or a `match`/`is` narrowing) is the only way to go
from `unknown` to `T`.

```glyph
type User = { id: number, name: string }

fn handle(body: unknown) -> string {
  return match User.parse(body) {   // untrusted input, validated
    Ok(user) => user.name,
    Err(_) => "invalid",
  }
}
```

A record descriptor is strict by default: it confirms the declared fields *and*
rejects a value carrying undeclared keys. Put `@open` above a `type` to allow
extra keys (`@open` then the `type` line).

A **generic** record type (`Paginated<T>`) also gets a descriptor. Call it with
the type argument: `Paginated.parse<User>(body)` validates the page deeply —
each `items` entry is checked as a `User`, not just for presence. The `is`
pattern works the same: `match v { is Paginated<User> => ..., else => ... }`.
The compiler synthesizes the per-parameter checker at the call site, so the type
argument must be given explicitly. A generic descriptor omits the `.schema`
member. Scope today: descriptors cover non-generic and generic record types;
tagged unions and imported/`.d.ts` types don't get one (materialize an imported
type with `glyph gen dts` to give it one). A `.d.ts` (or OpenAPI) discriminated
union (`{ kind: "a"; ... } | { kind: "b"; ... }`) materializes as a tagged union
of generated variant records plus a `parse_<Name>(v)` dispatcher that reads the
tag and validates into the right variant.

To build a validator *combinator* (a `zod`-style `object_schema`) whose output
type follows the shape you pass, use the `infer_output<Shape>` type operator so
you don't repeat the output type by hand:

```glyph
fn object_schema<Shape: Record<string, Schema<unknown>>>(
  shape: Shape,
) -> Schema<infer_output<Shape>> { ... }

// The shape must produce a `User`, or this does not compile:
const user_schema: Schema<User> = object_schema({
  name: string_schema(),
  age: number_schema(),
})
```

`infer_output<Shape>` unwraps each field's parser to the type it outputs, so the
compiler derives the schema's output type from the shape and checks it against
your annotation. It matches a parser field *structurally* (any
`{ parse(input: unknown) -> Result<V, _> }`), so the wrapper need not be named
`Schema` — your own `Codec<T>` works too. The generic parameter is bound with
`<Shape: Bound>` (this is how generic bounds are written; they lower to a
TypeScript `extends` clause).

### std/fs

```
type ErrorKind = NotFound | IsADirectory | NotADirectory | PermissionDenied | AlreadyExists | Other({ code: string })
type FsError = { kind: ErrorKind, message: string }
type FileInfo = { is_dir: bool, is_file: bool, size: int, modified: int }   // size bytes, modified epoch ms
fs.read_text(path) -> Result<string, FsError>
fs.write_text(path, contents) -> Result<void, FsError>
fs.append_text(path, contents) -> Result<void, FsError>   // append, creating the file (append-only logs)
fs.make_dir(path) -> Result<void, FsError>                // create dir + parents, idempotent (mkdir -p)
fs.exists(path) -> bool
fs.remove(path) -> Result<void, FsError>     // ErrorKind.NotFound for a missing file
fs.read_dir(path) -> Result<Array<string>, FsError>       // entry names, not full paths; not recursive, OS order
fs.is_dir(path) -> bool                                   // false for a missing or unreadable path
fs.stat(path) -> Result<FileInfo, FsError>                // follows symlinks
```

`match e.kind` is not checked for exhaustiveness, so keep an `else` arm. Walk a
tree with `read_dir` + `is_dir` + `path.join`; there is no `walk` helper.

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
time.format_iso(epoch_ms) -> string           // ISO-8601 UTC string (no need for a Date via extern_ts)
time.parse_iso(iso) -> Option<number>          // epoch ms; strict ISO-8601 only (see below)
time.add_days(epoch_ms, days) -> number
time.add_hours(epoch_ms, hours) -> number
time.year(epoch_ms) / month(epoch_ms) / day(epoch_ms) -> number   // UTC; month is 1-12
```

`parse_iso` takes a bare `YYYY-MM-DD` (UTC midnight) or a datetime with an
explicit `Z`/`+HH:MM`/`-HH:MM` offset, and returns `None` for anything else. An
offset-less datetime (`"2026-01-03T10:00"`) is rejected: ECMAScript reads it in
local time, which would move the day the UTC accessors report. `"2026-1-3"`,
`"January 5 2026"`, and an impossible day like `"2026-02-31"` are `None` too.

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
type Request  = { url: string, method: string, headers: Record<string, string>, body: unknown, raw: string }
type Response = { status: number, headers: Record<string, string>, body: unknown }
type HttpError = { status: number, message: string }
type Handler  = fn(Request) -> Result<Response, string>   // may be async

http.get(url) -> Result<Response, HttpError>          // client; async, await it
http.post(url, body) -> Result<Response, HttpError>   // client; async
http.serve(port, handler) -> Result<void, string>     // server; async, await keeps process alive
http.json(status, body) -> Response                   // application/json response
http.text(status, body) -> Response                   // text/plain response
http.html(status, body) -> Response                   // text/html response
http.redirect(status, location) -> Response           // 30x with a `location` header
http.with_header(resp, name, value) -> Response       // a copy carrying one more header
http.query(req) -> Record<string, string>             // parse the URL query string
http.path(req) -> string                              // URL path without the query
http.form(req) -> Record<string, string>              // parse an x-www-form-urlencoded body
http.raw(req) -> string                               // unparsed request body, for HMAC signature verification
```

`req.body` is the parsed body; `http.raw(req)` is the unparsed bytes as received,
what an HMAC signature must be verified over (re-serializing the parsed body
changes whitespace and key order, so a recomputed signature would not match).
`http.form(req)` parses that raw body as `x-www-form-urlencoded` (`+` is a space,
percent escapes decode, a repeated key keeps the last value) without touching
`req.body`.

`Response.headers` is required and every constructor fills it in. The content
type is inferred from the body only when the headers do not already name one.
Every character Node refuses to write in a header is stripped from the value
first, so a `location` built from user input can neither split the response
(CR/LF) nor crash the server (anything above U+00FF). A client call reports the
response headers it received, with the names lowercased.

A `Handler` returns `Ok(response)` for any status (a 404 is a normal `Ok`) or
`Err(message)` (sent as a 500). `await http.serve(port, handler)` starts the
server and suspends `main`, which keeps the process alive (see the execution
model below).

### std/sqlite (persisted SQL over node:sqlite)

```
type Row = Record<string, unknown>            // a queried row: the untrusted boundary
type Db                                        // an open database handle

open(path) -> Db                               // ":memory:" or a file path (persists)
db.exec(sql) -> void                           // DDL / statements with no params or result
db.run(sql, params) -> number                  // INSERT/UPDATE/DELETE; returns rows affected
db.last_insert_id() -> number                  // last AUTOINCREMENT rowid on this connection
db.query(sql, params) -> Array<Row>            // rows come back as Record<string, unknown>
db.query_one(sql, params) -> Option<Row>       // first row, or None
db.close() -> void
```

A query returns rows as `Record<string, unknown>`, so a row is a validated
boundary like a request body: reach for `RowType.parse(row)` before trusting it,
never a cast. SQLite has no boolean type (a flag column is an integer `0`/`1`),
so model the storage shape and the domain shape as separate types and map
between them. A full example is `examples/apps/tasks.glyph`.

### std/decimal (exact money math, no floats)

```
type Decimal
decimal(text) -> Result<Decimal, string>   // parse "10.50"; Err on malformed, never NaN
from_int(units, scale) -> Decimal            // from_int(1050, 2) is 10.50
zero: Decimal
d.add(o) / d.sub(o) / d.mul(o) -> Decimal    // methods; exact
d.div(o, scale) -> Decimal                   // method; rounds half away from zero to `scale` digits
d.round(scale) / d.neg() / d.abs() -> Decimal
d.cmp(o) -> int    d.eq(o) -> bool    d.is_zero() -> bool    d.is_negative() -> bool
d.to_string() -> string    d.to_number() -> number   // to_number is lossy, display only
```

Use this for money, never `number`: JS `number` is a float (`0.1 + 0.2 != 0.3`)
and loses precision past 2^53. Glyph has no operator overloading, so operations
are methods (`price.add(tax)`). Construction validates and returns a `Result`.

For exact large *whole* numbers (account ids, counters, 64-bit values) use the
`bigint` prelude type, not `number`: write literals as `123n`, and a record field
typed `bigint` validates `typeof === "bigint"` at the boundary (a JSON number is
rejected, not silently truncated). `number` loses precision past 2^53; `bigint`
does not. `tsc` keeps `bigint` and `number` apart (no mixed arithmetic).

A `where` refinement types an *invariant* that a boundary must validate:
`type Amount = int where value >= 0`, `type Rating = int where value >= 1 && value <= 5`,
`type NonEmpty = string where value.length > 0`. The predicate (over a bound
`value`) is woven into the type's descriptor, so `Amount.parse(x)` rejects a
negative value, not just a non-number. The predicate runs wherever the type is
used: a record field typed `Amount`, an `Array<Amount>` element, an
`Option<Amount>` payload, a union variant's payload, and `json.parse<Amount>`.
So does the descriptor of a type imported from another Glyph module, so a field
typed by an imported record is checked against that record and not just for
presence. v1 refines primitive base types only
(`int`/`number`/`string`/`bool`/`bigint`); a `where` on a record or union is a
compile error, not a silent drop.

### std/taint (untrusted-input discipline as types)

```
type Tainted<T>    type Trusted<T>
taint(value) -> Tainted<T>                                  // wrap untrusted input
sanitize(t: Tainted<T>, clean: fn(T) -> T) -> Trusted<T>    // escape/validate, then trust
trust_unchecked(value) -> Trusted<T>                        // escape hatch (literals); greppable
expose(t: Trusted<T>) -> T                                  // unwrap at the sink
reveal_tainted(t: Tainted<T>) -> T                          // read raw, only to inspect/sanitize
```

Type a sink's parameter `Trusted<string>` (a SQL runner, a shell command, an HTML
renderer) and a `Tainted<string>` cannot reach it without going through
`sanitize` first: `tsc` rejects the call. This is discipline enforced by types
(you opt in per sink), not automatic flow analysis. SQL injection becomes a
compile error.

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

To write **whole hand-written TypeScript** a Glyph module calls (an idiom Glyph
can't spell, a node-stream loop, a `new Promise`), put the `.ts` under
`<src>/extern/` and import it as `import extern/<name>`. The build stages
`<src>/extern/**` into the output and emits a relative specifier, so the file is
type-checked (its exported types enforce your Glyph calls) and resolved at
runtime, and a rebuild never prunes it. This is the only way a Glyph module
imports a local `.ts`: relative imports are illegal (D15), so `extern/*` is the
reserved, greppable path. (`extern_ts("...")` is the smaller escape hatch, for a
single inline type or expression rather than a module.) `glyph run`'s build
cache hashes every `.ts` and `.tsx` under `<src>/extern/` by path and by
contents, so editing, renaming, adding, or deleting a shim rebuilds and
re-type-checks; symlinked shims are followed.

A class-based client is instantiated with `new`, which Glyph has for exactly
this interop and nothing else: `import kafkajs { Kafka }` then
`let k = new Kafka({ clientId: "app", brokers: [b], })`. `new <callee>(<args>)`
emits a verbatim TypeScript `new` and is type-checked by `tsc` against the real
constructor (a wrong argument is a real error; an undefined callee is E0103).
Glyph has no `class` declarations of its own; `new` only constructs a type that
comes from an npm package, a `.types` ambient declaration, or `extern_ts`. A
factory-style client (`createClient()`, `createConnection()`) needs no `new` at
all: call it directly.

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

They run on every `glyph build` (needs `tsx` on PATH; `--no-test` skips them).
An `@example expr == expr` passes when both sides are structurally equal; a bare
`@example expr` asserts the expression is `true`. `@doc """..."""` blocks with a
` ```glyph @run ``` ` fence also execute. A failing one fails the build, under
`--json` too. **Limitation:** an `@example` that compares against a prelude
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
- **Write a `//` comment on its own line, above what it documents.** `glyph fmt`
  keeps a comment above the same item, including inside a record, a union variant
  list, an array or object literal, an argument list, and a `match` (a construct
  holding an interior comment stays one-element-per-line so the comment has an
  item to sit above). A comment always lands on its own line, so one written at
  the end of a code line moves to the line above the next item.
- **No `node:` import prefix.** Import Node builtins by bare name (`import http`).

## Diagnostic codes

Every error and warning carries a stable code and a one-line fix. `glyph
--explain <code>` prints the long form; `glyph build --json` emits them
machine-readably. The full catalogue:

| Code | Meaning | Fix |
|---|---|---|
| E0001 | Lexical error (unterminated string, bad escape, stray char) | Fix the string/escape/character |
| E0002 | Expected a different token (Glyph is stricter than TS) | Match the expected syntax |
| E0003 | Unexpected token here | Remove or relocate it |
| E0004 | Expected end of file | Balance your braces |
| E0005 | Construct recognized but not implemented | Use a supported form |
| E0006 | `if`/`else` used where Glyph has none (D3) | Rewrite as a `match` |
| E0007 | Range/comparison pattern (`500..599 =>`) in a match arm | Enumerate the values as separate arms |
| E0008 | Assignment without `mut` (`x = e`) (D5) | Write `mut x = e`, or `let x = e` for a new binding |
| E0100 | Duplicate top-level name | Rename one; names are unique |
| E0101 | Relative import | Use an absolute module path (`std/io`, `myapp/x`) |
| E0102 | Barrel file (only imports) | Add a declaration or remove the file |
| E0103 | Unresolved name | Declare it, import it, or fix the spelling |
| E0104 | Unresolved module path | Check the path / that the module exists |
| E0105 | Name not exported by the module | Check the export name |
| E0106 | Unused import (warning) | Remove it |
| E0107 | Unused variable (warning) | Remove it, or prefix the name with `_` |
| E0108 | Unreachable code after return/break/continue (warning) | Remove the dead code |
| E0109 | Reserved word (class, switch, eval, ...) used as a name | Rename the declaration or binding |
| E0200 | Non-exhaustive match on a tagged union | Handle every variant, or add an `else` |
| E0201 | `?` outside a Result-returning fn | Return `Result`, or handle with `match` |
| E0202 | `?` on a non-Result operand | Drop the `?`, or return a `Result` |
| E0203 | `?` error type mismatch (no `From` in v1) | `.map_err(...)` to line the error types up |
| E0204 | Type mismatch | Make the value and the expected type agree |
| E0205 | `owned` on a non-`resource` type | Mark the type `resource`, or drop `owned` |
| E0206 | `owned` resource not consumed on every path | Consume it (move to an `owned` param) on all paths |
| E0207 | `owned` resource used after consume | Reorder so uses precede the consume |
| E0208 | Non-exhaustive array match | Cover the length, or add a catch-all |
| E0209 | Non-exhaustive `bool` match | Cover `true` and `false`, or add `else` |
| E0210 | Field access with no such field | Fix the field name / add it to the type |
| E0211 | Call argument type mismatch | Pass a value of the expected type |
| E0212 | `mut` reassigns a `const` | Use a function-level `let` |
| E0213 | Wrong number of call arguments | One argument per parameter |
| E0214 | Component with multiple parameters | Take a single props record |
| E0215 | Aliasing an `owned` handle | Consume it directly, don't rebind |
| E0216 | Unreachable match arm after a total pattern | Remove it, or move the catch-all last |
| E0217 | Discarded `Result` (warning) | `match`/`?` it, or `let _ = ...` to say it's intentional |
| E0218 | Non-exhaustive match on `number`/`string` | Add an `else` arm |
| E0219 | `@redact` names a missing field | Fix the field name |
| E0220 | A `match` arm's PascalCase head is not a variant of the union (typo or wrong union) | Fix the spelling (a `did you mean` suggestion is offered), or add the variant |
| E0221 | Unknown `@annotation` (D27) | Use a recognized one: `@example`, `@doc`, `@redact`, `@open`, `@pure`, `@public` |
| E0222 | `await` outside an `async fn` | Mark the enclosing callable `async fn` (a sync lambda is its own context) |
| E0223 | A `match` arm produces no value while the match is used as a value | End the arm with an expression, or `return` from it |
| E0300 | Construct not supported by the emitter | Use a supported form |
| E0301 | An `<else>` that is not the immediate sibling of its `<if>` (D6) | Move the `<else>` next to its `<if>` |
| E0302 | `?` in an arm of a match nested inside a larger expression | Bind the match first (`let x = match ...`), then use `?` |
| E0303 | `?` where the unwrap has nothing to hoist into (a `match` scrutinee) | Bind the operand first (`let r = f(x)?`), then use `r` |
| E0310 | `glyph run` on a module with no `fn main` | Add `fn main`, or `glyph build` it as a library |

### A diagnostic in the self-correction loop

`glyph build --json` gives you the machine-readable version an agent can act on
directly. A program that forgets a `match` arm:

```
$ glyph build src --out dist --json
{
  "ok": false,
  "errors": 1,
  "diagnostics": [
    {
      "code": "E0200",
      "severity": "error",
      "message": "non-exhaustive match on `Status`: missing variants Cancelled",
      "file": "src/main.glyph",
      "range": { "start": { "line": 6, "col": 10 }, "end": { "line": 9, "col": 4 } },
      "stage": "typecheck",
      "help": "Add an arm for each missing variant, or an `else` arm to catch the rest."
    }
  ]
}
```

Read `code` + `help`, add the missing arm, rebuild. That is the loop the design
is built for.

## Recipes by task (copy, adapt)

- **read a file:** `import std/fs` then `match fs.read_text(path) { Ok(t) => ..., Err(e) => ... }`
- **parse untrusted JSON to a type:** `let v = json.parse(body)?` then `T.parse(v)` (validates structure and leaf values)
- **HTTP GET + decode:** `let r = await http.get(url).map_err(fn(e) { e.message })?` then `T.parse(r.body)`
- **serve HTTP:** `import std/http { serve, text, Request, Response }`; handler returns `Result<Response, string>` via `Ok(text(200, "..."))`
- **branch:** `match x { A => ..., B => ..., }` (no `if`; every case, trailing comma)
- **fail:** return `Result<T, E>`; propagate with `?`, recover with `match`
- **share state:** module-level `const s = store.create(init)`; `mut s.update(fn(v) { ... })`
- **run concurrently:** `await task.all([fn() { a() }, fn() { b() }])`
- **bounded concurrency:** `await task.pool(4, tasks)` runs the thunks with at most 4 in flight (fail-fast); `task.pool_settled(4, tasks)` keeps going past a failure
- **regex:** `regex.matches(pat, text)`, `regex.find_all(pat, text)`, `regex.captures_all(pat, text)` for the groups of every match
- **walk a directory:** `let names = fs.read_dir(dir)?` for entry names, `fs.is_dir(path.join([dir, name]))` to recurse, `let info = fs.stat(p)?` for size/mtime
- **hash / uuid:** `crypto.sha256(s)`, `crypto.random_uuid()`
- **paths:** `path.join(["a", "b"])`, `path.extname(p)`
- **cleanup:** `defer handle.close()` (runs on every exit path)
- **generic bound:** `interface Named { fn name() -> string }` then `fn f<T: Named>(x: T)`
- **hide a helper:** omit `pub` (module-private by default); `pub` to export

## Where to go deeper

- Five-minute tour: `docs/guide/tour.md`
- For TypeScript developers (deltas + gotchas): `docs/guide/for-typescript-developers.md`
- Tutorial (a todo CLI): `docs/guide/tutorial.md`
- Full standard-library reference: `docs/reference/stdlib.md`
- Language spec: `docs/language/spec.md`
- Error codes and fixes: `docs/error-codes.md`
- Editor setup: `docs/guide/editor-setup.md`
