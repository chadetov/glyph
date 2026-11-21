<!-- Markdown copy of https://glyphlang.io/answers/codegen/, generated from the same source as the page. Last changed 2026-11-21. -->

12 · From your spec

# Can I generate a typed client and server from my API spec?

Yes. Point `glyph gen openapi` at an OpenAPI spec: `--client` emits a typed HTTP client, one function per operation; `--handlers` emits server stubs and a working `route` dispatcher. If your source of truth is a `zod` schema, `glyph gen zod` does the same. Everything comes out as **committed, greppable Glyph** you own and edit, never a hidden codegen step.

$ glyph gen openapi tasks.yaml --out src/ --client

```
async fn createTask(
  base: string,
  body: NewTask,
) -> Result<Response, HttpError> {
  return await post(
    "${base}/tasks",
    body,
  )
}

async fn getTask(base: string, id: number)
  -> Result<Response, HttpError> {
  return await get("${base}/tasks/${id}")
}
```

$ glyph gen openapi tasks.yaml --out src/ --handlers

```
fn route(req: Request)
  -> Result<Response, string> {
  return match req.method {
    "GET" => match segments(req) {
      ["tasks"] => listTasks(req),
      // id captured from the path:
      ["tasks", id] => getTask(req, id),
      else => Ok(json(404, {
        error: "not found",
      })),
    },
    else => Ok(json(405, {
      error: "method not allowed",
    })),
  }
}
```

See it: the client. Each operation becomes an `async fn` with typed path parameters and a typed request body, returning the HTTP `Response`.

## Why it matters

Hand-written API clients and their DTOs drift from the spec, and the drift is a class of bug nobody enjoys.

Because Glyph generates *real* code (typed functions, real `type` declarations with runtime descriptors), the spec stays the source of truth, and regeneration is an idempotent, reviewable diff, not a black box.

## The `--handlers` router

The `--handlers` router matches the path with array patterns, so `/tasks/{id}` becomes `["tasks", id]` and binds the parameter for you.

1. /tasks/{id}
2. ["tasks", id]
3. getTask(req, id)

Each stub starts as a `501` with a comment showing the body-parse; you fill them in and wire it up with `await serve(PORT, route)`.

## From a zod schema instead

If your team already defines shapes with `zod`, generate from those directly. The schema becomes a first-class Glyph type with its own descriptor:

schemas.ts

```
export const Task = z.object({
  id: z.number(),
  title: z.string(),
  done: z.boolean(),
})
```

$ glyph gen zod schemas.ts --out src/

```
type Task = {
  id: number,
  title: string,
  done: bool,
}
// + a runtime descriptor, so
// Task.parse(value) validates.
```

## What it maps

A polymorphic **discriminated union** (an OpenAPI `discriminator`) comes out as a real Glyph tagged union plus a generated `parse_<Name>` that reads the discriminator property and validates into the right variant.

**Details why a dispatcher is generated **

Because Glyph tags by constructor name, not an arbitrary property, the generated dispatcher bridges the wire object to the union for you.

### Where it falls short

Generation is wire-faithful, so it maps the common 80%: objects, primitives, arrays, references, optional and nullable fields (kept apart: a nullable field becomes `Nullable<T>`, and a field the schema does not require gets `?`, which takes no `null`), and discriminated unions. It narrows what it can't represent exactly (a string enum, an *un*discriminated union) with a printed note rather than a validator that would reject real data. Richer enums are next.

**Details what the generators need installed **

And the tooling has real dependencies: `gen dts`/`gen zod` need Node plus `typescript`/`zod`. `gen dts` works with any TypeScript (5, 6, or the 7.x native port), resolved from your project first, and they tell you exactly what to install if it's missing.

## Keeping it in sync

Every generated file records the exact command that made it in its header. When a spec changes, you don't hunt for which commands to re-run: `glyph regen` scans for those headers and re-runs each one, rewriting the output.

It's idempotent, so a regen with no spec change touches nothing; it drops cleanly into a pre-commit hook or a CI check that fails if the committed code has drifted from the spec.

## Where it stands

### Shipping today

- `glyph gen openapi` (types, `--client`, `--handlers` with a real router), `glyph gen dts`, `glyph gen zod`, and `glyph regen` to refresh it all from source: every command emitting committed, descriptor-bearing, idempotently-regenerated Glyph.
- Two types that would be written under one Glyph name stop the generator instead of producing a file that cannot compile: it names every colliding source, writes nothing, and `--rename Source=GlyphName` resolves it.
- That choice is recorded in the generated header, so `glyph regen` replays it.

### On the way

- Richer enum mapping and broader OpenAPI coverage, each landing behind the same wire-faithful, note-when-narrowed rule.
- Generating from a package's own `.d.ts` reads its `interface` and `type` declarations, so a library whose API is classes, or whose fields use computed types like `Omit`, leaves references the generator names in a note and `glyph build` reports as unresolved.
- Importing such a class and constructing it with `new` needs no generation and is checked by `tsc`, so that path is unaffected.

Code examples on this page are checked against Glyph 0.1.154 on every build. Last changed 21 Nov 2026.
