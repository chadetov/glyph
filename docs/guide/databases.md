# Databases

Glyph talks to real databases the same way it talks to any external system: you
import the client, construct it, and validate what comes back. There is no ORM
and no Glyph-specific driver. A relational row and a Mongo document are both
untrusted values at the boundary, so you turn them into a typed record with
`.parse` before the rest of your code trusts them, exactly as you would a request
body.

Two facts drive everything on this page:

- **A query result is `unknown`-shaped.** The database schema and your code's idea
  of a row can disagree, and a cast hides that. Validate each row into a type.
- **Most clients are class-based**, so you construct them with `new` (see
  [external imports](external-imports.md#class-based-clients-new)). Factory-style
  clients (`createClient()`, `createConnection()`) are called directly, no `new`.

## SQLite, built in

SQLite needs no npm install: `std/sqlite` wraps Node's built-in synchronous
SQLite (`node:sqlite`, Node 26+). Queries return rows as
`Record<string, unknown>`, so the boundary is enforced.

```glyph
import std/sqlite { open }

let db = open("app.db")
db.exec("CREATE TABLE IF NOT EXISTS tasks (id INTEGER PRIMARY KEY, title TEXT, done INTEGER)")
db.run("INSERT INTO tasks (title, done) VALUES (?, 0)", ["write report"])
let rows = db.query("SELECT id, title, done FROM tasks", [])
```

A complete, persisted task API on `std/sqlite` is
[`examples/apps/tasks/main.glyph`](https://github.com/chadetov/glyph/blob/main/examples/apps/tasks/main.glyph).
It shows the one modelling point SQLite forces: it has no boolean type, so a
`done` column comes back as the integer `0`/`1`. Keep the storage shape and the
domain shape as separate types and map between them, rather than casting an
integer to a `bool`.

### The query text is a constant

The first argument of `exec`, `run`, `query` and `query_one` is a
`sqlite.Sql`, and its text has to be fixed when the program is written (D52).
A value goes in the parameter array, where SQLite binds it as data, never into
the text:

```glyph
import std/sqlite

fn find(db: sqlite.Db, name: string) -> number {
  let rows = db.query("SELECT * FROM users WHERE name = ?", [name])
  return rows.length
}
```

Building the text from `name` instead, with `${name}` or `+`, is E0239, and
so is hoisting it into a `let` first, because the checker follows the name to
what it was built from. The error's help writes the placeholder version for
you:

```
[E0239] `db.query` runs its first argument as SQL, and this one is built from
`name` at run time, so a value with a quote in it can change the query
Help: Pass the value as a parameter: `db.query("SELECT * FROM users WHERE name = ?", [name])`.
```

Constants are fine however they are put together: a literal, a module
`const`, a `let` no `mut` reassigns, a `match` whose arms are all constant,
`"SELECT * FROM ${TABLE}"` with `TABLE` a `const`, or a `+` of those. A function that takes a query from its caller
declares the parameter `sql: sqlite.Sql`, and its callers' arguments are then
checked the same way. `exec` takes no parameters, so a statement that needs a
value goes through `run(sql, [values])`.

A parameter stands for a value, never for a name, so `ORDER BY ?` sorts by a
constant and `FROM ?` is a syntax error. To pick a column or a table at run
time, choose it with a `match` whose arms are constant strings. The checker
reads that `match` as one of a fixed set of texts, all written in the
program, so the query stays constant:

```glyph
import std/sqlite

fn sorted(db: sqlite.Db, key: string) -> number {
  let col = match key {
    "name" => "name",
    else => "id",
  }
  return db.query("SELECT * FROM users ORDER BY ${col}", []).length
}
```

A `match` with an arm built from a run-time value is E0239 like any other.
When the text has to be built at run time and no `match` can list it, say an
`IN (?, ?, ?)` list sized at run time, build the text, mark it with
`taint.trust_unchecked` (or `taint.sanitize` with a cleaning function), and
pass it through `sqlite.from_trusted`:

```glyph
import std/sqlite
import std/taint
import std/array
import std/string

fn by_ids(db: sqlite.Db, ids: Array<int>) -> number {
  let marks = string.join(array.map(ids, fn(i: int) -> string { return "?" }), ", ")
  let text = "SELECT * FROM tasks WHERE id IN (" + marks + ")"
  let sql = sqlite.from_trusted(taint.trust_unchecked(text))
  return db.query(sql, ids).length
}
```

`from_trusted` takes a `taint.Trusted<string>`, so a plain `string` there is
refused, and both names are greppable: every query that is not a constant is
one search away. This holds for `std/sqlite` only. The npm drivers below take
a `string`, and the compiler does not check what goes into it.

## PostgreSQL (`pg`)

`pg` is class-based (`new Pool`) and ships its types in `@types/pg`, so install
both. A query result carries `.rows` typed loosely; validate each row.

```sh
npm install pg
npm install --save-dev @types/pg
```

```glyph needs-npm
module main

import pg { Pool }
import std/io { println }

type Task = {
  id: int,
  title: string,
  done: bool,
}

type TaskRow = {
  id: int,
  title: string,
  done: bool,
}

async fn list_tasks(pool: Pool) -> Array<Task> {
  let res = await pool.query("SELECT id, title, done FROM tasks ORDER BY id", [])
  let out: Array<Task> = []
  for row in res.rows {
    match TaskRow.parse(row) {
      Ok(r) => mut out.push({ id: r.id, title: r.title, done: r.done, }),
      Err(issues) => {},
    }
  }
  return out
}

async fn main() -> void {
  let pool = new Pool({ connectionString: "postgres://localhost/app", })
  let tasks = await list_tasks(pool)
  println("loaded ${number.to_string(tasks.length)} tasks")
}
```

Postgres does have a boolean type, so a `boolean` column parses into a Glyph
`bool` directly. The `TaskRow` here matches the domain `Task`; keep them separate
only where storage and domain genuinely differ (SQLite booleans, a stored enum,
a nullable column you want non-null downstream).

## MongoDB (`mongodb`)

`mongodb` is class-based (`new MongoClient`) and ships its own types, so no
`@types` package is needed. A document read back is untrusted; validate it.

```sh
npm install mongodb
```

```glyph needs-npm
module main

import mongodb { MongoClient }
import std/io { println }

type TaskDoc = {
  title: string,
  done: bool,
}

async fn main() -> void {
  let client = new MongoClient("mongodb://localhost:27017")
  await client.connect()
  let coll = client.db("app").collection("tasks")

  // `find` is synchronous (it returns a cursor) and `toArray` is the async
  // terminal. `await` on a fluent chain applies to the whole chain, so this
  // awaits `toArray`, as you'd expect.
  let docs = await coll.find({}).toArray()

  let valid = 0
  for doc in docs {
    match TaskDoc.parse(doc) {
      Ok(t) => mut valid = valid + 1,
      Err(issues) => {},
    }
  }
  println("valid task docs: ${number.to_string(valid)}")
  await client.close()
}
```

A note on how `await` binds: on a fluent chain of *value methods*
(`coll.find({}).toArray()`), it awaits the whole chain, which is what a builder
API wants. On the Result idiom, where an async function heads the chain and
synchronous combinators follow (`await load(p).map_err(f)`), it awaits the head
call so `map_err` runs on the awaited `Result`. Both are handled; you write the
natural thing.

## Redis and MySQL: factory clients

Not every client is class-based. `node-redis` and `mysql2` hand you a factory
function, so you call it directly and skip `new` entirely.

```glyph
import redis { createClient }

async fn main() -> void {
  let client = createClient()
  await client.connect()
  await client.set("k", "v")
}
```

```glyph
import mysql2/promise { createConnection }

async fn main() -> void {
  let conn = await createConnection({ host: "localhost", database: "app", })
  let rows = await conn.query("SELECT id, title FROM tasks")
  // `rows` is loosely typed; RowType.parse each entry before you trust it.
  let _ = rows
}
```

## The rule that generalizes

Whatever the database, the shape of correct Glyph is the same:

1. Construct the client (`new` for a class, a call for a factory).
2. Run the query.
3. Treat every row or document as `unknown` and `.parse` it into a typed record.

Step 3 is the one people skip in TypeScript with a cast, and it is where the
integer-that-was-supposed-to-be-a-boolean and the null-in-a-non-null-column bugs
enter. In Glyph the parse is the only way across the boundary, so those bugs are
caught at the edge instead of three functions later.
