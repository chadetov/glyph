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
- **Discriminated unions in generation** — ✅ **done (OpenAPI in 0.1.5-era;
  TypeScript `.d.ts` in 0.1.32).** *Original finding while building the mapper:* a
  Glyph tagged union tags by a `tag` field carrying the **constructor name**
  (`{tag:"Cat"}`), whereas a discriminator selects a variant by an **arbitrary
  property** (`petType`) carrying a **string value** (`"cat"`). The resolution
  was not a new union runtime representation but a generated `parse_<Name>`
  dispatcher: it reads the named discriminator property and validates into the
  right variant record, bridging the wire object to the tagged union. OpenAPI
  `discriminator` schemas used this first; 0.1.32 extends it to a bare TypeScript
  `.d.ts` union by *detecting* the tag (a property present in every variant whose
  type is a distinct one-element string enum) and generating a record per inline
  variant. Verified end to end (accepts valid variants, rejects wrong shapes and
  unknown discriminators).
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
  - **Cross-module `Imported.parse<T>(v)`** — ✅ **closed (follow-up, 0.1.36).** The
    original slice only threaded the checker when the generic descriptor was
    module-local: `generic_descriptor_arity` scanned the current module's items,
    so an imported generic type resolved to arity 0 and the checker argument was
    dropped, leaving a call the imported `parse<T>(value, __is_T)` rejected under
    `tsc`. The build now populates a project-wide `(module path, type name) ->
    arity` registry (the same shape as the record-variant registry), and the
    emitter resolves an imported receiver through its `ImportNamed` symbol before
    consulting it. `Imported.parse<User>(v)` now emits the checker unchanged, so a
    two-module program type-checks and rejects a badly-shaped element at runtime.
    - **`is` symmetry + receiver/argument coverage** — ✅ **closed (follow-up).**
      The registry lookup now backs the `is` side too: `is_check` and
      `field_value_check` both resolve a generic descriptor through
      `generic_descriptor_arity` (local-first, then the registry), so cross-module
      `match v { is Imported<User> => ... }` narrows instead of hard-erroring
      `EmitError::Unsupported`. The parse rewrite also handles a qualified receiver
      (`bm.Box.parse<User>(v)` through a namespace/aliased import) rather than
      dropping the checker, and cross-module tests now cover a multi-parameter
      descriptor (`Pair.parse<X, Y>`) and a nested type argument
      (`Box.parse<Box<User>>`, validated deeply, not at the presence floor). Known
      edge: a descriptor reached through a *re-exporting* intermediary module keys
      the registry to the intermediary, misses, and drops the checker; D15's
      rejection of barrel-only modules narrows the blast radius but does not
      eliminate it.
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

## hookrelay dogfood trip — eliminate the extern (0.1.33 → 0.1.35)

**Status: all three trips landed on `main` (0.1.33 and 0.1.34 published; 0.1.35
pending a publish). All sixteen findings closed.** The improve-glyph loop applied
to a real networked app: `hookrelay`, a webhook receiver and dispatcher, built
end to end in Glyph — like
`examples/apps/tasks.glyph` (0.1.25) but harder (raw-body HMAC verification, a
recursive and/or/not rule engine, bounded-concurrency dispatch with retry,
lossless NDJSON round-trip, a subcommand CLI with exit codes). All 16 findings
from that build fold into three releases, sequenced additive-first. The headline
result the trip is built to prove: **a webhook service needs zero hand-written
TypeScript** — reached for the ingress in 0.1.33.

The core wedge held up in the build — `@open` boundary validation, `unknown`
without casts, errors-as-values, and tagged unions across the FFI all worked with
no fight. The friction clustered at I/O surfaces, the local-`.ts` interop
direction, and a handful of unimplemented emit cases. Each item below traces to
one finding (Fn) from the build. This trip carries the Next marker; the
Road-to-1.0 interop items below continue in parallel (F8/F14/F15 are new,
concrete instances of interop classes already tracked there).

### 0.1.33 — Landed on main · Stay in Glyph (I/O boundaries + the red build)

**Status: all six on `main` (pending an npm publish).** hookrelay's ingress
extern is gone: a signed-webhook receiver verifies HMAC and serves entirely in
Glyph. A pure-Glyph server was proven end to end (202 valid / 401 bad / 401
missing) with no extern. Additive, low risk, plus the one outright bug.

- **Raw request body in `std/http`** (S, F7) — ✅ **done.** `Request` carries
  `raw: string` (the unparsed body, `""` when none), populated by the server and
  exposed as a typed accessor `http.raw(req) -> string` beside `path`/`header`/
  `query`. This is what lets a signature-verifying server stay in Glyph: HMAC must
  run over the exact received bytes, which the server used to discard (forcing the
  whole HTTP server into an extern). `Request.body` stays `unknown`
  (safe-by-construction, per the 0.1.5 note). Integration test type-checks a
  signature-verifying handler under tsc; stdlib reference + bootstrap updated.
- **`@types/node` no longer reddens Glyph's own runtime** (S, F15) — ✅ **done.**
  The root cause: the build skips *writing* the bundled shim when `@types/node` is
  present, but a shim from an earlier shimless build lingered and its
  `declare module "node:crypto"` merged with `@types/node`'s, resolving
  `randomBytes(n).toString("hex")` to a 0-arg `toString` (TS2554). The build now
  *removes* the stale shim when `@types/node` is present. A concrete instance of
  the tracked "node-shim / @types/node consistency" item. Hermetic regression test.
- **`fs.append_text` + `fs.make_dir`, Result-returning** (S, F10) — ✅ **done.**
  `append_text` appends (creating the file), O(1) per call, the primitive for an
  append-only log; `make_dir` is `mkdir -p` (recursive, idempotent). Both wrap the
  throwing node call in the runtime and return `Result`. Run-based round-trip test.
- **`glyph fmt --check`** (S, F1) — ✅ **done.** Writes nothing, exits non-zero if
  any file is not already canonical, listing each as "would reformat"; a parse
  failure also counts as non-clean. In-place stays the default. Test + CLI docs.
- **`T.parse` returns the documented `Result<T, Array<Issue>>`** (M, F2) — ✅
  **done.** The record descriptor's `parse` now validates field by field and
  returns `Result<T, Issue[]>`, each failing field naming itself
  (`{ path: ["balance"], message: ... }`); union and refinement descriptors move
  to the same `Issue[]` error type. The emit was the side that was wrong: the
  Glyph checker already modeled `Array<Issue>`, so code written to the docs
  type-checked in Glyph then failed tsc. Five conformance snapshots regenerated;
  an integration test binds `Err(issues)` and reads `.message` under tsc --strict.
- **Stdlib reference completeness** (S, F9) — ✅ **done.** The full reference was
  actually complete; the gap was the agent bootstrap (AGENTS.md) listing only
  `now`/`sleep`/`debounce` under `std/time`, so an agent could not see
  `time.format_iso` and reached for a `Date` via `extern_ts`. The bootstrap now
  lists the full `std/time` surface, and a new test asserts every runtime
  `export` appears in `docs/reference/stdlib.md`, so the reference can't drift
  behind the real surface again.

### 0.1.34 — Landed on main · Reads like Glyph (emit ergonomics + formatter)

**Status: all six on `main` (pending an npm publish).** The papercuts that most
inflated the hookrelay code are gone: short-circuit combinators use `Ok(true)`
directly, value-position validators early-return inline, async fan-out takes a
normal closure, and short calls stop exploding to one argument per line.

- **Nested constructor+literal patterns** (M, F4) — ✅ **done.** The existing
  degroup pass now treats a literal payload like a nested constructor, so
  `Ok(true) => A, Ok(false) => B` lowers to `Ok(__p) => match __p { true => A,
  false => B }`; a later same-variant wildcard/binding arm (`Some(_)`) is absorbed
  as the inner catch-all so the value dispatch stays exhaustive. Emit + run tests.
- **`return` / block in a value-position match arm** (M, F5) — ✅ **done.** A `let`
  whose initializer is a `match` with a block arm now lowers to a statement
  `switch` (a new `ArmTerm::Assign` assigns the binding in value arms; block arms'
  `return` still returns from the function; the exhaustive `default: throw` keeps
  tsc's definite-assignment happy) instead of an IIFE that would capture the
  `return`. A block arm nested in a sub-expression still uses the IIFE and is
  still rejected. Emit + run tests.
- **Inline structural unions in a signature** (M, F3) — ✅ **done.** The type
  emitter renders an inline union of type references (`string | number`,
  `Array<string | number>`) as a TS union, mapping primitives (`bool` ->
  `boolean`). A payload-carrying variant only parses inside a named `type`
  declaration, so that guard is defensive. Emit + `is`-narrowing run test.
- **Async closures** (M, F11) — ✅ **done.** A lambda takes an optional `async`
  prefix, emitting an `async` arrow, so a task thunk can await inside a closure.
  `Expr::Lambda` gained `is_async` (parser snapshots regenerated); parsed,
  formatted, and lowered.
- **Async-call closure inference** (S, F12) — ✅ **done.** An annotated return
  type on an async closure wraps in `Promise<T>` (an async arrow returns a
  Promise), exactly like an async `fn`, so `par.all(array.map(xs, async fn(n) {
  await work(n) }))` type-checks in both the annotated and bare forms and runs.
- **Width-aware formatter** (M, F6) — ✅ **done.** A list of more than two
  elements stays inline when its rendered form fits the print width (100 columns)
  from the current column, otherwise it goes one-per-line with a trailing comma;
  one or two elements are always inline. `leaf("body.type", Equals, "push")` now
  stays on one line. The decision is a pure function of content and column, so the
  layout still round-trips and is idempotent. Little snapshot churn (formatter
  output is not snapshotted; two formatting-shape tests updated).

### 0.1.35 — Landed on main · Interop & concurrency (the design-heavy set)

**Status: all three on `main` (pending an npm publish).** The last four findings
closed, so all sixteen from the hookrelay build are now fixed. Interop is
first-class in both directions, and bounded concurrency is a one-liner.

- **First-class local-`.ts` interop — Option A** (L, F8 + F16, touches D15) — ✅
  **done.** A reserved import prefix `extern/<name>` imports a hand-written `.ts`
  under `<src>/extern/`. The build stages `<src>/extern/**` into the output, the
  emitter emits a relative specifier for it (like a sibling module), and the
  stale-output prune pass skips `extern/` so a rebuild never deletes it (it used
  to remove any output `.ts` it did not itself emit — F16). `tsc` type-checks the
  extern with the Glyph code, so a wrong argument is a real error mapped to
  `.glyph` source. Keeps the no-relative-import rule (D15) intact for Glyph source
  and is greppable by `import extern/`. Resolver-free (an unknown-module import was
  already permitted); emitter + `build.rs` + `runtime.rs` + a D15 spec note and an
  external-imports guide section. Integration test covers import, type
  enforcement, and rebuild preservation.
- **Bounded concurrency in `std/task`** (M, F13) — ✅ **done.** `task.pool(limit,
  tasks)` runs the thunks with at most `limit` in flight and joins the results in
  order (fail-fast like `all`); a worker pool pulls from a shared index, so a fast
  task starts the next immediately rather than waiting for a batch boundary. A run
  test tracks the peak in-flight count through a store and asserts the pool never
  exceeds the limit.
- **Richer node shims for extern** (M, F14) — ✅ **done.** The bundled `http` shim
  now declares `req.on("error")`, `server.listen(port, callback?)`,
  `server.close(callback?)`, and optional `writeHead`/`end` args, matching
  `@types/node`'s shapes. A realistic extern http server (stream reads, an error
  handler, `listen` with a callback) type-checks against the shim with nothing
  installed; installed `@types/node` still supplies the full surface (its crypto
  conflict was fixed in 0.1.33, F15). Integration test plus a no-regression check
  on the `std/http` server tests.

*Sequencing:* F12 depends on F11; F16 pairs with F8; F14 is unblocked by F15. Risk
rises across the three — 0.1.33 additive, 0.1.34 core-compiler but self-contained
(expected snapshot churn), 0.1.35 touches the resolver and one spec decision. Per
repo convention each shipped item also updates its docs, adds a `web/answers/`
Q&A, and marks its finding resolved in the same change.

## minesweeper dogfood trip — a terminal app, no framework

The improve-glyph loop applied to a plain program: Minesweeper in the terminal
(`examples/apps/minesweeper.glyph`). No npm dependency, no server, no JSX. The
point was to find what an ordinary developer hits writing ordinary code, and what
they hit is a formatter that quietly rewrites their source, a stdlib missing the
three string functions any grid renderer needs, and an array index that lies.

The app ships in the tree as the evidence. Its first finding is released as
0.1.38; the rest are listed below with their effort and the two decisions they
wait on. The Next marker has moved on to the expense-CLI trip below.

### 0.1.38 — Shipped · The formatter keeps a comment where you wrote it

`glyph fmt` flushed pending `//` comments at declaration and statement
granularity only. A comment written inside a record body, a union variant list,
an array or object literal, a call argument list, or above a match arm stayed
pending until the next declaration or statement and was re-emitted there. Three
distinct corruptions came out of one format pass on a nine-line file: a match-arm
comment moved past the code it documented to the end of the function body, an
array-element comment escaped its `const` and landed above the next `type` where
it read as that type's documentation, and a record-field comment ended up
orphaned at end of file. Exit 0, no warning, `tsc` passes, and the result is a
fixed point, so `glyph fmt --check` in CI passes on the corrupted file.

The fix makes `delimited` comment-aware: it takes the construct's closing offset
and each item's start offset, an interior comment vetoes the inline form at any
element count and any width, and comments are flushed above the item that
followed them in source with a trailing drain before the closing delimiter. The
constructs that do not route through `delimited` (match arms, union variants,
interface members) flush directly. The veto is decided from spans before the
inline candidate is rendered: the candidate goes into a buffer that is discarded
while the comment cursor is shared, so a flush inside it would delete the comment
rather than move it.

A record that would collapse to `{ a: int, b: int }` now stays expanded when it
holds an interior comment. That is the same rule `lambda_block` already applied
to a body, and it is what diff stability wants anyway. Across the whole
`examples/` tree exactly one file's output changes, and it changes to match what
its author originally wrote.

Verifiability first: D14 leaves `//` as the only way to document a record field
or a match arm, so a formatter that reattaches one to the next declaration makes
the file assert something false about itself and gives the author no way to
comply. Diff stability second: editing one type used to produce a diff on an
unrelated declaration further down the file.

The edge that remains: every comment is still emitted on its own line above the
item that follows it, so a comment written at the end of a code line
(`w: int, // width in cells`) moves down to the line above the next item rather
than staying on the line it annotated. It no longer crosses a declaration
boundary or changes what it sits under, but it does move. Keeping a trailing
comment trailing needs the printer to track a comment's column and the item it
shares a line with, which is a separate change. Ten formatter tests cover the
five reported positions, params and call arguments, trailing comments before a
closing delimiter, the empty construct, the inline veto and its negative case,
and a guard that every comment appears exactly once through a nested construct.
Spec: D14 records the guarantee.

### Still open from this trip

- **`?` in an expression-form match arm** (S). `?` is rejected in an arm whose
  body is a single expression, because one call site in the emitter
  (`glyph-emit/src/lib.rs:3010`) uses `self.expr` where every other statement
  position uses `self.emit_value`. A block-form arm and a `=> return Ok(f(x)?)`
  one-liner both compile today, so this is a missed call site, not a design.
- **Value-position `match` cannot host block arms** (M). A `match` used as a
  sub-expression lowers to an IIFE that rejects block arms
  (`glyph-emit/src/lib.rs:3519-3530`). In that position `?` has no workaround.
  Structural, and separate from the call site above.
- **`std/string`: `repeat`, `pad_start`, `pad_end`** (S). Three wrappers in
  `runtime/std/string.ts` plus three names in the resolver seed. Every program
  that renders a grid or aligned columns needs them.
- **Unknown stdlib namespace member is a raw TS2339** (M). `import std/string
  { repeat }` gives a clean E0105 because `verify_imports` checks named imports
  against the seed; `string.repeat(...)` leaks a `tsc` error with an absolute
  build path, because nothing checks member access against the same seed the
  resolver already holds. Same typo, two experiences, decided by import style.
  The absolute-path leak in TS error remapping is a second, separable defect.
- **`glyph check <file>`** (S). `build.rs:100-103` rejects a non-directory
  source and there is no `check` subcommand; the only non-executing door into
  type checking is running the program.
- **Formatter, layout only** (S each, deliberately not bundled with the
  correctness fix above): a one-statement match-arm body is always exploded to
  three lines because the parser wraps it in a synthetic block and the formatter
  prints every block multi-line; and `items.len() <= INLINE_MAX` short-circuits
  the width test, flattening every two-argument `array.map(xs, fn(...) {...})`.
- **`llms.txt`** (XS). It does not say annotations are canonically sorted (D27),
  and it does not mention the expression-arm `?` restriction while that exists.

### Two forks for the orchestrator

Neither is decidable inside an iteration; both need a decision before a line is
written.

**A numeric range for `for`.** D21 is settled and not reopening, but nothing in
the stdlib produces the iterable a counted loop needs, so the most common bounded
loop in any program cannot use the keyword D21 built for bounded loops. That
costs greppability: `grep -n "^\s*for "` is supposed to audit every iteration
site, and instead half of them are hand-rolled `loop`/`match`/`break` counters.
The options:

- `array.range(start, end) -> Array<int>` — smallest, purely additive, allocates.
- A lazy iterable protocol `for` understands — no allocation, touches the
  emitter's `for...of` lowering, and defines a protocol we then have to keep.
- `0..n` range syntax — a grammar change and a new D-decision.

**What `xs[i]` means.** The checker types `Expr::Index` as `Ty::Unknown`
(`glyph-typechecker/src/assign.rs:501-510`), the generated tsconfig omits
`noUncheckedIndexedAccess` (`glyph-cli/src/runtime.rs:174-194`), and there is no
`array.get`. So `cells[999]` type-checks clean, passes `tsc --strict`, and hands
back `undefined` where the compiler claimed `Cell`. That is the verifiability
pillar as literally written ("anything the type system claims must be true at
runtime"), and `array.find -> Option<T>` and `record.get -> Option<V>` already
set the policy this hole sits outside of. The options:

- `array.get(xs, i) -> Option<T>` alone — additive and cheap, but `xs[i]` still
  lies and a helper nobody is forced to use does not restore a guarantee.
- `array.get` **plus** `noUncheckedIndexedAccess` **plus** a Glyph-level
  diagnostic **plus** the migration — closes it, and turns every existing `xs[i]`
  in `examples/corpus`, the Glyph-source stdlib, hookrelay, and the guide into an
  error. Without the Glyph-level diagnostic the user sees a raw TS2532, which is
  the same leaked-backend-error defect as the TS2339 item above.

Pre-1.0 with few users is the cheapest this will ever be, so it should not be
parked; it should be scheduled as a designed release once the fork is resolved.

## expense-CLI dogfood trip — a ledger, a parser, and money

The loop pointed at another ordinary program: an expense-report CLI
(`examples/apps/expenses.glyph`) that reads a CSV ledger, validates every row,
and prints a per-category report with exact `Decimal` money. Fifteen findings
came out of it. Thirteen are "Glyph made me type more". Two are a different
category: the standard library returned a value where its own reference docs
promised a rejection, and the two-binding `for` picks the wrong lowering when you
iterate a call's result. The Next marker has moved on to the text-adventure trip
below.

### 0.1.39 — Shipped · `time.parse_iso` parses ISO-8601, and nothing else

`parse_iso` was a bare `Date.parse`, which meant it accepted whatever the host
engine's date heuristics accepted. `"January 5 2026"` parsed. `"2026-1-3"`
parsed, in the *host's local timezone*, so on a machine west of UTC the
`time.year`/`month`/`day` accessors (documented UTC in the same file) reported
the previous day, and near a month boundary the previous month. `"2026-02-31"`
returned `Some` for March 3, because ECMAScript reports calendar rollover as
success and no `NaN` check can see it. `docs/reference/stdlib.md` documented all
three as `None`.

The fix is three stages, each catching what the others miss. An anchored ISO-8601
regex runs before `Date.parse` ever sees the string, accepting exactly two
shapes: a bare `YYYY-MM-DD`, and `YYYY-MM-DDTHH:MM(:SS)?(.sss)?` followed by an
explicit `Z`, `+HH:MM`, or `-HH:MM`. Then the existing `Date.parse` + `NaN`
check, which is still what rejects `"2026-13-01"`. Then an arithmetic check of
the year/month/day triple from the capture groups, with real month lengths and
leap years.

The asymmetry in the accepted set is deliberate. A bare date is UTC midnight per
the ECMAScript grammar, so it is safe. An offset-less datetime
(`"2026-01-03T10:00"`) is *local* time per the same grammar, so it is rejected
rather than silently reinterpreted: accepting it would mean the same string names
a different calendar day depending on where the process runs, which is exactly
the guarantee the file header makes and that the calendar accessors depend on.
The calendar validation is arithmetic rather than a `format_iso` round-trip
because an offset-bearing input can legitimately land on a different UTC date
than the one written (`"2026-02-31T00:00-05:00"`), so a round-trip would need a
special case while the arithmetic check is uniform.

Verifiability, pillar one: a boundary validator that fails open while its docs
promise it fails closed is worse than no validator, because an agent that reads
the docs and trusts them writes a broken program. The app had already noticed and
hand-rolled the guard at the call site (a regex shape check plus a `format_iso`
round-trip); when an app writes a correctness guard around a stdlib primitive,
the guard belongs in the primitive.

No lenient variant sits beside it. Leaving `parse_iso` loose and adding
`parse_iso_strict` would ship the wrong default under the more discoverable name.
Adding a `parse_loose` later stays forward-compatible; tightening later would
not. This is a behavior change and it is a correctness fix, not a breaking one:
the accepted-but-now-rejected inputs contradicted both the function's name and
its documentation.

Shipped with it, two documentation corrections that were wrong in the strict
direction (the kind that makes people write worse code): the two-binding
`for i, x in xs` form is fully implemented and appeared in no file a user or an
agent reads, so D21, the agent bootstrap, and the cookbook now document it (an
array binds a numeric index, a record binds a string key); and D22 claimed
interpolation interiors were restricted to literals, identifier reads, member
access, `?`, and parens, when the parser has always handed the interior to the
full expression grammar. Calls in `${...}` work. The one real restriction is a
nested string literal, which is a lexer artifact rather than a rule.

### Still open from this trip

- **A two-binding `for` over a call's result binds a string index** (S/M).
  Documenting `for i, x in xs` (above) meant the app could finally drop its
  hand-rolled line counter, and the natural spelling miscompiles.
  `iter_is_array` (`glyph-emit/src/lib.rs:1783`) asks the type map for the
  iterand's type and falls back to the record lowering when it is not a known
  `Array`, so `for i, raw in array.slice(lines, 1)` emits
  `Object.entries(...)` and `i` is the string `"0"`. This is the silent-green
  class: `glyph build` is clean, `tsc --strict` passes (`"0" + 1` is legal
  TypeScript), and the program prints `01:` where it should print `1:`. An
  inferred `let` does not rescue it; only an explicit `Array<T>` annotation on
  the binding does, which is what the app does today. The fix is either
  recording a call expression's result type at its span or defaulting the
  unknown case to the array lowering; both change what an unannotated iterand
  means, so it wants a decision rather than a patch.
- **`std/string`: `slice`; `std/array`: `fold`** (S). The `fold` gap costs a
  pillar, not just keystrokes: with no fold, every accumulation is a `mut` in a
  loop, which dilutes what `grep -n "^\s*mut "` is supposed to find (D5). Pairs
  with the `repeat`/`pad_start`/`pad_end` item from the minesweeper trip.
- **No mut-teaching diagnostic for a bare `x = e`** (S). E0006 exists to teach
  the `if` ban; D5 is the second-most-broken rule for a newcomer and gets a parse
  error that names a token instead of the rule.
- **The clap binding has no `allow_hyphen_values`** (S). A negative number as an
  option value (`--amount -12.50`) cannot be expressed.
- **`glyph check <file>`, a counted range for `for`, and decimal literals** are
  the same three forks the minesweeper trip left open; they are recorded above
  and are not re-litigated here.

## text-adventure dogfood trip — the command developers run all day

The loop pointed at `examples/apps/adventure.glyph`: rooms, an inventory, a
command parser, no dependencies. Thirteen findings came out of it. Twelve are the
compiler not knowing something or the stdlib not having something, and a user can
read an error about those. One is the compiler knowing the answer and hiding it,
on the command an agent runs in a loop. That one is fixed here. The Next marker
has moved on to the scheduler trip below.

### 0.1.40 — Shipped · `glyph run` reports what `glyph build` reports

`glyph run solo.glyph` printed the program's output and exited 0. `glyph build .`
on the identical tree printed `E0204` on a sibling module and `E0106` on
`solo.glyph` itself, and exited 1. Same compiler, same sources, seconds apart:
`run_file` computed a full `BuildReport` and read `report.emitted` out of it,
leaving `diagnostics`, `structured`, and `error_count` unread on the success
path. The eaten warning was on the entry file the user named, so the documented
"sibling modules are best-effort at run time" stance does not cover it. Nothing
decided to suppress it; it was never wired up.

`run_file` now returns a `RunResult`: the outcome plus every diagnostic the build
computed. `glyph run` prints them before dispatching on the outcome, so they
appear on the successful path too, and follows the program's output with a
`glyph run: N error(s), M warning(s) in the source tree` summary line in the
shape `glyph build` uses. Rendering stays in the CLI; `run_file` still never
prints.

The half that makes a naive fix worse is the run cache. A warm cache skips the
build entirely, so there is no report to read and run #2 of unchanged source
would have printed nothing after run #1 printed the warning: an intermittent
silence reads as a warning that went away, which is worse than a consistent one.
A build now writes its diagnostics to `.glyph-diagnostics.json` in the staging
directory, so the sidecar moves into the fingerprint-keyed cache dir with the
rest of the output, and a cache hit reads them back. A hit whose sidecar is
missing, unparseable, or rendered for a different color setting counts as a miss
and rebuilds, rather than reporting a tree it never checked. The format is the
same `Diagnostic` the `--json` output uses, now round-tripping.

Verifiability: the compiler is the source of truth, and the source of truth has
to be reachable from the command people actually type. `docs/dogfooding-gaps.md`
names "silent green" as the class to close before 1.0 and marks G9 and G10 fixed;
the class survived one layer up because those fixes were verified through
`glyph build`.

Exit codes are unchanged. On the `Ran` path `glyph run` still exits with the
program's own exit code, including when a sibling module failed to compile.

### Still open from this trip

- **Should a sibling error make `glyph run` exit non-zero?** (decision) Today it
  does not: a module that failed to compile is simply unavailable to import, and
  the program runs. With the diagnostics now reported, the question is whether
  telling is enough or whether the exit code should follow. Changing it makes
  `glyph run` fail on trees that run fine today, which is a real break for anyone
  running one entry point out of a directory with work-in-progress siblings.
  Leaving it means a red diagnostic can still accompany a green exit. Not decided
  here.
- **Unchecked member access and call arguments against `Ty::Unknown`**
  (architecture decision, then M/L). `s.slice(0, 1)` on a `string`, a misspelled
  `xs.pusj(x)`, and wrong-arity calls into a stdlib namespace all compile,
  because the receiver's type is `Ty::Unknown` and the checker has nothing to
  check against. The manifesto promises no `any`; this is one, spelled `Unknown`,
  load-bearing at exactly the boundary where the promise is made, and it is one
  defect wearing four hats rather than four bugs. The fork: model the stdlib from
  its own `.d.ts` sources (the Q21/Q40 direction, which also makes `gen dts` the
  single mechanism for every boundary) versus keep growing the hand-written
  `stdlib_fn_ty` table (cheaper per name, permanently behind the runtime). The
  first is the larger change and the one that stops the table from drifting; the
  second ships value next week. Needs the orchestrator's call before any code.
- **`std/string`: `slice`, `lines`; `std/array`: `fold`, `compare`** (S). The
  same stdlib-breadth item the expense-CLI trip left open, with two more names on
  it. `fold` still costs a pillar rather than keystrokes.
- **`EmitError::help()` returns one constant string for every unsupported
  construct** (S). A diagnostic that says the same thing whatever you did is a
  shrug, not help. Found while checking a report claim that turned out to be
  wrong for a different reason (block-bodied match arms compile fine).

## scheduler dogfood trip — the descriptors were there, nobody looked them up

The loop pointed at a scheduling app: time ranges, blocks, a JSON boundary, types
split across modules. The headline finding was that `type Instant = string where
value.length > 3` validated at `Instant.parse("no")` and nowhere else:
`Block.parse({ start: "no" })` returned Ok. Probing one step further found the
same hole on a second axis. This trip carries the Next marker.

### 0.1.41 — Shipped · Every descriptor the emitter emits, the emitter can find

Glyph's promise is that a type carries a runtime descriptor, so data crossing a
boundary is checked against what the type declares. The descriptors were emitted.
The resolver that decides whether to *call* one recognized two kinds: a
module-local non-generic record, and a module-local non-generic tagged union.
Everything else fell to a `!== undefined` presence check, which is not a check of
anything except that a key exists.

Two kinds fell through. A D39 refined alias in field position dropped its `where`
predicate, so `Block.parse({ start: "no" })` returned Ok on a value
`Instant.parse` rejects. And a field typed by a record **imported from another
project module** was validated by presence alone, which is every non-generic
cross-module composition in every multi-file Glyph program: `Outer.parse({ i:
42 })` returned Ok with `i` typed by an imported record whose descriptor was
emitted, exported, and already imported as a value on line 5 of the same file.
Both built clean and passed `tsc --strict`.

The fix is in the resolver, not at the symptom. `has_descriptor` now also
accepts a refined alias, and resolves an imported name through its `ImportNamed`
symbol and a project-wide descriptor registry — the same shape
`generic_descriptor_arity` had been using for imported *generic* descriptors
since 0.1.23. The hard version was built first and the easy one never was. Four
call sites read the resolver, so one change closes the record-field drop, the
`Array<Refined>` element drop, the `Option<Refined>` payload drop, the
union-variant payload drop, the synthesized checker passed to a generic
descriptor, `is T` narrowing, and `json.parse<T>` for both refined and imported
types. The namespaced form (`import types`, then a field typed `types.Inner`) is
covered too; it was previously not handled at all.

Verifiability. No syntax changed and nothing relaxed. The only behavior change is
that a boundary which returned Ok on unvalidated data now returns Err, so a
program relying on the old floor will start failing at the boundary — which is
the point.

One structural risk is pinned by a test: the emitted import is a value import
(`import { Inner } from "./types"`) even when the type is used only in type
position, and `Inner.is` depends on that. An "emit `import type` for type-only
uses" optimization would erase the binding with `tsc` still clean, so a
regression test asserts the value import.

### Still open from this trip

- **A descriptor's `.parse` result is not assignable to `Result`** (M). Confirmed
  as TS2322; the cookbook recipes that thread `T.parse(x)` into a function
  returning `Result<T, E>` do not compile. Separate from the resolution fix.
- **`glyph build` prints "no diagnostics" above its own `tsc` errors** (S). The
  Glyph-stage summary is printed before the TypeScript stage runs, so a red build
  is introduced by a green line.
- **The namespaced form is handled in field position only** (S). A field typed
  `types.Inner` now calls `types.Inner.is`, but `match v { is types.Inner => … }`
  and `json.parse<types.Inner>(s)` still take the two-segment path as unresolved.
  Both are the same registry lookup through `namespace_module_path` that the
  field check already does; nothing blocks it, it was simply out of the scope of
  this fix. Recorded rather than left silent, because an undocumented presence
  floor is how this class survived to 0.1.40.
- **A descriptor for an imported type from a non-project module** stays on the
  presence floor. `glyph gen dts` materializes those with real descriptors; a
  bare `.d.ts` type still has nothing to call. Unchanged by this fix, and the
  registry is deliberately the authority so no bogus `X.is` is emitted for a type
  that has none.

## Road to 1.0

**Status: the committed plan, from the third review.** The review (docs and code
grounded) credited the toolchain as real and tasteful but found that a 1.0 is
gated on a question the project has not decided: can a working engineer use their
existing npm dependencies without writing a hand-written adapter per library? The
one-line diagnosis: Glyph is safe on code it owns and leaky at the seam with npm,
and real projects are all seam. The road below closes that seam, decides and
builds interop, proves it on real apps, and settles the productivity claim.
Everything here traces to a specific finding with file evidence.

This track's boundary items: 0.1.13 shipped four of the six (node builtins
typecheck out of the box landed; imported-`.d.ts` validate-or-diagnose is still
open below), and 0.1.14 made the interop decision. The committed **Next** marker
now sits on the hookrelay dogfood trip (0.1.33 → 0.1.35, above); this interop
track continues in parallel, and F8/F14/F15 there are concrete new instances of
its open items.

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

**Stability enforcement (the code half of the 1.0 stability enablers) — in
place.** A spec conformance corpus (`glyph-emit/tests/conformance/`, one program
per feature keyed to its D-decision) pins the exact emitted TypeScript as a
committed snapshot, so a change to what a feature *means* fails the build and a
human signs off on the diff. This is the mechanism behind the "no silent behavior
changes" promise in `docs/stability.md`. The other half, `glyph fmt --migrate`,
is deliberately deferred: there is no breaking syntax change to migrate yet, and
a migration engine is best built with the first real migration rather than as
untested scaffolding.

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
- **Node builtins typecheck out of the box** (M). ✅ **Done.** The bundled Node
  shim now declares the common builtins (`fs`, `http`, `path`, `os`, `crypto`,
  `url`, plus the `process` global) under their **bare** names, which is what a
  user's `import fs` emits, so a program using node builtins type-checks with
  nothing installed. When the project ships `@types/node`, the build detects it,
  loads its full surface (`types: ["node"]` with an explicit `typeRoots` at the
  project's `@types`, since the out dir sits outside the project), and skips the
  bundled shim so there is no duplicate `declare module "fs"` conflict, so an API
  the shim does not cover (`os.uptime()`) type-checks the moment `@types/node` is
  present. The chosen design is bundled-shim-first (out of the box) with
  `@types/node` as the completeness escape, over `@types/node`-only (needs an
  install) or specifier rewriting (would break the example `.types` stubs).
  Hermetic + tsc-level tests; the examples still build unchanged.
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
  committed Glyph types with runtime descriptors. `glyph gen dts api-types --out
  src/types` gives you `Customer.parse(webhookBody)` that validates the wire
  value's *structure* deeply (nested records, arrays, optional fields all the way
  down); leaf values are still shallow (an `integer` field checks as a number,
  a string enum as a `string`). The generated file records its command so `glyph
  regen` refreshes it on a dependency bump. Proven end to end (a fake installed
  SDK materializes, its descriptor validates at build and runtime, regen re-runs
  it) with hermetic unit tests over the resolution and helpful errors.
  *Scope (per Linus review 04, now widened):* the `.d.ts` reader
  (`ts-to-schema.mjs`) walks a `declare namespace` tree (keying types by their
  qualified name and resolving bare cross-references through the scope) and
  degrades a generic parameter to `unknown`, so a bundled single-file SDK `.d.ts`
  materializes real types instead of nothing. Still not followed: cross-file
  re-exports (`export … from "./other"`), tracked below; a bundled `.d.ts` (the
  common shape) is fully walked. This keeps the verifiability wedge at the seam
  **for the types you materialize** (not for every installed package: an
  un-materialized package's outputs are type-checked against its `.d.ts` but not
  runtime-validated, which is the Option-2 trust boundary, so the "wedge at the
  npm seam" holds only where you opt in). No new grammar or non-committed build
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

**Interop code fixes from Linus review 04** (the verified gaps behind the honesty
edits; each is real engineering, not a doc tweak):

- **Deeper `.d.ts` materialization** (L). ✅ **Done.** `ts-to-schema.mjs` walks
  `declare namespace` trees (two-pass, qualified names, scope-aware reference
  resolution); follows cross-file re-exports (the entry file plus every `.d.ts`
  reachable through a relative `import`/`export … from`: an `index` barrel,
  `export *`, transitive imports, with cross-file references resolved); and
  materializes generics **first-class** (`interface Page<T>` → `type Page<T> = {
  items: Array<T> }`, keeping the parameter, and a `Page<User>` instantiation
  keeps its argument), so a generic type gets a real checker-threaded descriptor
  that validates deeply through nested generic fields (`UserList.parse` rejects a
  `Page<User>` whose item isn't a valid `User`). All regression-tested. A bare
  specifier is not followed (it points at another package), and cross-file
  following is best-effort on the TypeScript 7 native path. *Done:* a real
  multi-file, namespaced, generic SDK materializes usable descriptor-bearing
  types.
- **`gen dts` output integrity** (S, from Linus review 05). ✅ **Done.** Review 05
  found the materializer could emit a dangling `$ref` (an aliased re-export
  `import { X as Y }`, or an `export * as ns`) or, worse, silently bind a
  reference to the wrong shape when a type name collides across two reachable
  files, all with a clean exit. `gen` now **flags both at gen time**: a `$ref`
  whose target was not materialized is a note (naming the unresolved type), and a
  cross-file name collision is a note from the reader (first-wins, may bind the
  wrong shape). So the tool no longer produces a wrong-typed validator silently.
- **Import-binding tracking in the `.d.ts` reader** (M). ✅ **Done.** The reader
  now *follows* the re-export shapes it used to only flag: a per-file binding map
  resolves an aliased import (`import { Widget as W }` → `W` is `Widget`), a
  re-export rename (`export { X as Y } from`), and a namespace alias
  (`import * as ns` / `export * as ns` → `ns.Type` is `Type`), so those references
  materialize instead of dangling. Verified end to end on all three shapes. The
  one case it still cannot make safe (a genuine same-name collision across files)
  keeps its note.
- **Subpath-`exports` resolution** (M). ✅ **Verified working, no fix needed.**
  Review 04 *inferred* (did not run) that the Phase 1 `"*"` wildcard would bypass a
  package's `exports` map and fail to resolve a subpath like `@scope/pkg/sub`.
  Tested empirically: it resolves correctly and the subpath's real types are
  enforced (a wrong-typed call is a genuine `tsc` error, not `any`), because TS
  `moduleResolution: bundler` falls back to normal `exports`-aware node resolution
  when the wildcard path substitution misses a physical file. Both a subpath
  (`pkg/sub`) and the root `exports` (`.`) resolve. The incorrect honesty caveat
  added for this has been removed from the site. A cautionary tale for trusting an
  unverified review claim.
- **Leaf-value validation in generated descriptors** (M). ✅ **Done (string enums
  and integers); string formats remain.** String enums materialize as
  **string-literal union types** (D30): `gen dts`/`openapi`/`zod` emit `type Tier
  = "free" | "pro"`, `tsc` enforces the narrowed type, the descriptor checks
  **membership** at the boundary (`"enterprise"` is rejected), and a `match` over
  the union is **exhaustive without an `else`** (a missing literal is E0200).
  Integers materialize as **`int`** (D31): a JSON-Schema `integer` field emits as
  TS `number` with a runtime `Number.isInteger` check, so a wire `3.5` fails an
  `int` field's `.parse` where a `number` field would accept it. Still open (lower
  value): string formats (uuid, email, date-time) are unvalidated. *Done:* a
  generated descriptor rejects a wrong-*valued* string-enum or non-integer leaf,
  not just a wrong-typed one. *Follow-up (post-0.1.15 smoke test):* a field typed
  by a *named alias* to a literal union or `int` (`type Tier = "free" | "pro"; { t:
  Tier }`) used to fall back to a presence check; the descriptor now resolves a
  non-record alias to its leaf, so the aliased field validates identically to the
  inline form. On `main` for the next release.
- **`@open` policy for materialized wire records** (S, decision). ✅ **Done.**
  Decided: keep records strict-by-default across the language (safe by default,
  the verifiability pillar), and have codegen emit the existing `@open` (D27)
  marker on generated wire types, since a `.d.ts` and JSON Schema tolerate extra
  properties by default and a forward-compatible API response that adds a field
  must not fail `T.parse`. `gen dts`/`gen openapi`/`gen zod` now emit `@open` above
  a generated record unless the source schema closes the world
  (`additionalProperties: false`), which stays strict. The marker is at the
  declaration site and greppable, identical to a hand-written record's, so there
  is no provenance-dependent split in what a record means. Verified end to end: a
  materialized type tolerates an added field but still rejects a wrong-typed
  declared field; unit-tested for both the open default and the closed case. (The
  rejected alternative was flipping the global default to open with an `exact`
  marker, which would have retroactively weakened every existing record and the
  stdlib.)
- **Node-shim / `@types/node` consistency** (S). A build green against the bundled
  shim can flip red (or vice versa) when `@types/node` is later installed, because
  the surfaces differ. Minor, but worth a note in the docs and a thought about
  narrowing the shim's signatures to match `@types/node` where they diverge.

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
  - **JSX prop spread** (M). ✅ **Done (Phase 3, first primitive).** `<input
    {...register("email")} class="field" />` parses to a `JsxAttr::Spread`, lowers
    to an object spread inside the `createElement` props (`{ ...register("email"),
    className: "field" }`), round-trips through the formatter, and is resolved and
    typechecked like any expression. Proven under `tsc --strict` against
    react-hook-form-shaped types, so the canonical form idiom works end to end.
    Parser, emit, and formatter tests cover it.
  - **Interop escape hatch (type-level)** (M). ✅ **Done (D29).** `extern_ts("<raw
    ts>")` in type position emits its string verbatim as the TypeScript type, so
    an idiom Glyph's grammar does not spell is still nameable, most importantly a
    value-derived `type User = extern_ts("z.infer<typeof user_schema>")`. It is
    contained: `tsc` checks the raw type and every use of it (a bogus member is a
    real error mapped to Glyph source), and an `extern_ts` type is opaque to
    Glyph's own checker (no descriptor), exactly like an imported `.d.ts` type.
    Recognized only in the `extern_ts("...")` shape, so it never shadows a user
    name, and every escape is greppable. Proven end to end with real zod
    (`z.infer` typechecks and runs) plus parser/emit/formatter tests. This is the
    scoped escape hatch the interop plan called for; no library ever forces a
    hand-written adapter file.
  - **Interop escape hatch (expression-level)** (M). ✅ **Done (D29).** The
    symmetric completion: `extern_ts("Date.now()")` in expression position emits
    its string verbatim (parenthesized) and is typed `unknown`, so a
    grammar-hostile runtime idiom stays reachable and, being `unknown`, must be
    narrowed or validated before use. Same containment as the type form (`tsc`
    checks the raw TS) and recognized only in the `extern_ts("...")` shape, so a
    plain identifier `extern_ts` is unaffected. Proven end to end (emits
    `(Date.now())`, typechecks, runs, narrows through a `match`), with parser,
    emit, and formatter tests.
  - **First-class value-derived types** (M). ✅ **Done (D32, from Linus review 05,
    the #1 remaining 1.0-interop gate).** A `typeof value` type query makes the
    canonical idiom first-class: `type User = z.infer<typeof user_schema>` needs no
    `extern_ts` string. `typeof <path>` is the type of a value binding, its operand
    resolved as a real value reference (a typo is E0103), and `z.infer<...>` is an
    ordinary member-generic type that already parsed. It emits verbatim, `tsc`
    reduces it (`u.name` is a `string`), and the type is opaque to Glyph (no
    descriptor, like an imported `.d.ts`); validation comes from the schema's own
    `parse`, which is how zod works. Proven end to end with real zod (builds, passes
    `tsc --strict`, runs, and a bogus operand is an unresolved-name error), with
    parser, emit, and formatter tests. Phase 3 is complete for v1: prop spread, the
    escape hatch, and first-class value-derived types.

### 0.1.16 — Shipped · Language-design completeness

**Status: shipped.** A pass over the language against what a general-purpose
statically typed language is expected to carry surfaced a set of gaps closeable
within the current "looks almost like TypeScript" stance (as opposed to the ones
that are consequences of running on the JS runtime, now *declared* rather than
built). All five landed together over a shared keyword table, with the two
design forks resolved on manifesto grounds; none reopened the abandoned
annotation-rich direction. Spec decisions D33–D35.

- **Structural interfaces + generic bounds** (L) — ✅ **done (D34).** `interface
  Name { fn m(p: P) -> R  field: T }` declares a structural interface, usable as
  an ordinary type and, chiefly, as a generic bound (`fn label<T: Named>(x: T)`),
  which lowers to a TS `extends` clause `tsc` enforces. **Fork resolved:**
  structural `interface` (not a nominal `trait`+`impl`), because Glyph's records
  are already structural and a second nominal identity model would fight the
  family; it emits an `export interface` and carries no runtime descriptor, like
  an imported `.d.ts` type.
- **Module visibility** (M) — ✅ **done (D33).** Declarations are module-private
  by default and export only when marked `pub`. A private name is absent from the
  export surface, so importing it from another module is E0105 at the import
  site. `fn main` is always exported (the runner imports it), so single-file
  programs are unchanged. **Fork resolved:** private-by-default + `pub` (over
  public-default + `priv`), the pre-1.0 window to fix the default before it
  freezes: safe-by-default and the public API is `grep '^pub'`. A private
  record's descriptor and a union's constructors inherit the type's visibility.
- **Digit separators in numeric literals** (S) — ✅ **done.** A valid `1_000_000`
  / `0.000_1` already lexed and emitted; the gap was that `1_`, `1__0`, `1_.5`
  leaked a raw `tsc` TS6188/6189. The lexer now enforces the D13 rule (every `_`
  between two digits) with a Glyph-level MalformedNumber (E0001).
- **Deterministic cleanup: `defer`** (M) — ✅ **done (D35).** `defer <expr>` runs
  on every exit path, lowered to `try { rest } finally { expr }` around the
  statements that follow it (the tail return stays inside the `try`); multiple
  defers nest last-in-first-out. **Fork resolved:** `defer` (over a TS `using`
  binding), because it works with the existing stdlib and any cleanup expression
  with no `Symbol.dispose` retrofit or `lib` change, and is greppable. Composes
  with `owned` handles.
- **Structured-concurrency helpers: `std/task`** (M/L) — ✅ **done.** `import
  std/task` gives `all` (concurrent join, fail-fast), `race`, and `all_settled`
  (one outcome per task) over the promise model, plus a `Settled<T>` type. Honest
  scope: JS can't force-cancel a running task, so a failure in `all` abandons its
  siblings' results rather than halting them; the module documents threading an
  AbortSignal for cooperative cancellation.
- **Declared, not built (honesty)** (S) — ✅ **done.** The spec now owns the
  language-level properties that are true by inheritance from the JS runtime
  rather than by Glyph machinery: evaluation order, value-vs-reference, equality
  (`==` is `===`, no overload), and the single-threaded/GC concurrency-memory
  model. Manual memory, a non-JS GC, and function-color elimination are named as
  consequences of the transpile-to-TS target, out of scope by design, not gaps.
  Backward-compat guarantees and a formal editions mechanism land with the 1.0
  stability commitment.

**Follow-up (polish):** the E0105 diagnostic for a private import says "not
exported by M"; distinguishing "private (mark it `pub`)" from "no such name"
needs the export surface to carry both the public and full name sets. Tracked in
the rolling polish lane.

### 0.1.17 — Shipped · Stdlib breadth, tooling, and docs

**Status: shipped.** A breadth pass closing the gaps a real program hits early,
none of which needed new language surface.

- **Four new stdlib modules** — ✅ `std/regex` (stateless regular expressions),
  `std/set` (a value-semantics hash set; maps stay `Record<K, V>`), `std/path`
  (cross-platform paths over node's `path`), and `std/crypto` (sha256/512, HMAC,
  UUID, random hex over node's `crypto`).
- **std/time and std/io deepened** — ✅ `time.format_iso`/`parse_iso`,
  `add_days`/`add_hours`, and UTC `year`/`month`/`day`; `io.inspect`/`render` for
  structured value inspection while debugging.
- **`glyph fix`** — ✅ the safe autofixes in place; today it removes an import
  whose every bound name is unused (a partially used named import is left alone).
- **`glyph init --template <cli|web|lib>`** — ✅ scaffold a CLI, an http server,
  or a library; each template compiles through the real pipeline.
- **Diagnostics link to docs** — ✅ every Glyph `E`-code note points at its
  error-codes reference section and the `glyph --explain` command.
- **Guide expansion** — ✅ how-to-think, cookbook, troubleshooting, anti-patterns,
  performance, idioms, and a TypeScript-project migration guide, plus a learning
  path, a stated time-to-productivity, a your-word-to-our-word concept map, an
  edit-this-page path, and a task-tagged recipe list in the agent bootstrap.

### 0.1.18 — Shipped · Governance, distribution, and editor tooling

**Status: shipped.** Breadth across the non-language axes a serious project needs.

- **Project governance and community health** — ✅ the standard OSS surface the
  repo was missing: a Code of Conduct, a security policy with a private reporting
  path, a `GOVERNANCE.md` (the model, decision process, conflict resolution, and a
  real succession plan), a `MAINTAINERS.md` that states the one-maintainer bus
  factor honestly, an RFC process and template, a good-first-issue on-ramp and a
  DCO in CONTRIBUTING, and a release-cadence plus deprecation policy in the
  stability doc.
- **Distribution and deployment guides** — ✅ documents the npm-backed supply
  chain (canonical registry, scopes, semver, audit, provenance, private/mirrored
  registries, immutable versions) and where a Glyph program runs (node,
  containers, serverless, edge), with tree-shaking and reproducible builds.
- **`glyph bench`** — ✅ times every `pub fn bench_*()` in the project and reports
  ns/op, on the JavaScript runtime the program actually runs on.
- **Editor tooling** — ✅ the language server now surfaces the warning-tier lints
  (E0106/E0107/E0108) it previously computed only at build time, adds a
  code-action quick-fix that removes a fully-unused import (the `glyph fix` edit),
  and shows inlay type hints on untyped `let` bindings.

### 0.1.19 — Shipped · Dogfood the stdlib in Glyph (improve-glyph loop, batch 1)

**Status: shipped.** The first batch of the improve-glyph loop: write the
standard library's logic in real Glyph, and fix the compiler the moment it can't
express something (Linus's rule, use it on real code and fix what breaks). Ten
green, test-gated commits: eight pure-Glyph `examples/corpus/` modules proving
the language expresses real logic (set algebra, POSIX paths, list and string
algorithms, an immutable deque, Option/Result combinators, number formatting,
base64), and two compiler gaps the dogfooding surfaced and fixed:

- **Reject TS reserved words as identifiers** (E0109) — using `class`, `switch`,
  `new`, `typeof`, `this`, `var` (etc.) as a declaration, parameter, or binding
  name emitted broken TypeScript. A new resolver `reserved` module rejects them
  in binding position only (object keys, record fields, and member access stay
  valid), the resolver-level fix the emitter's known-gap note called for. With
  `--explain E0109`, the catalogue row, and 6 tests.
- **Negative-number literals as match patterns** — `match n { -1 => ... }` now
  parses, the natural companion to the positive-literal arms, lowering to a
  `case -1:` and counting as one pattern for exhaustiveness.

### 0.1.20 — Shipped · improve-glyph loop batch 2 + bitwise operators

**Status: shipped.** Batch 2 of the dogfood loop (nine more pure-Glyph corpus
modules: a JSON parser and serializer, a pairing heap, SemVer precedence, CSV and
hex codecs, a shell-glob matcher, a duration formatter, a reproducible PRNG),
plus the resolution of the one architecture fork it surfaced:

- **Bitwise operators `& | ^ ~`** (D36) — the loop found it could not write a
  mulberry32 PRNG in pure Glyph because Glyph had no bitwise arithmetic. The
  non-shift operators emit verbatim to TypeScript, number-typed, at JS
  precedence. The shift operators (`<< >> >>>`) were parked here with their
  disambiguation design and landed in a later loop iteration (see below);
  `math.imul` remains exposed for 32-bit multiply.

That an autonomous agent stopped at a genuine language-design fork and escalated
it, rather than inventing a shift syntax, is the loop working as designed.

### 0.1.21 — Shipped · improve-glyph loop batch 3 + cross-module union fix

**Status: shipped.** Batch 3 of the dogfood loop (ten more pure-Glyph corpus
modules: a URL parser, a bignum and rational arithmetic, a UTF-8 codec, graph
algorithms, edit distance, a precedence-climbing calculator, binary heaps,
bit-manipulation and a frequency multiset, several of which exercise the new
bitwise operators from 0.1.20), plus the correctness bug the dogfooding surfaced:

- **Imported record-payload union match** — a `Variant(v)` pattern that binds the
  whole payload emitted `v.value` (a tsc TS2339) when the union was *imported*
  from another module, because an imported-union scrutinee carries no concrete
  type for the emitter and it defaulted to the single-value shape. The build now
  collects a project-wide registry of record-payload variants keyed by
  `(module, variant)`, and the emitter resolves an imported variant to its source
  module through its `ImportNamed` symbol, so the whole `{tag, ...fields}` object
  is bound. This is a real miscompile of valid cross-module code, not just an
  ergonomic gap.

### 0.1.22 — Shipped · improve-glyph loop batch 4 + interfaces as types

**Status: shipped.** Batch 4 of the dogfood loop (ten more pure-Glyph corpus
modules: union-find, an LRU cache, a stack VM, a trie, an ordered-map BST,
interval-set algebra, checksums, descriptive statistics), plus the compiler work
it surfaced:

- **Structural interfaces as ordinary types** (D34 completion) — an `interface`
  worked as a generic bound but not as a plain type (member access and
  assignability were unchecked). The typechecker now expands an interface to its
  record-field set and reuses the structural record comparison at argument and
  return sites, and resolves interface members for member access. Three tests.
- **G21 leading-bracket statement glue — resolved won't-fix.** The loop escalated
  that a tail expression starting a line with `[`/`(` glues onto the previous
  statement. Decision: this is JavaScript's ASI behavior (`foo()\n[1,2]` is
  `foo()[1,2]` in JS too), expected not buggy; the fix would diverge from JS and
  break multi-line chains. Documented in the D1 spec note with the `return`
  workaround.

### 0.1.32 — Shipped · Fluent-await fix and TypeScript discriminated unions in `gen dts`

**Status: shipped.** Two deferred correctness items, addressed together.

- **Fluent sync-then-async `await`.** `await` now applies to the *whole chain*
  when the innermost call is a value method (`await coll.find({}).to_array()`
  awaits `to_array`, the async terminal), while the Result idiom still awaits the
  head call (`await load(p).map_err(f)` stays `(await load(p)).map_err(f)`). The
  two are told apart with no type information (colorless async erases which call
  is async) by a structural signal: a value-method head is fluent, a bare or
  namespaced function head is the Result idiom. The mongodb cursor pattern no
  longer needs the split-cursor workaround; the databases guide is updated.
- **TypeScript discriminated unions in `gen dts`.** A `.d.ts` union of object
  variants sharing a string-literal tag (`{ petType: "cat"; ... } | { petType:
  "dog"; ... }`) now materializes as a Glyph tagged union of generated variant
  records plus a `parse_<Name>` dispatcher that reads the tag and validates into
  the right variant. Previously this emitted `type X = unknown` with a note. The
  discriminator-dispatch machinery already existed for OpenAPI; the new work is
  detecting the tag in a bare `oneOf` (a property present in every variant whose
  type is a distinct one-element string enum) and generating a record per inline
  variant. Verified end to end: the generated `parse_Pet` accepts a valid `cat`
  and `dog`, rejects a `cat` with the wrong shape, and rejects an unknown
  discriminator, and the whole module type-checks under `tsc --strict`.
- **Test-gated:** emit tests for the fluent/namespaced/Result await cases, a gen
  unit test for the inline union (plus the primitive-`oneOf`-stays-`unknown`
  regression), and a run-verified dispatcher. 690 tests green.

### 0.1.31 — Shipped · Taint tracking (std/taint)

**Status: shipped.** The security item of the correctness trip: untrusted input
can't reach a dangerous sink without being sanitized, and it's a compile error if
it tries.

- **`std/taint`** — `Tainted<T>` and `Trusted<T>` are structurally distinct
  branded types. A sink whose parameter is `Trusted<string>` (a SQL runner, a
  shell command, an HTML renderer) cannot receive a `Tainted<string>` without
  going through `sanitize(t, clean)` first; `tsc` rejects the call
  (`TS2345: 'Tainted<string>' is not assignable to 'Trusted<string>'`). So a SQL
  injection path is a compile error. The vocabulary: `taint` (wrap untrusted
  input), `sanitize` (escape/validate, then trust), `trust_unchecked` (the
  greppable escape hatch for literals/constants), `expose` (unwrap at the sink),
  `reveal_tainted` (read raw only to inspect). The brand is phantom, so a
  `Tainted`/`Trusted` is just `{ value }` at run time.
- **Discipline, not flow analysis** (the Q33 v1 form): you opt in by typing a
  sink's parameter `Trusted<...>`; the compiler does not infer taint across the
  program. That is honest about what it does and does not do, and it is enforced
  where it matters, the call into the sink.
- **CI-locked both directions:** an integration test asserts the sanitized path
  passes `tsc` and a tainted value handed to the sink fails it. 686 tests green.
- **Docs:** stdlib reference, AGENTS.md and its llms.txt mirrors gain a std/taint
  section, and the typed-APIs answer page notes it.
- **The correctness trip is now feature-complete** (decimal, bigint, `where`,
  taint). Remaining: the 1.0 stability enablers (conformance corpus + `glyph fmt`
  migration), then a dedicated "finance & correctness in Glyph" answer page tying
  the four together (a new page, so the answer sub-nav renumbers with it, done as
  its own focused change rather than rushed here).

### 0.1.30 — Shipped · Refinement types (`where`, D39)

**Status: shipped.** The verifiability half of the finance-correctness work: an
invariant like non-negative or in-range becomes a validated *type*, not a check a
caller has to remember.

- **`where` refinement types (D39).** `type Amount = int where value >= 0`,
  `type Rating = int where value >= 1 && value <= 5`,
  `type NonEmpty = string where value.length > 0`. The boolean predicate (over a
  bound `value`) is woven into the type's runtime descriptor, so `Amount.parse(x)`
  and `Amount.is(x)` run the base leaf-check *and* the predicate. The base check
  narrows `value` first, so the predicate sees the base type and stays tsc-clean.
  A value that fails the predicate is rejected at the boundary where untrusted
  data enters.
- **Nominal-newtype semantics** (the form the open questions reserved): Glyph's
  own checker treats the refined name as its base type for assignability, like
  `int`, and defers to the descriptor at `.parse`/`.is`. New `where` keyword; the
  predicate parses after the type body and the formatter preserves it (it was
  dropped in a first cut, now covered by a round-trip test).
- **v1 scope is honest:** the base must be a primitive with a leaf check
  (`int`/`number`/`string`/`bool`/`bigint`). A `where` on a record or union type
  is a compile error (E0300), not a silent drop; record and cross-field invariants
  (`where value.paid <= value.total`) are a planned extension.
- **CI-locked:** a run-based test asserts `Amount.parse(-1)`, `Rating.parse(6)`,
  and `Amount.parse(3.5)` are rejected while valid values pass; plus emit and
  formatter round-trip tests. AST snapshots regenerated for the new refinement
  field. 685 tests green.
- **Docs:** spec D39, the typed-APIs answer page gains an "invariants, not just
  shape" section, and AGENTS.md and its llms.txt mirrors note it.
- **Next:** taint tracking, then the 1.0 stability enablers, then the dedicated
  "finance in Glyph" answer page tying decimal + bigint + `where` + taint together.

### 0.1.29 — Shipped · Exact large integers (bigint, D38)

**Status: shipped.** The paired half of the finance-correctness numeric work:
exact whole numbers past the float range. Completes "safe integers" alongside
0.1.28's `std/decimal`.

- **`bigint` prelude type (D38).** An exact arbitrary-precision integer, emitted
  verbatim as TypeScript `bigint` and kept distinct from `number` by `tsc` (no
  mixed arithmetic). Literals are `123n` (the lexer accepts the `n` suffix on an
  integer literal only, never on a fractional or exponent form, matching JS). Its
  runtime descriptor checks `typeof x === "bigint"`, so an account id sent as a
  JSON `number` fails `.parse` rather than being silently truncated past 2^53.
  Unlike `int` (a `number` with an integer *check*), `bigint` is a genuinely
  separate runtime type that holds large values exactly.
- **CI-locked exactness:** an integration test runs a self-checking program
  (`9007199254740993n + 2n`, `1e18n * 1e18n`) and asserts a clean exit; plus a
  lexer test for the `123n`/`1_000n` suffix (and that `1.5n` does *not* absorb the
  `n`) and an emit test for the type and its `typeof "bigint"` descriptor. 682
  tests green.
- **Docs:** spec D38, the "is this a real language" answer page gains a `bigint`
  paragraph beside the money section, and AGENTS.md and its llms.txt mirrors note
  it next to `std/decimal`.
- **Tier 1 (decimal + safe integers) is now complete.** Next: refinement types
  (`where`), then taint tracking, then the 1.0 stability enablers. The dedicated
  "finance in Glyph" answer page goes up once refinements and taint land.

### 0.1.28 — Shipped · Exact money math (std/decimal)

**Status: shipped.** The first of the bank-readiness "finance correctness" items:
money that isn't a floating-point bug waiting to happen. This is the one purely
technical gap that is genuinely disqualifying for finance and entirely within our
control (the others being users, support, and track record, which no commit
closes).

- **`std/decimal`** — exact base-10 fixed-point arithmetic over BigInt. JS
  `number` is IEEE-754 binary float (`0.1 + 0.2 !== 0.3`) and loses precision past
  2^53; neither is acceptable for money. A `Decimal` is an arbitrary-precision
  integer scaled by a number of fractional digits, so add/sub/mul are exact and
  `div` takes an explicit result scale and rounds half away from zero.
  Construction (`decimal("10.50")`) validates and returns a `Result`, never a
  silent `NaN`. Operations are methods (`price.add(tax)`) since Glyph has no
  operator overloading. Also `from_int(units, scale)`, `round`, `neg`, `abs`,
  `cmp`, `eq`, `is_zero`, `is_negative`, `scale`, `to_string`, and a lossy
  `to_number` for display.
- **Correctness is CI-locked**, not just claimed: an integration test runs a
  self-checking decimal program (it returns its count of wrong results as the
  exit code) and asserts a clean run, covering the float bug, half-up rounding,
  negatives, and exactness past 2^53. 679 tests green.
- **Docs:** stdlib reference, the "is this a real language" answer page gains a
  money section, and AGENTS.md and its llms.txt mirrors gain a std/decimal note.
- **Next in this trip:** a paired `bigint`/safe-integer type for exact large
  whole numbers (account IDs), then refinement types (`where`) so `type Amount =
  decimal where value >= 0` is a boundary-validated type, then taint tracking.
  These land as their own releases; a dedicated "finance in Glyph" answer page
  goes up once the trio is complete rather than one thin page per feature.

### 0.1.27 — Shipped · Real database interop, and two bugs it surfaced

**Status: shipped.** Writing the databases guide (proving Postgres and MongoDB
work end to end, not just claiming it) surfaced two real interop bugs, both fixed
here. The guide only claims what now type-checks.

- **`std/sqlite` was not tsc-clean under `@types/node`.** The wrapper passed its
  bound params (`ReadonlyArray<unknown>`) straight into `node:sqlite`, which under
  the real `@types/node` types expects `SQLInputValue`. It only passed before
  because a project without `@types/node` fell back to the looser bundled shim.
  Any project that also installed `@types/node` (most of them) saw the emitted
  `std/sqlite.ts` fail `tsc`. Fixed by asserting the params at that one platform
  seam, so the wrapper checks the same against the real types or the shim.
- **`@types/<pkg>` companions did not resolve.** The generated tsconfig's `"*"`
  path listed the bare package before its `@types` companion, and a `paths` entry
  short-circuits on the first candidate that resolves to *a module* even when it
  has no types. So a typeless JS package (`pg`, `react`, `express`, `lodash`, the
  whole "ships JS, types live in `@types/*`" ecosystem) resolved to its untyped
  `.js` and reported an implicit `any` (TS7016). Now `@types/<pkg>` is tried
  first; a package that ships its own types has no `@types` entry and falls
  through to its bundled declarations. `pg` with `@types/pg` now type-checks.
- **The databases guide** (`docs/guide/databases.md`): SQLite (built in),
  Postgres (`new Pool`, validate rows), MongoDB (`new MongoClient`, validate
  documents), and the Redis/MySQL factory clients. Every full program in it was
  compiled against the real installed client types before it was written down.
- **Documented limitation surfaced:** Glyph's `await` binds to the innermost call
  of a chain (the common case, where that call is the async one). A fluent API
  that puts a synchronous call before the async one, like mongodb's
  `find(...).toArray()`, needs the async call on its own line
  (`let cursor = coll.find({})` then `await cursor.toArray()`). Noted in the guide
  and parked below; the fix is a smarter await-spine that awaits the outermost
  Promise-typed call, not the innermost call.

### 0.1.26 — Shipped · `new` for class-based npm clients (D37)

**Status: shipped.** A language feature that closes the last real gap in the npm
interop story: instantiating class-based clients. Surfaced by asking whether every
database and message broker needs a bundled stdlib wrapper the way `std/sqlite`
did. The answer is no: the general npm path already handles them, except Glyph had
no `new`, so class-based clients (`pg`/`new Pool`, `mongodb`/`new MongoClient`,
`ioredis`/`new Redis`, `kafkajs`/`new Kafka`) could only be reached through the
`unknown`-typed `extern_ts` escape hatch. Factory-style clients (`node-redis`
`createClient()`, `mysql2` `createConnection()`) already worked directly.

- **D37 `new` interop constructor.** `new <callee>(<args>)` (with optional
  `new <callee><T, ...>(<args>)`) emits a verbatim TypeScript `new` and is
  type-checked by `tsc` against the real constructor: a wrong argument is a real
  error mapped to the Glyph source (TS2345), an undefined callee is E0103. The
  callee parses as a member/index chain that does not swallow a call, so
  `new a.b.C(x).m()` is `(new a.b.C(x)).m()`. The instance is opaque to Glyph's
  own checker (no `.parse` descriptor, like any imported `.d.ts` type), with `tsc`
  supplying the type. Greppable by `new`.
- **Deliberately interop-only.** Glyph has no `class` declarations and gains none.
  `new` exists solely to construct a type that comes from an npm package, a
  `.types` ambient declaration, or `extern_ts`. This keeps the function-oriented
  stance while making class-based libraries first-class instead of escape-hatch
  material.
- **Verified end to end** against real `kafkajs` types (`new Kafka({...})`,
  `.producer()`, `await ...connect()` all type-check), plus a `.types`-ambient
  class integration test that runs under `tsc --strict` in CI, an emit unit test,
  and negative tests (bad arg to `tsc`, undefined callee to E0103). 677 tests green.
- **Docs:** spec D37, the external-imports guide gains a class-based-clients
  section, AGENTS.md and its llms.txt mirrors gain a `new` interop note, and the
  imports answer page now covers class-based vs factory clients.

### 0.1.25 — Shipped · A persisted database, and a real app on it

**Status: shipped.** A database story and the first end-to-end backend app built on
it. The point was evidence for the 1.0 gate: build a real, persisted, validated
HTTP service in Glyph and see what the language does and doesn't do well. It went
green with the boundary catching a genuine bug on the first try.

- **`std/sqlite`** — a persisted SQL database over Node's built-in synchronous
  SQLite (`node:sqlite`, Node 26+), no native install and no flag. `open(path)`
  returns a `Db` with `exec` / `run` / `query` / `query_one` / `last_insert_id` /
  `close`. Queries return rows as `Record<string, unknown>`, so a row is a
  validated boundary exactly like a request body: you `RowType.parse(row)` before
  trusting it, never a cast. Wired into the bundled runtime, the resolver stub, and
  the stdlib reference; a `node:sqlite` shim was added for projects without
  `@types/node`. Drift tests hold the stub, the runtime, and the docs in sync.
- **`examples/apps/tasks.glyph`** — a persisted task API: `std/sqlite` for storage,
  `std/http` for routes (`GET /tasks`, `POST /tasks`, `POST /toggle`), wire bodies
  validated with `.parse`, all errors-as-values, and data that survives a restart.
  Built end to end, curled on every route, and restarted to prove persistence. It
  demonstrates the storage/domain type split the database boundary forces: SQLite
  has no boolean type, so a `done` column comes back as an integer `0`/`1`, and a
  separate `TaskRow { done: int }` maps to the domain `Task { done: bool }` in one
  visible line rather than a silent `row as Task` cast that would leave
  `task.done === true` never true.
- **The typed-APIs answer page** now covers the database as a boundary, with the
  int-vs-bool bug it removes and a link to the working app.



**Status: shipped.** The batch-5 completion run plus the headline compiler work of
this cycle: the **imported-union type-resolution pass** (the proper fix for the
recurring cross-module-`Unknown` root cause, described in the G22 item below) and
the **shift operators** completing the D36 family. Also widened the bundled node
`Buffer` shim to the byte boundary (`Iterable<number>`, a length and index
signature, a byte-array `from` overload) so a pure-Glyph base64 codec type-checks,
and added corpus modules. Every change test-gated; 675 tests green.

- **`examples/corpus/casing.glyph`** — identifier case conversion (snake, kebab,
  camel, pascal, constant, title). The word splitter breaks on separators and the
  camelCase lowercase-to-uppercase boundary, and the target style is a D30
  string-literal-union type whose `render` match is exhaustive without an `else`.
  Classifies characters with no char-code table (a letter is uppercase when
  `upper(c) == c && lower(c) != c`).
- **Empty-block match arm fall-through fix** (emit) — a void-typed `match` whose
  arm is an empty block (`true => {}`) lowered to a `switch` case with no
  terminating statement, so control fell through and ran the next arm's body (in
  the surfacing case, unbounded recursion). The empty-block arm now emits a
  `break` inside a `switch` case regardless of return/statement position. One
  emit test.
- **`examples/corpus/ipv4.glyph`** — IPv4/CIDR address arithmetic: dotted-quad
  parse with canonical-form validation, 32-bit address values, subnet masking done
  with integer arithmetic (JS bitwise `&` coerces through a signed int32 and turns
  any value past 2^31 negative, so the mask is computed from the block size
  `2^(32-prefix)` instead), broadcast/host-count, and containment. Builds under
  `tsc --strict` and runs correctly across the full 0..2^32-1 range. The module
  owns its `Result` error type and renders it with an in-module `explain`.
- **Cross-module nullary-variant match — FIXED (the imported-union type-resolution
  pass).** Matching `ipv4`'s imported error union directly surfaced the recurring
  root cause: an imported type annotation lowers to `Ty::Unknown` in the consuming
  module, so the reachability check read every bare no-payload variant arm
  (`EmptyOctet =>`) as an irrefutable binding (false E0216) and the emitter was
  blind the same way. Resolved on two levels: a PascalCase bare ident is now
  treated as a variant reference in both the reachability check and the emitter
  (0.1.23), and — the proper fix — the `DeclTyResolver` gained
  `imported_union_of_variant`, whose salsa implementation follows a variant's
  `ImportNamed` symbol to its source module and returns the owning union's variant
  set, so an imported-union `match` is now held to full exhaustiveness (a missing
  variant is a real E0200 naming the union). This retires the whole class the loop
  kept mining (record-payload bind, nullary match). G22 in
  `docs/dogfooding-gaps.md` is resolved.
- **Bitwise shift operators `<< >> >>>`** (D36) — dogfooding posix path logic and
  a pure-Glyph mulberry32 PRNG surfaced the last parked bit of the D36 operator
  family. The angle-bracket ambiguity turned out not to need lexer changes:
  generic type arguments are consumed on the postfix/type paths before the binary
  chain runs, so an *adjacent* `<<`/`>>`/`>>>` reaching the shift level (between
  comparison and additive) is unambiguously a shift. The parser recognizes it
  from the single angle tokens the lexer already emits, with a span-adjacency
  check so `Foo<T> > x` never misreads. Emit is verbatim, number-typed via `tsc`;
  the formatter round-trips and reprecedences. A pure-Glyph PRNG now matches
  `std/random`'s output exactly. One emit test.

### 0.1.23 — Shipped · improve-glyph loop batch 5 + imported-union match fixes

**Status: shipped.** Batch 5 (seven pure-Glyph corpus modules: an LCS line diff,
number theory, FNV/djb2 hashes, ISO-8601 formatting, IPv4/CIDR, structured
logging, and case conversion; the batch hit subagent rate-limits so three
iterations did not land) plus two emit/typecheck fixes:

- **Empty-block match arm fall-through** — an empty block arm in a return-position
  switch fell through instead of emitting `break;`. Now it breaks in any switch
  context.
- **Imported-union nullary-variant match** — matching an imported tagged union's
  no-payload variants drew a false E0216 and then E0300, because an imported
  union's type is `Unknown` in a consuming module (empty variant set), so both the
  reachability check and the emitter read a bare PascalCase arm as a binding
  catch-all. Both now treat a PascalCase bare ident as a variant reference (the
  resolver's rule), so imported nullary variants lower to a `case` on `.tag`. This
  is the third cross-module-`Unknown` symptom the loop has mined out (after the
  record-payload bind in 0.1.21); the recurring root cause is a candidate for a
  proper imported-union type-resolution pass.

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

- **Await-spine for fluent sync-then-async chains** — ✅ **done (0.1.32).** `await`
  used to bind to the innermost call of a chain, right for the Result idiom
  (`await load(p).map_err(f)`) but wrong for a fluent API whose synchronous call
  precedes the async one (mongodb's `find(...).toArray()` awaited `find`). Because
  colorless async erases which call is async, the fix uses a structural signal
  rather than a type: a value-method head (`coll.find(...)`) is fluent, so the
  whole chain is awaited; a bare or namespaced function head (`load(...)`,
  `http.get(...)`) is the Result idiom, so the head call is awaited. The
  split-cursor workaround in `docs/guide/databases.md` is removed. The rule is a
  genuine either/or under colorless async (you cannot tell which call is async
  without a type), so it has two documented edges, both with the split workaround:
  (1) a *value method* that is itself async and returns a `Result` chained into a
  synchronous combinator (`await store.load(k).map_err(f)`) now awaits the whole
  chain and would need `(await store.load(k)).map_err(f)` written out; and (2) a
  fluent terminal that returns a `Result` used with `?`. Both are uncommon in
  practice: the documented Result idiom heads with a function (`load(...)`), not a
  value method, and fluent terminals rarely return `Result`. The common cases
  (fluent cursors/builders on a value, function-headed Result chains) are correct.
- **MCP write tier: `glyph_fix` + `glyph_rename`** (M). The MCP server today is five
  read-only query tools; the write-capable tier is these two, done together. Both
  **return edits** (a list of `{range, newText}`), never mutating files, so the
  agent stays in control, matching the rename follow-up's original design note.
  `glyph_fix` reuses `fix.rs` (unused-all imports → line-removal edits), returning
  the edits instead of applying them; `glyph_rename` returns the workspace-wide
  rename edits the LSP already computes. Not required for capability (an agent
  already sees E0106 via `glyph_diagnostics` and can edit itself); this is the
  convenience/parity tier. When it lands: tool count 5 → 6 (or 7), and the website
  MCP/answers page plus `glyph mcp` help need the update.
- **Negative-number literal patterns** (S) — ✅ **done.** A `match` arm may now be
  a negative integer literal (`-1 => ...`), the natural companion to the existing
  positive-literal arms. The parser folds the sign into the literal's raw text, so
  emit lowers it to `case -1:` and value-match exhaustiveness treats it as one
  `Number` pattern; `-x`/`-(...)` stay non-patterns. Surfaced dogfooding a
  calendar module (`examples/corpus/calendar.glyph`, matching on `math.sign`);
  parser unit test added.
- **Structural interface as an ordinary type** (S, from 0.1.16 D34) — ✅ **done.**
  D34 says a structural `interface` is "usable as an ordinary type," but the
  typechecker only honored that for a generic bound (`<T: Iface>`, enforced by
  the emitted `tsc` `extends`). Used directly as a parameter or return type, an
  interface was compared **nominally**: a record that carried every member was
  falsely rejected (E0211), and member access on an interface-typed value went
  unchecked (typed `Unknown`). Assignability now expands an interface on the
  expected side to its member set and checks structural satisfaction (a method
  member `fn m() -> R` matches a field `m: fn() -> R`; extra members are fine by
  width subtyping; a missing member still mismatches), and `record_fields_of`
  resolves interface members so member access is verified. Surfaced dogfooding a
  ranking module (`examples/corpus/ranking.glyph`, `Ranked` used both as a bound
  and as a plain parameter type); three typechecker unit tests added.
- **Typo'd `match` variant is E0220, not a silent catch-all** (S, verifiability) —
  ✅ **done.** A PascalCase arm head that names no variant of the union used to be
  read as a fresh binding, an irrefutable catch-all that masked the missing-variant
  error (E0200) and misrouted values at runtime. `check_patterns_exhaustive` now
  escalates such a head to `UnknownVariantPattern` (E0220) with a nearest-variant
  suggestion (`Loadign` -> did you mean `Loading`?), for all three arm shapes:
  bare `Loadign`, payload-bearing `Loadign(x)`, and qualified `Feed.Loadign`. It is
  neither covered nor a catch-all, so a genuinely missing variant still surfaces as
  E0200 alongside. Scope is a module-local, decidable-scrutinee union; a union
  imported from another module is checked for coverage but not yet this typo (the
  G22 fork in `docs/dogfooding-gaps.md`). Surfaced by persona-agent testing; spec
  refinement recorded under D9. Typechecker and CLI integration tests, including
  the co-occurring E0200 and the cross-module scope boundary.
- **Private-vs-missing import diagnostic** (S, from 0.1.16 visibility). Importing a
  non-`pub` name reports E0105 "`N` is not exported by `M`", the same message as a
  genuinely missing name. Distinguishing "exists but private (mark it `pub`)" from
  "no such name" needs the module export surface to carry both the public set and
  the full name set so the verifier can tell which case it is. Correct today, just
  less precise than it could be.

- **Cross-module generic-descriptor `.parse<T>()` call** (M, correctness). Calling
  a generic type's descriptor parse with an explicit type argument works
  same-module (`Box.parse<User>(v)` synthesizes the `__is_User` checker), but a
  generic type **imported from another module** emits the call with the checker
  argument missing (`tsc` TS2554: "Expected 2 arguments, but got 1"). Found while
  materializing first-class generics; it is **pre-existing and general** (it
  reproduces with a hand-written cross-module generic, not just materialized
  ones), so it belongs to the 0.1.10 generic-descriptor call-site synthesis, not
  the interop work. Nested generic fields validate correctly (the emitter threads
  the checker when generating a descriptor), so the gap is only the explicit
  top-level `Imported.parse<T>(v)` call across a module boundary.

## Parked (v2 / later)

- **GitHub Linguist submission (a real "Glyph" language on GitHub).** Get `.glyph`
  recognized on github.com with its own name, color, and syntax highlighting: a PR
  to `github-linguist/linguist` (a `languages.yml` entry, a TextMate grammar for
  highlighting, distinct from the archived tree-sitter grammar, which powers code
  navigation, and sample files). **Adoption-gated:** Linguist's policy is that a
  language be in use across *hundreds* of repositories, so this is downstream of
  real Glyph adoption, not shippable before it. Until then the interim is a
  per-repo `.gitattributes` that maps `*.glyph` to TypeScript (Glyph's family) for
  a colored/highlighted bar; deliberately **not** baked into `glyph init`, because
  the hard `linguist-language` override would keep winning after Glyph joins
  Linguist and force every scaffolded repo to keep reporting TypeScript until a
  human removes it. The safe, forward-compatible rules (mark emitted `dist`/`.ts.map`
  as generated and `.glyph-runtime` as vendored) are fine to document, but the
  alias stays opt-in with a "remove once Glyph is in Linguist" caveat.

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
