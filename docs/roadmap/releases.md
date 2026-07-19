# Release roadmap (0.1.x → 1.0)

The 12-step plan in [`overview.md`](overview.md) built the toolchain — that work
is shipped. This file tracks the **feature releases** layered on top and
published to npm as `@glyphlang/glyph`. One release carries the "Next" marker and
is committed; everything after it is directional and re-sorts as we learn.

Each item keeps a rough T-shirt effort (S/M/L) and traces to a real source: the
persona-testing issue inventory, the generation follow-ups, the site's "on the
way" promises, or the standing deferrals in CLAUDE.md.

## Shipped

- **0.1.0–0.1.2** — first public preview: the language + Rust compiler, the
  standard library, the site and playground, `std/http` server, `glyph init`,
  and a wave of correctness/JSX fixes from persona testing.
- **0.1.3 — Generated types, not hand-written DTOs.** `type` is the zod
  replacement (declare a type, get a validated boundary); `glyph gen openapi`
  and `glyph gen dts` generate committed, descriptor-bearing types; the
  typed-APIs guide and the runnable REST example.
- **0.1.4 — TypeScript 7 handling for `glyph gen dts`.** A clean "install
  `typescript@6`" diagnostic instead of a cryptic crash on the native compiler.
- **0.1.5 — Typed clients and servers from your API spec.** `gen openapi
  --client`/`--handlers` and `gen zod`; untrusted input typed as `Option`
  (`header`/`query_param`) with the `put`/`patch`/`del` client verbs; `gen dts`
  resolves TypeScript from the target project first. Details and the deferred
  findings are in the section below.

## 0.1.5 — Shipped · Finish the generation / typed-API story

**Status: released.** Carried the 0.1.3/0.1.4 momentum to completion and made
the site's "on the way" promises real. Two items were deferred with findings
recorded (discriminated unions, full TS7-native `gen dts`).

- **`gen openapi` client codegen** (M) — ✅ **done.** `--client` emits one typed
  `async fn` per operation over `std/http` (typed path params + request body,
  interpolated URL, `Result<Response, HttpError>`). The full verb set
  (`get`/`post`/`put`/`patch`/`del`) shipped first as the enabler.
- **`gen openapi` handler codegen** (M) — ✅ **done.** `--handlers` emits a typed
  stub per operation plus a `route` dispatcher that matches method + path via
  array patterns over a new `http.segments(req)` (`/tasks/{id}` → `["tasks", id]`,
  binding the param). Verified routing live. Combines with `--client` (handler
  stubs are `handle_`-prefixed to stay unique).
- **Discriminated unions in generation** (M → **L, blocked on runtime rep**).
  *Finding while building the mapper:* a Glyph tagged union tags by a `tag` field
  carrying the **constructor name** (`{tag:"Cat"}`), whereas an OpenAPI
  `discriminator` selects a variant by an **arbitrary property** (`petType`)
  carrying a **string value** (`"cat"`). So a generated tagged union's descriptor
  would reject the real wire object — the same class of wire-mismatch that makes
  string enums narrow to `string`. A faithful mapping needs either a
  discriminator-aware union runtime representation or a new descriptor that reads
  a named property. Treat as its own runtime-representation task, not a mapper
  tweak; may slip past 0.1.5.
- **`gen dts` on TypeScript 7** (M/L) — 🟨 **partially done; full support
  deferred.** `gen dts` now resolves TypeScript from the *target file's own
  project* first, so a project that pins `typescript@6` (the norm) just works
  even when the global install is 7.x; diagnostics distinguish "no TypeScript"
  from "only the 7.x native port." *Finding while scouting the native API:* the
  7.x package's default export is only the version; the real API lives under
  `typescript/unstable/*` (`unstable/sync` = a project/handle-based `API`,
  `unstable/ast` = `SyntaxKind` + `is*` guards but **no `createSourceFile`**).
  Driving it for standalone `.d.ts` parsing needs the project/`Program`/
  `NodeHandle` path, which is under-documented and explicitly unstable — a real
  integration, not a tweak. Deferred past 0.1.5; the project-pin path covers the
  common case in the meantime.
- **`gen zod`** (M) — ✅ **done.** `glyph gen zod <file.ts>` executes the schema
  module via `tsx`, converts each exported zod schema to JSON Schema (zod 4's
  `z.toJSONSchema`, or `zod-to-json-schema` on zod 3), normalizes zod's
  null-union nullability into the shared mapper, and emits committed Glyph types.
  The node/tsx runner is now factored (`run_helper`) and shared with `gen dts`.
- **Untrusted input as `Option`** (M, correctness) — ✅ **done.** `http.header`
  and `http.query_param` return `Option<string>`, modeled so the exhaustiveness
  checker forces the `None` arm; a bonus fix models named-imported stdlib
  functions too, so signatures hold regardless of import style. (`Request.body`
  stays `unknown` — it's already safe-by-construction, since it can only be used
  through a descriptor's `.parse`, which rejects a missing/`null` body.)

## 0.1.6 — Next · Correctness & diagnostics

**Status: committed, the active target.** The sharp edges first-time-user agents
actually hit. The warning tier is foundational infrastructure, so this lane
leads with it.

- **Warning-severity diagnostics** (M, unblocker) — ✅ **done.** Diagnostics now
  carry a severity (`Error`/`Warning`); the renderer picks ReportKind + color by
  it, and `glyph build` tracks errors separately so a warning is surfaced without
  failing the build or blocking emission.
- **`Result` must-use warning** (S) — ✅ **done.** E0217 warns when a
  `Result`-typed expression is used as a *non-final* statement (so its `Err` is
  discarded). Scoped to non-final statements to never mistake a match-arm block's
  tail value for a drop; silent across every example.
- **Source-mapped `tsc` errors** (L, high value). Errors only `tsc` catches point
  at generated `.ts`, not `.glyph` source — the biggest gap against the
  Elm-quality-errors claim. Needs a source map through emit.
- **Nested record-payload whole-ident bind** (S) — ✅ **done.** `Err(BadQty(b))`
  binding a whole record payload in a nested match emitted `.value` (which the
  flattened `{tag, ...fields}` object lacks) and `tsc`-errored. Fixed by
  recording the synthesized grouping temp's payload type in an emitter side
  table so the inner match binds the whole object.
- **`\${...}` template-literal escaping** (M). A literal `\${` still emits a live
  interpolation — the documented D22 footgun; needs a real lexer template mode.

## 0.1.7 — Planned · Approved language features

Decided in earlier brainstorms; just need building.

- **JSX fragments `<>...</>`** (S/M) — parser + emitter.
- **Member-expression JSX `<Ns.Comp>`** (S/M) — parser + emitter.
- **Bounded generics `<T: Bound>`** (M) — parser + checker + emit.
- **Extend the targeted type hint** (S) to `int`/`any`/`Promise<T>`.

## Rolling · Ergonomics & polish

Small wins that can land in any release rather than wait for their own.

- **`glyph build --out X` cleans stale files first** (S).
- **A shared-state / store pattern** (M, needs a design call). Shared mutable
  state has exactly one legal home (a `let` in `main`); no clean store module.
- **`@redact` full enforcement** (M). Real masking in the emitted serializer +
  field-name validation (D24); a prior attempt broke the descriptor tests, so
  retry carefully.
- **`glyph regen` implementation** (M). The `@generate` → regenerate-body command
  (Q40) is still a stub; it now fits the `gen openapi`/`gen dts` family.

## Parked (v2 / later)

- `@ffi target:` syntax (v2).
- Mapped types / `infer_shape` (substep 5b).
- `owned` closure-capture soundness (needs real capture analysis).
- Self-hosting (a v1.0 non-goal).

---

*Sequencing note:* 0.1.5 is committed; the 0.1.6/0.1.7 split and the rolling lane
are a proposal, ordered by dependency (warning tier before must-use, etc.). We
re-sort at each release boundary.
