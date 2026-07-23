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

## 0.1.7 — Shipped · Works with React, speaks to agents

**Status: released (with a 0.1.8 hotfix, below).** All 15 brainstormed items
landed, built in adoption-rank order (1 → 15), each with tests. Full plan and
per-item testing strategy: [`../plan/0.1.7-language-and-agent-experience.md`](../plan/0.1.7-language-and-agent-experience.md).

**0.1.8 — Shipped · hotfix.** The published platform binaries lost their Unix
execute bit (GitHub artifact upload/download strips it), so `npx @glyphlang/glyph`
failed with `EACCES` — a latent bug in every release through 0.1.7. Fixed by
having the launcher `chmod 0o755` the binary before spawn and the release
workflow restore `+x` before publish; verified against the published package from
a clean npx cache.

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
11. **`glyph regen`** (M, Q40) — ✅ **done.** Every file `glyph gen` writes
    already carried its exact invocation in the header; that line is now
    complete (`--out` + flags) and machine-runnable. `glyph regen [path]` scans
    a dir/file for those headers, dedupes the commands, and re-runs each once,
    so a spec change flows into the committed Glyph with one command. Idempotent
    and deterministic; runs from the project root where recorded relative paths
    resolve. *Scope note:* this is Q40 Option B's deterministic half — refresh
    generated code from a spec. The sketch's other half (an LLM regenerating a
    `@generate` *body* from a prompt) is inherently non-deterministic and stays
    out of a tested v1 command; deferred. Rust unit + integration tests (full
    gen → edit spec → regen → idempotent-rerun cycle).
12. **`@redact` full enforcement** (M, D24) — ✅ **done.** `@redact fields:
    [...]` on a record type now (a) is validated: an unknown field name is E0219
    (masking a non-existent field would be a silent no-op), and (b) emits a
    `redact(value)` method on the type's runtime descriptor that returns a
    serialization-safe copy with those fields replaced by a `[REDACTED]`
    sentinel — so `json.stringify(User.redact(u))` masks the PII. The masking is
    additive to the descriptor (it never touches `is`/`parse`/`schema`, which is
    what a prior attempt broke), so the descriptor tests stayed green. Shared
    `glyph_ast::redact_fields` single-sources the `fields: [...]` parse for the
    typechecker and emitter. Integration test (masked output + E0219), a negative
    case, error catalogue + `--explain`. *Honest scope:* enforcement is via the
    explicit `T.redact(value)` descriptor method, not fully-automatic boundary
    interception (masking every `json.stringify`/log call would need a runtime
    type tag on values); that automatic form is future work. Related gap noticed:
    the D27 "unknown annotation is a hard error" rule is documented but not
    enforced yet — parked below.
13. **`glyph build --out X` cleans stale files first** (S) — ✅ **done.** The
    G17 stale-`.ts` prune already removed orphaned emitted modules; it now also
    prunes their `.ts.map` source-map sidecars (item 4 added those after G17), so
    a renamed/removed module leaves no orphan map either. A `.ts.map` is kept iff
    its `.ts` is; unrelated files the user placed in the out dir are preserved.
    Integration test (rename a module, rebuild, old `.ts`+`.ts.map` gone, user
    file kept).
14. **Extend the targeted type hint** (S) — ✅ **done.** The `boolean`→`bool`
    style "did you mean the Glyph spelling" hint on an unresolved name now also
    covers `int`/`Int`/`integer`/`float`/`double` → `number`, `any` → `unknown`
    (narrow via `.parse`/`match`), and `Promise` → "an `async fn` returns `T`
    directly." Unit tests.
15. **Nested nullary-in-object parser bug** (S) — ✅ **done.** A union with no
    leading `|` whose *first* variant carried a payload
    (`type W = Wrap({ inner: Inner }) | Empty`, or a lone `type W = Wrap(P)`)
    failed to parse — the type-decl body read `Wrap` as a plain type and choked
    on the `(`. `parse_type_decl_body` now promotes a payload-carrying first
    atom to a variant and continues as a union. Parser tests for both shapes;
    the emitted match lowers and passes tsc --strict.

## 0.1.10 — Shipped · Make the verifiability guarantee match the pitch

**Status: shipped.** From a deep code-level review (the "Linus" pass). The
review confirmed the compiler is real and several decisions tasteful, but caught
the marketing overclaiming relative to what the code guarantees — and the code's
own doc-comments were more honest than the site. The honesty fixes shipped with
0.1.9's tail (home card + verifiability pillar reworded from "no casts / no
erasure / true at runtime" to what's actually true: no `any`/`as` in source,
exhaustive `match`, strict validators for declared types, an enforced strict
dialect over `tsc`; the pillar now owns the `tsc` dependency and names the
generic edges). 0.1.10 closes the engineering behind them:

- **`infer_shape` for schema combinators** (L, Q40/substep-5b, D28) — ✅ **done.**
  `object_schema<Shape: Record<string, Schema<unknown>>>(shape) -> Schema<infer_shape<Shape>>`
  now derives the output type from the shape. `infer_shape<S>` is a narrow
  built-in type-level operator (not the full TS mapped-/conditional-type surface):
  it lowers to one per-module `type __GlyphInferShape<S> = { [K in keyof S]: S[K] extends Schema<infer V> ? V : never }`,
  and `tsc` reduces and enforces it at each call site. A shape that omits a field
  of the annotated type now fails to compile (regression-tested end to end,
  mapped back to Glyph source). The flagship `01_validator.glyph` dropped its
  hand-synced `<Out>`. See spec D28.
- **Prove or remove the generic-return `as` cast** (M) — ✅ **done, resolved to
  "narrow."** The empirical finding: the blanket cast was never legitimately
  needed. For honest generics (`identity<T> -> T`, `array_schema<T> -> Schema<Array<T>>`)
  it was pure noise TS proves on its own; the one place it was load-bearing was
  masking the `object_schema` unsoundness. It now fires *only* when the return
  type mentions `infer_shape` — the single case a combinator assembles a value of
  a shape-derived type from `unknown`. Every honest generic emits cast-free.
- **Formatter dropped generic bounds** (S) — ✅ **fixed as a side-catch.** `glyph fmt`
  silently discarded `<T: Bound>` (D28's `object_schema<Shape: Record<...>>` was
  the first program to exercise it), which changed the emitted TS. The formatter
  now round-trips bounds; caught by the round-trip semantics test.
- **Generic-type descriptors** (L) — ✅ **done.** A generic record type
  (`Paginated<T>`) now emits a descriptor whose `is`/`parse` take one runtime
  checker per type parameter (`__is_T`). `Paginated.parse<User>(v)` and
  `match v { is Paginated<User> => ... }` validate the payload *deeply* — each
  element is checked as a `User`, not just for presence — the compiler
  synthesizes the checker from the type argument at the call site (reusing the
  recursive `field_value_check`, the same machinery `json.parse<T>` routing uses).
  A generic descriptor omits the `.schema` member (a `Schema<Paginated<T>>`
  factory would need the checker threaded too). Function-typed fields were also
  tightened from presence to `typeof === "function"`.
- **Imported-type descriptors** (M) — still open. A type from an external
  `.d.ts` you only reference carries no descriptor, so a field of that type is
  presence-checked. Materializing it with `glyph gen dts` gives it one; a
  first-class path (validate against the `.d.ts` structure directly) is future
  work. This is the one remaining `T.parse` honest edge.
- **Strengthen `definitely_incompatible`** (M) — ✅ **done.** The conservative
  assignability relation now judges three shape pairs it used to punt to `tsc`,
  each proven-only (no false positives): a concrete scalar
  (`string`/`number`/`bool`) against a record or function type in either
  direction; two function types whose return types are incompatible (return
  covariance; `void` skipped for the un-annotated-lambda stub and callback
  contravariance); and two structural records with an incompatible shared field
  or a missing required field. Passing `5` where a `fn(number) -> number` is
  expected, or a `string`-returning function where a `number`-returning one is,
  is now caught at the Glyph level (E0211) instead of only by `tsc`. Record-vs
  record is sound but mostly latent until object-literal argument inference
  improves (today those infer to `Unknown` and stay permissive).

## 0.1.11 — Shipped · The editor & agent integration surface

**Status: shipped.** 0.1.10 made the language itself trustworthy; this release
widened how editors and agents reach it. The language server already ships
(`glyph lsp` over stdio: diagnostics, hover, go-to-definition, completion,
symbols, formatting); these are the two most-requested gaps on top of it.

- **Rename + find-references in the LSP** (M) — ✅ **done, workspace-wide.**
  `textDocument/references` and `textDocument/rename` ship. A binding is
  identified canonically: a file-local binding by its def-site, a module-level
  symbol by `(module path, name)` — where the module path is the file's own for
  a declaration, or the import's for an imported name — so every file agrees on
  one identity. Find-references and rename now span the whole workspace: a
  module-level rename edits the declaration, every reference, and each importing
  module's `import` binding, and validates the new name (legal identifier,
  non-keyword) first. Local bindings stay file-scoped (they can't cross files).
- **Cross-file workspace index for the LSP** (L) — ✅ **done (on-demand).** The
  server parses+resolves every `.glyph` file under the root (preferring open
  buffers, including unsaved files) and cross-references them by global identity.
  This is what makes the workspace-wide references/rename above complete. Honest
  scope: the index is rebuilt per request rather than cached (an optimization for
  later), and a file that doesn't parse is skipped. Caching + incrementality, and
  extending the same cross-file resolution to go-to-definition, are the
  follow-ups.
- **First-party MCP server exposing the language server** (M) — ✅ **done.**
  `glyph mcp [root]` speaks the Model Context Protocol over stdio (newline-framed
  JSON-RPC 2.0, hand-rolled — no new dependency beyond `serde_json`) and exposes
  five tools over the project: `glyph_diagnostics`, `glyph_hover`,
  `glyph_definition` (follows imports), `glyph_references` (workspace-wide), and
  `glyph_symbols`. Each is a thin adapter over the same pure `crate::analysis`
  query the editor path uses — no second implementation — so it can't drift from
  the compiler. Complements `glyph build --json` (batch diagnostics) with
  interactive semantic queries. Requested by an early user. Follow-ups: a rename
  tool (a write operation that returns edits), and sharing the workspace-scan
  helpers with the LSP path once the index is cached.

## 0.1.12 — Shipped · Docs patch

Republished so the npm README documents the MCP server and the language server
that shipped in 0.1.11 (the README only updates on publish). No code changes.

## Road to 1.0

**Status: the committed plan, from the third review.** The review (docs and code
grounded) credited the toolchain as real and tasteful but found that a 1.0 is
gated on a question the project has not decided: can a working engineer use their
existing npm dependencies without writing a hand-written adapter per library? The
one-line diagnosis: Glyph is safe on code it owns and leaky at the seam with npm,
and real projects are all seam. The road below closes that seam, decides and
builds interop, proves it on real apps, and settles the productivity claim.
Everything here traces to a specific finding with file evidence.

The **Next** marker: 0.1.13 shipped four of the six boundary items. The next
0.1.x picks up the two that need design (node builtins typecheck out of the box,
imported-`.d.ts` validate-or-diagnose), and 0.1.14 makes the interop decision.

The version numbers below mark themes and milestones, not a fixed schedule. The
0.1.x series stays open: expect several 0.1.x releases between the named ones as
the work lands incrementally. A minor bump (0.2.0, 0.3.0) marks a milestone
actually reached, not a date. The interop build in particular will span multiple
0.1.x releases before 0.2.0 declares "interop that scales" real.

We also run the "Linus" review pass periodically, not only at the end: a
read-only, adversarial third-party read that checks whether the direction is
honest and pointed at 1.0 rather than wandering. Do it at each milestone and
whenever a release makes a claim worth stress-testing. The first three passes are
recorded in this file's history; keep calling it.

### 0.1.13 — Shipped · Close the boundary (honesty and hygiene)

The cheap, concrete must-haves that stop the verifiability wedge from leaking
silently, which is the trap a 1.0 is most likely to fall into (rounding
"presence-checked at the boundary" up to "validated, no lies"). Four of the six
shipped; two need real design and moved to the next 0.1.x.

- **`tsc` stops being silently optional** (M). ✅ **Done.** `glyph run`, `build`,
  and `publish` now exit non-zero when `tsc` is missing on the checked path,
  pointing at the explicit `--no-check` opt-out (`run.rs` `RunOutcome::TscMissing`,
  `main.rs`). No code path advertises a type check it then skips silently.
- **Enforce D27** (S). ✅ **Done.** An unknown `@annotation` is now the hard error
  the spec always promised (`E0221`, `assign.rs` `check_annotations`); a typo like
  `@puer` no longer compiles clean. The typechecker's doc comment that claimed
  this was already true is now true.
- **Publish discipline** (S). ✅ **Done.** A CI job (`scripts/check_versions.py`)
  hard-fails when the Cargo version and the six npm package.json versions (plus
  optionalDependency pins) disagree, and flags non-fatally when npm `latest` has
  fallen behind the repo. Ashfaq reviewed a package two versions behind; this
  makes that drift visible.
- **Manifesto honesty** (S). ✅ **Done.** The unmeasured "reviewer finishes in half
  the time" line is reworded as a hypothesis to be measured (0.3.0), with no
  figure put on it.
- **Node builtins typecheck out of the box** (M). **Moved to a following 0.1.x.**
  The emitted tsconfig ships `"types": []` (`runtime.rs:113`), and the bundled
  shim only declares `node:fs`, not the bare `fs`/`http` a user imports. Making
  `import fs`/`http` typecheck with no stub needs a real design (bundled shim vs
  `@types/node` vs specifier rewriting), not a one-liner, so it is not in 0.1.13.
- **Imported `.d.ts` type in a `.parse` position: validate or diagnose** (M).
  **Moved to a following 0.1.x.** Needs a new warning when a descriptor field is
  presence-only because its type is opaque, which is design, not a quick fix.

### 0.1.14 — Decide interop, ship the first slice (gated on the Q43 decision)

The make-or-break question. The design decision is now made (see
`docs/plan/interop-q43.md`), so this release builds the first concrete win.

- **Resolve Q43** (the decision). ✅ **Resolved: Option 3 (phased hybrid), full
  React-included scope. Phase 2 materialization is opt-in per module, not
  auto-on-import** (predictable build cost, greppable descriptors, no implicit
  codegen). Option 2 (trust the `.d.ts`) rejected: it spends verifiability at the
  boundary. Phase 1 is the cheap immediate unblock (installed package types load,
  generalizing the `"types"` fix); Phase 2 materializes data types at the boundary
  where the wedge matters; Phase 3 is the escape hatch plus the React primitives.
- **Phase 1 — type availability** (M). ✅ **Done.** The generated tsconfig now
  wires the project's `node_modules` into `paths` (a `"*"` entry, found by walking
  up from the source to the project root marked by `.git`/`package.json`, never
  climbing into an unrelated ancestor's `node_modules`), so an installed package
  that ships its own types (or has an `@types/*`) typechecks with no hand-written
  `.types/` stub. The emitter emits project imports as relative specifiers, so the
  wildcard only ever catches external packages. Proven end to end: a fake
  installed package resolves and a wrong-typed call to it is rejected by tsc
  (types loaded and enforced, not `any`). A dependency-free project (the examples)
  emits the identical tsconfig as before. Node builtins (bare `fs`/`http` via the
  `"types": []` ambient path) are still the separate deferred item.
- **First slice** (L). ✅ **Done: real zod, no adapter.** With zod installed in a
  project, `import zod { z }` and its real API (`z.object`, `z.string`, `.parse`)
  type-check against zod's own published types and run end to end via `glyph run`,
  with no `.types/zod.d.ts` and no glue file. A call zod does not define is a real
  error mapped back to the Glyph source; the parse result is fully typed
  (`user.name` is a `string`). The single tsconfig `paths` entry from Phase 1
  resolves the package for both `tsc` and the tsx runtime. Captured in the
  `external-imports` guide with the reproducible steps; the hermetic integration
  test (a structurally-real fake package) guards the mechanism in CI without a
  network install. Not yet expressible: a value-derived `type U = z.infer<typeof
  s>` (the Phase 3 value-derived-type work). *This slice does not yet include the
  opt-in boundary materialization (Phase 2); it is type availability plus runtime
  resolution.* The walker now also skips `node_modules` so it never compiles a
  dependency's stray `.glyph`-named file.
- **Phase 2 — boundary materialization, first increment** (M). ✅ **Done.** The
  opt-in surface is resolved to **committed `glyph gen dts <package>`** (over an
  import annotation or a manifest list): the existing `.d.ts` materializer now
  resolves an installed package by name from `node_modules` (reading its
  `types`/`typings`/`exports` entry, or a top-level `index.d.ts`), and writes real
  committed Glyph types with runtime descriptors. `glyph gen dts stripe --out
  src/types` gives you `Customer.parse(webhookBody)` that validates the wire value
  deeply; the generated file records `glyph gen dts stripe --out src/types` so
  `glyph regen` refreshes it on a dependency bump. Proven end to end (a fake
  installed SDK materializes, its descriptor validates at build and runtime,
  regen re-runs it) with hermetic unit tests over the resolution and helpful
  errors for a missing package or one that ships no types. This keeps the
  verifiability wedge at the npm seam without new grammar or non-committed build
  magic.
- **Phase 2 — package-name parity for `gen zod`** (S). ✅ **Done.** `glyph gen zod
  <package>` now resolves an installed package's *runtime* entry (`main`/`module`,
  or the `import`/`default` condition of `exports["."]`, or a top-level
  `index.js`) and executes it for its exported zod schemas, so a shared-schema
  package (`@acme/schemas`) materializes with no file path. The resolver is shared
  with `gen dts` via a `PackageEntry` kind (types vs runtime entry). Proven with a
  scoped package exporting `z.object` schemas. `gen openapi` deliberately stays
  file-based: an OpenAPI document is a committed file in your repo, and
  package.json has no convention pointing at one, so there is nothing to resolve
  from `node_modules`. Still ahead and folded into Phase 3: value-derived
  materialization (`z.infer<typeof s>`).

### Interop that scales (0.1.15 onward, milestone 0.2.0)

The build that broadens the mechanism to the cases that broke every hands-on
tester (Serhiy, Hayk, Adi, Ashfaq). This spans several 0.1.x releases; 0.2.0 is
the version that declares it real, not the version it all lands in.

- **The grammar-hostile idioms** (L). Prop spread (`{...register()}`), value-derived
  types (`z.infer<typeof s>`), scoped/hyphenated package names. Whatever the Q43
  decision, these need either a language primitive or a scoped, visible escape
  hatch, not a hand-written adapter file.
- **Real dependencies used directly** (L). Import `react-hook-form` and a Postgres
  client and use their real APIs with no adapter.
- **Stdlib breadth or a documented "use npm for X"** (M) for crypto, database, and
  real servers, so the 744-line hand-written stdlib is not the only answer.

*Done:* a real app's dependency list installs and is used with zero per-library
adapters.

### React track — required (scope decided: React-included)

The scope decision is made: Glyph commits to being a serious React language, so
the React work is a must-have, not a maybe. This is what makes the road longer.

- **Answer Q44** (L). A Context primitive (`createContext`/provider/`useContext`
  equivalent) and a story for effectful custom hooks that composes with the
  `@pure` JSX-callable rule (D9). Today a hook that calls `use_state`/effects can
  neither be written nor JSX-called. *Done:* a custom hook and a Context provider
  written in `.glyph`, no TS adapter, used in a component.
- **The React-library grammar primitives** (L), folded into the interop work
  above: prop spread in JSX (`<input {...register()} />`) and value-derived types
  (`z.infer<typeof s>`, generalizing `infer_output`). *Done:* `react-hook-form`
  used from `.glyph` with its real API and no adapter.

### 0.2.x — Prove it (the evidence gate)

One CLI dogfood app (`examples/apps/fridge.glyph`) is not enough to bet a project
on.

- **A second real app with persisted data on a real DB client** (L), no wrapper.
- **A persisted React app** (L) exercising Context, a custom hook, and a real form
  library, since React is in scope for 1.0.

*Done:* real apps built and kept on the shipped interop path, at least one backed
by a database and one a real React app.

### Settle the productivity claim (milestone 0.3.0)

- **One honest agent study** (M): the same task, N trials, Glyph vs TypeScript,
  tracking correctness, tries-to-green, and review time. Either it backs the
  manifesto's claim, or the claim stays a hypothesis and the copy says so.

### 1.0 gate

All of these true: interop without per-library adapters, proven on two or more
real apps (one with a DB); every boundary verifiability hole closed or loudly
labeled; node builtins typecheck out of the box; publish discipline CI-enforced;
the productivity claim measured or downgraded.

### Decisions

1. **Is Glyph a serious React language?** ✅ **Resolved: yes (React-included).**
   Q44 (Context + effectful hooks) and the React-library grammar primitives are
   must-haves, and a persisted React app is a required proof. This is the larger
   1.0.
2. **Interop mechanism (Q43).** ✅ **Resolved: Option 3 (phased hybrid), full
   scope** (Option 4 backend-first narrowing is off, since React is in). Phase 2
   boundary materialization is **opt-in per module**, not auto-on-import. See
   `docs/plan/interop-q43.md`. Unblocks the 0.1.14 build.

### Explicitly out of 1.0

Self-hosting; the annotation wishlist (refinement types Q15, contracts Q14,
effects Q17, typestate Q28, units Q36, taint Q33, budgets Q34, and the rest of
Q13 to Q40); non-TS FFI (Q41); the full dual human/agent view (Q32). Keep them
parked. They are the scope-creep trap `overview.md` already names.

## Verifiability hardening — Linus 2nd-pass follow-ups

**Status: from the second deep code-level review.** The review verified 0.1.10
against the source and granted the "grudging nod" (the honesty fixes are real,
the engineering is real, `definitely_incompatible` is good taste). It left four
concrete follow-ups, in priority order:

- **Don't let `tsc` be optional for the guarantee we advertise** (M) — `glyph run`
  type-checks with `tsc` by default, but if `tsc` is not on `PATH` it prints a
  warning and runs anyway (`glyph-cli/src/run.rs`). That quietly downgrades the
  soundness story on a box without `tsc`. Make the skip loud and non-zero-exit,
  or require `tsc` for the checked path, so the guarantee never evaporates
  silently. (`glyph build --check` and CI already hard-fail; this is the `run`
  path.)
- **Rename and generalize `infer_shape`** (M) — ✅ **done.** The operator was
  welded to the literal type name `Schema` (the emitted mapped type said
  `S[K] extends Schema<infer V>`), so a validator type named anything else
  silently mapped every field to `never`. Renamed to **`infer_output`** (honest:
  it derives the output types the parsers produce) and generalized to match a
  parser field **structurally** — any `{ parse(input: unknown) -> Result<V, _> }`,
  reading the `Ok` payload out of the result's wire form — so it is independent of
  the wrapper's name (a user's own `Codec<T>` works too, pinned by an integration
  test). The one boundary cast now fires on `infer_output` returns. See spec D28.
- **Prove the `tsc`-error source remap in a test** (S) — the `infer_shape` bite
  integration test asserts only that `tsc` *failed*, not that the diagnostic maps
  back onto the `.glyph` line, while the commit message claims "mapped back to
  Glyph source." The remap works (verified manually); strengthen the test to
  assert the diagnostic lands on the Glyph source span so the claim is pinned.
- **Close the imported-`.d.ts` presence-only hole** (M) — already tracked as the
  one remaining `T.parse` honest edge (see the 0.1.10 imported-type-descriptors
  item). Reaffirmed as the softest spot in the runtime story: an imported type is
  checked for presence only until materialized with `glyph gen dts`.

## Rolling · Ergonomics & polish

The former rolling-lane items (`--out` cleanup, store pattern, `@redact`,
`glyph regen`) are now scoped into 0.1.7 above. New small wins that surface later
land here until they're assigned a release.

## Parked (v2 / later)

- **D27 unknown-annotation rejection.** The spec says an unknown `@<name>`
  annotation is a hard error, but the typechecker doesn't enforce it yet (a
  `@bogus` is silently ignored). Add a recognized-annotation dispatch table.
- **Automatic `@redact` boundary masking.** Today redaction is via the explicit
  `T.redact(value)` descriptor method; masking every serialize/log call
  automatically needs a runtime type tag on values.
- `@ffi target:` syntax (v2).
- General TS mapped-/conditional-type surface (`{ [K in keyof T]: ... }`,
  `X extends Y ? A : B`, user-written `infer`). Deliberately *not* shipped: the
  narrow `infer_shape<S>` operator (D28, 0.1.10) covers the schema-derivation
  case without the unreadable, hard-to-grep general machinery. Revisit only if a
  concrete need outside schema derivation appears.
- `owned` closure-capture soundness (needs real capture analysis).
- Self-hosting (a v1.0 non-goal).

---

*Sequencing note:* 0.1.5 is committed; the 0.1.6/0.1.7 split and the rolling lane
are a proposal, ordered by dependency (warning tier before must-use, etc.). We
re-sort at each release boundary.
