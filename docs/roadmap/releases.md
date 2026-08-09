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
- **Width-aware formatter** (M, F6) — ✅ **done.** A list stays inline when its
  rendered form fits the print width (100 columns) from the current column,
  otherwise it goes one-per-line with a trailing comma. `leaf("body.type",
  Equals, "push")` now stays on one line. The decision is a pure function of
  content and column, so the layout still round-trips and is idempotent. Little
  snapshot churn (formatter output is not snapshotted; two formatting-shape tests
  updated). *This shipped with a one-or-two-element exemption that skipped the
  width test entirely; that exemption was removed later (G54/G29), because it let
  the formatter's own fixed-point output hold 142-column lines.*

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
- **`glyph check <file>`** (S) — ✅ **done** (G28). `glyph check [path]` takes a
  file or a directory, reuses `build`'s pipeline into a temp dir it deletes on
  the way out, and runs `tsc --strict` over the emitted TypeScript unless you
  pass `--no-tsc`. Nothing is written to your tree and nothing is executed, which
  is the whole point: the regression test checks a program whose `main` writes a
  sentinel file and asserts the file is absent. A file is checked in the context
  of its directory (a sibling's error fails the check, as it does under `build`
  and `run`), and the `@example` / `@doc @run` gate does not run, because running
  it would run your code. `glyph build one.glyph`, the command that produced the
  gap, no longer ends at "source path is not a directory": the refusal now names
  `glyph check <file>`, since a capability the user cannot find where they are
  standing is not shipped.
- **Formatter, layout only** (S each, deliberately not bundled with the
  correctness fix above) — ✅ **done** (G29, G54): a one-statement match-arm body
  was always exploded to three lines because the parser wraps it in a synthetic
  block and the formatter prints every block multi-line; and
  `items.len() <= INLINE_MAX` short-circuited the width test, flattening every
  two-argument `array.map(xs, fn(...) {...})`. An arm body that is a synthetic
  one-statement block now prints as `X => { break }`, and `INLINE_MAX` is deleted
  so the width test runs at every element count. Shipped in 0.1.50 below, with
  G60. Removing the exemption turns the still-open G18 pathology (no chain-aware
  path in the printer, so the innermost argument list is the only breakable
  point) from theoretical into visible; recorded under G18 in
  [`../dogfooding-gaps.md`](../dogfooding-gaps.md).
- **`llms.txt`** (XS). It does not say annotations are sorted by kind, with
  repeats of one kind keeping source order (D27), and it does not mention the
  expression-arm `?` restriction while that exists.

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

✅ **Resolved: the stdlib function.** `array.range(count)` and
`array.range_from(start, end)` are in `std/array`. Range syntax was rejected
because it is language surface that costs grammar and forecloses later choices,
while a function costs neither and is forward-compatible with adding syntax over
it if that ever earns its keep. `range` takes a count and clamps it the way
`string.repeat` does (`range(-1)` is `[]`, a fractional count truncates).
`range_from`'s second argument is an exclusive end bound, the reading
`array.slice` and `string.slice` already give a second numeric argument and the
one the hand-rolled `span(lo, hi)` in `bracket.glyph` has, so
`range_from(2, 5)` is `[2, 3, 4]` and the port is textual. It was written first as
`(start, count)`, which meant the same call returned a different array than the
function it replaces with no type error to catch it; the review caught that
before it left the working tree. The
typechecker models both as `Array<number>` so `for i in array.range(n)` binds `i`
as a number rather than `Unknown` — otherwise replacing the typed hand-rolled
`upto(n) -> Array<int>` would have been a typing regression. The lazy-iterable
option stays unbuilt; it is an optimization, not a capability, and nothing has
asked for it yet. The apps are ported: `upto` and `span` are deleted from
`bracket.glyph` and `minesweeper.glyph`, all 16 call sites read `array.range` or
`array.range_from`, and both apps emit byte-identical TypeScript to what the
hand-rolled helpers produced. G30 stays `[HALF FIXED]` only for its index-safety
half, which is untouched and belongs with G39.

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
- **The clap binding has no `allow_hyphen_values`** (S) — ✅ **done** (G36). The
  surface is `glyph run`'s trailing argv passthrough and nothing else; a built
  program run under node was never affected. `allow_hyphen_values` on that
  argument lets `--amount -12.50` and a bare `-12.50` reach the program intact.
  A flag glyph knows still binds to glyph wherever it appears (clap starts the
  var-arg on unknown flags only), so `glyph run x.glyph --no-check` is unchanged
  and `--` is still the answer for a colliding program flag.
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
same hole on a second axis. The Next marker has moved on to the linkcheck trip
below.

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

- **A descriptor's `.parse` result is not assignable to `Result`** (M) —
  ✅ **done** (G41). The emitter wrote `parse` as a bare `{ tag, value }` union to
  keep a descriptor free of a `std/result` dependency, but `Result<T, E>`
  intersects the `map`/`map_err` combinators, so the bare union was not
  assignable to it: `return User.parse(v)` was TS2322 and
  `User.parse(v).map_err(f)` was TS2339, while Glyph's own checker reported
  `parse` as `Result<T, Array<Issue>>`. All three descriptor kinds now annotate
  `parse` as the real `Result` and build both arms with the prelude constructors,
  under one injected aliased `std/result` import shared with the `?` lowering
  (two lines would redeclare `__glyph_err`). Two costs, stated exactly: the
  import is a value import, so every module with a `pub type` now carries a
  runtime edge to `std/result` even if it never mentions `Result`, where `?` and
  `T.schema` are paid only by the modules using them; and every `T.parse`
  allocates the two combinator closures the constructors build, which is what an
  `Ok(...)` costs per call but lands on the per-request boundary path rather
  than on a function return. `?` on a parse result and `infer_output<S>` both
  still work. `bracket.glyph` was the app holding the workaround: the two
  identity re-wrap `match`es around `Bracket.parse` and `SeedFile.parse`
  (`Ok(b) => Ok(b)` beside an `Err` arm that only rewords the message) are now
  `.map_err(...)`, and both rejection paths were run against a malformed file.
- **`glyph build` prints "no diagnostics" above its own `tsc` errors** (S) —
  ✅ **done** (G42). The summary now prints after the `tsc` gate and the example
  gate, beside the `tsc --strict passed.` line that was already held back for the
  same reason. A red build never prints it; a green build's transcript still
  reads summary then `tsc --strict passed.`, and both orders have tests. `--json`
  was never affected. Under `--no-check` the line still reads "no diagnostics"
  with no TypeScript stage behind it; it is honestly about the Glyph stage, and
  rewording it is a separate call.
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

## linkcheck dogfood trip — one wrong condition, three TypeScript errors

The loop pointed at `examples/apps/linkcheck.glyph`: walk a directory, scan
Markdown for links, fan out HTTP requests with a bounded pool, report what is
broken. Thirteen findings came out of it and most are stdlib breadth
(`fs.read_dir`, capture groups in `regex`, timeouts in `http`). The one that
matters is not on that list. Five findings ended with the same sentence, `glyph
build` reported no diagnostics and `tsc` caught it, and three of the five turned
out to be one defect in the emitter wearing three faces. That one is fixed here;
the other two are recorded below, unfixed. The Next marker has moved on to the
bracket trip below.

### 0.1.42 — Shipped · A `match` you assign is a `switch`, not a closure

Glyph has no `if`/`else`, so `match` is the conditional, and in a real program it
is usually the right-hand side of a binding. The emitter has two lowerings for
it: a flat statement `switch` that assigns per arm, and a value IIFE for a
`match` used as a sub-expression. `Stmt::Let` chose between them by asking "does
any arm have a block body?" when the question that decides correctness is "is
this `match` the whole initializer?". Three failures came out of that one guard,
and each looked like its own bug:

- An `await` in an arm landed inside a synchronous arrow, so `tsc` rejected the
  emitted file with TS1308. The app's one place where offline mode skips the
  network is exactly this shape.
- A self-referential accumulator, `mut on = match on { ... }` inside a `for`, went
  through an untyped IIFE and TypeScript refused to infer the binding it was
  being defined from (TS7024).
- `Stmt::Mut` had no `Expr::Match` path at all, so a block arm under `mut` was a
  hard `EmitError` while the identical `let` compiled. That is G25, open since the
  Minesweeper round, where it was filed as its own structural limit.

The fix removes a special case rather than adding one. A `match` that is the
whole value of a `let` or a `mut` assignment always lowers to the flat `switch`,
which declares (or reuses) the binding and assigns it in each arm; the existing
`default: throw` is what keeps TypeScript's definite-assignment analysis happy.
The IIFE now fires only where it is actually needed, a `match` nested inside a
larger expression, and there an `await` in an arm makes it an awaited async arrow
instead of a sync one. The old block-arm rejection on that nested path stands,
because a function-level `return` inside an arrow cannot mean what it says.

Two follow-ons that the wider path exposed and closed with it: a `break` or
`continue` in a `let`- or `mut`-bound arm now labels its loop, since an unlabeled
`break` would have escaped only the `switch`; and an empty array literal in an arm
is pinned to `never[]`, because a bare `[]` assigned to an unannotated binding
starts TypeScript's evolving-array inference and every later read becomes an
implicit `any[]` (TS7034/TS7005).

Verifiability, inverted and put back. Every one of these built clean under
`glyph build` and failed at `tsc` on the emitted TypeScript. The compiler is
supposed to be the source of truth; on this path it was a preprocessor with
opinions. `examples/apps/linkcheck.glyph` ships with the release with all three
workarounds deleted, and its offline output is byte-identical to what the
workaround version produced.

### Still open from this trip

- **`await` in a non-`async fn` is not checked by Glyph** (M). `fn nope() -> int
  { return await slow() }` builds with no diagnostics and fails at `tsc` with
  TS1308. There is no Glyph-side check of async context anywhere; the whole async
  story is delegated to the emitted TypeScript. Same family as the fix above and
  the next verifiability item after it.
- **An `async` function type is unspellable** (decision, then S). `parse_atom_type`
  has one function-type entry and `TypeExpr::Fn` carries no async bit, so a
  parameter that takes an async callback cannot be typed. The fork is `async
  fn() -> T` emitting `() => Promise<T>` versus `fn() -> T` emitting
  `() => T | Promise<T>`. Needs the orchestrator's call.
- **`{}` as a match arm is silent green** (decision, then M). `true => {}` parses
  as an empty block, emits `case true: { break; }`, and the function falls out of
  its own switch returning `undefined` while claiming a record type. `tsc` catches
  it as TS2366. `X => {}` as a deliberate no-op statement arm is meaningful, so
  `{}` cannot simply be reread as a record literal; the deeper half is that
  `check_return_type` never asks whether a value-position arm produces a value.
  `=> ({})` compiles, and then `glyph fmt` removes the parentheses and the
  formatted file fails again, so the workaround is a named constructor.
- **`@example` execution is opt-in behind `--test`, contradicting D23** (decision).
  D23 is tagged verifiability so an agent rewriting a body cannot bypass the
  examples, and a flag is a bypass by default. Making it default-on requires `tsx`
  on `PATH` during a plain `glyph build`, which is a product call. Recorded
  opinion: default it on, and degrade to a warning when `tsx` is absent rather
  than skipping in silence.
- **`std/fs` has no `read_dir`, `is_dir`, or `stat`, and `FsError.kind` has one
  constant** (M). *Landed in 0.1.48, except for the checking half of the
  taxonomy.* The app discovered directories by reading every path in a tree
  and inspecting the errno it got back. Blocking for any CLI that takes a path;
  one change to `fs.ts` covers both. `read_dir` (entry names, one level),
  `is_dir` (a `bool`, like `exists`), and `stat` (a `FileInfo` of
  `is_dir`/`is_file`/`size`/`modified`) ship, with `read_dir` and `stat` modeled
  in `stdlib_fn_ty` so `?` decides its error type. `modified` is epoch
  milliseconds truncated to a whole number: node reports mtime as a float, the
  docs type it `int`, and `int` is a checked boundary, so passing the raw
  `mtimeMs` into a descriptor would have failed at run time. `ErrorKind` is now the closed
  set `NotFound`, `IsADirectory`, `NotADirectory`, `PermissionDenied`,
  `AlreadyExists`, `Other({ code })`. Still open: nothing checks a `match e.kind`,
  because the typechecker models stdlib function returns and has no field model
  for a stdlib named type, so an omitted kind is a run-time throw rather than an
  E0200. Also open: whether `read_dir` should sort, since node's order differs by
  platform and a reproducible report currently needs the caller to sort.
- **`std/http` cannot bound or observe a request** (M). No headers, no final URL,
  so a redirect is invisible; no timeout, no redirect policy, no `head`. The
  `task.race` timeout workaround leaves the loser in flight, the exact thing
  structured concurrency exists to prevent.
- **`regex.find_all` drops capture groups** (S). *Landed in 0.1.48 as
  `captures_all`.* It maps each match to `m[0]`, so
  a scanner that needs the capture text is hand-rolled. It turned a 15-line link
  extractor into a 180-line character scanner. `regex.captures_all(pattern, text)`
  returns groups 1 onward per match, following `captures`: the whole match is not
  included and a non-participating group is `""`. That convention was expected to
  keep an alternation-based scanner hand-rolled, since an empty capture and an
  absent one read the same. Rewriting the app showed otherwise: wrap each
  branch's group around the whole construct instead of its payload and a group
  that fired cannot be empty. `Option` per group would still help a scanner whose
  discriminator can legitimately match empty, at the cost of disagreeing with
  `captures`.
- **`task.pool` is fail-fast with no settled variant** (S). *Landed in 0.1.48 as
  `pool_settled`.* One rejection abandons
  the rest; `all_settled` is unbounded. `pool_settled` is a few lines. It reuses
  `Settled<T>`, keeps result order, never rejects, and takes the same `limit`
  clamp. The task error is still `unknown`. What "abandons the rest" means was
  documented precisely at the same time: a `pool` rejection discards the other
  workers' results, it does not stop them, because the remaining workers keep
  draining the queue. So the value of `pool_settled` is the results you keep, not
  requests you avoid sending.
- **Stdlib breadth, third sighting** (S). *Landed, except for two pieces.*
  `string.repeat`, `pad_start`, `pad_end`, `slice`, `index_of`, `replace_all`,
  `trim_start`, `trim_end` and `array.fold`, `index_of`, `flat_map` all ship, in
  the runtime and in the resolver seed. Replacement is `replace_all` only, so
  there is no first-only form to confuse with TS `replace`, and both `index_of`
  functions return `Option` instead of `-1`. Still open: codepoint-aware
  `string.chars`/`char_at`, which needs a decision on whether `std/string` indexes
  UTF-16 code units (what `len` and `split` do) or codepoints; and
  `iter.take_while`, which no longer appears in D21 (it never existed) and which
  needs the `std/iter` module it would live in before it can be promised again.
- **Model the two `index_of` returns in `stdlib_fn_ty`** (S, follow-up to the
  above). `stdlib_fn_ty` (`glyph-typechecker/src/assign.rs`) is what makes an
  Option-returning stdlib function's `match` exhaustiveness-checked, and it lists
  `http.header`, `http.query_param`, and `json.discriminant`. Neither `index_of`
  is in it, and `recover_union_from_arms` only recovers module-local unions, so
  the prelude `Option` is never recovered: a `match string.index_of(s, x)` with
  no `None` arm builds clean, passes `tsc --strict`, and throws at run time
  (verified). The docs now say so rather than implying the checker catches it.
  The fix is blocked on one decision: `stdlib_fn_ty` returns a concrete `Ty::Fn`
  and E0213 fires on argument-count mismatch against it, so `string.index_of(s,
  needle, from?)` cannot be modeled at a fixed arity without breaking one of its
  two legal call shapes. Either teach the table a min/max arity, or model
  `array.index_of` alone (it has no optional parameter) and leave the string one
  unmodeled.
- **Take the hand-rolled copies out of `examples/apps/`** (S, the other half of
  G26 and G34). *Landed in 0.1.47.* Six apps defined `fn repeat` as a `loop` with
  two `mut`s and five defined `pad_start`/`pad_end`, which is the `grep mut`
  dilution G34 was written up to remove. All of it is deleted and the call sites
  go through `std/string` and `std/array`; `grep -c "mut "` over `examples/apps/`
  went from 192 to 161. G26 and G34 close here, because a function that ships and
  a workaround that survives it are different claims.
- **Two formatter defects** (S). The short-list branch short-circuits past the
  width check, so a two-argument call with a nested lambda is emitted at any
  length (137 columns observed). And D27 asks for canonical ordering of annotation
  *kinds*, not of repeated arguments within one kind, so sorting `@example`
  arguments costs the author's sequence for nothing.
- **Three findings that were not gaps** (docs) — ✅ **done** (G55). Multi-line
  strings, `math.max`, and the two-import rule for `std/time` all already work;
  the author reimplemented around them. Each is now documented where the reader
  was looking: AGENTS.md's "Template strings" section says a raw newline inside
  `"..."` is kept verbatim, the stdlib reference lists one call per line instead
  of slash-grouping them (grepping it for `math.max` used to find nothing), and
  both AGENTS.md and the reference show the two `std/time` import lines with what
  each one buys. The reference preamble was wrong about that last one and is
  corrected: `import std/time` alone gives you `time.Duration.ms(5)`.
- **`E0300` still says "value-position match arm"** (XS) — ✅ **done.** The
  remaining block-arm rejection fires only for a `match` nested inside a larger
  expression, so the wording pointed at a position that works. It now reads "a
  block body in an arm of a match nested inside a larger expression", and the
  sibling `?` rejection is `TryInNestedExpressionMatch` rather than
  `TryInValuePositionMatch` for the same reason: a value-position match takes
  both, and the name said otherwise.

## bracket dogfood trip — the build said ok on a false assertion

The loop pointed at `examples/apps/bracket.glyph`: a single-elimination
tournament bracket, seeding, results, standings. Twelve findings came out of it,
six of them re-reports of gaps already on this page. The one worth the release is
none of the syntax complaints: the app carried its own `@example`s, and the build
that runs them was reporting success without running them. The Next marker has
moved on to the shortlink trip below.

### 0.1.43 — Shipped · `@example` runs on every build, including `--json`

D23 says the compiler runs every `@example` on `glyph build` and a failing one
fails the build. The implementation put that behind `--test`, so the default
build, the one CI runs and the one an agent runs in a loop, skipped the
project's own assertions and printed `tsc --strict passed`. The `--json` path
was worse: the JSON emitter diverges and was reached before the example block,
so even `glyph build --test --json` printed `"ok": true, "errors": 0, "tsc":
"passed"` on a project whose `@example` asserted `2 * 2 == 999`. The channel
agents read could not report a failing colocated test even when explicitly asked
to run one.

The checks now run on every `glyph build`. `--no-test` opts out and says how many
tests it skipped, so the bypass is on the record rather than silent; `--test` is
still accepted and ignored. Under `--json` the examples run before the JSON is
built, and the object carries an `examples` field (`total`, `ran`, `skipped`,
`failures`) whose failures fold into `errors` and `ok`, so the two channels
report the same verdict and the same exit code. A missing `tsx` on a project that
has examples is handled the way a missing `tsc` already was: no success line,
`"ok": false`, exit 2, because a build that could not run its verification must
not look verified. Projects with no examples pay nothing, which is now pinned by
a test rather than assumed.

On the app that found it, a plain `glyph build src --out dist` now prints `23
example(s) passed`; flip one of bracket's own assertions and the same command
prints `example failed: bracket example #16` and exits 1, with the `tsc --strict
passed` line withheld. The repo's own CI step, `glyph build ../examples`, reports
`100 example(s) passed` and still exits 0, so default-on cost nothing that was
already green. The edge that remains: the runner shells out to `tsx`, so a
project that has examples and no `tsx` on `PATH` now fails at exit 2 where it
used to build quietly. That is the intended trade (a build that cannot run its
verification should not claim to be verified), but it means an offline or
minimal-image build of a project with examples needs either `tsx` installed or
`--no-test`.

### Still open from this trip

- **`?` is rejected in an expression-position `match` arm** (M) — ✅ **done.**
  `None => lookup(0)?` failed with E0300 while the block form `None => {
  lookup(0)? }` compiled and emitted correctly. The arm body now goes through
  `emit_value`, and the nested-IIFE case is rejected with the rule it breaks
  (E0302) rather than a false "not implemented yet". A `?` in the *scrutinee* is
  still refused (E0303); see the batch item in the rolling lane.
- **A two-binding `for` picks the record lowering on an unknown iterand** (G37,
  second sighting). Re-scoped by the settle trip below: the case where the
  iterand came out of a `match` (directly or through a field of a match-bound
  record) is fixed, because the checker now gives a `match` its arms' type. What
  is left is the iterand whose type is honestly unknown. `stdlib_fn_ty` models
  about a dozen functions, so `string.split` and the `array` combinators return
  `Ty::Unknown` and the emitter chooses between two incompatible lowerings on a
  type it does not have. Two directions, both needing the orchestrator: model
  stdlib return types (durable, large) or hard-error on an unknown-typed iterand
  (cheap, wedge-shaped).
- **Stdlib breadth, fourth sighting** (S). Same list as the linkcheck trip.
- **Escaping `$` in a template literal is a docs gap, not a lexer gap** (docs).
  `"${string.join(p, \".\")}"` works today and is regression-tested; the app
  author invented a `const SPACE = " "` around a limitation that does not exist.

## shortlink dogfood trip — a green build running yesterday's shim

The loop pointed at `examples/apps/shortlink.glyph`: shorten a URL, redirect a
visitor, count the hits. The loudest finding is `std/http`: `Response` is
`{ status, body }` with no headers, `respond` hard-codes the content type, and
the constructors are `json` and `text`, so a 302 and an HTML page are both
unspellable and the server story covers JSON APIs and nothing else. That is a
real blocker, and it carries a surface-shape decision for the orchestrator. It is
not what this release fixes, because the trip also turned up the only false
*green* in the batch, and a stale-cache lie has to go first: shipping
`http.redirect` would push people to migrate off the shims they wrote to work
around it, editing `extern/` on every iteration, straight into the bug below.
The Next marker has moved on to the second shortlink trip below, which is where
the `std/http` response surface is fixed.

### 0.1.44 — Shipped · A change to an `extern` shim busts the `glyph run` cache

`glyph run` keys its build cache on `source_fingerprint`, which hashed every
`.glyph` file and every `<src>/.types/**/*.d.ts` and stopped there.
`<src>/extern/**` was not hashed, even though `runtime.rs` stages it verbatim
into `<out>/extern` and the generated tsconfig type-checks it, the exact
condition the comment above the `.types` block states as the rule. Because
staging only runs on the rebuild path, and because the output prune deliberately
skips `extern/` so a staged shim survives a rebuild, the stale copy also survived
a cache hit. Editing a hand-written shim and running `glyph run` printed a clean,
type-checked build and executed the previous version of the TypeScript. Nothing
on screen told the two apart.

The fingerprint now hashes `<src>/extern/**/*.ts` and `.tsx` the way it hashes
`.types`: relative path as well as contents, so renaming or deleting a shim busts
it too. The extern walk follows symlinks, which closes a second door onto the
same failure. The `.glyph` walker skips symlinks outright, and this app ships a
symlinked shim in `extern/` today, so a link would otherwise have been invisible
to the cache. Following reads the target's contents while still hashing the
link's own path, and a canonical-path set stops a symlink cycle. Five tests pin
the behaviour: editing, deleting and adding a shim each change the fingerprint, a
symlinked shim whose target changes changes it, and a README under `extern/`
does not, so the cache is not busted for files no one type-checks.

Out of scope here, both recorded rather than fixed: `extern/` resolves against a
build root that differs between `glyph run` and `glyph build` (loud and
self-announcing, but its fix is a real fork), and whether symlinked `.glyph`
sources should be walked at all, which is a question about whether they are
supported, not a bug to patch.

## settle dogfood trip — the only branching construct had no type

The loop pointed at `examples/apps/settle.glyph`, a shared-expense settler: parse
a ledger, split expenses, minimize the transfers that square everyone up. The
review that came back listed sixteen findings, twelve of which already carried a
G-number. The one taken here is the one with no design question attached.

### 0.1.45 — Shipped · A `match` whose arms agree has their type

Glyph has no `if`, so `match` is the branching construct, and the typechecker
ended its `Expr::Match` arm by recording `Ty::Unknown` for every `match` in every
program. A value taken out of a branch therefore reached the rest of the compiler
untyped, and two things went wrong downstream. Field access on it was left to
`tsc`, so a typo in a field name came back as `[TS2339]` on generated TypeScript
instead of Glyph's own `E0210`. Worse, a two-binding `for` over one of its fields
picked the wrong lowering: `iter_is_array` asks the type map whether the iterand
is an `Array` and falls back to `Object.entries` when it cannot tell, so
`for i, row in w.rows` bound `i` to the string `"0"` and the program printed
`01:a` where it should print `1:a`. `glyph build` was clean, `tsc --strict` was
clean, and the output was wrong with no diagnostic anywhere.

A `match` now takes its arms' type when they agree. The join is equality and
nothing more: each arm contributes its value type, an arm that ends in
`return`/`break`/`continue` diverges and contributes nothing, and if the
contributing arms disagree or any of them is undecidable the result is `Unknown`
exactly as before. No widening, no union, no subtyping, and no bottom type, which
Glyph does not have. Making the join useful needed one prerequisite:
`bind_arm_payloads` handled module-local unions only, so `Ok(v)` over the prelude
`Result` bound `v` to nothing and the arm had no type to contribute. It now reads
prelude payloads off the scrutinee's type arguments too.

The visible effect is a workaround that stops being necessary.
`let w = match get() { Ok(v) => v, Err(_) => return 1, }` gives `w` the success
type, so `w.rows` iterates with a numeric index and `w.rowz` is an `E0210` from
Glyph rather than a `TS2339` from `tsc`. Nothing new became spellable and no rule
was relaxed; the compiler simply stopped throwing away what it already knew.
Rebuilding `examples/` produces byte-identical output apart from `settle`.

Deleting the annotation from the app took one more piece. `settle` gets its
ledger from `WireLedger.parse(decoded)`, and the checker had no signature for the
`parse` that a type declaration's runtime descriptor emits, so the scrutinee was
still undecidable and the join still had nothing to join. `T.parse` now types as
`Result<T, Array<Issue>>`, read off the shape the emitter writes, for the
non-generic record, tagged-union, and refined-primitive types that get a
descriptor. A plain alias (`type Cents = int`) emits none and gets none, and a
generic record's descriptor threads a runtime checker per type parameter, so its
arity differs and it stays `Unknown`. This is the boundary between untrusted
input and typed data, so leaving it opaque undid every inference downstream of
it. With both pieces in, `let rows: Array<WireExpense> = wire.expenses` and its
"the annotation is load-bearing" comment are gone from
`examples/apps/settle.glyph`, the loop below it reads `for i, w in wire.expenses`
and still binds a number: feed it a ledger whose third entry has three decimal
places and it reports `expense 3`, not `expense 21`.

Also in the release: the app ships as `examples/apps/settle.glyph`. Record a
shared expense, split it evenly, by exact shares, or by weights, print the
balances, and settle up in as few payments as possible, with every amount an
exact whole number of cents. It builds and runs under the same `examples/` gate
as the rest.

### Still open from this trip

- **The unknown-typed iterand** (G37, what remains). An iterand whose type the
  checker honestly does not have, a call into one of the stdlib functions
  `stdlib_fn_ty` does not model, still takes the record lowering and binds a
  string index with a clean build. Modeling stdlib return types (durable, large)
  or hard-erroring on an unknown-typed iterand (cheap, wedge-shaped) are both
  orchestrator calls, unchanged by this release.
- **`parse` on a generic type stays `Unknown`** (S). A generic record's
  descriptor threads one runtime checker per type parameter, so its `parse` has a
  different arity than the non-generic one, and typing it needs the checker to
  know what the call site passed.
- **Arms that disagree stay `Unknown`** (M). The join is equality, so a `match`
  producing different types per arm gives the rest of the compiler nothing, same
  as before. Improving on that means widening or a union type in Glyph's own
  checker, which is a type-system decision rather than a patch.

## shortlink dogfood trip, second pass — a server that could only speak JSON

The loop went back to the URL shortener with the `extern/` shim removed and asked
for the app in plain Glyph. It could not be written that way. `std/http`'s
`Response` was `{ status, body }`, `respond` hard-coded the content type from the
body's shape, and the only constructors were `json` and `text`, so a `location`
header and a `text/html` page were both unspellable. The app declared its own
`Response`, hand-wrote a server on `node:http` behind a `.d.ts` the checker does
not look inside, and ran as unverified TypeScript wearing Glyph syntax. The stale
`extern/` cache that forced the deferral last time shipped in 0.1.44, so the
reason to wait is gone. The shim is gone with this release:
`examples/apps/shortlink.glyph` imports no `extern/*` and no Node builtin, and
`examples/extern/web.ts` is deleted. 0.1.47 below was picked off this trip's open
list. 0.1.48 sits after it in shipping order but belongs to the linkcheck trip
above, whose four stdlib findings it closes. No release carries the Next marker
now; the next trip picks from what is left of both lists.

### 0.1.46 — Shipped · `std/http` can serve a web page

`Response` gains a required `headers: Record<string, string>` field. Required
rather than optional: nothing in the repo builds a `Response` literal (every
construction goes through `json`), the channel cannot then be forgotten, and
reading `resp.headers` never needs an absence check. `json` and `text` keep their
signatures and fill in their own content type. Three constructors join them:
`html(status, body)` for a `text/html` page, `redirect(status, location)` for a
30x with a `location` header, and `with_header(resp, name, value)`, which returns
a new `Response` because Glyph has no record-field mutation.

`respond` stops hard-coding. It writes the response's headers through to
`writeHead` and infers a content type from the body only when the headers do not
already carry one, compared case-insensitively, so every program written before
this release puts the same bytes on the wire. Header values are sanitized on the
way out: every character Node's `writeHead` rejects, which is everything outside
`\t`, printable ASCII, and Latin-1, is dropped. CR and LF are the security half,
since a `location` built from a query parameter is otherwise response splitting.
The rest is the availability half, and the app found it. `writeHead` throws
`ERR_INVALID_CHAR` from outside `respond`'s `try`, so redirecting to a target
with an emoji in it killed the server, and the emoji came from a form field.
Stripping only CR and LF left that open; stripping the full rejected set closes
both.

The client half fills `headers` from the fetch response with the names
lowercased, which closes the observable half of G52. `form(req)` parses an
`x-www-form-urlencoded` body with `URLSearchParams`, which gets `+`-as-space and
percent decoding right; it reads `req.raw`, so `req.body` is unchanged for
programs that already parse it themselves. A key repeated in the body keeps the
last value.

The typechecker models `html`, `redirect`, and `with_header` as returning
`http.Response`. Without that they type `Unknown` and a handler's
`Ok(http.html(...))` is checked only by `tsc` on the emitted TypeScript, which is
the exact leak the change exists to close. A tsx-driven integration test asserts
the wire behaviour against a Glyph handler: a 302 with `location: /page`, a
`text/html` content type, a custom header, `application/json` still exactly that,
a 500 from `Err`, CR/LF stripped out of an injected `location`, an astral
character stripped from a `location` with the server still answering afterwards,
and a form body decoded.

The app is the proof. `examples/apps/shortlink.glyph` went from 615 lines to 494:
its own `Response` type, its `node:http` server, its form parser, and its
keep-alive loop are all deleted, and it still serves the 302 and the HTML page it
was written for. `serve` stays pending while the server listens, so `main` needs
no loop to stay alive, and a bind failure comes back as an `Err` to match on.

Pillar: abstraction, buying verifiability. An application boundary that had to be
hand-written TypeScript moves back inside the checker, and a redirect or a content
type now has one spelling to grep for.

### Still open from this trip

- **The bound half of G52** (M). No request timeout, no redirect policy, no
  `head`, and no final URL on a response, so a client still cannot tell that it
  followed a redirect, only where it ended up.
- **`std/url` percent encoding** (S). Encoding and decoding a URL component is
  still hand-written in the app.
- **`std/string` breadth, fifth sighting** (S). *Scheduled and shipped as 0.1.47,
  below.* `slice` and `index_of` had been reported by five consecutive trips.
- **`string.split(s, "")` splits UTF-16 units** (S). Splitting on the empty string
  breaks a surrogate pair, so a non-BMP character comes apart. Clean build, clean
  `tsc`, wrong output.

### 0.1.47 — Shipped · The eleven functions the apps kept hand-rolling

Five consecutive dogfood trips reported the same thing: `std/string` and
`std/array` are short of the basics, so every app writes them again. This release
adds them and then deletes the copies.

`std/string` gains `repeat`, `pad_start`, `pad_end`, `slice`, `index_of`,
`replace_all`, `trim_start`, and `trim_end`. `std/array` gains `fold`,
`index_of`, and `flat_map`. Each one is a wrapper in `runtime/std/` plus a name
in the resolver seed, and the seed is what turns a typo into E0105 instead of a
`tsc` error about a property on a module object. Two names the reference already
documented were missing from that seed and are added with them: `json.parse_with`
and the `fs.FsError` type, both of which a named import rejected as unknown.

Four of them diverge from their TypeScript namesakes on purpose. `repeat` clamps
a negative count to `""` where TS throws, which is what makes the natural call
`repeat(pad, width - len(s))` safe instead of a crash on a string that is already
too long. `pad_start` and `pad_end` leave a string that is already at least
`width` long alone. Replacement ships only as `replace_all`, so there is no
first-only form to confuse with `String.prototype.replace`, which quietly does
one occurrence. Both `index_of` functions return `Option<number>` rather than the
`-1` sentinel, which is the point: the sentinel is a number that type-checks
everywhere a real index does. `string.slice` matches `array.slice`, exclusive
`end` and negative indices counting back from the end. Indices are UTF-16 code
units, the same space `len` and `split` already use.

Then the apps. Seven programs in `examples/apps/` carried hand-rolled copies, and
all of them are gone: 191 lines of helper bodies deleted outright, 467 deletions
against 256 insertions, net 211 lines. Beyond the copies, `linkcheck`'s four line
scanners take a `string` instead of an `Array<string>` of characters, its
`index_from` sentinel became a `match` on `Option`, and `shortlink`'s five
`regex.replace_all` calls (a compiled regex per literal needle) became five
`string.replace_all` calls. Two comments that stated the gap in the author's own
words are deleted with the code that needed them.

`array.fold` is the pillar item, and G34 named the reason: with no fold, every
accumulation is a `mut` in a loop, so `grep mut` returns arithmetic that mutates
nothing a reader cares about. Seventeen fold sites landed and `grep -c "mut "`
over `examples/apps/` went from 192 to 161.

The rewrite is proved by output, not by review. A harness ran all seven apps
against fixed fixtures before and after, 26 output files covering five CLIs'
stdout and exit codes including their error paths, minesweeper's full transcript
on two seeds, and shortlink's HTTP surface. Every file is byte-identical.
Shortlink's surface is what exercises the changed lines end to end: all five HTML
entities in a rendered row, percent decoding round-tripping a 4-byte emoji, and
the persisted `shortlink.json`.

Pillar: greppability first, through the `mut` count, then abstraction. The
`@example` count in the repo drops from 127 to 113 because fourteen assertions
rode on the deleted functions; the equivalent assertions are in
`glyph-cli/tests/integration.rs` against the stdlib itself.

### Still open from this trip

- **Codepoint-aware `chars` and `char_at`** (decision, then S). Shipping them
  means deciding whether `std/string` indexes UTF-16 code units, which is what
  `len` and `split` do today, or codepoints. The two answers disagree on any
  non-BMP string, and shipping both index spaces in one module is worse than
  either. Same root as the `string.split(s, "")` bullet above. *Decided in
  0.1.53: UTF-16 code units, and no codepoint accessor. G50 reads `[DECIDED]`.*
- **`iter.take_while`** (decision, then M). There is no `std/iter` module and
  never was. `std/stream` is a test-data generator, not a lazy sequence, so this
  is a design rather than a wrapper. D21's prose no longer cites the function.
- **Model the two `index_of` returns in `stdlib_fn_ty`** (S). Unchanged from the
  linkcheck list above: a `match` on `index_of` with no `None` arm builds clean
  and throws at run time, and fixing it needs a min/max arity in the table
  because `string.index_of` has an optional third argument. *Half done in
  0.1.53: `array.index_of` is modeled; `string.index_of` still waits on the
  arity range.*

### 0.1.48 — Shipped · The link checker stops working around the stdlib

The linkcheck trip filed thirteen findings and four of them said the same thing:
the stdlib cannot do this, so the app does it by hand. This release adds the four
capabilities and then rewrites `examples/apps/linkcheck.glyph` until none of the
workarounds are left. Both halves are the release. A function that ships and a
workaround that survives it are different claims, and the app is what settles the
second one.

`std/fs` learns about directories. `read_dir` returns the entry names one level
down, `is_dir` answers with a `bool` the way `exists` does, and `stat` returns a
`FileInfo` of `is_dir`, `is_file`, `size` in bytes, and `modified` in epoch
milliseconds, which feeds `time.format_iso` directly. `read_dir` and `stat` are
modeled in `stdlib_fn_ty`, so `?` on them picks its error type the way
`read_text` does. `modified` is truncated to a whole number because node reports
mtime as a float and `int` is a checked boundary, so the raw `mtimeMs` would have
failed at run time. No recursive `walk` and no glob shipped: a walk is `read_dir`
plus `is_dir` plus `path.join` in about ten lines, and a walk primitive is
surface the gap did not ask for.

`FsError.kind` stops being one constant. `ErrorKind` is now the closed set
`NotFound`, `IsADirectory`, `NotADirectory`, `PermissionDenied`, `AlreadyExists`,
and `Other({ code })` carrying the raw errno for anything unnamed. EACCES and
EPERM both arrive as `PermissionDenied`, so those two codes are the ones that do
not survive.

`regex.captures_all(pattern, text)` returns the capture groups of every match,
one inner array per match, holding groups 1 onward. It follows `captures` on both
conventions that differ from JavaScript's `matchAll`: the whole match is not in
the array, and a group that did not participate is `""` rather than `undefined`.

`task.pool_settled(limit, tasks)` is `pool`'s worker loop with each call guarded.
One `Settled<T>` per task, in order, never rejecting, with the same clamp of a
`limit` below 1. `pool`'s doc comment now points at it, and the docs say what
fail-fast actually costs: a `pool` rejection discards the other workers' results,
it does not stop them, because the remaining workers keep draining the queue.

Then the app. The only raw-node import in `examples/apps/` is deleted with its
`readdirSync` call. Both `e.kind.tag == "EISDIR"` probes are gone, replaced by a
`match` on `fs.ErrorKind` in a new `fs_reason`. The 180-line character scanner is
gone: `scan_inline`'s stepping loop, `type Step`, `skip_code_span`,
`autolink_at`, `looks_like_autolink`, `bracket_link_at`, and the `index_of` chain
in `reference_definition` are 97 deleted lines replaced by two patterns and a
12-line dispatch. `task.pool` became `task.pool_settled`. 122 deletions against
114 insertions, and the insertions are not a wash: three of them are new
capability rather than restored workaround, and about 24 lines are the design
comment explaining the scanner's group layout.

The alternation problem the `captures_all` design was expected to lose on was
solved rather than traded away. Write each branch's group around the whole
construct instead of around its payload and a group that fired can never be
empty, because it starts with a backtick, `[`, `!`, or `<`. The possibly-empty
capture, the link target, nests inside a discriminator group and is never asked
whether it fired, so `[]()` still reports an empty target. Putting the code span
first in the alternation reproduces the "a link inside backticks is not a link"
rule without tracking an offset, which is what made the offset unnecessary too.

Three things are proved by running, not by review. Output first: the app's own
header block and a hostile fixture (two levels of nesting, uppercase `.MD`, code
spans including an unterminated one, `[]()`, nested brackets, angle-bracketed
targets, an autolink containing a bracket pair) are byte-identical against a
rebuilt pre-batch binary. Then the two behaviour changes, each measured against
the app it replaced. On a tree with a `chmod 000` subdirectory the new app prints
`permission denied` and counts the path unreadable; the old app dropped that
directory with no row and no mention. With a throw injected into one of three
fetches, `pool_settled` printed all three rows and named the failing URL, while
`pool` printed nothing and died on an unhandled rejection, losing both surviving
results.

Pillar: verifiability. Every one of these workarounds was a place where the
program's behaviour was decided by an untyped string comparison, an unhandled
rejection, or a hand-written scanner that no type could constrain.

### Still open after this release

- **Nothing checks a `match e.kind`** (decision, then M). The typechecker models
  what a stdlib function returns and has no field model for a stdlib named type,
  so `e.kind` types as unknown, `required_variants` resolves nothing, and a match
  missing `PermissionDenied` builds clean, passes `tsc --strict`, and throws at
  run time. Keep the `else` arm. Fixing it means a stdlib named-type table, which
  is the general "model the stdlib's types, not just its signatures" question G39
  and Q21 already own, and the same root as the unmodeled `index_of` returns
  above.
- **Whether `read_dir` should sort** (decision, then XS). It returns entries in
  node's order today, which differs across platforms and filesystems, so a
  reproducible report sorts them itself and `linkcheck` does. Sorting ascending
  in the stdlib would make every report deterministic and every `@example` on a
  directory reproducible, at the cost of deviating from `readdirSync` in a way
  that has to be documented. Not decided here.
- **`captures_all` still has no offsets** (S). A scanner whose discriminator can
  legitimately match empty, or one that needs to know where in the line a match
  started, wants a `Match` record carrying the offset and the whole match text.
  `linkcheck` needed neither in the end, so this is no longer blocking an app in
  the repo.

### 0.1.49 — Shipped · Four things `tsc` caught and Glyph did not

This one is not a trip's release. It picks off four gaps filed by four different
apps, and what they have in common is the failure mode: `glyph build` said ok,
and the mistake surfaced later, either as a `tsc` error on generated TypeScript
or as `undefined` at run time. Four new diagnostics, all against your own source.

`await` in a plain `fn` used to build clean and fail at `tsc` with TS1308. It is
now **E0222**, "`await` is only valid inside an `async fn`" (G44). The innermost
enclosing callable decides, which is TypeScript's rule too, so a synchronous
lambda inside an `async fn` is flagged and the same lambda written `async fn(...)`
is not. One case is deliberately permissive: an `await` in a module-level `const`
initializer has no enclosing callable and the emitted ESM accepts top-level
`await`, so nothing is reported.

`X => {}` in a `match` you assign used to emit `case X: { break; }`, and the
binding was `undefined` while the type said otherwise. `tsc` caught it as TS2366
only when the function had a declared return type. It is now **E0223**, "this
match arm produces no value, but the match is used as a value" (G48), for an
empty block or a block whose tail is a `let`, `mut`, `for`, or `loop`. It fires
where the position is decidable: a `let`, a `mut`, a `return`, or the tail of a
callable with a declared non-`void` return type, recursing into nested
value-position arms. A statement-position `X => {}` is untouched, and none of the
nine such arms across `examples/apps/` fires.

A bare `x = e` used to report "unexpected token: Equals". It is now **E0008**,
"assignment requires `mut`", with the D5 help line and an `--explain` body
carrying the before and after (G35). It covers `r.field = e` and `xs[0] = e`, and
it fires inside a `match` arm, where the old message was "expected `,` after
match arm".

`?` in an expression-form arm (`None => lookup(b, name)?`) used to be refused as
"not implemented yet" while the same code in a block arm compiled and emitted
correctly (G24). The arm body now goes through the same hoisting path as any
other statement value, so the unwrap and its early `return` land inside the arm's
own `case`. `examples/apps/bracket.glyph` lost the block-arm workaround and the
comment explaining it, eight lines for two, and the emitted TypeScript is
byte-identical. The two positions that stay rejected say why now instead of
claiming a missing feature: **E0302** for an arm of a `match` nested inside a
larger expression (that match is a closure, so the `return` would leave it), and
**E0303** for a position with no statement slot at all, such as a `match`
scrutinee.

A parser fix came with E0223. `A => { "Content-Type": "application/json", }` now
parses as an object literal rather than a block, because a block cannot begin
with a string literal followed by `:`.

Pillar: verifiability. Every one of these was a rule Glyph relies on that Glyph
itself did not check.

### Still open after this release

- **There is still no way to spell an empty record in an arm** (decision, then
  S). `{}` is a no-op block in statement position, used nine-plus times across
  the corpus, so it cannot silently become the empty record. E0223 reports the
  value-position case rather than answering the question, and
  `examples/apps/linkcheck.glyph` still carries the `no_cache()` constructor.
  Four candidates: a named `record.empty<V>()`, a context-sensitive `{}`,
  rejecting `{}` as an arm body outright, or a grouping node in the AST so
  `=> ({})` survives `glyph fmt`. The last one is also the fix for G60 below.
- **Top-level `await` is left permissive** (decision). An `await` in a
  module-level `const` initializer means a module has implicit async
  initialization with nothing in the source marking it. The spec has no stance,
  and flagging it would be a new language restriction, so it is a spec question,
  not a checker bug.
- **E0223 cannot judge an unannotated lambda** (S). It needs a decidable
  position, and a callable with no declared return type gives it none, so
  `array.map(xs, fn(x) { match x { A => {}, ... } })` is still silent. Closing it
  means a second backstop in the emitter, driven by arm termination, which is a
  second diagnostic path for one rule and invisible to the LSP.
- **`?` in a `match` scrutinee** (S). `match load(p)? { ... }` is rejected as
  E0303. Supporting it means hoisting the scrutinee's unwrap ahead of the lowered
  `switch` at four call sites, and extending the IIFE guard to cover the
  scrutinee.

### 0.1.50 — Shipped · `glyph fmt` stops being the thing that breaks your build

Three formatter gaps, one of them a correctness bug and the other two the layout
complaints that had been sitting behind it since G23.

The correctness one is G60. An arm that means "the empty record" is written
`=> ({})`. That parses as a parenthesized object literal, builds, and passes
`tsc --strict`. `glyph fmt` reprinted it as `=> {}`, which is an empty *block*,
and the formatted file no longer built: it failed the E0223 check the previous
release added. A formatter that turns a green program red is the worst thing a
formatter can be, because you run it without reading the output. Arm-body
position is the only place in the grammar where a leading `{` is ambiguous, so
the printer now parenthesizes exactly the shape that would reparse as a block.
`X => ({})` reprints as `X => ({})`, is a fixed point, and emits the same
TypeScript. The AST is unchanged; a general `Expr::Paren` node was considered
and the reasoning against it is under G60 in
[`../dogfooding-gaps.md`](../dogfooding-gaps.md).

G54 is the width bug. The width-aware formatter shipped in 0.1.34 with an
exemption: a list of one or two elements skipped the width test entirely. That
exemption short-circuited the newline test too, so `array.map(xs, fn(...) {
... })` stayed on one line at any length, and the formatter's own fixed-point
output held 142-column lines while the guide said it wrapped at 100. `INLINE_MAX`
is deleted and the test runs at every element count. A second bug had to go
first: the inline candidate is rendered into a detached buffer that started at
column zero, so a list nested inside a candidate measured its width from the
wrong column and thought it fit. The printer now carries the real starting
column into the capture. The same entry's second half is annotation ordering.
D27 orders annotation *kinds*; a `raw_args` tiebreaker was also sorting the
arguments of repeated annotations of one kind, so three `@example` lines came
back in an order the author did not write. That tiebreaker is gone and `sort_by`
is stable, so repeats keep source order.

G29 is the arm body. A one-statement arm body was always exploded to three lines,
because the parser wraps it in a synthetic block and every block printed
multi-line. It now prints as `X => { break }` through the helper a one-statement
lambda body already used.

`examples/apps` is reformatted under the new rules and `glyph fmt --check` is
clean and idempotent on it. All 116 examples build and pass `tsc --strict`.

Pillar: diff stability, with verifiability underneath it. A formatter you cannot
run without reading the diff is not a formatter, and G60 made "run `fmt`, then
run `build`" an ordering you had to know.

### Still open after this release

- **The printer has no chain-aware path** (M, decision first). G18. A long
  `a.b(x).c(y).d(z)` still never breaks at a `.`; the only breakable point is an
  argument list and it takes the innermost. The width fix makes this more visible
  rather than less: three sites in `examples/apps` now break an inner argument
  list in the middle of a `||` chain instead of sitting on one over-wide line.
  The layout rule is an undecided fork, in the polish lane below.

  Half the rule is now decided: when a chain does not fit, break before every
  link, one link per line, indented one level under the receiver. A partial break
  is what makes the diff unstable when a later edit changes the width, and diff
  stability is the pillar this serves. `await` and `?` stay attached to the
  expression they apply to. Of the two questions that followed, the operator one
  is answered and shipped; the receiver one still blocks the `.`-chain path:

  - **What counts as the receiver.** Glyph's stdlib is namespaced-function style,
    so a literal every-link break turns `array.map(...).filter(...)` into
    `array\n  .map(...)\n  .filter(...)` and `grep "array.map("` misses it.
    Greppability is a wedge pillar, so this is the one place the layout rule
    collides with one. Either the literal rule (uniform, one less thing to know,
    costs the `namespace.fn(` grep) or a syntactic first-group rule the formatter
    can evaluate without a symbol table ("a bare-identifier receiver keeps its
    first `.`-link; all later links break"), which keeps `array.map(` greppable
    and matches how every example already reads. The minimum for a chain to be
    breakable at all (two `.`-links, so `string.repeat(s, n)` never explodes) and
    whether a call-free member path like `world.player.location.name` breaks are
    part of the same question.
  - **Operator-chain break style** — ✅ **done, leading operator.** A `&&`,
    `||`, or `??` chain that does not fit breaks one operand per line with the
    operator leading the continuation line, indented one level, which is what
    the three damaged `||` sites needed. Leading because the operator lands at a
    fixed column where `grep` finds it, it matches Glyph's own leading-`|` union
    form, and adding an operand touches one line instead of two. Trailing (what
    Prettier and rustfmt do) re-parses identically, so nothing but style rode on
    it. Only the top-level run of one operator flattens: `a && b || c && d`
    breaks at `||` and keeps each `&&` group whole, so the shape shows the
    precedence rather than the width. The D1 guard is a printer flag set only
    under a `{ ... }` the printer opened, which implies bracket depth of at
    least one; it is one-sided on purpose, so a bracketed expression outside any
    block loses a break the parser would have taken rather than risking one it
    would not. `examples/apps` is reformatted: the three damaged `||` sites read
    as chain breaks, every emitted `.ts` in the tree is byte-identical to the
    pre-reformat build, and a second `glyph fmt` pass changes no byte.

  The verified constraint either answer has to respect: newlines are tokens only
  at bracket depth zero, so a break before a `.` or an operator is invisible to
  the parser inside any `(`/`[`/`{` (which includes every function body), and ends
  the statement outside one. A module-level `const` initializer, a `where`
  predicate, and an annotation's raw args therefore need a depth guard rather than
  a workaround, and mirroring the lexer's own bracket-depth rule is what makes
  that guard verifiable instead of a heuristic (`self.indent > 0` is not a safe
  proxy: a multi-line union body is indented with no enclosing bracket).
- **The width check stops at the closing delimiter** (S). It measures a list up
  to its own `)` and misses any suffix printed after it, so a `fn` signature
  whose ` -> T {` tail crosses 100 columns reads as fitting. Three of the six
  over-wide code lines left in `examples/apps` are that shape. `lambda_block`
  misses the same way on a different shape: it inlines a one-statement body
  whenever the captured statement holds no newline, without checking the column,
  which is what puts the two `map_err` lambdas in `bracket.glyph` over 100.
- **A list whose inline candidate is intrinsically multi-line explodes one
  argument per line** (S) rather than letting a trailing lambda keep hugging the
  call, which is more vertical than Prettier's rule for the same shape.
- **`examples/corpus` and the numbered examples are not `fmt`-clean** and would
  change under a reformat. They were not clean before this batch either, for
  unrelated reasons such as redundant parentheses. That reformat wants its own
  commit.

### 0.1.51 — Shipped · Ask whether it compiles without running it

Four items off the CLI and docs backlog, and the three findings an adversarial
read of that batch turned up before it shipped.

`glyph check [path]` is the headline (G28). Every dogfooding trip that wanted a
type-check answer for one file had the same experience: `glyph build one.glyph`
stopped at "source path is not a directory", so the only door into the
typechecker was running the program. `check` takes a `.glyph` file or a
directory, runs `build`'s pipeline into a temp directory it deletes on the way
out, and runs `tsc --strict` over the emitted TypeScript unless you pass
`--no-tsc`. Nothing lands in your tree and nothing executes: the regression test
checks a program whose `main` writes a sentinel file, then asserts the file is
absent and stdout is empty. On `examples/apps/expenses.glyph` it reports "10
module(s) checked, no diagnostics" in about a second and leaves `git status`
clean. `glyph build one.glyph` still refuses a file, but the refusal now names
`glyph check <file>`.

Two things about `check` are worth knowing before you wire it into a hook. A file
is checked in the context of its directory, so a sibling's error fails the check,
the same way it does under `build` and `run`; that falls out of reusing one
engine and keeps the three commands from disagreeing about a tree. And `check`
does not run your `@example` / `@doc @run` tests, because running them runs your
code, which is the thing it promises not to do. Both are in the command's help
text and in the troubleshooting guide, and the first has a test pinning it.

`glyph build` no longer prints a green line above its own red one (G42). The
Glyph-stage summary sat before the `tsc` gate, so a build that failed
type-checking opened with "no diagnostics". It now prints after both gates,
beside the `tsc --strict passed.` line that was already held back for the same
reason. A red build does not print it at all; a green build's transcript is
unchanged. Both orders have tests.

Hyphenated arguments reach your program through `glyph run` (G36). The wall was
narrower than the backlog claimed: clap went raw once it had consumed a non-hyphen
positional, so `glyph run app.glyph data.csv --min -12.50` already worked and
`glyph run app.glyph --min -12.50` did not. `allow_hyphen_values` on the trailing
argument closes that. A flag glyph knows still binds to glyph wherever it
appears, so `glyph run app.glyph --no-tsc` is unchanged and `--` remains the
answer for a program flag that collides. `examples/apps/expenses.glyph` grew a
`--min AMOUNT` filter that exercises it with an exact `Decimal` comparison.

G55 was the docs round it asked for: three things the author of a previous app
reimplemented because they could not find them. `math.max` was reachable only
through a slash-grouped line, so grepping the reference for it found nothing;
every grouped line on that page is now one call per line, and
`examples/apps/linkcheck.glyph` lost its hand-rolled `max_of`. The two-import
rule for `std/time` was stated once in a page preamble that had it wrong, and is
now in both AGENTS.md and the reference with what each line buys. Multi-line
strings are described in AGENTS.md's "Template strings" section, with the caveat
below.

The review of this batch fixed three more things in the same session. `--no-tsc`
is now the one name for the TypeScript stage on `build`, `check`, and `run`;
`--no-check` stays as a hidden alias on `build` and `run` so existing scripts keep
working. `build --json` reported `ok: true` and exit 0 on a machine with no
`tsc`, while the same build's text path and `check --json` both reported failure;
a requested stage that could not run is not a pass, and `build` now agrees. And
`check` stopped hashing every source file to name a scratch directory it deletes
immediately.

Pillar: verifiability. The answer to "does this compile" was previously only
available by running the program, and a build that had failed opened by saying it
had not.

### Still open after this release

- **`glyph fmt` collapses a multi-line string that interpolates** (S, G62) —
  ✅ **done**. A literal without interpolation was
  copied verbatim from source;
  one with interpolation was rebuilt through the escaper, so its raw newlines
  came back as `\n` and the whole string collapsed onto one line. Same family as
  G60: formatting must not change how a program prints. `template` now takes its
  own span (already correct in the parser, no change needed there) and copies the
  literal verbatim, sharing one helper with `string_literal`. The verbatim path is
  gated on the slice containing a raw newline, so a single-line template still
  gets its `${...}` interiors normalized. Whether to drop that normalization and
  copy every template verbatim, as `Expr::String` does, is still open.
  `examples/apps/shortlink.glyph` writes all five HTML builders as real
  multi-line strings now: the rewrite that had to be reverted before is back, the
  emitted `shortlink.ts` is byte-identical to the `\n`-escaped version, and
  `glyph fmt --check` reports the file already formatted.
- **`check` reports a sibling's error** (S, decided for now). Filtering
  diagnostics to the named file, or a `--tree` opt-in for today's behaviour, is
  an open call; `Diagnostic.file` already carries what a filter needs.
- **`check` has no example gate** and is therefore weaker than `build` on the
  same tree. Whether that stays acceptable is open.
- **The `--no-tsc` summary still says "no diagnostics"** on a build where no
  TypeScript stage ran. The line is honestly about the Glyph stage; rewording it
  is a separate call.
- **The stdlib drift guard checks presence, not call form** (S). A name in
  `stdlib_docs.rs` passes on a bare mention, so the slash-grouped lines this
  release expanded would have passed either way. Making it require the qualified
  form (`math.max(`) needs a per-module exception list for methods and prelude
  names.

### 0.1.52 — Shipped · A parsed value is a real `Result`

Three backlog items, and the one that mattered had been wrong since descriptors
first shipped.

A descriptor's `.parse` returns the prelude `Result` (G41). It used to return a
bare `{ tag, value }` object, written that way so a module with a `type` in it
did not have to depend on `std/result`. But `Result<T, E>` is that object
intersected with the `map`/`map_err` combinators, so the bare form was not
assignable to it. `return User.parse(v)` from a function returning
`Result<User, Array<Issue>>` was TS2322, and `User.parse(v).map_err(f)` was
TS2339, while Glyph's own typechecker had always reported `parse` as
`Result<T, Array<Issue>>`. That is the shape of disagreement the compiler exists
to prevent: two checkers, two answers, and the one you read in the editor was
the wrong one. All three descriptor kinds now annotate `parse` as the real
`Result` and build both arms with the prelude constructors, through one injected
aliased `std/result` import shared with the `?` lowering (`?` binds
`__glyph_err` too, and two import lines would redeclare it).

Two costs, stated exactly. The import is a value import, so a module with a
`pub type` in it now carries a runtime edge to `std/result` whether or not it
mentions `Result`; `?` and `T.schema` are paid only by the modules that use
them. And a `T.parse` call allocates the two combinator closures the
constructors build, which is what any `Ok(...)` costs, except that this one sits
on the per-request boundary path rather than on a function return. `?` applied
to a parse result and `infer_output<S>` both still work.
`examples/apps/bracket.glyph` was the app carrying the workaround: two identity
re-wrap `match`es around `Bracket.parse` and `SeedFile.parse`, each an
`Ok(b) => Ok(b)` arm beside an `Err` arm that only reworded the message. Both
are `.map_err(...)` now, and both rejection paths were run against a malformed
file.

`array.range(count)` and `array.range_from(start, end)` are in `std/array`
(the first half of G30). Two apps had hand-rolled the counted loop three times
between them, under two names, because `for` had no source for one. `range`
clamps its count the way `string.repeat` does, so `range(-1)` is `[]` and a
fractional count
truncates. `range_from`'s second argument is an exclusive end bound, which is
what `array.slice` and `string.slice` already mean by a second numeric argument,
so `range_from(2, 5)` is `[2, 3, 4]`. It was written first as `(start, count)`,
which would have made the same call return a different array than the
hand-rolled function it replaced with no type error anywhere to catch it; the
review of this batch caught that before it left the working tree. Range syntax
(`0..n`) was considered and rejected: it is grammar and a new D-decision for
something a function does at no cost, and a function stays forward-compatible
with adding syntax over it later. The typechecker models both as
`Array<number>`, so `for i in array.range(n)` binds `i` as a number rather than
`Unknown`. `upto` and `span` are deleted from `bracket.glyph` and
`minesweeper.glyph` and all 16 call sites read `array.range` or
`array.range_from`, with both apps emitting byte-identical TypeScript to what
the helpers produced.

`glyph fmt` no longer collapses a multi-line string that interpolates (G62). A
literal with no `${...}` was copied verbatim from source; one with interpolation
was rebuilt through the escaper, so its raw newlines came back as `\n` and the
whole string landed on one line. The documented multi-line form was a trap under
format-on-save, which is the same family as G60: formatting must not change what
a program prints. `template` now copies the literal verbatim through the helper
`string_literal` already used, gated on the slice containing a raw newline so a
single-line template still gets its `${...}` interiors normalized.
`examples/apps/shortlink.glyph` writes all five HTML builders as real multi-line
strings now, the emitted `shortlink.ts` is byte-identical to the `\n`-escaped
version, and `glyph fmt --check` reports the file already formatted.

Pillar: verifiability for the parse fix, since the point of the descriptor is
that the boundary is checked by the same rules everywhere, and it was not.

### Still open after this release

- **The `.`-chain layout rule** (M, G18). The `&&`/`||`/`??` half shipped in
  0.1.51 and the `.`-chain half is still blocked on one question nobody has
  answered: whether the first link of `array.map(xs, f).filter(g)` counts as a
  breakable link. Glyph's stdlib is namespaced-function style, so the literal
  every-link rule turns that into `array\n  .map(...)` and `grep "array.map("`
  stops finding it. Greppability is a wedge pillar, so this is the one place the
  layout rule collides with one, and picking the answer is a decision, not an
  implementation detail. The alternative is a syntactic first-group rule the
  formatter can evaluate with no symbol table: a bare-identifier receiver keeps
  its first `.`-link and every later link breaks.
- **`xs[i]` is typed `Unknown`** (the other half of G30), which is untouched and
  belongs with G39 rather than with the range functions.
- **A single-line template still gets its `${...}` interiors normalized** by
  `fmt`, where `Expr::String` is copied verbatim in every case. Dropping the
  normalization so every template takes the verbatim path is an open call.
- **`lambda_block` inlines a one-statement body without checking the column**,
  which is the same shape of miss as the width check stopping at the closing
  delimiter. It is what puts the two new `map_err` lambdas in `bracket.glyph`
  over 100 columns.

### 0.1.53 — Shipped · The checker knows what the stdlib returns

Five entries off the backlog, four of them closed. The theme is the same one
under G39, G37 and G47: the typechecker modeled stdlib function *signatures* and
nothing about stdlib *types*, so a value that came out of `std/string`,
`std/array` or `std/fs` was an opaque blob the moment it left the call.

`stdlib_fn_ty` now models the fixed-arity half of `std/string` (14 of its 18
exports) and `std/array` (11), returns first: `string.split` is an `Array<string>`,
`string.starts_with` is a `bool`, `array.find` is an `Option<T>`. The element
type travels on a `Ty::Param` bound from the first argument, which the existing
unifier already handles, so `array.filter(names, keep)` over an `Array<string>`
is an `Array<string>`. That closes the half of G37 that was a decision: `for i,
part in string.split(text, ",")` emits `.entries()` and binds a number with no
annotation.

The other half is new: a shape model for a stdlib named type. `fs.FsError`,
`fs.FileInfo` and `fs.ErrorKind` have field sets, a variant set, and a payload,
hooked into the same `record_fields_of` / `required_variants` /
`variant_payload` the checker uses for a union you declared yourself, and a
written `fs.FsError` annotation lowers to that type instead of `Unknown`. So
`match e.kind` on an fs error is exhaustively checked (E0200 naming
`fs.ErrorKind`), `fs.ErrorKind.Other({ code })` binds `code` as a `string`, and
`e.mesage` is E0210 rather than a `tsc` error. That is G47's remaining half.

The exhaustiveness claim only holds for the functions the table carries, and the
first cut of it carried five of the seven `Result`-returning `std/fs` exports.
`fs.append_text` and `fs.make_dir` typed `Unknown`, so a one-arm `match e.kind`
on either built clean, passed `tsc --strict`, and threw `non-exhaustive match` at
run time. Both are modeled now, and
`glyph-typechecker/tests/stdlib_model.rs` asserts that every `Result`-returning
export under `runtime/std/*.ts` is either in the table or on an exclusion list
carrying its reason, so a doc that generalizes over the table cannot outrun it
again.

D40 adds the `async fn(A, B) -> T` function type (G45), which emits `(a0: A, a1:
B) => Promise<T>`. Before it, a handler map or a function returning an async
thunk could not be annotated at all. Glyph's own checker enforces the
distinction: `definitely_incompatible` compares `is_async`, so a plain `fn() ->
T` where an `async fn() -> T` is expected is E0204 at a return and E0211 at a
call argument, rather than a TS2322 from `tsc` on the emitted TypeScript. The
diagnostic says `expected async function, found function`. And D12 was rewritten
to describe both
string spellings the lexer accepts (G61): `"..."` decodes escapes, `"""..."""`
does not, both interpolate, and `"""` is legal anywhere a string is. Removing a
form would break working code, so the spec was what was wrong.

G50's codepoint half closed as a decision, not code. Glyph strings index by
UTF-16 code unit and will keep doing so; `chars` and `char_at` are not shipping,
because an accessor that can split a surrogate pair is worse than none. A
program that needs codepoints encodes to bytes with `encoding.hex_encode` and
walks pairs, as `examples/apps/shortlink.glyph` does. No workaround comes out of
any app for that one; the hex walk is now the documented answer.

Pillar: verifiability. Every item above moves a check that `tsc` was doing (or
that nothing was doing) to Glyph's own checker, at the boundary where the
manifesto's no-`any` promise is made.

### Still open after this release

- **Phase 2 of G39** — hard-erroring on the `Unknown`s that remain. Member
  access, call arity and argument types against a receiver that is still
  `Ty::Unknown` at a stdlib boundary (`s.pusj(x)`, a wrong-arity call into an
  unmodeled namespace) are all still silent. Includes the decision of whether an
  unknown-typed iterand is an error or defaults to the array lowering. G39 stays
  open for exactly this.
- **Optional-trailing-argument arity in the stdlib table** (S, then unblocks
  six functions). `Expr::Call` reports E0213 on `params.len() != args.len()`, so
  `array.slice`, `string.slice`, `string.index_of`, `string.pad_start`,
  `string.pad_end` and `json.stringify` cannot be modeled without a false error
  on every call that omits the last argument. Three shapes: an `optional: bool`
  on `Ty::FnParam`; a min-arity returned alongside the signature so optionality
  never enters the type representation; or marking stdlib-sourced `Ty::Fn`s
  arity-unchecked and buying return types only. This is what keeps `for i, raw
  in array.slice(lines, 1)` needing its annotation in `expenses.glyph`.
- **The element type of `array.map` / `flat_map` / `zip`**, which comes from the
  callback's return. `collect_type_param_bindings` walks `Param` positions and
  `App` arguments, not into `Ty::Fn`. Extending it would resolve `U` for a named
  callback, but an unannotated lambda's return lowers to `void`, so the walk has
  to refuse to bind from one or it manufactures false diagnostics. The cheaper
  answer is modeling these three as returning an `Array` with an unbound element
  type, which fixes the `for` lowering and asserts nothing false.
- **Whether modeled stdlib parameters get real scalar types.** Today only
  `Array<T>` and `T` are typed, keeping the table's "no new argument-type
  diagnostic" invariant, so `string.len(42)` is still not an E0211. Typing them
  is the same class of hard error as phase 2 and belongs with it.
- **Where the `iter.take_while` / `std/iter` question lives** now that G50 reads
  `[DECIDED]`. The decision covers the codepoint half only; the note says so, but
  the item has no home of its own.

## spreadsheet dogfood trip — the names a module already owns

Writing a spreadsheet engine turned up fourteen findings, and two of them were
filed separately as an emitter bug and a parser bug. They are one defect. A
top-level Glyph name lands in the emitted module verbatim, and nothing checks it
against the names that module already depends on. `type Value = | Num(number) |
Error(string)` emits `export function Error(...)`, so the `new Error(...)` the
compiler writes below it calls the variant; the build fails at the `match` with
a `tsc` error, in the wrong place and with the wrong explanation. A variant named
`Number` is fine until some type in the module gains an `int` field, because the
`int` boundary check emits `Number.isInteger`. And `type Key = string | number`
built clean, passed `tsc --strict`, and emitted `export const string` /
`export const number` that shadowed the prelude, which is a silent wrong meaning
on a green build. The app is `examples/apps/sheet/`, and all fourteen findings
are written up in [`../dogfooding-gaps.md`](../dogfooding-gaps.md) under Round
14; the eight worth a backlog entry are G63–G70. Six of the eight entries are
open, and the two biggest need a decision before they need code. The depsolve
trip below came next.

### 0.1.54 — Shipped · A declaration cannot quietly take a name the module needs

`E0110` rejects a top-level `fn`, `type`, `const`, `component`, or tagged-union
variant whose name is already bound in every emitted module: the JavaScript
globals the emitted TypeScript refers to (`Object`, `Array`, `Promise`, `Number`,
`Error`) and the prelude names in scope without an import (`number`, `par`,
`print`, `assert`, and the primitive type names). The variant case is the one
that shipped broken, because `emit_variant_constructor` writes the Glyph name
straight into `export function`. The span is the declaration, so the diagnostic
lands where the fix goes rather than where `tsc` noticed.

The JS list is derived from the emitter, not from a list of JavaScript globals,
and a test greps `glyph-emit` and fails when a new global reference appears
without a matching entry. `Number` was harmless right up until `int` shipped,
which is exactly how this got into a release. `Date` is deliberately absent:
nothing Glyph emits mentions it, so rejecting `type Date` would make the
diagnostic's claim false, and `examples/corpus/calendar.glyph` is the real
program that would have broken. Std namespace names are absent for the same
reason: they are only in scope in a module that imports them, and that collision
is already `E0100`.

`E0111` is the `string | number` half. It is a separate code because the
rejection alone teaches nothing: `A | B` is D8 tagged-union syntax, so bare
primitives on the right declare variant constructors, and the help says that,
points at named variants, and names `extern_ts("string | number")` as the escape
hatch.

`docs/reference/reserved-words.md` tabulates all three lists in one place (the 32
keywords, the 33 TypeScript reserved words behind `E0109`, and the new shadow
list), which closes the "what can I not name a thing" question the trip also
filed. A test keeps the page complete against the compiler.

Pillar: verifiability. Two silent-green cases become loud, and Glyph owns the
diagnostic instead of handing back a `tsc` error with the wrong span. Greppability
second, through the reserved-word page.

### Still open from this trip

The release closes the detection half of the two shadow findings and none of the
expressiveness. Six of the eight backlog entries are untouched.

- **You still cannot name a type or variant `Error`, `Number`, `Object`,
  `Array`, or `Promise`** (G63, decision then L). This is the finding that
  started the trip and `E0110` does not close it. A spreadsheet cell is a
  number, a label, nothing, or an error, and the app ships `Cellerr` because
  `Error` is taken. Trading a silent miscompile for a clear rename request is
  worth doing on its own, and it is not the same as being able to write the
  program. Closing it means mangling Glyph names in the emitter, which changes
  what a stack trace, a `grep` over `dist/`, and a hand-written `extern/` shim
  see. That is an architecture decision and it has not been made.
- **A union of primitive types is rejected but still not expressible** (G64,
  decision then M). `type Key = string | number` now fails with `E0111` instead
  of meaning something else, but Glyph still has no way to say it except
  `extern_ts("string | number")`, which is opaque to Glyph's own checker. Adding
  real untagged unions is a type-system decision, not a patch: it touches
  exhaustiveness, descriptors, and `is`, and D8's tagged unions exist because
  sealed variants are what makes a `match` verifiable.
- **`==` is `deepEqual` in `@example` and `===` in the program** (G65, decision
  then M). The worst of the fourteen and not this release's scope: `@example
  make(1) == { x: 1 }` reports a passing example and clean `tsc`, and the
  program prints NOT EQUAL. Nothing catches it. The emitter's own comment claims
  value equality, so one of the two is wrong and picking which is a language
  decision.
- **An optional record field is declarable but cannot be read** (G66, M). Member
  access propagates the field's declared type and ignores `optional`, so Glyph
  reads `value?: string` as `string` while `tsc --strict` reads it
  `string | undefined`, and no Glyph construct narrows one to the other. The app
  made the field required and documented why.
- **A `for` binding carries no element type** (G67, M), so a D30 string-literal
  union read inside the loop degrades to `string`, the `match` over it stops
  being exhaustive, and the `E0218` help asks for the `else` arm that forfeits
  the guarantee the check exists to sell.
- **`json.parse<T>` collapses every field issue to one `expected T`** (G68, M)
  while `T.parse` on the same type reports paths, and `docs/guide/typed-apis.md`
  teaches the lossy one without saying what it gives up.
- **`glyph run` and `glyph check` never run `@example` blocks** (G69, S). Only
  the `Build` arm calls `run_examples`, while `docs/guide/getting-started.md`
  says run and build never disagree.
- **`E0109` and `E0110` can report twice** (G70, S). `is_reserved_ts_word` runs
  in both `collect.rs` and `resolve.rs`, so a reserved name is counted twice;
  the new shadow check runs only in `collect`, but the collect pass itself is
  reached more than once for a single-file build, so the rendered diagnostic
  still appears twice. It inflates the error count without changing the verdict.

## depsolve dogfood trip — the loop index that was a string

Writing a dependency resolver (`examples/apps/depsolve/`) put the two most
ordinary things in the language next to each other: read a `Record`, `match` the
`Option` it gives back, iterate the array inside with an index. That chain
miscompiled. `glyph build` reported no diagnostics, `tsc --strict` passed, and
the program printed `01:x` and `11:y` where it should print `1:x` and `2:y`,
because the two-binding `for` took the `Object.entries` lowering and bound the
index as a string key. The app carried a `let path: Array<string>` annotation
and a three-line comment saying the annotation was load-bearing, which is what a
workaround for an unfiled defect looks like.

### 0.1.55 — Shipped · `record.get` into a `match` into a `for` binds a number

Two independent causes, and each one reproduced the miscompile on its own, so
both had to go.

`std/record` was not modeled anywhere in the typechecker. `record.get(t, k)`
typed `Unknown`, so the `Some(p)` arm bound nothing and everything downstream of
it was untyped. `stdlib_record_fn_ty` now models all six functions, with the
value type riding a `Ty::Param("V")` on the record parameter the same way
`std/array` carries `T`, so `record.get` over a `Record<string, Array<string>>`
is an `Option<Array<string>>`. The key is always `string`, so it is not a
parameter, and every parameter slot that is not `V` stays `Unknown`, which is
the rule the rest of that table keeps. The knock-on is the ordered walk:
`array.sort(record.keys(t), cmp)` was binding `sort`'s element type from
`Unknown`, and now keeps `string`.

The other cause was the arm join. `join_match_arms` compared arm types by
equality, and an empty array literal is `Array<Unknown>`, so `None => []` read
as disagreeing with `Some(p) => p`'s `Array<string>` and sank the match. The
join now goes argument-wise underneath an already-agreeing head, with `Unknown`
absorbing the other side. Two different heads still join to `Unknown`, and an
arm whose value is entirely undecidable still sinks the match: projecting one
arm's type onto an arm nothing is known about would be a guess, and the join is
feeding a lowering choice, not a hint.

The annotation and its comment came out of the resolver and the output is
byte-identical. A conformance snapshot pins the emitted `for` for both shapes
alongside the record case that should stay `Object.entries`, and an integration
test runs the program, because the point is that the emitted TypeScript
type-checks either way.

Pillar: verifiability. The `for` lowering is real semantics with no `tsc`
backstop behind it, chosen from the inference lattice, so a hole in the lattice
is a wrong program on a green build. Abstraction second: `record.get` plus
`match` plus `for` is the idiom the stdlib was built around.

### Still open from this trip

`iter_is_array` falling back to the record lowering for an iterand whose type is
honestly unknown is untouched, and stays a decision rather than a patch (the
residue of G37). The trip's other finding is G72, which predates it: `glyph
check` on a single file compiles every `.glyph` under that file's directory, so
checking one app in `examples/apps/` reports a `TS2307` about a different app's
import and the cost of checking one file scales with the directory it lives in.
Both are written up in [`../dogfooding-gaps.md`](../dogfooding-gaps.md) under
Round 15. No release carries the Next marker now; the next trip picks from what
is left of this list and Round 14's.

## workflow dogfood trip — how you imported the union decided whether it was checked

Writing a statechart replay engine (`examples/apps/workflow/`) surfaced a hole in
the guarantee Glyph advertises hardest. A `match` over an imported tagged union
was held to D9 when the variants came in through a named import, and not checked
at all when the same union was reached through a namespace import. The app never
hit it, because its author paid two eighteen-name import lists instead, which is
its own signal.

### 0.1.56 — Shipped · A `match` is checked whichever way you imported the union

`match c { model.Yes(_) => …, model.No(_) => … }` on a three-variant union
reported no diagnostics and passed `tsc --strict`, then threw
`Error: non-exhaustive match` with a raw JS stack trace at run time. The named
spelling of the identical match was E0200. `import model as m` was broken the
same way.

The scope is wider than the report that found it. It also hit the prelude unions,
which is the part that matters most: `match o { option.Some(s) => s }` on an
`option.Option<string>` with `None` missing was green through both checkers.
`option.Option<T>` lowered to `Unknown` because the two-segment stdlib type table
knew only the three `fs.*` types, so the most-used union in the language lost its
exhaustiveness check to a one-token change in how it was imported.

Both halves are wiring, not new machinery. `stdlib_path_ty` now falls back to
`imported_prelude_container`, the same function that unifies
`import std/option { Option }` with the prelude built-in, so `option.Option<T>`
and `Option<T>` lower to one `Ty` and the ordinary exhaustiveness path runs. For
project siblings, the imported-union lookup resolves a qualified arm through its
head symbol (`ImportNamespace` or `ImportAlias`) instead of looking the variant
name up as a symbol, which under a namespace import it never is.

A misspelled qualified variant used to reach `tsc` and come back as `TS2678`
against a literal union type. The required variant set is in hand once the union
resolves, so it is E0220 now, on the arm, with the nearest-variant hint. That was
a diagnostic-quality bug rather than a hole: the typo never shipped.

Seven integration tests pin it, one per spelling and one each for the two ways
the check could over-fire (an `else` arm, and a fully covered match). There was
no coverage for any of this before, which is how it survived to 0.1.55.

Proving the fix on the app rather than on unit tests turned up one more thing.
Making `result.Result<T, E>` decidable also made it classifiable, and
`type_expr_is_result` knew only the prelude and named-import spellings, so every
`?` in a function returning the qualified type came back E0201. Nothing shipped
that way, since the qualified return type had been `Unknown` and therefore
permissive before this release, but it is the same defect as the one being fixed:
a predicate that recognizes two of the three legal spellings of a type. It is a
third arm and an eighth test now.

Pillar: verifiability. A false green on sealed unions is the failure class the
wedge exists to eliminate. Greppability second: nothing in the text of a `match`
told you which of the two spellings had been checked, so a codebase could not be
searched for its own unverified matches.

### Still open from this trip

E0200 quotes the missing variant names for a module-local union and for the
prelude ones, and leaves them bare for a union imported from another Glyph
module: `missing variants Maybe` where the same diagnostic elsewhere reads
``missing variants `B` ``. Two code paths build that list and one of them
formats. It is cosmetic, it splits by where the union is declared rather than by
how it was imported, and it predates this release on both spellings. It is G74
in [`../dogfooding-gaps.md`](../dogfooding-gaps.md) under Round 16, and it is the
only finding this trip left open. No release carries the Next marker now; the
next trip picks from what is left of this list, Round 15's, and Round 14's.

## csvql dogfood trip — the type system stopped at the module boundary

`examples/apps/csvql` is a relational query engine over CSV: a catalog, a CSV
reader, a SQL parser, a binder, a planner, an executor, a renderer. Eleven
modules, which is why it found what it found. It is the first app in this loop
big enough that the interesting types are declared in one file and consumed in
another, and three separate guarantees turned off the moment it split.

The one that shipped a bug into the app: a string-literal union (D30) lost its
exhaustiveness guarantee across an import. `type ColType = "text" | "int" |
"real" | "bool"` in `catalog.glyph`, matched on all four literals in
`table.glyph`, was E0218 with help text reading "Add an `else` arm. A
`number`/`string` match with only literal arms can never be exhaustive." That is
false about the code in front of it, and taking the advice turns a compile error
into a runtime fallthrough. The author took it. The dead `else => None` was in
the shipped app until this fix removed it.

### 0.1.57 — Shipped · D30 exhaustiveness survives an import

`DeclTyResolver` grew `imported_string_literal_union(module_path, type_name)`
alongside the `imported_union_of_variant` that carries D8's half of the same
guarantee, with the same `None` default for db-less callers and the same salsa
impl reading the sibling module out of `project_files_input`. `Lowerer` grew
`with_imports`, used at the two sites where an annotation's type has to be right
for an imported name (the Assigner's walk and the `decl_ty` query), and returns
the ordinary `Ty::StringLiteralUnion` rather than a foreign `Ty::Named`, so the
existing exhaustiveness check works unchanged. All three import spellings are
covered: `import catalog { ColType }`, `import catalog` with `catalog.ColType`,
and `import catalog as c` with `c.ColType`. A match covering every literal
compiles with no `else`; one that omits a literal is E0200, not E0218.

Five integration tests pin the three spellings plus a `let` annotation, each
asserting both halves: the covering match builds clean, and dropping a literal is
E0200 rather than E0218. Two unit tests hold the trait default at today's
behaviour so the `None` a db-less caller gets cannot quietly become the thing the
guarantee rests on. The E0218 help text was left as it is; it is correct about an
unbounded `string`, and no path reaches it with an imported literal union now.

Pillar: verifiability. A guarantee that depends on which file a type is declared
in is not a guarantee, and this one failed in the worst direction: the compiler
asked the author to delete it, and the author complied. Greppability second, on
the same argument as D9's half: nothing in the text of the `match` said whether
it had been checked.

### Still open from this trip

- **Imported record fields are still `Ty::Unknown`.** A field read on a record
  type imported from a sibling has no field set, so `s.rowz` draws no
  `UnknownField` error, and `for i, x in s.rows` emits `Object.entries` and binds
  `i` to the string `"0"` instead of the number `0`. It is the same hole this
  release closed for string-literal unions, but it does not have the same shape
  of fix: lowering a foreign `TypeExpr` needs the *source* module's resolver,
  because `Lowerer::lower` resolves paths through
  `self.resolved.resolutions.get(span)` and an imported declaration's spans
  belong to another file. So the work belongs on the source side of the query,
  not in the consumer's `Lowerer`. Bundled with it: imported-record member
  checking and the `for i, x` `Object.entries` lowering, which are consequences
  of the same missing field set. G75 in
  [`../dogfooding-gaps.md`](../dogfooding-gaps.md).

- **The `examples/` gate needs a decision about roots (G72, still).** A sibling
  import resolves against the build root, so a multi-module app under
  `examples/apps/<name>/` is only a project when its own directory is the root.
  Rolling the whole tree into one root does not merely fail to link the modules,
  it silently turns off every check that needs a sibling's declaration. The
  integration gate now stages a copy with those app directories removed and
  builds each of them at its own root; the CI step at
  `.github/workflows/ci.yml` still runs `glyph build ../examples` over one root
  and is red there (`TS2307` on `apps/workflow/wire`, and now E0218 on
  `apps/csvql/table` because the check that used to be off is on). Whether the
  fix is the walk (make `glyph build` recognize a nested project) or the gate
  (build each app at its own root) is a call, not a bug.

Both are written up in [`../dogfooding-gaps.md`](../dogfooding-gaps.md) under
Round 17.

### 0.1.58 — Shipped · A match arm cannot swallow the value it produces, and an imported type keeps its name

Four fixes from the adversarial review of the csvql round: three it named, plus a
visibility hole the third one exposed on the way in.

`let text = match t { TPunct({ text }) => text, ... }` emitted `let text;`, then
`const text = __m0.text;` inside the case, then `text = text;` — an assignment to
a `const`, and, in any collision TypeScript happened to accept, a value dropped on
the floor. The statement form of `let x = match` declares `x` outside the switch
and has each arm assign it, while the arm binder emits a `const` inside the case;
neither consulted the other, and the emitter had no uniquing at all. The
assignment now routes through a synthesized `__aN` temporary, but only when an arm
really does bind the name: rebuilding every app in `examples/apps/` emits
byte-identical TypeScript to the 0.1.57 binary, all 65 files. The collision test
walks arm patterns (identifier, constructor args, object fields, array elements
and rest) and asks each one for exactly the name it binds, which for a renamed
field `{ text: p }` is `p` and never `text`. It also walks a top-level `let` in a
block arm body and a nested `match` that is the whole arm body, since that one
lowers into the same case block. A `for` binder is not walked: it lowers to
`for (const i of ...)` and is scoped to the loop. The `mut <lvalue> = match` twin is
guarded against every identifier the rendered lvalue mentions, so
`mut s.count = match r { Ok(s) => s, ... }` no longer assigns through the arm's
`s`. G77.

E0200 now backticks the missing variant names on the imported-union path, which
is what the module-local path already did. One rule, one diagnostic shape. G74.

An imported type now has an identity. `Ty` gained an `Imported { module, name }`
variant keyed on the source module's registry path and the name that module
declares, so `import catalog { Sheet }`, `import catalog` with `catalog.Sheet`,
and `import catalog as c` with `c.Sheet` all produce the same type. It carries no
symbol id, because a foreign module's ids index an unrelated symbol in the
consumer's table. Lowering emits it without asking any cross-module query, which
is why `type Node = { next: Option<Node> }` and a two-module cycle terminate with
no cycle guard. One general query answers it, `imported_type_decl(module_path,
type_name)`, backed by a tracked `exported_type(db, file, decl_idx)` that lowers
the declaration on the *source* side. A consumer cannot do that, because
`Lowerer::lower` resolves paths through spans that belong to the declaring file.
Keying it on the source declaration means one lowering is shared by every
consumer and every spelling. So `s.rowz` on an imported record is now E0210 and
says `Sheet` rather than `record`; `for i, r in sheet.rows` lowers to
`.entries()` and binds a number; a sibling type named inside another sibling type
resolves on demand; a generic sibling record substitutes its arguments; and a
`match` on an imported record's string-literal-union field is exhaustive without
an `else`. The three `let` hoists csvql carried to tell the emitter what an
imported field's type was are deleted, along with the comment that explained
them: `table.build` loops straight over `sheet.rows` and `spec.columns`, and
`bind.fields_of` over `spec.columns`. The app builds with no diagnostics, passes
`tsc --strict`, and prints byte-identical output for all twelve queries. Emitted
TypeScript for every other app in `examples/apps/` is byte-identical. G75.

Visibility now works the same way under both spellings. `import lib { Secret }`
on a non-`pub` type has always been E0105; `import lib` plus `lib.Secret`
reported nothing, and once an imported type had a field set that silence started
handing out a private type's fields. The resolver's type walk records every
`ns.Name` annotation it passes and `import_diagnostics` runs the same export
check over them, so the same declaration gets the same answer whichever way you
name it.

Deliberately not in it: cross-module assignability stays permissive
(`ty_is_decidable` is false for the new variant); a sibling `interface` still has
no cross-module member set; and the two per-shape queries
`imported_string_literal_union` and `imported_union_of_variant` were left as they
are, so the release carried one risk instead of two.

Pillar: verifiability for the emitter bug (the lowering has to preserve the
program's meaning, and this one did not) and for the module boundary
(a guarantee that stops at a file edge is not a guarantee), greppability for the
diagnostic and for E0210 naming the real type.

### Still open from this trip

- **Three things the G75 fix left for later.** Cross-module *assignability* is
  still permissive: an imported type now has an identity, so passing a
  `catalog.Sheet` where a `table.Row` is expected could be a Glyph error instead
  of a `tsc` one. Applying the existing local rule uniformly across a file edge
  is mechanical, and the fork decision's own tiebreaker says to do it; what is
  actually open underneath is whether that rule should be nominal or structural,
  which is Q15. A sibling `interface` (D34) is the one that really is a language
  decision: giving its members the same cross-module shape a record's get would
  silently redefine D34's structural satisfaction rule. And a `for` binder is
  still untyped, so `col.kind` inside `for j, col in spec.columns` remains
  unchecked; that is separate from G75, since the `for` lowering reads the
  *iterand's* type.

- **Folding the two per-shape cross-module queries onto the general one.**
  `imported_string_literal_union` and `imported_union_of_variant` answer questions
  `imported_type_decl` could also answer. They were left untouched so the release
  that introduced the general query carried one risk instead of two. Polish lane.

- **The cross-module queries still do not check `pub`; the use site does.** None
  of the three queries looks at visibility, so on a non-`pub` sibling type Glyph
  will happily resolve the field set and pick the array lowering. What stops you
  is one stage earlier, and it now covers both spellings: `import lib { Secret }`
  reports E0105 at the import, and `import lib` plus `lib.Secret` reports the
  same E0105 at the annotation. Before 0.1.58 the second one reported nothing at
  all and only `tsc`'s TS2694 caught it, which meant Glyph's answer to "can I see
  this type" depended on how you spelled the import. Two things are still open:
  the namespace check covers type annotations, not `lib.helper()` in value
  position (that remains a `tsc` error), and pushing `pub` down into the queries
  themselves has to be done to all three at once, since one query checking and
  two not would put the inconsistency back where it was.

- **A sibling import only works when the sibling sits at the build root, and
  everything downstream of that is silently wrong.** A module path is derived
  root-relative (`derive_module_path`), while an `import` is matched by the path
  as written. Put `shape.glyph` and `main.glyph` in `sub/` and build `sub/`: green.
  Build the parent directory instead, same bytes, and the module paths become
  `sub/shape` and `sub/main`, the import `shape` matches nothing, and three things
  go wrong at once. The emitter stops recognising it as a project module and writes
  a bare specifier, so `tsc` says `Cannot find module 'shape'`. The tagged-union
  coverage check (0.1.56) stops firing, so a `match` missing a variant compiles.
  The string-literal-union check (0.1.57) reports a false E0218 on a match that
  covers every literal. This predates both fixes: `glyph build examples` has been
  failing since the first multi-file app landed under `examples/apps/`, which is
  the command CI runs to type-check the examples, and csvql inherited it. The fork
  is whether an import resolves relative to the importing module's directory or
  stays root-relative and each app is its own project root (in which case CI has to
  build each app separately and the compiler should say so instead of emitting a
  bare specifier).

The trip after 0.1.59 picks from what is left of this list, Round 16's, and
Round 15's.

### 0.1.59 — Shipped · The boundary says which rule you broke

From the auth_api dogfood trip. Building a signup/login API in Glyph, the thing
that cost real code was not writing the validator, it was reading its answer.

A record descriptor's `parse` computed exactly which rule a field broke and then
threw the distinction away: absent, failing its `where` predicate, and holding
the wrong type all pushed the byte-identical string ``field `password` is missing
or has the wrong type``. The refinement descriptor said only `expected Password`,
never naming the predicate it had rendered verbatim one line above, so the half
of D39 that promises the constraint is greppable from the rejection did not hold.
And the object test let arrays through, so a record with no required fields
answered `Ok` for an array and a posted `[1, 2, 3]` came back as one misleading
issue per declared field.

What shipped:

- The object test excludes arrays in both `is` and `parse`, and `parse` names an
  array when it gets one (`expected Signup (an object), got an array`).
- Each field is tested in order: absent first, then wrong, with the message
  naming the declared type as the declaration spells it.
- A field whose type has its own descriptor delegates to that type's `parse`, so
  nested issues arrive with the field name prepended to their `path` and a
  refinement's message reaches the caller.
- The refinement rejection reads `expected Password (string where value.length >= 8)`.
- `Issue` gained an optional `code` (`"missing" | "type" | "refinement" |
  "unexpected"`) so a handler branches on the classification rather than matching
  message text. Optional keeps every existing `Issue` consumer compiling.

G79 in [`../dogfooding-gaps.md`](../dogfooding-gaps.md).

### Still open from this trip

- **A module-local type named `Issue` shadows the prelude one** and breaks every
  descriptor in that module, because `parse` annotates its error array as
  `Issue[]`. G80. *Closed in 0.1.60, by the other route: `Issue` is reserved, so
  the error lands on the declaration. The injected alias was not built.*
- **`std/crypto` is 31 lines with no KDF and no timing-safe compare**, so the app
  hand-rolled a 4000-round HMAC as PBKDF2. The module header claims security
  primitives belong in the stdlib, which makes the gap worse than shipping
  neither. An afternoon of wrapping `node:crypto`, no design content.
- **`std/http` cannot see the client address**, so the app keyed its lockout on
  email and password spraying across accounts is unthrottled.
- **`std/http` headers are `Record<string, string>`**, so two `Set-Cookie` cannot
  coexist and a repeated request header is silently dropped: absence and
  multiplicity collapse into the answer that reads as safe.
- **`std/http` accumulates the whole body before the handler runs**, unbounded.
- **`Request.url` holds a request target**, not a URL, while the name, the type,
  and the docs all agree on something untrue.
- **No `bytes` type**, so `base64url(HMAC)` and therefore a standards-compliant
  JWT is inexpressible. This one is a design decision, not an iteration.
- **A project-local import that resolves to nothing is silent.** Strongest
  runner-up for the next trip. *Closed in 0.1.60 as `E0104`. Resolution
  semantics did not change, which is still G78.*
- **The `module` header is read only by the formatter** and is free to lie, which
  is the greppability pillar exactly inverted. Ten cheap lines.

### 0.1.60 — Shipped · The compiler stops blaming the wrong line

Five small things, none of which changes the language. Three are the same defect
wearing different clothes: the compiler knew something was wrong and reported it
somewhere the user could not act on.

- **A module-local `Issue` or `Record` is `E0110`.** Declaring either used to
  compile and then fail `tsc` with a TS2353 pointing at generated code, because
  every descriptor's `parse` writes `Issue[]` on its own initiative and the local
  declaration won. `Issue` joined `PRELUDE_GLOBALS` and `Record` joined
  `JS_GLOBALS` (it is a TypeScript built-in, so filing it under the prelude would
  have made the message state a falsehood about where the name comes from). The
  drift scan that guards the JS-global list now covers ambient prelude types, and
  it had to learn to see `Issue[]`, which is how this shipped in the first place.
  `Schema`, `Component` and `Option` stay legal on the `Date` precedent: the
  emitter writes them only because the author wrote them. G80.
- **An unresolvable local import is `E0104`.** A local import resolves from the
  build root (D15), so building an enclosing tree left the failure silent, the
  imported type degraded, and what the user saw was `E0218`, a non-exhaustive
  match on a match that was exhaustive. The message names the module and, when a
  `.glyph` file whose module path ends in that import exists elsewhere under the
  root, where it actually is. What makes the check safe against false positives
  is that the build first collects the module names resolvable without a `.glyph`
  file: every `declare module "X"` under `<root>/.types/**/*.d.ts` and in the
  bundled Node shim, plus every package in the project's `node_modules`.
- **A wrong type and a failed predicate stopped reading alike.** `42` against
  `type Password = string where long_enough(value)` reports
  `expected Password (string)` with `code: "type"`; only a string that fails the
  predicate gets `expected Password (string where long_enough(value))` and
  `code: "refinement"`. Before, both were the refinement text.
- **`glyph run <dir>`** runs that directory's `main.glyph`, matching
  `glyph build <dir>`. The two commands disagreeing about what a directory means
  is the kind of thing you only hit once, and then never trust again.
- **G70 was not real.** `E0109`/`E0110` cannot double-report: the two
  `is_reserved_ts_word` call sites check disjoint name sets, and a collect
  failure already skips the resolve pass, which 0.1.54 added for exactly this.
  Counting assertions now pin it, and `tests/negative/reserved_word_decl_name.glyph`
  gives `E0109` its first fixture. Bookkeeping, not a fix.

### Still open from this release

- **The refinement split is proved by unit assertion, not by an app.** No
  `examples/apps/auth_api` transcript step posts a wrong-typed password, so the
  `code == "refinement"` branch has never been seen answering 400 where the old
  code answered 422. G79 stays half closed until that step exists.
- **The union descriptor's `parse` still emits a bare `expected Name`** with no
  `code`, so the record path and the union path do not classify alike.
- **The LSP does not report `E0104`.** `glyph_lsp::analysis::analyze` takes text
  and nothing else, so it has no build root, and the workspace folder is not one
  either (`examples/apps/auth_api` is a root inside a workspace whose folder is
  the repo). Guessing a root would recreate the false-positive class this check
  was built to avoid, so it needs the LSP to learn which root a document builds
  under. Not decided.
- **A reserved-word parameter stays invisible behind an unrelated collect error.**
  Because a collect failure skips resolve entirely, `fn f(class: number)` is not
  reported until a duplicate-name error in the same file is fixed. Cascade
  suppression rather than duplication; narrowing it trades one kind of noise for
  another. Undecided, and it is the residual of G70.
- **`glyph check examples` reports the apps' sibling imports as `E0104`**,
  because the tree is not one build root. Documented in `examples/README.md`; the
  layout decision itself is G78.
- **`ResolveError::UnresolvedModule` stores the build root as a caller-supplied
  display string**, so an absolute path typed on the command line ends up inside
  a structured diagnostic. Rendering it at the CLI boundary is the fix.

### 0.1.62 — Shipped · A program can answer while you are still typing

The chat dogfooding trip. `io.read_line` had never been a line reader: it called
`readFileSync(0, "utf8")`, which returns when stdin closes, split the result and
handed out the pieces. Piping a file works. Typing into it does not, so every
program a person talks to was unwritable in Glyph, and the app that found this
shipped as a session replayer with a comment in its source explaining why.

- **`read_line` returns when a line arrives, not when stdin closes.** stdin is
  read incrementally now: module state in `runtime/std/io.ts` holds a decoded
  `pending` string, an `eof` flag, one reused 64 KiB `Buffer` and one
  `StringDecoder`, and a private `fill()` does one `readSync(0, ...)` per call.
  `read_line` returns the text before the first `"\n"` and fills only when no
  newline is buffered. A trailing `"\r"` is stripped, so CRLF input yields the
  same lines as LF, and input that ends without a newline hands back that last
  partial line once before `None`. A read that reports `EAGAIN` on a non-blocking
  tty backs off about 10ms and retries rather than spinning; any other read
  failure degrades to empty input, so a program started with no stdin still
  terminates. G81.
- **`read_to_string` drains the same buffer** instead of re-reading fd 0, so
  `read_line` and then `read_to_string` gives you the rest rather than losing
  what the other call had buffered. Called first, it still returns all of stdin.
  Its one-line doc changed from "all of stdin" to "the rest of stdin", which is
  what it always should have said.
- **Three apps already in the repository became interactive with no change to
  them.** `minesweeper.glyph` redraws the board between moves, `adventure.glyph`
  answers `look` while you type, and `minilang --repl` evaluates a line and
  prints the result before reading the next. All three were shaped around the
  old behaviour without anyone naming it.
- **The regression test is a timing test.** A pipe-a-file test passes against the
  broken implementation, so `read_line_returns_a_line_before_stdin_closes` runs
  a Glyph echo loop with stdin held open, writes one line, and requires the echo
  back within 20 seconds before writing a CRLF line and closing the stream.
  Against the old `io.ts` it times out.
- **The bundled Node shim gained three declarations** the runtime needs for
  `tsc --strict` with no `@types/node` installed: `fs.readSync`, `Buffer.alloc`
  with `subarray`, and a `string_decoder` module. Ambient types for the runtime's
  own use; no Glyph surface changed.

### Still open from this release

- **`std/io` cannot write without a newline.** `println` and `eprintln` append
  `"\n"` and are the whole write surface, so the `> ` prompt an interactive
  program opens with is unwritable and the chat app prints a banner instead.
  `io.print`/`io.eprint` is the answer and needs a decision about flushing, since
  `process.stdout.write` on a pipe is buffered where `console.log` is not. G82.
- **A program cannot tell a terminal from a pipe.** `std/process` has no `isTTY`,
  so an app that behaves one way for a person and another for a piped file has to
  be told by a flag. The chat app takes `--stdin`, and passing it with nothing
  piped hangs rather than falling back. G83.
- **The `EAGAIN` backoff is unproven on macOS**, where `readSync` on a pty blocks
  and the retry path is never reached. It was measured not to burn CPU (15 idle
  seconds at a pty prompt cost 0.00s of child CPU), but the branch itself is
  exercised only where stdin is non-blocking.

### 0.1.63 — Shipped · A Glyph program can be a server

The chat trip again, with the assignment it was given the first time: a server
several clients talk to at once. The previous round substituted a session
replayer and did not say why. The reason turned out to be one line in the
runner, and it was invisible: `glyph run` on a program that starts a server
printed nothing and exited 0.

- **A program that is still doing something when `main` returns keeps running.**
  The generated entrypoint called `process.exit(code)` as soon as `main` came
  back, which Node honours immediately, while the event loop still holds live
  handles. So a Glyph program that created a TCP server bound its port and died
  in the same tick. Every long-lived program was affected: servers, watchers,
  bots, REPLs. The entrypoint now assigns `process.exitCode`, leaving Node's own
  rule in place: leave when there is nothing left to wait for. A program that
  only computes still exits the moment `main` returns, with the same code. G84.
- **A thrown `main` still terminates, but its diagnostic survives a pipe.**
  `console.error` is asynchronous when stderr is a pipe, which is every CI job
  and every agent capturing output, so the old `process.exit(1)` on the next
  line could truncate the error it had just written. The failure path now sets
  the code, waits for stderr to drain, and then exits.
- **A nested project's `.types/` declarations reach its own type check.** The
  generated `tsconfig.json` includes `**/*.ts`, which reaches into nested
  projects' output, while `.types/**/*.d.ts` covered only the outer project's
  own directory. The outer `tsc` run therefore checked an inner project's files
  without the declarations they depend on: `examples/apps/chat` passed on its
  own and failed as part of `examples/` with `Cannot find name 'net'`. Each
  project's config now excludes the output of the projects nested inside it, so
  every project is checked exactly once by its own config. The exclusion is
  derived from output paths, never source paths, because a project's output
  directory comes from its package directory while its sources may sit in `src/`
  below it. G85.
- **The chat app is a real server.** `examples/apps/chat` gained `framing.glyph`
  (TCP line reassembly, pure and `@example`-tested against split and coalesced
  packets), `audience.glyph` (which clients receive which event), and
  `daemon.glyph` (the only file that touches a socket). Verified with three
  concurrent clients: a room post reaches that room's members and nobody else, a
  direct message reaches exactly two, `/who` answers only the asker, a rename is
  visible to the client that sent it, a message split across three packets
  arrives as one line, and three clients dropping at once are each announced
  under the right name.
- **`getting-started.md` documents long-running programs.** `main` returning no
  longer means the process stops, and four guide pages said it did.

### Still open from this release

- **Nothing sets the exit code after `main` returns.** A listener that fails to
  bind fires well after `main` is done, and without the program calling
  `process.exit` itself the loop drains and the process exits 0: success for a
  server that never started. The chat daemon does call it, and every server will
  have to remember to. G86.
- **`owned` (D25) does not reach sockets, the case it was specced for.** It
  requires a type declared with `resource`, and a socket arrives from an ambient
  `.d.ts` as an opaque foreign type that cannot be. So the one program in the
  repository that holds N handles each needing exactly one close does not use
  the language's one carve-out from "no linear types". Either the spec is wrong
  or the app is; nobody has established which. G87.
- **A record holding an opaque external value gets a `parse` that lies.** For a
  field the emitter has no descriptor for, the generated check is
  `field !== undefined` under the message ``field `socket` must be Socket``, so
  `parse` accepts a string there and reports success. G88.
- **A program cannot say that it does not terminate.** `serve` explains in a doc
  comment what a `-> never` return type would state, and `main` carries an
  unreachable `return 0` plus a dead match arm to keep a later `match`
  exhaustive. `std/process.exit` is typed `-> never`, so the concept exists in
  the stdlib and is not spellable in user code. G89.
- **A silent `glyph run` is still indistinguishable from a working one.** The
  reason G84 went unreported for a whole round is that its failure was exit 0
  with no output. A run that terminates having produced nothing, consumed no
  measurable time, and returned 0 could say so.

### 0.1.64 — Shipped · A match that was exhaustive could throw

The Discord bot round. A gateway client is the first app here that speaks a
protocol Glyph did not design, against a server Glyph does not control, and it
turned up a miscompile that twenty rounds of apps had missed.

- **A match arm that produced no value fell through into the compiler's own
  "non-exhaustive match" throw.** A lambda body is a value block in return
  position, so an arm ending in a `mut` (or `let`, `for`, `loop`, all of which
  yield nothing) emitted no `return`, because there is no value, and no `break`
  either, because the emitter only added one in statement position. The
  generated `switch` case ran straight into
  `default: throw new Error("non-exhaustive match")`. Twelve lines reproduce it,
  it compiles clean, `tsc --strict` passes, and it throws at run time on a match
  that is exhaustive. The same code inside a top-level `fn` was correct, which
  is how it survived: no test put a valueless arm inside a lambda, and socket
  callbacks are nothing but lambdas containing matches. A nested match had the
  identical hole one level down. The `break` now depends only on being inside a
  `switch` case, which is the rule the empty-arm case beside it already used.
  Two emitter tests cover it, both verified against the old lowering. G94.
- **`examples/apps/discord` is a working gateway client.** Handshake, identify
  or resume, heartbeat on the interval the server dictates, sequence tracking,
  zombie detection, exponential backoff, and commands. The protocol and the
  state machine are pure and carry 37 `@example` rows; one module touches the
  socket.
- **Verified against an adversarial gateway, not only a cooperative one.**
  Written from Discord's documentation rather than from the client: an
  unprompted opcode 1, a close with code 4004, and a gateway that greets you and
  then stops acknowledging heartbeats. The cooperative mock had passed while the
  bot ignored opcode 1 entirely and retried a rejected token forever.
- **`@redact` (D24) is used by an example for the first time.** The bot holds a
  token; the replay asserts it does not survive being printed.

### Still open from this release

- **Ambient *global* declarations in `.types/` are invisible to the resolver.**
  Only `declare module` blocks are read, so `WebSocket` and the repeating timers
  cannot be named. The sharp end is D37: `new` was added so class-based clients
  would not need an `extern_ts("new ...")` string, and `new WebSocket(url)` is
  E0103, so for every global class the string is still the only route. G90.
- **An `Option<T>` field cannot be read from ordinary JSON.** `null`, an absent
  field and a bare value are all rejected; only Glyph's tagged encoding parses,
  and no third-party API sends that. Two ways forward, and they are different
  decisions: loosen `Option.parse`, or add a boundary-only nullable that decodes
  into `Option`. G91.
- **Locally bound closures cannot call each other**, so event-driven code has to
  lift them to top level and thread a context record. G92.
- **An `@example` must fit on one line.** G93.

### 0.1.65 — Shipped · An app does not need TypeScript

The Discord bot needed a hand-written declaration file and six escapes to raw
TypeScript to open a socket and run a timer. The chat server needed a
declaration file to reach `net`. That is the wrong shape for a language whose
claim is that you write Glyph: every one of those lines is a line the Glyph
type checker does not see, in the language Glyph exists to replace.

The measure for this release was that both apps had to lose their TypeScript
entirely, and both did.

- **`std/timers`.** `after`, `every`, `cancel`, `unref`, `sleep`. Scheduling is
  a global in JavaScript and Glyph resolves imported module names rather than
  ambient globals, so before this there was no way to run something later
  without declaring Node's `timers` by hand. A pending timer holds the process
  open, which is what makes a scheduled program a program; `unref` opts a
  background tick out of that. `cancel` takes a handle from either constructor
  and does nothing to one that has already fired, so teardown paths are safe to
  run twice.
- **`std/websocket`.** `connect`, `on_open`, `on_message`, `on_close`,
  `on_error`, `send`, `close`, `is_open`. Each event is its own function taking
  exactly what that event carries rather than the host's
  `addEventListener(name, handler)`, so no handler parameter needs narrowing and
  an event name cannot be misspelled because there are no event-name strings.
  `on_close` gets the code, because that is what separates an outage worth
  retrying from a rejection that will be rejected identically forever. `send`
  into a socket that is not open returns `false` instead of throwing on a
  callback stack the program does not own.
- **Six more Node builtins type-check with nothing installed:** `net`, `timers`,
  `events`, `child_process`, `dns/promises`, `zlib`, joining `fs`, `http`,
  `path`, `os`, `crypto` and `url`.
- **Both apps are pure Glyph, and were re-run rather than merely rebuilt.** The
  chat server dropped `.types/net.d.ts` and still holds three concurrent TCP
  clients. The bot dropped `.types/timers.d.ts` and all six `extern_ts` escapes
  and still passes the cooperative gateway and all three adversarial ones: an
  unprompted opcode 1, a terminal 4004, and a gateway that greets you and then
  stops answering.
- **`scripts/check_apps_are_glyph.py` gates it in CI.** Any `.d.ts`, `.ts` or
  `extern_ts` under `examples/apps/` fails the build, so the answer to the next
  missing capability is to extend the stdlib rather than to write TypeScript.
  G90.

### Still open from this release

- **A host global the stdlib does not wrap is still unnameable.** The general
  form: an ambient `declare var` in `.types/` is invisible to the resolver, so a
  global Glyph ships no wrapper for is reachable only through `extern_ts`, typed
  `unknown`. Two ways to close it and they are different decisions: read ambient
  globals in the resolver, which would also make D37's `new` work on a global
  class; or keep the resolver module-only and treat each unwrapped global as a
  stdlib gap, which is what timers and WebSocket just did. G95.
- **An `Option<T>` field still cannot be read from ordinary JSON.** G91.
- **Locally bound closures still cannot call each other.** G92.
- **An `@example` must still fit on one line.** G93.

### 0.1.66 — Shipped · Reading a key that may not be there

The batch that had accumulated behind a publish hold, led by the first half of
G39. A `Record<K, V>` has arbitrary keys, so `m.name` could not be checked and
was typed `V` anyway; absent, the value was `undefined` under a type saying
otherwise, and nothing reported it.

- **E0224 rejects reading a key out of a map**, and points at `record.get`,
  which returns `Option<V>`. Writes (`mut m[k] = v`) are untouched, because
  building a map is safe. Array indexing is untouched too:
  `noUncheckedIndexedAccess` was measured first and gives 589 errors across the
  examples, almost all `argv[i]` in parsers that have just measured
  `array.len`, and `T | undefined` is not expressible in Glyph, so a program
  could not have fixed them. Zero E0224 across 124 modules. Half of G39; a map
  arriving from another module or the stdlib is still unchecked.
- **`==` is value equality on every type (D42).** It lowered to `===`
  unconditionally, so it silently meant *reference* equality for records,
  tagged unions and arrays: `Some("a") == Some("a")` was false with no
  diagnostic, while the identical expression as an `@example` compared
  structurally and passed. A test could report success on code that did not
  work. Primitives still emit `===`, so ordinary comparisons are unchanged. G65.
- **A match arm that produced no value could throw.** In a lambda it emitted
  neither a `return` nor a `break`, so the case fell into the emitter's own
  `default: throw new Error("non-exhaustive match")`. Twelve lines reproduce it;
  it compiled clean, passed `tsc --strict`, and threw at run time on a match
  that was exhaustive. G94.
- **`glyph run` means the project you are standing in.** The four commands a new
  user is handed ended in a usage error, because `run` required a PATH while
  `check` already defaulted to the current directory.
- **A scaffold pins the compiler that wrote it.** It recorded which TypeScript
  built it and not which Glyph did, so `npm install` now makes a checkout
  buildable with no global install, and a project says in a machine-readable way
  that it depends on Glyph.
- **`glyph check` and `glyph run` run the `@example` gate**, so a failing
  colocated test can no longer turn `build` red while leaving the other two
  green. `--no-test` opts out. G69. Checking a single file also builds only that
  file's own project, not every project beneath it. G72.
- **`std/io` can write without a newline and tell a terminal from a pipe**
  (`print`, `eprint`, `is_terminal`, `stdin_is_terminal`), so a prompt can share
  a line with its answer. G82, G83.
- **`process.set_exit_code`** records the code a program will leave with without
  stopping it, so a failure detected after `main` has returned can be reported
  without tearing down work still in flight. G86.
- **An annotation can wrap onto a line beginning with an operator**, so a long
  `@example` no longer needs a helper function to fit. G93.
- **`glyph fmt` no longer moves a comment written between two annotations into
  the parameter list.** G96. **A bare `let _` discards** rather than declaring,
  so two of them in one scope is no longer a `tsc` redeclaration error. G97.
- **Agents are told they can run Glyph without installing it.** `AGENTS.md` gave
  `npm install -g` and nothing else, which is not a route in a sandbox that
  cannot install globally.
- **`examples/apps/jobq`**, a durable job queue: HTTP API, SQLite store, workers,
  retry with backoff, dead-lettering. The first app here to run `http.serve`.

### 0.1.67 — Shipped · Three entries that had outrun their evidence

G48, G64 and G66 were scheduled together. All three closed by establishing what
the compiler does today rather than by changing it, which is a result worth
stating: a backlog is a snapshot, and two of these had been overtaken by fixes
made for other reasons.

- **G48 closes.** Both halves. The silent-green half went with E0223; the
  spelling half is closed too and nobody noticed, because `({})` compiles and
  `glyph fmt` keeps the parentheses. The entry's complaint that the workaround
  "does not survive the toolchain" was fixed by a formatter batch. A formatter
  test now pins it: un-spelling a workaround puts the file back into the error it
  was formatted out of.
- **G64 is decided rather than fixed.** Glyph will not get untagged primitive
  unions. D8's tagged unions are sealed so a `match` over one is verifiable, and
  an untagged union puts a hole in exactly that. Two Glyph-native answers exist
  and both stay checked: name the cases when you own the type, and take the
  value as `unknown` and narrow with `is` when it arrives from somewhere you do
  not. E0111 now explains both, and says why `extern_ts` is the last resort
  rather than the answer.
- **G66 is resolved by an idiom the entry predates.** An optional field is
  readable; what is not allowed is reading one into a non-optional `T`, and
  `tsc` draws that line exactly right. Optional fields belong on a *wire* type,
  consumed by its own `parse`, and decoded into a domain type carrying
  `Option<T>` — which is what `examples/apps/workflow` does. A Glyph-level error
  for the read was written and reverted: it fired on eight sites across the
  examples and every one was the safe idiom.

No language change ships in this release. The compiler is unchanged except for
E0111's explanation and one formatter regression test.

### 0.1.68 — Shipped · Null is absence, and a loop keeps its type

- **An optional field accepts a JSON `null` as absent.** `field?: T` already
  took an omitted key and a present value; it rejected an explicit `null`, which
  is what every real API sends and what a Discord gateway frame carries in every
  HELLO. The declared type is `T`, and `null` is not a value of `T`, so a key
  holding null is a key holding no value. `glyph gen openapi` had documented
  exactly this mapping while the runtime did not implement it, so a generated
  type rejected the payload it was generated from. `Option<T>`'s tagged encoding
  is untouched. Half of G91, and it did not need the design decision the entry
  was holding: measuring showed the gap was one JSON spelling, not the type.
- **A `for` binding carries the iterand's element type**, so D30 exhaustiveness
  survives a loop. A `match` over a string-literal union inside a `for` went
  from E0218 ("a string match can never be exhaustive, add an `else`") to E0200
  ("missing variants `pro`") — from advice to switch the check off to advice to
  satisfy it. Single-binding only; `for i, x in xs` needs per-binding spans in
  the AST, which is what G37 is about. Half of G67.

### Assessed and deliberately not done

- **G98**, the confusing message when an `is` arm re-reads its scrutinee, stays
  open. The detectable condition has a legitimate counterexample
  (`match f() { is string => "yes", else => "no" }` tests the type without using
  the value), so a check would fire on correct code, which is worse than the
  message it would replace. Doing it properly means attaching a note during the
  `tsc` remap rather than adding a check.

### 0.1.69 — Shipped · Every open gap, and a client that can be bounded

Re-sequenced after re-reproducing each open entry against 0.1.68, then built
as one release rather than five. The earlier order carried three items that had
already closed (G87, G24, G48) and put the open ones behind partly-fixed ones.

All six shipped. Published to npm on 2026-08-09 and smoke-tested from a clean
npx cache in an isolated HOME, which is the check that has caught real problems
here before, and caught one this time too (see the tail of this entry).

- **G88, and it was three gaps wearing one number.** The always-false field
  check fired eight times across the examples tree. A field typed by an
  imported string-literal union lost its D30 membership check the moment the
  type crossed a module, the same hole G76 closed for `match`. A field whose
  type the emitter cannot see into got `field !== undefined` under a message
  naming the type it never checked, so `parse` returned `Ok` for a value it had
  not validated. The first is fixed by resolving an imported descriptorless
  alias to its leaf. The second is `E0304`: the field check now answers a real
  predicate, presence-only, or nothing, and `parse`/`is` are refused at the call
  site on the last. Declaring such a record stays legal, so holding a socket in
  one is still ordinary. `unknown` turned out not to be the third case at all,
  because it claims nothing and presence is the whole check. All eight branches
  are gone.
- **G68.** `json.parse<T>` collapsed every field failure into one issue reading
  `expected T`, while the two-step form named the field and its path for the
  same input. The schema factory now carries the descriptor's own `parse`, so
  the one-step form the guide teaches reports what the two-step form does, and
  an array prefixes the element index.
- **G98.** A note during the `tsc` remap says that `is` narrows the binding, so
  a TS2322 over a re-read scrutinee explains itself. This is the fix the 0.1.68
  assessment named when it declined a *check*: the shape has a legitimate
  counterexample, and a note on a failure that already happened cannot fire on
  correct code.
- **G92.** A name used inside a nested function body resolves against the whole
  enclosing block, so two locally bound closures can call each other. A direct
  forward reference out of an initializer stays an error. The remaining hole is
  TypeScript's own: a forward-referencing closure called before the target's
  `let` runs throws `ReferenceError` at runtime, and `tsc --strict` does not
  catch that either.
- **G89 (D43).** `never` is spellable in user code and behaves as a bottom type.
  A `main` that dispatches to a non-returning `serve` on one arm and
  `process.exit` on the other needs no unreachable `return` and no dead arm.
- **G95, decided.** The resolver stays module-only: a host global the stdlib
  does not wrap is a stdlib gap, filled on demand. Everything reachable stays
  Glyph the compiler knows. `--explain E0103`, the scaffolded `.types/README.md`
  and the imports guide all say so, and the cost is stated: `new` (D37) does not
  work on a global class.

- **G52, the bound half.** A client could not put a bound on a request:
  no timeout, no redirect policy, no `head`, and no final URL, so a redirect was
  invisible. `http.send` takes the whole request as one `Fetch` record carrying
  `timeout_ms` and a `"follow" | "manual" | "error"` policy, because an optional
  trailing argument is the one shape the checker cannot model. The timeout
  aborts the request instead of racing a timer against it, which is what left
  the loser in flight before. `Response.url` is where the response came from, so
  a followed redirect is visible.

Also in this release: the npm README leads with `npx` instead of a global
install, and `glyph init` stops telling a reader who arrived through `npx` to
run a command they do not have.

**Found by the post-release smoke test, fixed for 0.1.70.** `glyph init` names
the command the reader can actually type, and the PATH check behind that was
wrong for the case it was written for. `npx` puts its own `node_modules/.bin` on
the child's PATH, so a run through `npx @glyphlang/glyph init` saw a `glyph`
that disappeared the moment npx returned, and printed `glyph run` to exactly the
reader who cannot use it. A binary running out of an npx cache is temporary
whatever PATH says, so that is what it checks now. The lesson is the one the
release memo already carries: run the published package, not the built one.

The G63 investigation landed without the fix. Keeping a user's `Error` means
aliasing the *compiler's* references rather than mangling the author's names,
and both halves of that were checked against `tsc --strict` including
`globalThis.Array<T>` in type position, which was the half in doubt. It is 53
emission sites in the crate where a mistake is a silent miscompile, so it earns
its own release rather than a corner of this one.

### After the open list

G63, G52 and G30 are done. What follows is the plan through 0.1.75, and it puts
the language before the library on purpose: every new stdlib module widens the
surface where a type Glyph has lost leaks, so the leak is worth closing first.

Every entry below was reproduced against the shipped compiler before it was
written down, which changed two of them. G39 turned out to be far narrower than
its gap entry reads, and the four language items turned out to differ in kind
rather than only in topic: one is a silent wrong answer, one is an uncatchable
crash, and two are ergonomics. They are ordered by that, not by the order they
were listed in.

#### 0.1.71 — the `any` the manifesto forbids

**G39, phase 2.** The entry describes this as unchecked member access in
general, and reproducing it narrows the danger a long way. A misspelled
`array.lenn`, `s.slyce`, or `string.repeeat` does *not* build: `tsc` catches all
three with TS2551. The diagnostic is a back-end error rather than a Glyph one,
which is worth fixing and is not urgent.

What is urgent is the one shape that builds clean and fails at run time:

    return match string.index_of(s, "x") {
      Some(i) => i,
    }

No `None` arm, `glyph build` reports `0 error(s)`, and the program throws
`non-exhaustive match` on the first input that does not contain an `x`. It
happens because the checker cannot model `index_of`, so the scrutinee is
`Unknown` and D9 exhaustiveness never runs.

That surface is exactly nine functions: `array.slice`, `string.slice`,
`string.index_of`, `string.pad_start`, `string.pad_end` and `json.stringify`
(each takes an optional trailing argument, and the arity check compares one
number against one number, so modeling them today would report a false error on
every call that omits it), plus `array.map`, `flat_map` and `zip` (each takes
its element type from a callback the checker does not walk into).

So the work is bounded, and smaller than "model the stdlib from its `.d.ts`":
teach the signature model optional parameters, model the six, then walk one
level into a callback's return for the three. That closes the silent-green class
and leaves the diagnostic-quality half for 0.1.72.

#### 0.1.72 — a typo answers in Glyph's own voice

**G27**: `string.repeeat(...)` is checked against the resolver seed the way a
named import already is, so the same typo stops giving two different experiences
depending on import style. **G79**: the remaining half is that a descriptor does
not synthesize a check for a type it has no descriptor for, which is its
documented soundness limit and now interacts with E0304.

#### 0.1.73 — a mutation that loses an update

**Decided.** All four language items below were reproduced on 2026-08-09 and
their options settled, and the evidence reordered them: what was a list became a
severity ranking, and the silent one goes first.

    let a = bump(c)
    let b = bump(c)      // both read c.n, both write it

Builds clean, prints `expected 2, got 1`. A lost update, silently, which is the
class this language exists to remove.

**The rule: a `mut` whose read and write straddle an `await` is an error.**
Local, decidable without whole-program analysis, and it catches exactly the
failure above. Rejected: forbidding `mut` on any binding captured by more than
one live task (needs escape analysis, and false positives on correct code are
worse than the rule is worth), and an `owned`-style marker for shared mutable
state (bigger, and still available if the narrow rule proves too narrow). A new
D-decision when it lands.

#### 0.1.74 — the host boundary, and the app that will tell us what it needs

Two halves of one theme: give the stdlib the host calls an app currently makes
raw, and then deliberately step outside the stdlib to find what is still
missing.

**`std/net`, `std/dns`, `std/tls`, `std/url`, `std/bytes`.** The driver is
concrete: `chat/daemon.glyph` imports node's `net` directly and holds an opaque
`Socket`, which E0304 now refuses to validate. It is also the only raw host call
in the entire examples tree, so wrapping it closes the last one.

**The dogfood round is an application built on real npm packages.** Nothing in
`examples/apps/` uses one today, which means the 1.0 interop gate ("can a
working engineer use their existing npm dependencies without a hand-written
adapter") has never been tested by an app, only by guides.

That round carries one hypothesis to confirm or kill: **`std/ai`**. The proposal
was `tokenize`, `count_tokens`, `truncate`, `chunk`, plus `llm.generate` and
`llm.embed`, driven by a real shape:

    let tokens = ai.count_tokens(prompt)
    match tokens > context_limit {
      true => { mut prompt = ai.truncate(prompt, context_limit) },
      false => {},
    }

It is **not scheduled**, for three reasons. Token counting is model-specific:
GPT's BPE, Claude's tokenizer and Llama's SentencePiece give different answers
for the same string, so a `count_tokens` that does not name a model is wrong for
every model but one, and shipping several means megabytes of vocabulary tables
in a compiler's standard library. `llm.generate`/`llm.embed` are a network
client for a third-party API with per-vendor auth, streaming and rate limits,
which is the profile already rejected for `std/jwt` and `std/email`: good npm
packages exist and `gen dts` types them. And there is a positioning cost, which
is the one that would be hardest to undo: shipping `llm.generate` in the stdlib
makes the language look like it is *for* AI plumbing rather than *safe under* AI
authorship.

What the example actually shows is a **boundary with a rule** (`tokens >
context_limit`), and `where` refinements plus descriptors already express that.
So the npm round builds against a real LLM SDK from npm and reports what Glyph
lacks. If the answer is `count_tokens`, it will come with the tokenizer it
needs and the reason.

##### Why a host-throw construct is *not* scheduled here

It was, and the evidence removed it. A throwing host call is uncatchable today:

    fn risky() -> number {
      return extern_ts("(() => { throw new Error('host blew up'); })()")
    }

`tsc --strict` passes and the process dies. Two designs were worked through
before either was written down, and both are recorded because the second was
nearly built:

- **`host.attempt(fn() -> T) -> Result<T, HostError>` is wrong**, and testing
  the lowering is what showed it. A stdlib function cannot see whether its
  argument is async, so `attempt` over a rejecting async call returns **`Ok`**
  and the rejection escapes unhandled. That is a boundary reporting success for
  something it never checked, the same defect E0304 exists to prevent, and it is
  worse than the crash it replaces because a crash is loud. Splitting it into
  `attempt`/`attempt_async` moves the error to whoever picks; awaiting
  unconditionally makes it async-only, and a sync `fn` calling an async one is a
  `tsc` error, so sync host calls would lose containment entirely.
- **A `try` expression works** (the compiler can see async-ness and lower
  accordingly) **and is not justified yet.** Twenty-six rounds produced exactly
  two host-throw incidents, and both were fixed in the library rather than by
  catching: `pool` dying on an unhandled rejection became `pool_settled`, and
  Node's `writeHead` throwing `ERR_INVALID_CHAR` on an emoji from a form field
  (which took a whole server down, reachable by any visitor) became
  `sanitize_header_value`. The apps make one raw host call between them. Adding
  mandatory grammar at every host seam to catch a class the stdlib has absorbed
  twice is ceremony bought with evidence that points the other way.

So the construct waits for the npm app. If that app reaches for containment
forty times, it will be designed against what it actually needed. Two notes to
carry into that decision: `try` must be mandatory at the seam or it gives back
the silent crash, and `HostError` has to carry the thrown value
(`{ message: string, value: unknown }`) because JavaScript can throw anything and
claiming `{ message: string }` is a shape nothing checked.

#### Riding along: the two ergonomics items

Neither earns a release of its own, and both are decided.

**`?` on an `Option`, inside a function returning `Option<T>`.** Today it is
`E0201`, so every optional chain is a nested `match`. The rule is one sentence,
the lowering mirrors the `Result` case exactly, and E0201 already exists to
police the misuse. A new D-decision when it lands.

**`?` across two error types stays explicit.** `E0203` fires when a
`Result<_, int>` is propagated into a `Result<_, string>`, and the fix is
`.map_err(...)` before the `?`. Rejected: converting implicitly through a
declared conversion the way Rust's `?` does, because it changes an error's type
at a character that does not look like a conversion. The work is to make E0203
quote both types and name `.map_err` as the fix.

#### 0.1.75 — finish what is half-built

WebSocket binary messages, a WebSocket server, connection options and
subprotocols, WebSocket integration tests, and `std/sse`.

#### After that, the loop decides

`std/channel`, `std/queue`, `std/lock`, signals and graceful shutdown,
`std/compress`, `std/multipart`, URL encoding, `std/db`, `std/transaction`,
`std/cli`, `std/config`, `std/metrics`, `std/tracing`, a first-class test
framework, `std/mock`, `std/cache`, `std/job`, `std/observable`. All plausible,
none scheduled. The rule that has worked for twenty-six rounds is that an
application asks for the next module, and the ones nothing asks for do not get
written.

**Deliberately not planned:** `std/jwt`, `std/email`, `std/mime`, `std/auth`.
Good npm packages exist, they are not host boundaries, and `glyph gen dts`
already materializes their types. Shipping our own would be parity-chasing, and
parity is not the bet.

Already covered, so off the list entirely: `std/uuid` and secure random
(`crypto.random_uuid`/`random_hex`), `std/env` (`process.env`), `std/text`
(`std/string`), `std/assert` (the prelude `assert`), `std/serialize`
(descriptors plus `std/json`), and every G30 item, which shipped in 0.1.70.

New this release, worth its own entry when it comes up: a **generic tagged
union** carries no runtime descriptor (only generic *records* do), so a field
typed `Heap<T>` is unverifiable and E0304 now refuses to parse a record holding
one. That is the honest floor rather than the feature; generic union descriptors
are the feature.

### 0.1.70 — Shipped · An index that is wrong, and a type that can be called Error

Published 2026-08-09 and smoke-tested from a clean npx cache in an isolated
HOME. That check confirmed the previous release's own finding is fixed in
production: `glyph init` reached through `npx` now says `npm install && npx
glyph run` rather than naming a command the reader does not have. The
provenance bundle attached to this release verifies offline with
`gh attestation verify --bundle`, and rejects a tampered archive, which is the
half of Signed-Releases that Scorecard could not see before.

- **G30, the index-safety half.** `cells[999]` type-checked clean, passed
  `tsc --strict`, and handed back `undefined` where the compiler had promised a
  `Cell`, which then travelled until something dereferenced it somewhere else.
  The two options on the table were measured and both rejected:
  `noUncheckedIndexedAccess` is 428 errors across the examples and our own
  stdlib for a diagnostic that arrives as a mapped `tsc` error, and returning
  `Option<T>` from `xs[i]` rewrites 437 index expressions. What shipped keeps
  `xs[i]` typed `T` and bounds-checks the emitted read, so out of range throws a
  `RangeError` naming the index and the length. Glyph had been worse than Rust
  here, which lies in the same way but panics at the bad index rather than
  letting `undefined` travel. `array.get(xs, i) -> Option<T>` is the safe read.
  All 323 examples pass unchanged with the check on.
- **G63.** A domain type may be called `Error` again. A union with an `Error`
  variant emitted `export function Error(...)` at module top level, and the
  `new Error(...)` the compiler wrote below it called the variant, so the name
  was taken away by E0110 and the spreadsheet app carried `Cellerr`. A module
  that shadows one of the globals the emitter uses now captures the real one
  (`const __glyph_Error = globalThis.Error;`) and the compiler's own references
  go through that. The author's name emits verbatim, and a module that shadows
  nothing emits exactly what it did before. Only four of the five could be
  freed: `Array` is also a Glyph type name, so a local `type Array` redefines
  how the module spells an array rather than shadowing a global, and no capture
  fixes that. It stays reserved with E0110 naming the prelude origin. The app
  that reported the gap was updated in the same pass: `sheet` reads
  `Number | Text | Empty | Error` now, and needed 162 captured references to
  do it, which is the 139 `new Error(...)` and 23 `Number.isInteger` the entry
  counted two releases ago.
- **Clippy runs in CI**, which found a test that never ran, another registered
  twice, and two doc comments detached from their functions. **CodeQL** covers
  the Rust, the TypeScript and the workflows.
- Supply-chain hygiene: every action pinned by commit, workflows read-only by
  default, Dependabot on, and the release now attaches its provenance bundle so
  the attestation is checkable without the network.

The type still says `T`, so this changes when you find out rather than making
the type honest. The fuller fixes stay on the table if the runtime failure ever
proves too late.

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
  real servers, so the 744-line hand-written stdlib is not the only answer. The
  server half moved in 0.1.46: `std/http` sets response headers, so an HTML page
  and a redirect no longer need a hand-written server. Still open.

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

- **Mark the six apps under `examples/apps/` as projects, and collapse the CI
  loop.** D41 landed the mechanism (a `package.json` with a `"glyph"` key is a
  module-resolution root), but none of the apps carries the marker yet, so
  `glyph build examples` still reports their sibling imports as E0104 and
  `.github/workflows/ci.yml` still builds each app on its own from a copy of
  `examples/` that excludes `apps/`. Adding the six manifests and reducing that
  job to a single `glyph build ../examples` is what actually closes G78.
- **One implementation of project-root resolution for the CLI and the LSP.**
  `glyph-cli`'s `config::project_for_file` and `glyph-lsp`'s `project_root_for`
  read the same marker the same way and have to keep agreeing, or
  go-to-definition finds nothing in a tree `glyph build` compiles fine. They
  cannot share code today because `glyph-cli` depends on `glyph-lsp`. Hoisting
  the manifest reading onto a crate both can depend on is the fix; the duplicated
  climb is pinned by tests on both sides until then.

- **One name for the TypeScript stage: `--no-tsc`.** ✅ **done.** Shipping
  `glyph check --no-tsc` next to `build --no-check` and `run --no-check` gave one
  stage two names, so there was no single string to grep for "the flag that skips
  `tsc`". `--no-tsc` is now canonical on all three commands; `--no-check` stays
  on `build` and `run` as a hidden alias, the way `--check` and `--test` are
  already carried, so existing scripts and CI keep working. The messages that
  suggest the opt-out name the canonical flag.
- **`build --json` no longer reports green with no `tsc`.** ✅ **done.**
  `emit_build_json` computed `ok` from the diagnostic count alone, so a machine
  without `tsc` on `PATH` got `"tsc": "not-found"`, `ok: true`, and exit 0, while
  the same build's text path exits 2 and `check --json` reports `ok: false`. A
  stage that was requested and could not run is not a pass; `build` now agrees
  with `check` on the object and on the exit code, which is what the docs claimed
  all along. A test spawns both commands with an empty `PATH` and compares them.
- **`glyph check` stopped paying for a cache it deletes.** ✅ **done.** The
  scratch directory was named from `source_fingerprint` (a read and hash of every
  `.glyph` and `.types/**/*.d.ts` in the tree) plus pid and counter, and then
  removed on the way out, under the name `glyph-check-cache`. The pid and counter
  are all the uniqueness a directory nobody keeps needs; the fingerprint is gone,
  the directory is `glyph-check-scratch`, and the now-empty parent is removed
  too. Making `check` a real cache (keyed on the fingerprint, kept between runs,
  skipping emit and `tsc` on an unchanged tree, the way `glyph run` already
  works) is the other option and stays open: it is a behaviour change with its
  own invalidation and disk-retention questions, not a cleanup.
- **Two string syntaxes, one undocumented** (G61). D12 promises one string
  syntax; the lexer dispatches `"""` from the general string path, so a
  triple-quoted literal is reachable from any expression position, decodes no
  escapes, and still interpolates. Either the parser rejects `"""` outside a
  `@doc` annotation or the spec documents the second form; both are spec calls.
  Filed in `docs/dogfooding-gaps.md` as G61.
- **Four silent holes closed: `await` context, valueless match arms, bare
  assignment, `?` in an arm.** ✅ **done.** Four diagnostics that a real app hit
  and the compiler did not: `await` in a non-`async fn` built clean and failed at
  `tsc` (now E0222); `X => {}` in a value-position `match` emitted `case X: {
  break; }` so the binding stayed `undefined` (now E0223); a bare `x = e` said
  "unexpected token: Equals" instead of teaching `mut` (now E0008); and `?` in an
  expression-form arm was refused with a false "not implemented yet" although the
  same code in a block arm compiled (the arm body now emits through the same
  hoisting path as a `let`/`return` value). A string-keyed object literal in an
  arm (`{ "Content-Type": v }`) also parses as a literal now instead of a block.
  Two of the four gaps it was aimed at (G35, G44) close here; **G24 and G48 are
  half closed and are marked that way in `docs/dogfooding-gaps.md`.** What stays
  open. (1) There is still no way to spell an empty record in arm position, so
  `linkcheck` still needs its `no_cache()` constructor and the comment naming
  G48: `{}` is a no-op block in statement position (9+ uses across the corpus)
  and E0223 reports the value-position case rather than resolving the ambiguity.
  (2) Top-level `await` (a `const` initializer with no enclosing callable) is
  left permissive; the emitted ESM accepts it and the spec has no stance. It
  means a module can have implicit async initialization with nothing in the
  source marking it, which needs a spec answer rather than staying undecided.
  (3) E0223 can only judge a tail-position match when the callable declares a
  non-`void` return type, so an unannotated lambda (`array.map(xs, fn(x) { match
  x { A => {}, ... } })`) is still silent; closing it means a second,
  emitter-side backstop that the LSP cannot see. (4) `match load(p)? { ... }` (a
  `?` in the *scrutinee*) is still rejected, now as E0303 ("`?` cannot be used in
  this position") with a bind-first fix, instead of the old false "not
  implemented yet". Supporting it means hoisting the scrutinee's unwrap ahead of
  the lowered `switch`, at four more call sites.
- **A teaching parse error that hides the rest of the file** (S). E0008 and E0006
  both abort the parse, so an author with five bare assignments fixes them one
  build at a time and sees no other diagnostic until the last one is gone. Both
  exist to teach a rule a newcomer breaks repeatedly, and a diagnostic that
  reports one instance per build teaches slowly. Recovering to the next statement
  is the fix; it needs a parser error-recovery point that does not exist yet.
- **`glyph fmt` breaks a working program at `=> ({})`** (S) — the empty-record
  arm workaround `=> ({})` builds and passes `tsc --strict`; `glyph fmt` reprints
  it as `=> {}`, an empty block, and the formatted file fails with E0223.
  Reproduced end to end. The formatter has no grouping node, so the parentheses
  never reach the AST it reprints. D14 promises `fmt` round-trips, and this one
  changes meaning, so it is a formatter correctness bug independent of how G48's
  spelling question is answered. Tracked as G60. The grouping node is the narrow
  fix and it also takes `no_cache()` out of `linkcheck` instead of blessing it.
- **Value-position `match` lowers to a flat switch** — ✅ **done (0.1.42).** The
  emitter picked between its statement `switch` and its value IIFE on the wrong
  condition, which cost three different `tsc` errors on code Glyph called clean.
  Written up with the trip that found it under "linkcheck dogfood trip" above.
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

- **A layout rule for chains** (M, formatter). The `&&`/`||`/`??` half is done:
  an operator chain that does not fit breaks one operand per line with the
  operator leading, under a bracket-depth guard for D1, which is what the three
  damaged `||` sites needed (they get the new layout the next time
  `examples/apps` is reformatted). What is left is the `.`-chain, and it is
  blocked on one question: whether `array.` counts as a link, since breaking it
  costs `grep "array.map("` and greppability is a wedge pillar.

  The printer has no chain-aware
  path, so when a long expression has to break, the only breakable point it can
  find is an argument list, and it takes the innermost one. Removing the
  `INLINE_MAX` exemption (G54) made that visible: three sites in the reformatted
  `examples/apps` break an innermost argument list in the middle of a `||` chain,
  listed under G18 in [`../dogfooding-gaps.md`](../dogfooding-gaps.md). Two
  candidate rules (break before every `.` once the chain overflows, or break only
  as many links as it takes), plus a narrower option that is not the chain rule
  itself: refuse the multi-line form when the list's immediate parent is an
  expression that cannot break either (a binary operand, a JSX attribute value).
  D1 constrains all of them: a break outside `()`/`[]`/`{}` ends the statement,
  so a chain in a top-level `const` initializer stays on one line whatever we
  pick.

- **The width check stops at the closing delimiter** (S, formatter). It measures
  a list up to its own `)` or `]` and never sees what is printed after it, so a
  `fn` signature whose ` -> Result<T, E> {` tail pushes it past 100 columns reads
  as fitting. Three of the five over-wide code lines in `examples/apps` are that
  shape. Also outstanding: `examples/corpus` and the numbered examples are not
  `fmt`-clean and would change under a reformat, which wants its own commit.

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

*The release ritual, in the order the branching strategy requires.*

    1. branch   release/0.1.NN     bump the versions, the notes, the counts
    2. PR       CI verifies all eleven version strings agree before `main` sees them
    3. merge    rebase onto main
    4. tag      from main, after the merge
    5. push     the tag; release.yml publishes

Step 4 must follow step 3, and the reason is specific to rebase merging: a rebase
gives every commit a **new SHA** on `main`. A tag placed on the release branch
before merging would point at a commit that no longer exists in `main`'s history,
so the published artifacts would build from a SHA nobody can `git log` to.

Release prep goes through a pull request because it is the change with the
highest ratio of purely mechanical to catastrophic-if-wrong: eleven version
strings that must agree, plus the badge URL, the versions page and the README
counts. `check_versions.py` checks exactly that, and it should run before `main`
has the commit rather than after. Two releases have shipped broken from this
step already.

*Branch protection, and what it deliberately does not include.* `main` blocks
force pushes and deletion, requires linear history, and applies all three to
admins, so no token can rewrite or remove the branch's history. Required status
checks are deliberately absent: GitHub blocks a direct push to a branch that
requires them, and the checks cannot have run on a commit that has not been
pushed, so enabling them would mean adopting pull requests. That is the same
decision `Code-Review` and `CI-Tests` are waiting on, and it is a workflow
change rather than a setting.

*Supply-chain score, and what is left.* The first OpenSSF Scorecard run read
3.6. Pinning all eighteen action references by commit, dropping every workflow's
default token to read-only, and adding Dependabot took it to 5.4. The remaining
zeros, in rough order of value:

- **Signed-Releases (0).** The SLSA attestation is real but lives in GitHub's
  attestation store, so the check cannot see it. Attaching the provenance bundle
  to the release as an asset would fix it, and takes effect on the next release.
- **SAST (0).** No CodeQL workflow. It covers both Rust and TypeScript here.
- **Code-Review (0) and CI-Tests (unmeasured).** Both read pull requests, and
  this repo commits straight to `main`. Changing that is a process decision, not
  a config one.
- **Branch-Protection (0).** A repository setting, so it needs an owner rather
  than a commit.
- **Contributors (0)** wants people from two or more organisations, and
  **Fuzzing (0)** wants a recognised fuzzing setup; the repo has a fuzz target
  but it is not registered with one.

*Release note:* the Socket badge URL carries the package version, so a bump
touches it too. `check_versions.py` fails when it falls behind rather than
leaving it to be noticed by a reader looking at a report for a version they are
not installing.

*Sequencing note:* the release carrying the **Next** marker above is the committed
target. Everything past it is a proposal ordered by dependency, and it gets
re-sorted at each release boundary.
