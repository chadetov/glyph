# Cookbook

Task-oriented recipes. Each is a small, complete snippet you can paste and
adapt. For the standard-library surface, see
[`reference/stdlib.md`](../reference/stdlib.md).

## Read a file, handle the error

```glyph
import std/fs

fn load(path: string) -> Result<string, string> {
  return match fs.read_text(path) {
    Ok(text) => Ok(text),
    Err(e) => Err(e.message),
  }
}
```

## Walk a directory tree

`fs.read_dir` lists one level and does not recurse, so a walk is `read_dir` plus
`is_dir` plus `path.join`. There is no `walk` or glob helper.

```glyph
import std/array
import std/fs
import std/path

fn walk(dir: string) -> Result<Array<string>, fs.FsError> {
  let names = fs.read_dir(dir)?
  let found: Array<string> = []
  for name in names {
    let child = path.join([dir, name])
    mut found = match fs.is_dir(child) {
      true => {
        let sub = walk(child)?
        array.concat(found, sub)
      },
      false => array.push(found, child),
    }
  }
  return Ok(found)
}
```

`read_dir` hands back names in whatever order the OS gives, which differs across
platforms and filesystems. Sort with `array.sort(names, compare)` when the output
has to be reproducible.

## Recover from a filesystem error by its kind

`FsError.kind` is a closed set, so recovery reads as a `match` on names instead of
a comparison against an errno string.

```glyph
import std/fs

fn reason(e: fs.FsError) -> string {
  return match e.kind {
    fs.ErrorKind.NotFound => "no such path",
    fs.ErrorKind.IsADirectory => "that is a directory",
    fs.ErrorKind.NotADirectory => "that is not a directory",
    fs.ErrorKind.PermissionDenied => "cannot read it",
    fs.ErrorKind.AlreadyExists => "it is already there",
    fs.ErrorKind.Other({ code }) => "errno ${code}",
  }
}
```

No `else` arm, and none is needed: `fs.ErrorKind` is a closed set of six kinds
and the checker knows it, so leaving one out is E0200 rather than a run-time
throw.

## Read stdin a line at a time

`io.read_line` returns as soon as a full line has arrived, so the loop below
answers each line while the person typing is still connected. It returns `None`
at end of input, which is what ends the loop.

```glyph
import std/io
import std/option { Some, None }
import std/string

fn main() -> void {
  io.println("type a word, or Ctrl-D to stop")
  loop {
    let line = match io.read_line() {
      Some(l) => l,
      None => { break },
    }
    let word = string.trim(line)
    match word == "" {
      true => {},
      false => { io.println("${word} has ${string.len(word)} characters") },
    }
  }
}
```

A trailing `\r` is stripped, so a CRLF file and an LF file read the same, and
input that ends without a newline still hands back that last line once.

`read_to_string` drains the same buffer, so it returns whatever stdin has left.
Call it first for the whole stream, or after a few `read_line`s to take the rest:

```glyph
let header = io.read_line()
let body = io.read_to_string()
```

Two things an interactive program cannot do yet. There is no way to write without
a newline, so a `> ` prompt has to be a line of its own, and nothing reports
whether stdin is a terminal or a pipe, so a program that behaves differently for
each has to be told by a flag.

## Parse untrusted JSON into a validated type

```glyph
import std/json

type User = { name: string, age: int }

fn parse_user(body: string) -> Result<User, string> {
  let value = json.parse(body)?
  return User.parse(value)
}
```

`User.parse` checks the structure and the leaf values at runtime; a `3.5` age is
rejected because the field is `int`, not `number`.

## Call an HTTP API and decode the response

```glyph
import std/http

type Todo = { id: int, title: string, done: bool }

async fn fetch_todo(id: int) -> Result<Todo, string> {
  let response = await http.get("https://api.example.com/todos/${id}")
    .map_err(fn(e) { e.message })?
  return Todo.parse(response.body)
}
```

## Serve HTTP

```glyph
import std/http { serve, text, Request, Response }
import std/result { Result, Ok }

async fn main(argv: Array<string>) -> number {
  let result = await serve(8080, fn(req: Request) -> Result<Response, string> {
    Ok(text(200, "hello"))
  })
  return 0
}
```

## Serve an HTML page and redirect

```glyph
import std/http { serve, path, form, html, redirect, text, Request, Response }
import std/record
import std/result { Result, Ok }
import std/option { Some, None }

fn route(req: Request) -> Result<Response, string> {
  return match path(req) {
    "/" => Ok(html(200, "<h1>hello</h1>")),
    "/new" => match record.get(form(req), "url") {
      Some(url) => Ok(redirect(302, "/")),
      None => Ok(html(400, "<p>missing url</p>")),
    },
    else => Ok(text(404, "not found")),
  }
}
```

`html`, `redirect`, `text`, and `json` each set their own content type;
`with_header(resp, name, value)` returns a copy carrying one more header.
`form(req)` parses an `x-www-form-urlencoded` body (`+` is a space, percent
escapes decode, a repeated key keeps the last value).

## Match on a tagged union exhaustively

```glyph
type Shape =
  | Circle({ radius: number })
  | Rect({ w: number, h: number })

fn area(s: Shape) -> number {
  return match s {
    Circle({ radius }) => 3.14159 * radius * radius,
    Rect({ w, h }) => w * h,
  }
}
```

Add a variant and this `match` stops compiling until you handle it.

When the union lives in another module you can import it either way, and both
are checked the same:

```glyph
import shapes { Shape, Circle, Rect }  // arms read Circle({ radius }) => …
import shapes                          // arms read shapes.Circle({ radius }) => …
import shapes as s                     // arms read s.Circle({ radius }) => …
```

The named form puts every variant you match on in the import list, which is what
a `grep` for `Circle` finds. The namespace form keeps the list to one line and
keeps the union's origin visible at the arm. Pick per module. The same holds for
the standard library's unions, so `option.Some(v)` is exhaustiveness-checked
exactly as a bare `Some(v)` is.

## Share state across functions

```glyph
import std/store

const counter = store.create<number>(0)

fn bump() -> void {
  mut counter.update(fn(n) { n + 1 })
}

fn total() -> number {
  return counter.get()
}
```

## Run work concurrently

```glyph
import std/task

async fn dashboard() -> Array<string> {
  return await task.all([fn() { fetch_a() }, fn() { fetch_b() }, fn() { fetch_c() }])
}
```

## Run many tasks, bounded, and survive the failures

`task.pool` caps how many run at once but is fail-fast: the first task that
throws rejects the pool, and every result its siblings already produced is
discarded. `task.pool_settled` is the same bound with `all_settled`'s behaviour,
so one dead host costs you one row instead of the whole report.

```glyph
import std/array
import std/io
import std/string
import std/task

async fn check_all(urls: Array<string>) -> void {
  let tasks = array.map(urls, fn(u: string) -> fn() -> number { return fn() -> number { return probe(u) } })
  let outcomes = await task.pool_settled(4, tasks)
  let i = 0
  for s in outcomes {
    let line = match s.ok {
      true => "${urls[i]} ${s.value}",
      false => "${urls[i]} failed: ${string.from(s.error)}",
    }
    io.println(line)
    mut i = i + 1
  }
}
```

Outcomes come back in the order the tasks went in, so the nth outcome belongs to
the nth URL. The counter is hand-rolled rather than `for i, s in outcomes`
because `outcomes` came out of `task.pool_settled`, which the checker does not
model, and an iterand with no type binds a string index. See the index-loop
recipe below. A failed task's `error` is `unknown`; read it with `string.from`.

## Give an async callback a type

`fn(A) -> T` emits `(a: A) => T`, which an async body does not fit, so a function
that hands back a deferred task or a record of async handlers used to go
unannotated. `async fn(A) -> T` is the type for it: it emits `(a0: A) =>
Promise<T>`, and `async fn()` with no return type emits `() => Promise<void>`.
Write it anywhere a type goes.

```glyph
import std/option { Some, None }
import std/record

type Handler = async fn(string) -> string

fn task_for(url: string) -> async fn() -> Fetched {
  return async fn() -> Fetched { return { url: url, body: await fetch_one(url) } }
}

async fn dispatch(routes: Record<string, Handler>, name: string, arg: string) -> string {
  return match record.get(routes, name) {
    Some(h) => await h(arg),
    None => "no route",
  }
}
```

Glyph checks the distinction itself instead of leaving it to `tsc`: a plain
`fn() -> T` where an `async fn() -> T` is expected is E0204 at a return and E0211
at a call argument, and the message says `expected async function, found
function`. A `void` return is the one case it lets pass in both directions,
because TypeScript accepts any function there.

## Pull the capture groups out of every match

`regex.find_all` gives you the matched text and drops the groups.
`regex.captures_all` gives you the groups of every match, one inner array per
match, starting at group 1.

```glyph
import std/array
import std/regex

const PAIR = "([a-z_]+)=([^;]*)"

fn settings(text: string) -> Array<{ key: string, value: string }> {
  return array.map(regex.captures_all(PAIR, text), fn(g: Array<string>) -> { key: string, value: string } {
    return { key: g[0], value: g[1] }
  })
}
```

A group that did not participate comes back as `""`, the same as one that matched
empty. When a pattern alternates over several shapes and you need to know which
one fired, put each branch's group around the whole construct rather than around
the payload you want: a group that fired then starts with a literal character and
can never be empty. `examples/apps/linkcheck/main.glyph` does this to tell a code
span, an inline link, and an autolink apart in one pass.

## Validate with a string-literal union

```glyph
type Tier = "free" | "pro" | "enterprise"

fn price(t: Tier) -> int {
  return match t {
    "free" => 0,
    "pro" => 20,
    "enterprise" => 100,
  }
}
```

The `match` is exhaustive with no `else`; add a tier to the union and every
`match` over it must handle it.

That holds when the union lives in another module. Declare it once and import it,
by name (`import billing { Tier }`), by namespace (`import billing` with a
`billing.Tier` annotation), or through an alias: a `match` covering every literal
still needs no `else`, and one that misses a literal is still E0200. If a `match`
you expect to be checked comes back E0218 instead, the scrutinee is typed
`string`, not `Tier`.

## Hash and random identifiers

```glyph
import std/crypto

fn session_id() -> string {
  return crypto.random_uuid()
}

fn fingerprint(payload: string) -> string {
  return crypto.sha256(payload)
}
```

## Loop over a collection

```glyph
import std/io
import std/string

fn print_all(items: Array<string>) -> void {
  for item in items {
    io.println(item)
  }
}
```

## Loop with the index (or the key)

A second binding gives you the position without a hand-rolled counter. Over an
array it is the numeric index; over a `Record` it is the string key.

The emitter picks the array lowering from the iterand's type. Most stdlib calls
carry one, so `for i, part in string.split(text, ",")` and `for i, x in
array.filter(xs, keep)` bind a number directly. The exceptions are the calls the
checker still cannot type: `array.slice` and `string.slice` (an optional trailing
argument the arity check cannot express yet) and `array.map`/`flat_map`/`zip`
(their element type comes from the callback). `for i, x in array.slice(xs, 1)`
hands you the *string* `"0"` and nothing fails, so bind it first: `let ys:
Array<string> = array.slice(xs, 1)`, then loop over `ys`. Tracked in
`docs/dogfooding-gaps.md` as G37.

You do not need the annotation for a value that came out of a `match` or out of
`T.parse` either. Those carry their type, so `for i, e in ledger.expenses` below
binds a number.

```glyph
import std/io
import std/number

type Expense = { description: string, cents: int }
type Ledger = { expenses: Array<Expense> }

fn list_expenses(decoded: unknown) -> Result<void, string> {
  let ledger = match Ledger.parse(decoded) {
    Ok(l) => l,
    Err(issues) => {
      return Err("not a ledger")
    },
  }

  for i, e in ledger.expenses {
    io.println("${number.to_string(i + 1)}. ${e.description}")
  }
  return Ok(void)
}
```

```glyph
import std/io

fn numbered(items: Array<string>) -> void {
  for i, item in items {
    io.println("${i + 1}. ${item}")
  }
}

fn print_scores(scores: Record<string, number>) -> void {
  for name, score in scores {
    io.println("${name}: ${score}")
  }
}
```

## Total, count, or group without a `mut`

`array.fold` takes the collection, then the seed, then a callback that gets
`(acc, x)`. Reach for it whenever a loop's only job is to build one value:
`grep mut` is supposed to find the places state changes, and an accumulator
loop fills it with arithmetic nobody needs to audit.

```glyph
import std/array
import std/math
import std/record
import std/string

type Expense = { category: string, cents: number }

fn total(expenses: Array<Expense>) -> number {
  return array.fold(expenses, 0, fn(sum, e) { sum + e.cents })
}

fn widest(labels: Array<string>) -> number {
  return array.fold(labels, 0, fn(w, l) { math.max(w, string.len(l)) })
}

fn by_category(expenses: Array<Expense>) -> Record<string, number> {
  let seed: Record<string, number> = {}
  return array.fold(expenses, seed, fn(acc, e) {
    let running = match record.get(acc, e.category) {
      None => 0,
      Some(n) => n,
    }
    return record.set(acc, e.category, running + e.cents)
  })
}
```

A grouping fold needs that annotated `seed` binding. A bare `{}` in argument
position carries no type, so the callback's `acc` comes back as `unknown` and
`tsc` rejects the body.

`array.flat_map` is the other half: one call replaces the
`mut all = array.concat(all, f(x))` shape.

```glyph
import std/array

type Document = { links: Array<string> }

fn links_of(doc: Document) -> Array<string> {
  return doc.links
}

fn all_links(docs: Array<Document>) -> Array<string> {
  return array.flat_map(docs, links_of)
}
```

## Align a column of text

```glyph
import std/array
import std/io
import std/math
import std/string

type Row = { name: string, count: string }

fn print_table(rows: Array<Row>) -> void {
  let width = array.fold(rows, 0, fn(w, r) { math.max(w, string.len(r.name)) })
  io.println(string.repeat("-", width + 8))
  for r in rows {
    io.println("${string.pad_end(r.name, width)}  ${string.pad_start(r.count, 5)}")
  }
}
```

`repeat` returns `""` for a negative count instead of throwing, so
`repeat(pad, width - string.len(s))` is safe even when the string is already too
long, and `pad_start`/`pad_end` leave a string that already reaches `width`
alone.

## Find a substring, or find out it is not there

`string.index_of` and `array.index_of` return `Option<number>`, not `-1`. The
sentinel is a number that type-checks everywhere a real index does, which is how
it turns into an off-by-one somewhere far away.

```glyph
import std/string

fn after_colon(line: string) -> string {
  return match string.index_of(line, ":") {
    None => line,
    Some(i) => string.trim_start(string.slice(line, i + 1)),
  }
}
```

Write the `None` arm. `string.index_of` is one of the six stdlib functions with
an optional trailing argument, which the checker cannot model until the arity
check learns a range, so a `match` that leaves the arm out builds clean, passes
`tsc --strict`, and throws `non-exhaustive match` at run time. `array.index_of`
is modeled and does report E0200.

## Use a generic bound

```glyph
interface HasId {
  id: int
}

fn ids<T: HasId>(rows: Array<T>) -> Array<int> {
  return array.map(rows, fn(r) { r.id })
}
```
