<!-- Markdown copy of https://glyphlang.io/answers/typed-apis/, generated from the same source as the page. Last changed 2026-11-21. -->

11 · Data boundaries

# Do I have to hand-write a validator for every API?

No. In Glyph a **type is its own validator**. Declare a `type` and the compiler generates a runtime descriptor beside it, so an untrusted request or response body validates with one call, no separate `zod` schema to write and keep in sync.

Untrusted body req.body unknown, e.g. {"title": 123}

Descriptor NewTask.parse generated from `type NewTask`

Ok NewTask a typed value, every field checked

Err Array<Issue> the bad body stops here

One declaration, one check. The descriptor is generated from the type, so the two can't disagree, and the only way to a `NewTask` is through it.

## Why it matters

In most TypeScript stacks every payload is described *twice*: once as a `type` for the compiler, and again as a hand-written validator so the value is actually that shape at runtime.

The two drift, and the drift is exactly where bad data slips in. Glyph collapses them into one declaration: the type *produces* the validator, so they cannot disagree.

## See it

Declare the DTO once. Its descriptor gives you `.parse` (validate an untrusted value into the type) and `.is` (a type guard), with nothing else to write:

api.glyph

```
type NewTask = {
  title: string,
  done: bool,
}

fn create(req: Request)
  -> Result<Response, string> {
  return match NewTask.parse(req.body) {
    Ok(task) => Ok(json(201, task)),
    Err(issues) => Ok(json(400, {
      error: "invalid task",
    })),
  }
}
```

what you did *not* write

```
// No zod schema:
const NewTask = z.object({
  title: z.string(),
  done: z.boolean(),
});
// No drift between the type and
// the validator, because there is
// only one declaration.

// A bad body, {"title": 123},
// takes the Err arm. Only a
// well-formed body reaches Ok.
```

### Keep the typed value after the `match`

You can also pull the value out instead of doing the work inside the arm.

api.glyph: the value leaves the match typed

```
let task = match NewTask.parse(req.body) {
  Ok(t) => t,
  Err(issues) => { return bad_request(issues) },
}
task.titel   // E0210: type `NewTask` has no field `titel`; did you mean `title`?
```

since 0.1.45 generic types: not yet

**Details how the type is carried, and the generic exception **

`T.parse` is typed as `Result<T, Array<Issue>>` and a `match` whose arms agree carries their type, so `let task = match NewTask.parse(req.body) { Ok(t) => t, Err(issues) => { return bad_request(issues) }, }` gives you a `NewTask` for the rest of the function, with field names checked by Glyph rather than by `tsc` on the generated TypeScript. That landed in 0.1.45; before it, the value went untyped once it left the `match`. A generic type's `parse` is the one that still doesn't, because its descriptor takes a runtime checker per type parameter.

## The parse result is an ordinary `Result`

In Glyph `T.parse` returns the same `Result` every fallible function in the language returns, so you hand it straight back, run it through `map_err` to turn validation issues into your own error type, or propagate it with `?`.

boundary.glyph

```
// Hand it straight back.
fn read_task(body: unknown)
  -> Result<NewTask, Array<Issue>> {
  return NewTask.parse(body)
}

// Or restate the failure in your own terms.
fn read_checked(body: unknown)
  -> Result<NewTask, string> {
  return NewTask.parse(body).map_err(
    fn(issues: Array<Issue>) -> string {
      return "invalid task: ${issue_text(issues)}"
    },
  )
}

// Or propagate it and get on with the work.
fn create(body: unknown) -> Result<Response, string> {
  let task = read_checked(body)?
  return Ok(json(201, save(task)))
}
```

what this replaced

```
// Until 0.1.52 the generated descriptor
// returned a bare { tag, value } object,
// so both of the first two shapes were
// tsc errors (TS2322 and TS2339) even
// though Glyph's own checker reported
// Result<T, Array<Issue>>. The workaround
// was an identity re-wrap:

match NewTask.parse(body) {
  Ok(t)  => Ok(t),          // does nothing
  Err(e) => Err(reword(e)),
}

// Four lines to say map_err. bracket.glyph
// carried this twice until the fix landed.
```

**Details the runtime cost of using Result here **

A validator that gives you back a value in its own private shape is only half useful, because the rest of your error handling can't touch it. Nothing about a parse result is special.

The reason it was ever otherwise: a descriptor is generated next to your `type`, and emitting the bare object kept a module that declares a type from depending on `std/result` at runtime. That trade is now made the other way. A module with a `pub type` in it carries an import of `std/result` whether or not it ever writes `Result`, and a `parse` call allocates the combinator closures any `Ok(...)` allocates. What you get for it is that the validated boundary and the rest of your error handling are the same API.

## Composing your own: the output type is derived, not trusted

Glyph's `infer_output<Shape>` **derives** the output type from the shape, and `tsc` checks your annotation against it. A shape that omits a field of the type you claim doesn't compile:

schema.glyph

```
fn object_schema<Shape: Record<string, Schema<unknown>>>(
  shape: Shape,
) -> Schema<infer_output<Shape>> { ... }

type User = { name: string, age: number }

// The shape must produce a User, or this
// line does not compile:
const user_schema: Schema<User> =
  object_schema({
    name: string_schema(),
    age: number_schema(),
  })
```

the drift Glyph removes

```
// TS/zod: shape and output type are two
// separate things you keep in sync by hand.
const s = z.object({ name: z.string() });
type User = { name: string; age: number };
// `s` is inferred { name }, but nothing
// checks it against User. The `age` you
// forgot is caught at runtime, if ever.

// Glyph: drop `age` from the shape and
// `Schema<User>` fails to compile. The
// shape is the single source of truth.
```

**Details the TypeScript trap, and the cast that follows the return since 0.1.92 **

Sometimes you want a `zod`-style combinator: build a validator by composing smaller ones. The trap in TypeScript is that the combinator's *output type* and the *shape* you passed it drift apart. You write the shape, then repeat yourself in a second type parameter, and nothing checks that they agree.

The derived type holds however the combinator returns. `infer_output` needs one cast at the function's boundary, because the body builds a value the type system can't prove carries the shape-derived type, and the compiler writes that cast for you. Through 0.1.91 it was attached to a plain `return <value>`. Put a `match` between the `return` and the value, or end the body in `r?`, and the same function stopped compiling: a `TS2322` against generated TypeScript, over Glyph the compiler had already accepted. As of 0.1.92 the cast follows the return rather than its spelling, so `return match strict { ... }` type-checks the same as the single `return` spelling of the identical body.

## The rest of the untrusted boundary

The descriptor is the same one that makes `json.parse<T>` safe: give it a JSON string and it parses *and* validates in one step.

| Input | Call | Returns |

|---|---|---|

| A JSON string | `json.parse<T>(text)` | `Result<T, Array<Issue>>` |
| A decoded body | `T.parse(value)` | `Result<T, Array<Issue>>` |
| A header | `header(req, name)` | `Option<string>` |
| A query parameter | `query_param(req, name)` | `Option<string>` |

**Details request bodies, headers and query parameters **

A request body arrives already decoded as an `unknown`, so there you reach for `T.parse(value)` instead: same guarantee, no cast.

The rest of the untrusted boundary is typed the same way: `header(req, name)` and `query_param(req, name)` return `Option<string>`, so a missing header or query parameter is `None` and the `match` *forces* you to handle it. Omit the `None` arm and it won't compile. Absent input can't be read as if it were present. A body field follows the same rule: `timeout_ms?: number` is an absent key or a number, so `{"timeout_ms": null}` is an `Err` at `["timeout_ms"]`, from `T.parse`, `is T` and `json.parse<T>` alike. Declare `timeout_ms?: Nullable<number>` when the sender writes `null`. Until 0.1.152 that `null` came back inside the `Ok`.

## Does a map keyed by an enum check its keys?

Yes, since 0.1.148. A `Record<Tier, int>` with `type Tier = "free" | "pro"` parses only an object whose keys are exactly `free` and `pro`: a stray key and a missing one are both an `Err`, which is also what `tsc` says about the same object written as a literal.

| Key type | What `parse` checks on each key |

|---|---|

| A string-literal union | exactly the listed keys, none missing, none extra |
| A `where`-refined `string` or number | the predicate |
| `int` | the plain spelling of a whole number; `"01"` and `"1.5"` fail |
| `string` | nothing new |

**Details refined and int keys, and what the checker holds **

A `where`-refined key type runs its predicate on every key, and an `int` key has to be the plain spelling of a whole number, so `"01"` and `"1.5"` are refused. A `Record<string, V>` is checked exactly as before. A `where` cannot refine a string-literal union (`type T2 = Tier where value != "pro"` is `E0300` since 0.1.152); write the members you mean as their own union, `type T2 = "free"`, and it keys a `Record` with the same exact-keys check. Since 0.1.153 Glyph's own checker holds the key of a value you write in code as well: a `Record<string, int>` does not stand where a `Record<Tier, int>` is declared, or the other way round, so the crossing is a `json.parse` or an `is Record<Tier, int>` arm, and `record.set` keeps the key it was given. A generic record keyed by its own type parameter is refused unless that parameter is `string`.

## If I parse a date, do I get a real date?

Yes, and the interesting part is what you get back for input that isn't one. `time.parse_iso` returns `Option<number>`, and it is `None` for everything that is not ISO-8601.

accepted

```
"2026-01-03"              // UTC midnight
"2026-07-25T18:33:08.000Z"
"2026-03-15T09:30:00-05:00"
"2028-02-29"              // a real leap day
```

None, every one of them

```
"January 5 2026"   // not ISO
"2026-1-3"         // not padded
"2026-13-01"       // no month 13
"2026-02-31"       // no such day
"2026-02-29"       // not a leap year
"2026-01-03T10:00" // no offset
```

The last one is the one that surprises people, and it is the whole point. A datetime with no `Z` and no `+HH:MM` offset is local time by the ECMAScript grammar, so accepting it would mean the same string names a different calendar day depending on which machine ran the program.

**Details what Date.parse does instead, and why UTC **

That is a stronger claim than JavaScript's `Date.parse`, which takes free-form text (`"January 5 2026"`), reads an unpadded date like `"2026-1-3"` in whatever timezone the process happens to run in, and reports an impossible day as a success by rolling it forward: `"2026-02-31"` becomes March 3, and you get a number back with no indication anything happened.

Glyph's calendar accessors (`time.year`, `time.month`, `time.day`) are UTC and say so, and a validator that quietly imports the host's timezone would make them lie. So the string is rejected instead, and you decide what it meant.

### The guard moved into the primitive

When an application writes a correctness guard around a standard-library primitive, the guard belongs in the primitive. Here is what the app deleted:

before: the app guards the stdlib

```
const ISO_DATE = "^\\d{4}-\\d{2}-\\d{2}$"

fn parse_date(line: int, value: string)
  -> Result<number, RowError> {
  return match regex.matches(ISO_DATE, value) {
    false => Err(BadDate({ line: line, value: value })),
    true => match time.parse_iso(value) {
      Some(ms) => match string.starts_with(
        time.format_iso(ms), value
      ) {
        true => Ok(ms),
        false => Err(BadDate({ line: line, value: value })),
      },
      None => Err(BadDate({ line: line, value: value })),
    },
  }
}
```

after: the primitive holds the line

```
fn parse_date(line: int, value: string)
  -> Result<number, RowError> {
  return match time.parse_iso(value) {
    Some(ms) => Ok(ms),
    None => Err(BadDate({ line: line, value: value })),
  }
}
```

The `import std/regex` went with it. Same ledger, same rejected rows, same report byte for byte.

**History the expense-report app this came from **

This one came out of writing an [expense-report CLI in Glyph](https://github.com/chadetov/glyph/blob/main/examples/apps/expenses/main.glyph). Every ledger row carries a date, and filing a row under the wrong month is the bug that matters, so the app didn't trust the old `parse_iso`: it checked the shape with a regex first, then round-tripped the timestamp back to text to catch a rolled-over day.

## You don't even have to write the types

If the shape already exists in an OpenAPI spec or a TypeScript declaration file, generate the Glyph types from it: real, committed, greppable `type` declarations, each with its descriptor, not an inferred phantom:

from an OpenAPI / JSON Schema spec

```
glyph gen openapi petstore.yaml \
  --out src/
```

from an installed package, by name

```
glyph gen dts api-types \
  --out src/types
```

A wire type becomes a validated boundary, with nothing hand-written:

webhook.glyph

```
import types/api_types { Customer }

match Customer.parse(webhook_body) {
  Ok(c) => handle(c),
  Err(issues) => reject(issues),
}
```

Generation is **wire-faithful**: what the spec says about a value is what `parse` checks.

| In the spec or `.d.ts` | Generated Glyph |

|---|---|

| A string enum | a string-literal union; `parse` checks membership |
| An `integer` field | `int`; a wire `3.5` fails |
| An object schema | a record marked `@open`; strict with `additionalProperties: false` |
| `interface Page<T>` | `type Page<T>`, each item checked as `T` |
| An undiscriminated union | narrowed, with a printed note |
| A name in two reachable files | flagged with a note, not bound |

**Details what materializes today, regeneration, generic types **

Point `glyph gen dts` at a `.d.ts` file, or, as above, at an installed npm package by name: it resolves the package's own declaration entry from `node_modules` and writes committed types you import.

What materializes today: `gen dts` reads the `interface` and `type` declarations a package exports, including those inside a `declare namespace` tree (bare cross-references resolve through the scope) and those in sibling files an `index` barrel re-exports (the entry `.d.ts` plus every relatively-imported `.d.ts` is walked). A generic is kept first-class: `interface Page<T>` materializes as `type Page<T>` and gets a descriptor that validates each item as its type argument, not just for presence. Aliased and `export * as` re-exports resolve through a per-file binding map. The one case it can't make safe, a type name that collides across two reachable files, it *flags with a note* rather than bind the wrong shape silently. For anything the materializer can't reach, hand-write the shapes you cross the boundary with, or use the `extern_ts` escape hatch.

Regeneration is idempotent, so a spec change is a clean, reviewable diff. Generation is **wire-faithful**: a string enum materializes as a string-literal union (`"free" | "pro"`), so `parse` checks membership, not just that the value is a string, and a `match` over it is exhaustive without an `else`. An `integer` field becomes `int`, so a wire `3.5` fails `parse` where a plain number would pass. Where a construct has no exact Glyph form (an undiscriminated union), it narrows and *prints a note* rather than emit a validator that would reject real data. Generated records also carry `@open`, so `parse` tolerates a field the API adds later while still validating every field it declares; a schema that sets `additionalProperties: false` stays strict. Records are strict by default in Glyph, and this is the same greppable declaration-site marker a hand-written record would use, so nothing about what a record means changes.

Generic types validate too. A `Paginated<T>` gets a descriptor whose `parse` takes the type argument: `Paginated.parse<User>(body)` checks every `items` entry *as a `User`*, not just for presence, and `match v { is Paginated<User> => ... }` narrows the same way. This holds across module boundaries: define `Paginated<T>` in one file, `import` it into another, and both `Paginated.parse<User>(body)` and `is Paginated<User>` there still check each element as a `User`. The compiler builds the per-element check from the type argument at the call site, so which file the generic type lives in makes no difference to what gets validated.

Where it stops: descriptors cover the shapes most payloads are made of: objects, primitives, arrays, references, optional and nullable fields, and now generic types at a given instantiation. A type Glyph did not generate (an external package or `.d.ts` you only import) is type-checked but carries no descriptor until you materialize it, which is now one command against the installed package (`glyph gen dts <package>`, for the top-level interfaces and type aliases it exports). That boundary is deliberate: a type Glyph didn't emit can't promise runtime validation, so you opt in per package and the result is committed and greppable, not conjured on every build.

## The database is a boundary too

A row read out of a database is untrusted in exactly the way a request body is. The same `.parse` you use on a request body validates a row.

tasks.glyph

```
type Task    = { id: int, title: string, done: bool }
type TaskRow = { id: int, title: string, done: int }

fn row_to_task(r: TaskRow) -> Task {
  return { id: r.id, title: r.title,
           done: r.done != 0 }
}

// A row is validated, then mapped. No cast.
match TaskRow.parse(row) {
  Ok(r)  => Ok(row_to_task(r)),
  Err(_) => Err("row failed validation"),
}
```

the bug this removes

```
// TS, better-sqlite3 / node:sqlite:
const row = stmt.get(id) as Task;
// `row.done` is really 0 or 1, a number.
// Nothing complains. Then:
if (row.done === true) { ... }   // never runs

// The row was `unknown`; the cast invented
// a type. Glyph makes you validate the row
// into TaskRow, and the int-to-bool step is
// a line of code, not a silent lie.
```

**Details std/sqlite, and the 0-or-1 boolean **

A row read out of a database is untrusted in exactly the way a request body is: the database schema and your code's idea of the row can disagree, and a cast (`row as Task`) papers over it. `std/sqlite`, the DB module over Node's built-in SQLite, leans into that. A query returns `Array<Record<string, unknown>>`, so there is no way to treat a row as a `Task` without validating it first.

This catches a bug people ship constantly. SQLite has no boolean type, so a `done` column comes back as the integer `0` or `1`. Cast the row to a record with a `bool` field and `task.done === true` is quietly never true. In Glyph the storage shape and the domain shape are separate types, and the mapping between them is one line you actually see:

The whole thing is a working example: [`examples/apps/tasks/main.glyph`](https://github.com/chadetov/glyph/blob/main/examples/apps/tasks/main.glyph) is a persisted task API, `std/sqlite` for storage and `std/http` for routes, with data that survives a restart and validation on both the wire and the database side. No hand-written validators anywhere in it.

## A signed webhook is a boundary too

Verifying a webhook signature needs the exact bytes the sender signed, not a re-serialized copy (re-stringifying a parsed body changes whitespace and key order, so the HMAC would never match). `http.raw(req)` hands you those bytes, so the check stays in Glyph:

```
fn handle(req: Request) -> Result<Response, string> {
  let expected = crypto.hmac_sha256(secret, raw(req))
  return match header(req, "x-hook-signature") {
    Some(sig) => match sig == expected {
      true  => accept(req),          // then validate req.body with T.parse
      false => Ok(text(401, "bad signature")),
    },
    None => Ok(text(401, "missing signature")),
  }
}
```

**Details raw bytes for the signature, parsed body for the shape **

`req.body` is still the parsed value for the shape check; `raw` is the untampered payload the signature is computed over. A receiver that verifies a signature and validates the body needs no hand-written TypeScript.

## A type can validate its invariants, not just its shape

A shape check tells you a field is a number. It does not tell you the amount is non-negative or the rating is between 1 and 5. A `where` refinement puts that invariant in the type, and the descriptor enforces it at the boundary:

refinements.glyph

```
type Amount   = int where value >= 0
type Rating   = int where value >= 1 && value <= 5
type NonEmpty = string where value.length > 0

// Amount.parse(-1)  -> Err   (not just "is a number")
// Rating.parse(6)   -> Err
// Amount.parse(3.5) -> Err   (int check too)

type Line = { amount: Amount, }
// Line.parse({ amount: -1 }) -> Err too:
// the predicate follows the type into
// the field, and across modules.
```

what you're not writing

```
// The check that usually lives three
// functions away from the boundary,
// if someone remembers it:
if (amount < 0) throw new Error(...);

// In Glyph it's the type. A value that
// fails the predicate never becomes an
// Amount in the first place.
```

## Where it stands

### Shipping today 10

Descriptors

- Every declared record and tagged union carries a runtime descriptor (`.parse`, `.is`, `.schema`), generic records included a generic tagged union does not
- `.parse` returns the same `Result` the rest of the language uses, so it composes with `map_err`, with `?`, and with any function that returns one
- A `where` refinement (`int where value >= 0`) validates an invariant at the boundary, not just a shape
- `json.parse<T>` validates JSON
- `infer_output<Shape>` derives a combinator's output type from its shape so the two can't drift

Generation

- `glyph gen openapi` / `glyph gen dts` generate committed, descriptor-bearing types from a spec, a `.d.ts`, or an installed package by name

Boundaries

- `std/sqlite` hands back rows as `unknown` so the database is a validated boundary like any other
- A signed-webhook receiver stays entirely in Glyph

  **Details**

  `http.raw(req)` exposes the unparsed request body, so a webhook signature is verified over the exact bytes and a signed-webhook receiver stays entirely in Glyph.

- Untrusted input can't reach a SQL or shell sink without `sanitize`

  **Details**

  `std/taint`'s `Tainted`/`Trusted` types keep untrusted input from reaching a SQL or shell sink without an explicit `sanitize`, so `tsc` rejects an injection path at compile time.

- `time.parse_iso` takes ISO-8601 and nothing else

  **Details**

  The smaller boundary primitives hold the same line: `time.parse_iso` takes ISO-8601 and returns `None` for free-form text, an impossible day, and an offset-less datetime that would otherwise be read in the host's local timezone.

### On the way 3

- Discriminated unions generate from a spec and from a `.d.ts`; enum and mapped-type coverage next

  **Details**

  Discriminated unions now generate from both an OpenAPI `discriminator` and a TypeScript `.d.ts` union (a `parse_<Name>` dispatcher reads the tag and validates into the right variant). Next: richer enum and mapped-type coverage.

- Typed client and server code from a spec already ships
- [See client & server](https://glyphlang.io/answers/codegen/)
- A timestamp and a duration are both `number`

  **Details**

  On dates, the strictness is the whole feature and it stops at the parse: `parse_iso` hands back epoch milliseconds, so a timestamp and a duration are both `number` and nothing stops you adding one to the other. A distinct date type is a later decision, not a small one.

Code examples on this page are checked against Glyph 0.1.163 on every build. Last changed 21 Nov 2026.
