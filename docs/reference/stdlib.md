# Standard library reference

Every module in the Glyph v1 standard library, with exact signatures. Signatures
are written in Glyph terms.

**How to call it.** Functions are namespaced: `import std/array` then
`array.map(xs, f)`. Types and constructors come in through named imports:
`import std/result { Result, Ok, Err }`, then `Ok(value)` and `Err(e)` are used
bare. A type's static factory (e.g. `Duration.ms`) is reached through its named
import (`import std/time { Duration }`).

> This page is kept in step with the runtime by a drift-guard test
> (`glyph-cli/tests/stdlib_docs.rs`): every exported name in
> `glyph-compiler/runtime/std/*.ts` must appear here, so a new stdlib function
> can't ship undocumented.

## Prelude (no import required)

These names are global; you use them without an import.

```
number.to_string(n: number) -> string         // format a number
number.parse(s: string) -> Result<number, string>   // parse, validating
par.all<T>(xs: Array<T>) -> Array<T>           // async; await a list of async values
par.all_ok<T, E>(xs: Array<Result<T, E>>) -> Result<Array<T>, E>   // collapse results
print(message: string) -> void                 // quick line to stdout
assert(condition: bool) -> void                 // throw if false (used by @doc @run)
```

Ambient types (no import): `number`, `string`, `bool`, `void`, `Array<T>`,
`Record<K, V>`, `Schema<T>`, `Issue`.

## std/result

The error-handling core. A `Result` is `Ok(value)` or `Err(error)`; match on it,
or use the postfix `?` operator to propagate an `Err`.

```
type Result<T, E>
Ok(value: T) -> Result<T, E>                    // construct a success
Err(error: E) -> Result<T, E>                   // construct a failure
result.map(f: fn(T) -> U) -> Result<U, E>       // method: transform the Ok value
result.map_err(f: fn(E) -> F) -> Result<T, F>   // method: transform the Err value
```

## std/option

```
type Option<T>
Some(value: T) -> Option<T>                     // a present value
None                                            // the absent value (a constant)
```

## std/array

Operations are value-oriented: they return new arrays and never mutate the input.

```
array.find<T>(xs, predicate: fn(T) -> bool) -> Option<T>
array.filter<T>(xs, predicate: fn(T) -> bool) -> Array<T>
array.map<T, U>(xs, f: fn(T) -> U) -> Array<U>
array.zip<A, B, C>(a, b, f: fn(A, B) -> C) -> Array<C>
array.len<T>(xs) -> number
array.push<T>(xs, x: T) -> Array<T>             // returns a new array with x appended
array.concat<T>(a, b) -> Array<T>
array.reverse<T>(xs) -> Array<T>
array.slice<T>(xs, start: number, end?: number) -> Array<T>
array.any<T>(xs, predicate: fn(T) -> bool) -> bool
array.contains<T>(xs, value: T) -> bool
array.sort<T>(xs, compare: fn(T, T) -> number) -> Array<T>
```

## std/string

```
string.from(value) -> string                    // any value to its string form
string.join(parts: Array<string>, separator: string) -> string
string.split(s: string, separator: string) -> Array<string>
string.len(s: string) -> number
string.trim(s: string) -> string
string.lower(s: string) -> string
string.upper(s: string) -> string
string.contains(s: string, substring: string) -> bool
string.starts_with(s: string, prefix: string) -> bool
string.ends_with(s: string, suffix: string) -> bool
```

## std/io

```
io.println(message: string) -> void             // stdout, with newline
io.eprintln(message: string) -> void            // stderr, with newline
io.read_line() -> Option<string>                // one line from stdin (None at EOF)
io.read_to_string() -> string                   // all of stdin
```

## std/json

```
json.parse<T>(text: string) -> Result<T, Array<Issue>>          // decode; casts to T
json.parse_with<T>(text: string, schema: Schema<T>) -> Result<T, Array<Issue>>
json.stringify(value, options?: { indent: number }) -> string
json.discriminant(value: unknown, field: string) -> Option<string>  // read a string discriminator property; dispatch a discriminated union
```

For a record/union type `T`, the namespace form `json.parse<T>(text)` is
auto-rewritten to validate against `T.schema`. Use that form (not the
named-import `parse`) when you want validation rather than a bare cast.

## std/fs

Synchronous text file I/O. Errors are values: match on `e.kind` to recover.

```
type ErrorKind = { tag: string }               // ErrorKind.NotFound for a missing file
type FsError = { kind: ErrorKind, message: string }
fs.read_text(path: string) -> Result<string, FsError>
fs.write_text(path: string, contents: string) -> Result<void, FsError>
fs.exists(path: string) -> bool
fs.remove(path: string) -> Result<void, FsError>
```

## std/process

```
process.args() -> Array<string>                 // program arguments
process.exit(code: number) -> never
process.env(name: string) -> Option<string>
process.cwd() -> string
```

## std/record

Helpers over `Record<string, V>`. Reads are absence-aware; updates return a new
record and never mutate the input.

```
record.get<V>(r, key: string) -> Option<V>
record.has<V>(r, key: string) -> bool
record.keys<V>(r) -> Array<string>
record.values<V>(r) -> Array<V>
record.set<V>(r, key: string, value: V) -> Record<string, V>
record.remove<V>(r, key: string) -> Record<string, V>
```

## std/time

```
type Duration                                   // Duration.ms(n) constructs one
time.now() -> number                            // epoch milliseconds
time.sleep(duration: Duration) -> void          // async; await it
time.debounce<A>(delay: Duration, f: fn(A) -> void) -> fn(A) -> void
```

## std/store

A shared-state primitive. A `Store<T>` holds a value; create one at module scope
(`const s = create(initial)`) so many functions share it without threading a
`let` through `main`. The binding stays `const` and no `mut` is involved — only
the store's internal value changes, through a method call — so every mutation is
a greppable `s.set(...)`/`s.update(...)`.

```
type Store<T>
create<T>(initial: T) -> Store<T>               // a store seeded with initial
store.get() -> T                                 // method: read the current value
store.set(next: T) -> void                       // method: replace it
store.update(change: fn(T) -> T) -> void         // method: map it
```

An empty-collection seed can't infer its element type, so pass an explicit type
argument: `const tasks = create<Array<Task>>([])`.

## std/stream

Deterministic generators for property testing (sampled by index, no RNG).

```
type Stream<T>
stream.ints() -> Stream<number>                 // 0, -1, 1, -2, 2, ...
stream.bools() -> Stream<bool>                  // alternating
stream.from<T>(values: Array<T>) -> Stream<T>   // cycle through a fixed list
```

## std/test

```
test.property<T>(predicate: fn(T) -> bool, gen: Stream<T>, count?: number) -> Result<void, string>
```

Invoke inside an `@example` or `@doc @run` block; it runs at build time and
returns `Ok(void)` when every sample passes, or `Err` with the first
counterexample. Example:

```glyph
@example test.property(fn(n: number) -> bool { n + 0 == n }, stream.ints()) == Ok(void)
```

## std/http

A `fetch`-based client and a small server, both errors-as-values.

```
type Request  = { url: string, method: string, headers: Record<string, string>, body: unknown }
type Response = { status: number, body: unknown }
type HttpError = { status: number, message: string }
type Handler  = fn(Request) -> Result<Response, string>         // may be async
```

Client (async; `await` them):

```
http.get(url: string) -> Result<Response, HttpError>
http.post(url: string, body) -> Result<Response, HttpError>
http.put(url: string, body) -> Result<Response, HttpError>
http.patch(url: string, body) -> Result<Response, HttpError>
http.del(url: string) -> Result<Response, HttpError>    // `del`, not `delete` (reserved word)
```

Server:

```
http.serve(port: number, handler: Handler) -> Result<void, string>   // async; await to keep alive
http.json(status: number, body) -> Response          // application/json response
http.text(status: number, body: string) -> Response  // text/plain response
http.query(req: Request) -> Record<string, string>   // parse the URL query string
http.path(req: Request) -> string                    // URL path without the query
http.header(req: Request, name: string) -> Option<string>       // a header (case-insensitive), None if absent
http.query_param(req: Request, name: string) -> Option<string>  // one query parameter, None if absent
http.segments(req: Request) -> Array<string>         // path split into non-empty segments, for array-pattern routing
```

A `Handler` returns `Ok(response)` for any status (a 404 is a normal `Ok`) or
`Err(message)` to send a 500. `serve` resolves `Ok(void)` when the server closes
and `Err(message)` on a bind failure; while it listens it stays pending, so a
`main` that does `await http.serve(...)` keeps the process alive — no keep-alive
hack. A minimal server:

```glyph
import std/http { serve, query, text, Request, Response }
import std/record
import std/result { Result, Ok }
import std/option { Some, None }

fn multiply(req: Request) -> Result<Response, string> {
  let a = match record.get(query(req), "a") { Some(v) => number.parse(v), None => number.parse(""), }
  let b = match record.get(query(req), "b") { Some(v) => number.parse(v), None => number.parse(""), }
  return match a {
    Ok(av) => match b {
      Ok(bv) => Ok(text(200, number.to_string(av * bv))),
      Err(e) => Ok(text(400, e)),
    },
    Err(e) => Ok(text(400, e)),
  }
}

async fn main(argv: Array<string>) -> number {
  let _ = await serve(8080, multiply)
  return 0
}
```

## std/schema

Mostly internal: the factory behind a record type's auto-generated `T.schema`.

```
schema<T>(name: string, is: fn(unknown) -> bool) -> Schema<T>
```

`Schema<T>` and `Issue` are ambient prelude types:

```
type Issue = { path: Array<string | number>, message: string }
type Schema<T> = {
  name: string,
  parse(input: unknown) -> Result<T, Array<Issue>>,
  array() -> Schema<Array<T>>,
}
```
