# Typed APIs: the `type` is the validator

In most TypeScript stacks a payload has to be described *twice*: once as a
`type`/`interface` for the compiler, and once as a runtime validator (a hand
written `zod` schema, an `io-ts` codec, a pile of `if (typeof ...)` checks) so
the value is actually what the type claims at the boundary. The two drift, and
the drift is where bad data gets in.

Glyph collapses that into one declaration. When you write

```glyph
type NewTask = {
  title: string,
  done: bool,
}
```

the compiler generates a runtime **descriptor** for `NewTask` alongside the
type — a value with `is`, `parse`, and `schema` members. You get the static
type *and* the runtime validator from the same line, and they cannot drift
because one produces the other. This is the manifesto-native inverse of
`z.infer`: not *value → inferred type*, but *type → generated validator*, with a
real greppable `type` declaration as the single source of truth.

There is nothing to import and nothing to wire up. Declare the type; the
descriptor is there.

## The three descriptor members

For every `type X = { ... }` you can use, from Glyph source:

| Call | Takes | Returns | Use when |
|---|---|---|---|
| `X.parse(v)` | an already-decoded `unknown` value | `Result<X, Array<Issue>>` | you have a parsed value (an HTTP body, a config object) and need it validated into `X` |
| `X.is(v)` | an `unknown` value | `bool` (type guard) | you want a boolean check, or to narrow inside a `match` |
| `X.schema` | — | `Schema<X>` | you need to hand the schema to another API (e.g. `json.parse_with`) |

And the two entry points on `std/json`:

| Call | Takes | Returns |
|---|---|---|
| `json.parse<X>(text)` | a JSON **string** | `Result<X, Array<Issue>>` |
| `json.parse_with<X>(text, schema)` | a JSON string + an explicit `Schema<X>` | `Result<X, Array<Issue>>` |

## The one distinction that matters: string vs. already-parsed value

This trips people up once, so learn it once. There are two different situations
and they take two different calls:

- You have **JSON text** (you read a file, you got a raw string): use
  `json.parse<X>(text)`. It parses the string *and* validates in one step.
- You have an **already-decoded value** of static type `unknown` (an HTTP
  request body, a value handed to you by another library): use `X.parse(value)`.
  The bytes are already JSON-decoded; there is no string left to parse — you
  only need to validate the shape.

```glyph
// From JSON text (a string you read, received, or have as a literal):
let text = "{\"title\": \"buy milk\", \"done\": false}"
match json.parse<NewTask>(text) {
  Ok(task) => print(task.title),
  Err(issues) => print("bad json"),
}

// From an already-parsed value (std/http decodes the body for you):
match NewTask.parse(req.body) {       // req.body is `unknown`, already decoded
  Ok(task) => print(task.title),
  Err(issues) => print("bad body"),
}
```

Reaching for `json.parse<Task>(req.body)` is the common mistake: `req.body` is
not a string, so it will not type-check. `Task.parse(req.body)` is the call.

## A request body, validated into a DTO

Put together, a validated `std/http` handler is short. The body arrives as
`unknown`; one `match` on `NewTask.parse` either gives you a fully typed
`NewTask` or a list of `Issue`s to turn into a 400.

```glyph
import std/http { Request, Response, serve, json, path }
import std/result { Result, Ok, Err }

type NewTask = {
  title: string,
  done: bool,
}

fn create(req: Request) -> Result<Response, string> {
  return match NewTask.parse(req.body) {
    // `input` is a fully typed NewTask here — validated, not asserted.
    Ok(input) => Ok(json(201, { title: input.title, done: input.done })),
    Err(issues) => Ok(json(400, { error: "invalid task" })),
  }
}
```

No `zod` schema, no `as NewTask` cast, no separate validator to keep in sync. If
a request sends `{"title": 123}` the `Err` arm runs; if a field is missing the
`Err` arm runs; only a well-formed body reaches `Ok`. The full worked server —
auth, in-memory store, every method — is
[`examples/05_rest_api.glyph`](../../examples/05_rest_api.glyph); run it with
`glyph run examples/05_rest_api.glyph`.

## Generating DTOs from a spec

You do not have to hand-write the types either. If you have an OpenAPI 3,
Swagger 2, or JSON Schema document, `glyph gen openapi` turns it into committed
Glyph types:

```sh
glyph gen openapi petstore.yaml --out src/
# → src/petstore.glyph: `module petstore` with a real `type` per schema
```

The output is ordinary, greppable, `glyph fmt`-clean Glyph — one `type` per
schema, each with its descriptor — not an inferred phantom. Regeneration is
idempotent, so re-running after a spec change produces a minimal diff you can
review. Because every generated type is a real record, a response body from that
API validates through `PetstoreType.parse(...)` exactly like a hand-written one.

The generator maps the wire-faithful core (objects, `string`/`number`/`bool`,
`array`, `$ref`, optional and `nullable` fields, `additionalProperties` →
`Record<string, T>`). Where a construct has no faithful Glyph representation — a
`string` enum (Glyph has no string-literal union), or an undiscriminated
`oneOf` — it narrows to `string`/`unknown` and **prints a note** rather than
emit a validator that would reject real payloads. Read the notes; they tell you
exactly what was approximated.

## What this does and does not cover

- **Your own DTOs**: fully covered. Any `type` you declare in Glyph carries a
  descriptor, so every boundary you own validates for free.
- **Someone else's types** (an npm package's `.d.ts`, an external `zod` schema):
  those are *type-only* — they type-check but carry no Glyph descriptor, because
  Glyph did not generate them. That is the boundary the manifesto draws
  deliberately: a type Glyph did not emit cannot promise runtime validation.
  Bring such a shape in as a real Glyph `type` (hand-written, or generated) and
  it gets a descriptor like any other. See
  [`external-imports.md`](external-imports.md) for the `.types/` path.

## See also

- [`../reference/stdlib.md`](../reference/stdlib.md) — exact signatures for
  `std/json` and `std/http`.
- [`../language/spec.md`](../language/spec.md) — D8, the runtime-descriptor
  decision, in full.
