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

## 0.1.6 — Shipped · Correctness & diagnostics

**Status: released.** The sharp edges first-time-user agents actually hit. All
five items shipped, led by the warning tier.

- **Warning-severity diagnostics** (M, unblocker) — ✅ **done.** Diagnostics now
  carry a severity (`Error`/`Warning`); the renderer picks ReportKind + color by
  it, and `glyph build` tracks errors separately so a warning is surfaced without
  failing the build or blocking emission.
- **`Result` must-use warning** (S) — ✅ **done.** E0217 warns when a
  `Result`-typed expression is used as a *non-final* statement (so its `Err` is
  discarded). Scoped to non-final statements to never mistake a match-arm block's
  tail value for a drop; silent across every example.
- **Source-mapped `tsc` errors** (L, high value) — ✅ **done.** The emitter emits
  a coarse source map (`(byte offset, Glyph span)` per declaration and top-level
  statement, shifted past the prepended import header); the CLI parses tsc's
  `path(line,col): error TSxxxx` output, maps each position to a Glyph span, and
  re-renders it against `.glyph` with an ariadne caret (keeping the TS code).
  Statement-level granularity; lambda-body errors map to the enclosing statement.
  Unattributable lines (stdlib `.ts`, summaries) pass through. Wired into both
  `build --check` and `run`.
- **Nested record-payload whole-ident bind** (S) — ✅ **done.** `Err(BadQty(b))`
  binding a whole record payload in a nested match emitted `.value` (which the
  flattened `{tag, ...fields}` object lacks) and `tsc`-errored. Fixed by
  recording the synthesized grouping temp's payload type in an emitter side
  table so the inner match binds the whole object.
- **`\${...}` template-literal escaping** (M) — ✅ **done.** A literal `\${` now
  stays literal via an internal escaped-`$` marker + a char-aware template
  splitter; the same rewrite fixed non-ASCII template text being mangled. (A
  nested string literal *inside* `${...}` still needs a `let` hoist — the full
  lexer template-literal mode remains a v1.1 item.)

## 0.1.7 — Next · Works with React, speaks to agents

**Status: committed, the active target.** All 15 brainstormed items are scoped
into 0.1.7, built in adoption-rank order (1 → 15), each with tests. Full plan and
per-item testing strategy: [`../plan/0.1.7-language-and-agent-experience.md`](../plan/0.1.7-language-and-agent-experience.md).

1. **JSX fragments `<>...</>`** (S/M) — ✅ **done.** Parser (`<`-then-`>`
   lookahead + `</>` close, empty-name element), resolver/emitter `Fragment`
   kind → `React.createElement(React.Fragment, ...)`, formatter round-trips.
2. **Member-expression JSX `<Ns.Comp>`** (S/M) — ✅ **done.** `jsx_element_name`
   parses dotted tag names; resolver resolves the base segment; emitter uses the
   dotted string as the `createElement` type. React Context providers work.
3. **Machine-readable diagnostics (`--json`)** (M) — ✅ **done.** `glyph build
   --json` emits a JSON object (ok/errors/warnings/tsc/emitted + a `diagnostics`
   array with code, severity, message, file, 1-based line/col range, stage, help,
   note). A structured `Diagnostic` is built at every diagnostic site, and
   remapped tsc errors are included pointing at the Glyph source.
4. **Runtime source maps** (M/L) — ✅ **done.** Every emitted `.ts` ships a
   standard v3 `.ts.map` (VLQ, `sourcesContent` embedded) + a `sourceMappingURL`
   comment, built from the emitter checkpoints. A debugger or bundler chaining
   maps traces the `.ts` back to `.glyph`. (Boundary: `glyph run`'s own stack
   still shows `.ts` — tsx doesn't chain the map through its `.ts`→`.js`
   transform; remapping the run stack is a follow-up.)
5. **`gen dts` on TypeScript 7 native API** (M/L) — ✅ **done.** Drives the
   `typescript/unstable/sync` API (open file → inferred project → program →
   source file) with `unstable/ast`'s `SyntaxKind`; one walker handles both the
   classic (5/6) and native (7) compilers via a small toolkit (the native AST's
   missing `questionToken` is detected from the member text). The deferred 0.1.5
   finding is resolved.
6. **Bounded generics `<T: Bound>`** (M) — ✅ **done.** Parser records the bound
   (single bound in v1); emitter lowers it to a TS `extends` clause that tsc
   enforces, so a violated bound is caught and mapped back to the `.glyph` call
   site.
7. **Discriminated-union generation** (L) — ✅ **done.** The deferred 0.1.5
   finding, resolved manifesto-safely by generating code, not changing the
   language: a discriminated `oneOf` emits a Glyph tagged union of the variants
   plus a `parse_<Name>` dispatcher that reads the discriminator property (via a
   new `std/json.discriminant`) and validates into the right variant. Verified
   the generated union compiles, dispatches a real wire object, and is
   idempotent.
8. **Shared-state / store pattern** (M, design first) — ✅ **done.** A new
   `std/store`: `create(initial)` returns a `Store<T>` with `get`/`set`/`update`.
   A module-level `const s = create(...)` gives many functions one shared state
   without a `let` in `main` or capturing closures — and needs no rule relaxed,
   since the `const` binding never moves (D20) and no `mut` reassignment is
   involved (D5); only the store's internal value changes, through a greppable
   `.set`/`.update` method call. Design note + guide in
   [`../guide/shared-state.md`](../guide/shared-state.md); a corpus program and a
   build test cover it; the codegen-style answer page (08) is on the site.
9. **More warning-tier lints** (S each) — ✅ **done.** Three advisory warnings
   (never block the build): unused import (E0106), unused `let` (E0107, `_`
   exempt), and unreachable code after `return`/`break`/`continue` (E0108).
   Computed in a self-contained `module_lints` pass that runs only on
   error-free modules and reads the authoritative resolution map for usage, so
   incompleteness can only miss a lint, never invent one. Building the examples
   surfaced (and we removed) four genuinely-dead imports. *Bug found and fixed
   in passing:* template interpolations were parsed from offset 0, so adjacent
   `${a} ${b}` produced colliding spans that overwrote each other in the
   resolution map — silently dropping a resolution (and breaking go-to-def/
   rename inside templates). Fixed by offsetting each interpolation's parse.
   Exact byte-accurate template spans still need a lexer template-literal mode
   (v1.1); the offset is unique, which is what the resolution map requires.
10. **number/string value-match exhaustiveness** (M) — ✅ **done.** A `match` on
    a `number`/`string` with only literal arms is now E0218: those domains are
    unbounded, so it can never be exhaustive, and the emitter's `switch`
    `default` would throw at runtime. Requires an `else` (or a bare-identifier
    binding). Detected by the scrutinee's static type or recovered from a
    literal arm (mirroring the bool checker). Unit tests, a negative case, error
    catalogue + `--explain` entry.
11. **`glyph regen`** (M, Q40).
12. **`@redact` full enforcement** (M, D24).
13. **`glyph build --out X` cleans stale files first** (S).
14. **Extend the targeted type hint** (S) to `int`/`any`/`Promise<T>`.
15. **Nested nullary-in-object parser bug** (S).

## Rolling · Ergonomics & polish

The former rolling-lane items (`--out` cleanup, store pattern, `@redact`,
`glyph regen`) are now scoped into 0.1.7 above. New small wins that surface later
land here until they're assigned a release.

## Parked (v2 / later)

- `@ffi target:` syntax (v2).
- Mapped types / `infer_shape` (substep 5b).
- `owned` closure-capture soundness (needs real capture analysis).
- Self-hosting (a v1.0 non-goal).

---

*Sequencing note:* 0.1.5 is committed; the 0.1.6/0.1.7 split and the rolling lane
are a proposal, ordered by dependency (warning tier before must-use, etc.). We
re-sort at each release boundary.
