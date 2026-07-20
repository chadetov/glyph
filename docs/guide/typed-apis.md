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

The rest of the untrusted boundary is typed the same way. `http.header(req,
name)` and `http.query_param(req, name)` return `Option<string>`, so a missing
header or query parameter is `None` and the `match` forces you to handle it — it
can't be read as if it were present:

```glyph
match header(req, "authorization") {
  Some(token) => check(token),
  None => reject(),   // omitting this arm is a compile error
}
```

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

Every generated file records the exact command that produced it in its header
(`Regenerate with \`glyph gen openapi petstore.yaml --out src/\``). When a spec
changes, you don't have to remember which commands to re-run:

```sh
glyph regen          # re-run every recorded gen command under the current dir
glyph regen src/api  # or scope it to a subtree / a single file
```

`glyph regen` scans for those headers, runs each unique command once (from the
project root, where the recorded relative paths resolve), and rewrites the
output. It is idempotent — a regen with no spec change leaves the files
untouched — so it fits naturally in a pre-commit hook or CI drift check.

Add `--client` to also generate a typed `std/http` client — one `async fn` per
operation, with typed path parameters and request bodies:

```sh
glyph gen openapi tasks.yaml --out src/ --client
# → async fn createTask(base: string, body: NewTask) -> Result<Response, HttpError>
# → async fn getTask(base: string, id: number)       -> Result<Response, HttpError>
```

Each function takes the server `base` URL, sends the right verb to the right
path, and returns the HTTP `Response`. The response body stays `unknown` by
design — validate it with the matching type's `.parse`, so the boundary is
checked on the way in just like a request body.

Building the server side instead? `--handlers` emits a typed stub per operation
and a `route` dispatcher that matches the method and path for you:

```sh
glyph gen openapi tasks.yaml --out src/ --handlers
```

The router matches path segments with array patterns, so `/tasks/{id}` binds
`id` and passes it to your handler:

```glyph
fn route(req: Request) -> Result<Response, string> {
  return match req.method {
    "GET" => match segments(req) {
      ["tasks"] => listTasks(req),
      ["tasks", id] => getTask(req, id),   // id captured from the path
      else => Ok(json(404, { error: "not found" })),
    },
    else => Ok(json(405, { error: "method not allowed" })),
  }
}
```

Fill in the stubs (each starts as a 501, with a comment showing the body-parse
for POST/PUT/PATCH) and wire it up with `await serve(PORT, route)`.

The generator maps the wire-faithful core (objects, `string`/`number`/`bool`,
`array`, `$ref`, optional and `nullable` fields, `additionalProperties` →
`Record<string, T>`). Where a construct has no faithful Glyph representation — a
`string` enum (Glyph has no string-literal union), or an undiscriminated
`oneOf` — it narrows to `string`/`unknown` and **prints a note** rather than
emit a validator that would reject real payloads. Read the notes; they tell you
exactly what was approximated.

### From a TypeScript `.d.ts`

The same command works against a TypeScript declaration file, so you can pull an
external package's or a `zod` schema's types into first-class, descriptor-bearing
Glyph:

```sh
glyph gen dts node_modules/some-pkg/types.d.ts --out src/
```

It maps the same wire-faithful core — `interface`/`type` declarations, objects,
primitives, arrays, references, optional (`field?:`) and `| null` members, and
string-literal unions (narrowed to `string` with a note). This needs `node` and
the `typescript` package, resolved from the target file's own project first (a
pinned version wins) then a global install. Both the classic compiler (5/6) and
the 7.x native port are supported.

If your source of truth is a `zod` schema rather than a plain type, use
`glyph gen zod` instead:

```sh
glyph gen zod schemas.ts --out src/    # needs tsx + zod
```

It executes the module, converts each exported schema to a Glyph type (via zod
4's `z.toJSONSchema`, or `zod-to-json-schema` on zod 3), and normalizes zod's
nullable/optional shapes into the same wire-faithful mapping. That closes the
`value → type` loop from the other side: your zod schema becomes a first-class
Glyph type with its own descriptor.

Materializing a `.d.ts` this way turns
an ambient, unvalidated phantom into a real Glyph type you own and can validate —
which is the whole point of the boundary below.

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
