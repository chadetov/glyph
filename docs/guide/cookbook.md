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

## Use a generic bound

```glyph
interface HasId {
  id: int
}

fn ids<T: HasId>(rows: Array<T>) -> Array<int> {
  return array.map(rows, fn(r) { r.id })
}
```
