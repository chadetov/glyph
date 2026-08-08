# Standard library reference

Every module in the Glyph v1 standard library, with exact signatures. Signatures
are written in Glyph terms.

**How to call it.** Functions are namespaced: `import std/array` then
`array.map(xs, f)`. A function can also be named-imported and then called bare:
`import std/io { println }` gives you `println("hi")`. Types and constructors
come in through named imports:
`import std/result { Result, Ok, Err }`, then `Ok(value)` and `Err(e)` are used
bare. A type and its static factory come in either way: `import std/time` gives
you `time.Duration` and `time.Duration.ms(5)`, `import std/time { Duration }`
gives you the bare `Duration` and `Duration.ms(5)`. Write both lines when you
want both spellings.

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
array.fold<T, A>(xs, init: A, f: fn(A, T) -> A) -> A
array.index_of<T>(xs, value: T) -> Option<number>
array.flat_map<T, U>(xs, f: fn(T) -> Array<U>) -> Array<U>
array.range(count: number) -> Array<number>                    // [0, 1, ..., count-1]
array.range_from(start: number, end: number) -> Array<number>  // [start, ..., end-1]
```

`fold` takes the callback last, so it reads like the rest of the module; the
callback gets `(acc, x)` and no index. `index_of` compares with `===`, like
`contains`, so for records use `find` with an explicit comparison, and it returns
an `Option` whose missing-`None`-arm case Glyph does not yet catch (see the note
at the end of `std/string`). `flat_map` flattens one level. `range`/`range_from`
are the counted loop `for` has no other source for (`for i in array.range(n)`).
`range` clamps its count the way `string.repeat` does, so a negative count gives
`[]` and a fractional one truncates. `range_from`'s second argument is an
exclusive end bound, the same reading `array.slice` and `string.slice` give a
second numeric argument: `range_from(2, 5)` is `[2, 3, 4]`, and an end at or
below `start` gives `[]`.

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
string.repeat(s: string, count: number) -> string
string.pad_start(s: string, width: number, pad?: string) -> string
string.pad_end(s: string, width: number, pad?: string) -> string
string.slice(s: string, start: number, end?: number) -> string
string.index_of(s: string, needle: string, from?: number) -> Option<number>
string.replace_all(s: string, from: string, to: string) -> string
string.trim_start(s: string) -> string
string.trim_end(s: string) -> string
```

Indices are UTF-16 code units, the same space `len` and `split` use, and
negative `slice` indices count back from the end exactly as `array.slice` does.
That is the whole model, and it is not going to change: there is no codepoint
accessor and none is planned, because an accessor that can hand back half of a
surrogate pair is worse than no accessor at all. A program that has to walk
codepoints (a percent-encoder, a width calculator) encodes to bytes first with
`encoding.hex_encode` and reads two hex digits at a time; `examples/apps/shortlink.glyph`
does exactly that in its slug encoder.
Two functions diverge from their TypeScript namesakes on purpose: `repeat`
clamps a negative count to `""` where TS throws, which is what makes
`repeat(pad, width - len(s))` safe, and `index_of` returns `None` instead of
`-1`. `replace_all` replaces every occurrence; there is no first-only form.
`pad_start` and `pad_end` leave a string that is already at least `width` long
untouched, and default the pad to a single space.

One limit on `string.index_of`. Glyph's checker models the return type of most
`std/string` and `std/array` functions, so a `match` that leaves out the `None`
arm is an E0200. The six functions with an optional trailing argument
(`string.slice`, `string.index_of`, `string.pad_start`, `string.pad_end`,
`array.slice`, `json.stringify`) are the exception. The arity check compares one
number against one number, so modeling them would report a false error on every
call that omits the last argument, and until it learns a range they stay
untyped. A `match` on `string.index_of` that omits the `None` arm therefore
builds clean and throws at run time; write the arm. `array.index_of` is modeled
and does report E0200.

## std/io

```
io.println(message: string) -> void             // stdout, with newline
io.eprintln(message: string) -> void            // stderr, with newline
io.read_line() -> Option<string>                // one line from stdin (None at EOF)
io.read_to_string() -> string                   // all of stdin
io.inspect(value: unknown) -> void              // pretty-print any value to stderr (debugging)
io.render(value: unknown) -> string             // the same rendering as a string
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

Synchronous text file I/O and directory inspection. Errors are values: match on
`e.kind` to recover.

```
type ErrorKind =
  | NotFound | IsADirectory | NotADirectory | PermissionDenied | AlreadyExists
  | Other({ code: string })                    // the raw errno for everything unnamed
type FsError = { kind: ErrorKind, message: string }
type FileInfo = { is_dir: bool, is_file: bool, size: int, modified: int }  // size in bytes, modified in epoch ms
fs.read_text(path: string) -> Result<string, FsError>
fs.write_text(path: string, contents: string) -> Result<void, FsError>
fs.append_text(path: string, contents: string) -> Result<void, FsError>   // append, creating the file; the primitive for an append-only log
fs.make_dir(path: string) -> Result<void, FsError>                        // create the dir and any parents; idempotent (`mkdir -p`)
fs.exists(path: string) -> bool
fs.remove(path: string) -> Result<void, FsError>
fs.read_dir(path: string) -> Result<Array<string>, FsError>               // entry names, not full paths; not recursive
fs.is_dir(path: string) -> bool                                           // false for a missing or unreadable path
fs.stat(path: string) -> Result<FileInfo, FsError>                        // follows symlinks
```

The five named kinds cover what a filesystem program recovers from. EACCES and
EPERM both arrive as `PermissionDenied`, so those two raw codes are the ones you
cannot recover; every other unnamed errno keeps its code on `Other`. The
typechecker knows this shape, so a `match e.kind` is held to the same
exhaustiveness bar as a union you declared yourself: cover all six kinds and no
`else` arm is needed, omit one and the build fails with E0200 on
`fs.ErrorKind`. `e.kind` and `e.message` are checked members too, so a typo is
E0210 rather than a `tsc` error.

```
fn reason(e: fs.FsError) -> string {
  return match e.kind {
    fs.ErrorKind.NotFound => "no such file or directory",
    fs.ErrorKind.PermissionDenied => "permission denied",
    fs.ErrorKind.IsADirectory => "is a directory",
    fs.ErrorKind.NotADirectory => "not a directory",
    fs.ErrorKind.AlreadyExists => "already exists",
    fs.ErrorKind.Other({ code }) => "unrecognized error ${code}",
  }
}
```

`read_dir` returns names in whatever order the OS gives, which differs across
platforms and filesystems. Sort them when the output has to be reproducible.
Walking a tree is `read_dir` + `is_dir` + `path.join([dir, name])`; there is no
`walk` or glob helper.

```
fs.read_dir(dir).map(fn(names: Array<string>) {
  return array.filter(names, fn(n: string) { return fs.is_dir(path.join([dir, n])) })
})
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

All six are modeled in the checker, and `V` is read off the record you pass. So
`record.get(t, k)` over a `Record<string, Array<string>>` is an
`Option<Array<string>>`, the `Some(p)` arm of a `match` over it binds an
`Array<string>`, and `for i, hop in p` binds `i` as a number without an
annotation. `record.keys(t)` is an `Array<string>`, which is what lets
`array.sort(record.keys(t), cmp)` keep its element type.

## std/time

Two import lines, and they buy different names:

```
import std/time                                 // the `time` namespace: time.now(), time.sleep(d),
                                                // and time.Duration as both a type and a factory
import std/time { Duration }                    // the bare name `Duration`, as a type and a factory
```

They are independent. `import std/time` alone gives you `time.Duration.ms(5)`
and `x: time.Duration`; `import std/time { Duration }` alone gives you
`Duration.ms(5)` and `x: Duration`, but not `time.sleep`. Code that wants both
spellings writes both lines, which is why `examples/apps/linkcheck.glyph` has
them on consecutive lines.

```
type Duration                                   // Duration.ms(n) or time.Duration.ms(n)
time.now() -> number                            // epoch milliseconds
time.sleep(duration: Duration) -> void          // async; await it
time.debounce<A>(delay: Duration, f: fn(A) -> void) -> fn(A) -> void
time.format_iso(epoch_ms: number) -> string     // ISO-8601 UTC string
time.parse_iso(iso: string) -> Option<number>   // epoch ms; strict ISO-8601 only
time.add_days(epoch_ms: number, days: number) -> number
time.add_hours(epoch_ms: number, hours: number) -> number
time.year(epoch_ms: number) -> number           // UTC calendar accessors
time.month(epoch_ms: number) -> number          // 1-12 (not JS's 0-11)
time.day(epoch_ms: number) -> number
```

All calendar work is in UTC, so results don't shift with the host timezone.

`parse_iso` accepts two shapes and returns `None` for everything else: a bare
date `YYYY-MM-DD`, read as UTC midnight, or `YYYY-MM-DDTHH:MM(:SS)?(.sss)?`
followed by `Z`, `+HH:MM`, or `-HH:MM`. The surprising rejection is a datetime
with no offset: `"2026-01-03T10:00"` is `None`, because ECMAScript reads that
form in the host's local time and the day it lands on would then depend on where
the process runs. `"2026-1-3"` and `"January 5 2026"` are `None` for the same
reason. An impossible day is `None` as well: `"2026-02-31"` does not become
March 3, and February 29 is only accepted in a leap year.

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

## std/task

Structured-concurrency helpers over promises. Pass task thunks (`fn() -> T`,
usually `async`); the scope bounds the lifetime you await, so concurrent work is
joinable rather than detached. `all` is fail-fast; `all_settled` keeps every
outcome so a partial failure never loses the successes.

```
type Settled<T>                                          // { ok: true, value: T } | { ok: false, error: unknown }
all<T>(tasks: Array<fn() -> T>) -> Array<T>              // run concurrently, join in order (fail-fast)
race<T>(tasks: Array<fn() -> T>) -> T                    // first task to settle
pool<T>(limit: number, tasks: Array<fn() -> T>) -> Array<T>   // at most `limit` in flight, join in order (fail-fast)
all_settled<T>(tasks: Array<fn() -> T>) -> Array<Settled<T>>  // one outcome per task, never rejects
pool_settled<T>(limit: number, tasks: Array<fn() -> T>) -> Array<Settled<T>>  // bounded, one outcome per task, never rejects
```

`pool` is fail-fast: the first rejection rejects the pool. It does not stop the
run, because nothing in JavaScript can. The other workers keep draining the queue
and every result they produce is thrown away, so a fail-fast pool over 500 URLs
still sends all 500. `pool_settled` is the same bound with `all_settled`'s
behaviour: a task that throws costs one result, and you get the other 499. Read
the `unknown` in a failed outcome with `string.from(e)`.

Each is `async`, so `await` the result. JavaScript can't force-cancel a running
task, so a failure in `all` abandons its siblings' results rather than halting
their work; thread an AbortSignal into your task bodies for cooperative
cancellation.

## std/regex

Regular expressions over the JS engine. Patterns are strings; every call is
stateless (a fresh `RegExp`), so there is no shared-`lastIndex` surprise.

```
matches(pattern: string, text: string) -> bool           // does it match anywhere
find_all(pattern: string, text: string) -> Array<string> // every match (global)
find_first(pattern: string, text: string) -> string      // first match, or ""
captures(pattern: string, text: string) -> Array<string> // capture groups of the first match
captures_all(pattern: string, text: string) -> Array<Array<string>>  // capture groups of every match
replace_all(pattern: string, text: string, replacement: string) -> string
split(pattern: string, text: string) -> Array<string>
```

`captures` and `captures_all` return groups 1 onward: the whole match is not in
the array, so group 1 is at index 0. That differs from JavaScript's `matchAll`,
which puts the whole match at index 0. They also differ on absence: a group that
did not participate is `""`, not `undefined`, so an empty capture and a missing
one read the same.

That matters when one pattern alternates over several shapes and you need to know
which one fired. Wrap each branch's group around the whole construct rather than
around the part you want, and nest the payload group inside it. A group that
fired then always starts with a literal character and can never be empty, so
non-empty is a reliable test, and the payload group is never the thing you ask
about:

```
// 1 fired: a code span. 2 fired: a link, and 3 is its target, which may be empty.
const INLINE = "(`[^`]*`?)|(!?\\[[^\\]]*\\]\\(([^)]*)\\))"
```

Neither function reports where a match started. A scanner that needs the offset,
or whose discriminator can legitimately match empty, still writes its own loop.

`std/regex` takes the pattern first. Every other module takes the subject first,
so `regex.replace_all(pattern, text, replacement)` and
`string.replace_all(s, from, to)` are the same idea in opposite orders, as are
`regex.split(pattern, text)` and `string.split(s, separator)`. Every parameter
in all four is a `string`, so a swapped call compiles, passes `tsc --strict`,
and produces the wrong text.

## std/set

A hash set with value semantics for primitives; maps use the built-in
`Record<K, V>`. Like `std/store`, state lives in a closure and every mutation is
a greppable method call.

```
type Set<T>
create<T>(initial?: Array<T>) -> Set<T>          // an empty or seeded set
set.add(value: T) -> void                         // method: insert
set.has(value: T) -> bool                         // method: membership
set.remove(value: T) -> bool                      // method: delete (was it present)
set.size() -> number                              // method: cardinality
set.values() -> Array<T>                          // method: members as an array
unique<T>(values: Array<T>) -> Array<T>           // de-duplicate, order-preserving
```

## std/path

Cross-platform filesystem paths over node's `path`; the host separator is
respected, so the same code runs on Unix and Windows.

```
join(parts: Array<string>) -> string             // join segments
dirname(p: string) -> string
basename(p: string) -> string
extname(p: string) -> string
is_absolute(p: string) -> bool
normalize(p: string) -> string
relative(from: string, to: string) -> string
```

## std/crypto

Hashing, HMAC, and randomness over node's `crypto`, returning hex strings.
Security primitives belong in the standard library, not an unvetted dependency.

```
sha256(input: string) -> string
sha512(input: string) -> string
hmac_sha256(key: string, input: string) -> string
random_uuid() -> string                           // a v4 UUID
random_hex(count: number) -> string               // count random bytes, hex (length count * 2)
```

## std/math

Numeric helpers over JavaScript's `Math`.

```
math.PI, math.E                                  // constants
math.abs(x: number) -> number
math.floor(x: number) -> number
math.ceil(x: number) -> number
math.round(x: number) -> number
math.trunc(x: number) -> number
math.sqrt(x: number) -> number
math.sign(x: number) -> number                   // -1, 0, or 1
math.min(a: number, b: number) -> number
math.max(a: number, b: number) -> number
math.pow(base: number, exponent: number) -> number
math.imul(a: number, b: number) -> number        // 32-bit integer multiply
math.clamp(x: number, lo: number, hi: number) -> number
```

## std/random

A seeded, reproducible PRNG (mulberry32). Not cryptographic, use `std/crypto`
for security-sensitive randomness.

```
type Rng
seeded(seed: number) -> Rng                       // a generator fixed by the seed
rng.next() -> number                              // method: next float in [0, 1)
rng.int(lo: number, hi: number) -> number         // method: whole number in [lo, hi)
rng.bool(probability: number) -> bool             // method: true with this probability
rng.pick<T>(items: Array<T>) -> T                 // method: a uniform element
```

## std/encoding

base64, base64url, and hex text encodings.

```
encoding.base64_encode(s: string) -> string
encoding.base64_decode(s: string) -> string
encoding.base64url_encode(s: string) -> string    // URL-safe alphabet, no padding
encoding.base64url_decode(s: string) -> string
encoding.hex_encode(s: string) -> string
encoding.hex_decode(s: string) -> string
```

## std/log

Structured (JSON-line) logging. Each call emits one JSON object with `level`,
`msg`, and a timestamp to stdout (info/debug) or stderr (warn/error).

```
type Level                                        // "debug" | "info" | "warn" | "error"
log.debug(message: string) -> void
log.info(message: string) -> void
log.warn(message: string) -> void
log.error(message: string) -> void
log.with_fields(level: Level, message: string, fields: Record<string, unknown>) -> void
```

## std/collections

Ordered collections beyond `Array`/`Record`. A `Deque<T>` is a double-ended
queue; ends that may be empty return `Option<T>`.

```
type Deque<T>
deque<T>(initial?: Array<T>) -> Deque<T>
dq.push_back(value: T) -> void                    // method
dq.push_front(value: T) -> void                   // method
dq.pop_back() -> Option<T>                        // method
dq.pop_front() -> Option<T>                       // method
dq.peek_front() -> Option<T>                      // method
dq.peek_back() -> Option<T>                       // method
dq.len() -> number                                // method
dq.values() -> Array<T>                           // method
```

## std/sqlite

A persisted SQL database over Node's built-in synchronous SQLite
(`node:sqlite`). `open` returns a `Db` handle; rows come back as
`Record<string, unknown>` (the untrusted boundary), so validate each with a
type's `.parse` before trusting it.

```
type Row                                          // Record<string, unknown>
type Db
open(path: string) -> Db
db.exec(sql: string) -> void                       // method: DDL, no params/result
db.run(sql: string, params: Array<unknown>) -> number     // method: INSERT/UPDATE/DELETE, rows affected
db.last_insert_id() -> number                      // method: last auto-increment rowid
db.query(sql: string, params: Array<unknown>) -> Array<Row>       // method
db.query_one(sql: string, params: Array<unknown>) -> Option<Row>  // method: first row or None
db.close() -> void                                 // method
```

## std/decimal

Exact base-10 fixed-point arithmetic for money. A `Decimal` is an
arbitrary-precision integer scaled by a number of fractional digits (BigInt
under the hood), so there is no floating-point error (`0.1 + 0.2` is exactly
`0.3`) and no precision loss past 2^53. Operations are methods (Glyph has no
operator overloading). Construction validates and returns a `Result`.

```
type Decimal
decimal(text: string) -> Result<Decimal, string>   // parse "10.50"; Err on malformed
from_int(units: int, scale: int) -> Decimal          // from_int(1050, 2) is 10.50
zero: Decimal
d.add(other: Decimal) -> Decimal                     // method; exact
d.sub(other: Decimal) -> Decimal                     // method; exact
d.mul(other: Decimal) -> Decimal                     // method; exact
d.div(other: Decimal, scale: int) -> Decimal         // method; rounds half away from zero to `scale` digits
d.round(scale: int) -> Decimal                       // method
d.neg() -> Decimal                                   // method
d.abs() -> Decimal                                   // method
d.cmp(other: Decimal) -> int                         // method; -1 | 0 | 1
d.eq(other: Decimal) -> bool                         // method
d.is_zero() -> bool                                  // method
d.is_negative() -> bool                              // method
d.scale() -> int                                     // method
d.to_string() -> string                              // method; canonical "10.50"
d.to_number() -> number                              // method; lossy, for display only
```

## std/taint

Untrusted-input discipline as types. `Tainted<T>` marks a value from outside the
program (a request body, a query param, user input); `Trusted<T>` marks one that
has been sanitized. They are structurally distinct, so a sink whose parameter is
`Trusted<string>` (a SQL runner, a shell command, an HTML renderer) **cannot**
receive a `Tainted<string>` without going through `sanitize` first: `tsc` rejects
it. This is discipline enforced by types, not automatic flow analysis; you opt in
by typing a sink's parameter `Trusted<...>`.

```
type Tainted<T>                                      // untrusted, from outside
type Trusted<T>                                      // sanitized
taint(value: T) -> Tainted<T>                        // wrap untrusted input
sanitize(t: Tainted<T>, clean: fn(T) -> T) -> Trusted<T>   // escape/validate, then trust
trust_unchecked(value: T) -> Trusted<T>              // escape hatch (literals/constants); greppable
expose(t: Trusted<T>) -> T                           // unwrap at the sink
reveal_tainted(t: Tainted<T>) -> T                   // read raw, only to inspect/sanitize
```

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
type Request  = { url: string, method: string, headers: Record<string, string>, body: unknown, raw: string }
type Response = { status: number, headers: Record<string, string>, body: unknown }
type HttpError = { status: number, message: string }
type Handler  = fn(Request) -> Result<Response, string>         // may be async
```

`body` is the parsed body; `raw` is the unparsed bytes exactly as received (read
it with `http.raw(req)`, below), which is what a signature check (HMAC over the
payload) must run over, since re-serializing a parsed body changes whitespace and
key order.

`Response.headers` is a required field, on both halves of the module: a
constructor always fills it in, and a client call reports the response headers it
received with the names lowercased, so a program never has to check whether the
header set is there before reading it.

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
http.html(status: number, body: string) -> Response  // text/html response
http.redirect(status: number, location: string) -> Response  // 302/301/303/307/308 with a `location` header
http.with_header(resp: Response, name: string, value: string) -> Response  // a copy carrying one more header
http.query(req: Request) -> Record<string, string>   // parse the URL query string
http.path(req: Request) -> string                    // URL path without the query
http.form(req: Request) -> Record<string, string>    // parse an x-www-form-urlencoded body
http.raw(req: Request) -> string                     // the unparsed request body, for signature (HMAC) verification
http.header(req: Request, name: string) -> Option<string>       // a header (case-insensitive), None if absent
http.query_param(req: Request, name: string) -> Option<string>  // one query parameter, None if absent
http.segments(req: Request) -> Array<string>         // path split into non-empty segments, for array-pattern routing
```

A `Handler` returns `Ok(response)` for any status (a 404 is a normal `Ok`) or
`Err(message)` to send a 500. `serve` resolves `Ok(void)` when the server closes
and `Err(message)` on a bind failure; while it listens it stays pending, so a
`main` that does `await http.serve(...)` keeps the process alive — no keep-alive
hack.

The content type comes from the constructor: `json` sets `application/json`,
`text` sets `text/plain; charset=utf-8`, `html` sets `text/html; charset=utf-8`.
A response whose headers do not name a content type gets one inferred from the
body (a string is text, anything else is JSON), which is what every response did
before `headers` existed. `with_header` returns a new `Response` rather than
mutating one, since Glyph has no record-field mutation, and it replaces a header
of the same name compared case-insensitively. Every character Node refuses to
write in a header is removed from the value on the way out, so a `location` built
from user input cannot inject a second header or a second response (CR and LF),
and cannot take the server down either (a character above U+00FF makes Node throw
from inside the response writer, where there is no `Result` to catch it in).

`form` reads `req.raw`, so it does not change what `req.body` holds: a handler
that wants the raw bytes or a JSON body still gets exactly those. It decodes `+`
as a space and percent-escapes as their bytes, and a key repeated in the body
keeps the last value.

A minimal server:

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

A page, a form post, and a redirect:

```glyph
import std/http { path, form, html, redirect, text, Request, Response }
import std/record
import std/result { Result, Ok }
import std/option { Some, None }

fn route(req: Request) -> Result<Response, string> {
  return match path(req) {
    "/" => Ok(html(200, "<form method=\"post\" action=\"/new\"><input name=\"url\"></form>")),
    "/new" => match record.get(form(req), "url") {
      Some(url) => Ok(redirect(302, "/")),
      None => Ok(html(400, "<p>missing url</p>")),
    },
    else => Ok(text(404, "not found")),
  }
}
```

## std/schema

Mostly internal: the factory behind a record type's auto-generated `T.schema`.

```
schema<T>(name: string, is: fn(unknown) -> bool) -> Schema<T>
```

`Schema<T>` and `Issue` are ambient prelude types:

```
type Issue = {
  path: Array<string | number>,
  message: string,
  code?: "missing" | "type" | "refinement" | "unexpected",
}
type Schema<T> = {
  name: string,
  parse(input: unknown) -> Result<T, Array<Issue>>,
  array() -> Schema<Array<T>>,
}
```

`code` says which rule the value broke, so a handler branches on it instead of
matching the message text: `"missing"` for a required field that was absent,
`"type"` for a value of the wrong shape (including a non-object or an array
where a record was expected), `"refinement"` for a value that passed its base
type but failed a `where` predicate, and `"unexpected"` for a key the type does
not declare. It is optional, so an `Issue` you build by hand still checks.

The `message` names the field and what it needed. A record field whose type has
its own descriptor delegates to that type's `parse`, so nested failures arrive
with the full path (`["body", "password"]`) and a refinement's rejection carries
its predicate: `expected Password (string where value.length >= 8)`. A value
that is not a `string` in the first place failed the base type, not the
predicate, and reports `expected Password (string)` with `code: "type"`.
