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

The emitter picks the array lowering from the iterand's type, and there is one
place it still cannot see one: a call into a stdlib function the checker does not
model. `for i, x in array.slice(xs, 1)` hands you the *string* `"0"` and nothing
fails, so bind it first: `let ys: Array<string> = array.slice(xs, 1)`, then loop
over `ys`. Tracked in `docs/dogfooding-gaps.md` as G37.

You do not need the annotation for a value that came out of a `match` or out of
`T.parse`. Those carry their type, so `for i, e in ledger.expenses` below binds a
number.

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

## Use a generic bound

```glyph
interface HasId {
  id: int
}

fn ids<T: HasId>(rows: Array<T>) -> Array<int> {
  return array.map(rows, fn(r) { r.id })
}
```
