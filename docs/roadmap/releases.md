# Release roadmap (0.1.x → 1.0)

The 12-step plan in [`overview.md`](overview.md) built the toolchain — that work
is shipped. This file tracks the **feature releases** layered on top and
published to npm as `@glyphlang/glyph`. One release carries the "Next" marker and
is committed; everything after it is directional and re-sorts as we learn.

The website has its own roadmap and lifecycle in [`website.md`](website.md):
it ships on every merge to `main`, independently of npm releases.

Each item keeps a rough T-shirt effort (S/M/L) and traces to a real source: the
persona-testing issue inventory, the generation follow-ups, the site's "on the
way" promises, or the standing deferrals in CLAUDE.md.

Two gates keep this file honest, because both of its failure modes have happened.
`check_findings_scheduled.py` fails when a finding in `dogfooding-gaps.md` is open
or partly fixed and is named nowhere here, which is how three entries were
reproduced release after release with nobody deciding anything about them. And
`check_plans_fresh.py` fails when an unshipped `#### 0.1.NN` plan has not been
re-read within five releases: a plan is a set of claims about a compiler that
keeps moving, and the 0.1.79 section had five that had quietly stopped being true
plus one that never was. Re-reading means checking the claims against the
compiler you just built and correcting what changed, then moving the
`*Reviewed against X.Y.Z.*` stamp. Moving the stamp without reading defeats it
entirely and is the one thing no gate can catch.

## The question the whole roadmap answers

**Can an agent make a non-trivial semantic change to a Glyph program without
rediscovering the compiler's reasoning?**

Every release below is measured against that sentence or it does not belong
here. The gap numbers are bookkeeping; they are not the goal, and a release that
closes eight of them while moving that sentence nowhere has not earned its
number.

The progression, read as one story rather than seven:

```
0.1.109  the stdlib is complete enough that an agent writes ordinary code
0.1.110  prove the repair loop, on a real application, measured
0.1.111  make relationships traversable, with a vocabulary
0.1.112  generalize impact, without becoming a graph database
0.1.113  make semantic facts identical across every surface that emits
0.1.114  make identities stable under incremental edits
0.1.115  make the same knowledge fast enough for a human in an editor
```

**The success criterion changes at 0.1.110, and this is deliberate.** Until now
the honest measure has been defects closed against a compiler that keeps moving.
From 0.1.110 the measure is a task an agent either completes or does not:

```
Task: add `Cancelled` to OrderState.

                          ordinary TypeScript agent      Glyph
  files searched                              ?            0
  compiler queries                            ?            1
  semantic sites missed                       ?            0
  incorrect edits                             ?            ?
  repair iterations                           ?            ?
  time to green                               ?            ?
```

Those question marks are the point. They are not filled in, because filling them
in before running the experiment is how a benchmark becomes marketing. The
instrument is defined before the result is seen, the same agent and the same
prompt run on both sides, and the losing run is published.
`benchmarks/impact-before-edit/` is the seed of this, and today it measures the
prediction rather than the task.

## The next phase: teach the agent how to ask, not how to know

The line from 0.1.103 to 0.1.117 built the beginnings of a compiler that can
explain a program to an agent: identities, relations, impact, provenance,
measured latency, and a checker that catches what it used to let through. The
next releases do not spend that on more traversal or more checker features.
They spend it on the surface an agent actually reads, because an LLM does not
struggle to write text; it struggles with what is true about this codebase and
this language right now, and a compiler can answer that deterministically.

Three priorities, in order, and each is measured by a question an agent can
put to the compiler and get a complete answer to:

1. **An agent-native semantic API.** One call describes a symbol completely
   enough to write correct Glyph against it: kind, fields with types, variants
   with payloads, the construction syntax of each form, whether a match over it
   must be exhaustive, callers, dependencies, assignability between two types,
   and a compiler-verified example. Today the tools answer navigation questions
   (where is it, what refers to it, what breaks); the description question is
   answered in pieces or not at all. The gaps below are measured by probing the
   tools on a real project and recording what came back.
2. **Diagnostics as a repair protocol.** A diagnostic carries, as fields rather
   than prose, what was expected and what was found, the symbol that caused it,
   the related symbols (a union's variants for a coverage error), the valid
   alternatives, and the constraints a repair must keep. The loop becomes
   generate, prove, repair, and the agent does not need to know Glyph perfectly
   because Glyph tells it precisely what it got wrong and what would be right.
3. **A knowledge surface generated from the compiler.** A new language has no
   training data. The spec, the prelude, the stdlib signatures, the diagnostic
   codes, the canonical examples and the negative examples (a wrong program
   paired with the diagnostic it draws and the correct form) are emitted by the
   compiler as machine-readable files and kept true by the same gates that keep
   the suite green, so an agent's context is manufactured rather than hoped for.

What this phase does not build: an autonomous coding agent, a chat assistant, a
retrieval layer over the docs, vector search, another code graph. The compiler
is the intelligence layer and the agent outside it can be simple. The claim
the phase works towards is that an agent writing Glyph is not relying only on
what it learned about programming; it is consulting the compiler that defines
what is valid, continuously.

Two lanes sit before that work and are kept small: 0.1.119, the repository's
own hygiene, and 0.1.120, the emitter reading the checker's facts instead of
rediscovering them, which is a precondition for a compiler whose answers can be
trusted as one truth. The three priorities are 0.1.121 onwards, and their
entries are ledger findings with reproductions, not a wish list: each records
what the tool answers today, verbatim, and what the complete answer needs.

**The rule that outranks everything below.** Never return an inferred
relationship as if it were a compiler-proven one. `tests/exact-or-absent/` is
the gate, and it grows a case per relation as each one lands.

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
`examples/apps/tasks/main.glyph` (0.1.25) but harder (raw-body HMAC verification, a
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
(`examples/apps/minesweeper/main.glyph`). No npm dependency, no server, no JSX. The
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
(`examples/apps/expenses/main.glyph`) that reads a CSV ledger, validates every row,
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

The loop pointed at `examples/apps/adventure/main.glyph`: rooms, an inventory, a
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

The loop pointed at `examples/apps/linkcheck/main.glyph`: walk a directory, scan
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
opinions. `examples/apps/linkcheck/main.glyph` ships with the release with all three
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

The loop pointed at `examples/apps/bracket/main.glyph`: a single-elimination
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
  Refused since 0.1.169 (G441): write `"""${string.join(p, ".")}"""`, or bind
  the joined text with `let` first.

## shortlink dogfood trip — a green build running yesterday's shim

The loop pointed at `examples/apps/shortlink/main.glyph`: shorten a URL, redirect a
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

The loop pointed at `examples/apps/settle/main.glyph`, a shared-expense settler: parse
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
`examples/apps/settle/main.glyph`, the loop below it reads `for i, w in wire.expenses`
and still binds a number: feed it a ledger whose third entry has three decimal
places and it reports `expense 3`, not `expense 21`.

Also in the release: the app ships as `examples/apps/settle/main.glyph`. Record a
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
`examples/apps/shortlink/main.glyph` imports no `extern/*` and no Node builtin, and
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

The app is the proof. `examples/apps/shortlink/main.glyph` went from 615 lines to 494:
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
capabilities and then rewrites `examples/apps/linkcheck/main.glyph` until none of the
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
own `case`. `examples/apps/bracket/main.glyph` lost the block-arm workaround and the
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
  `examples/apps/linkcheck/main.glyph` still carries the `no_cache()` constructor.
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
absent and stdout is empty. On `examples/apps/expenses/main.glyph` it reports "10
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
answer for a program flag that collides. `examples/apps/expenses/main.glyph` grew a
`--min AMOUNT` filter that exercises it with an exact `Decimal` comparison.

G55 was the docs round it asked for: three things the author of a previous app
reimplemented because they could not find them. `math.max` was reachable only
through a slash-grouped line, so grepping the reference for it found nothing;
every grouped line on that page is now one call per line, and
`examples/apps/linkcheck/main.glyph` lost its hand-rolled `max_of`. The two-import
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
  `examples/apps/shortlink/main.glyph` writes all five HTML builders as real
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
`examples/apps/bracket/main.glyph` was the app carrying the workaround: two identity
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
`examples/apps/shortlink/main.glyph` writes all five HTML builders as real multi-line
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
walks pairs, as `examples/apps/shortlink/main.glyph` does. No workaround comes out of
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
- **G89 (D47).** `never` is spellable in user code and behaves as a bottom type.
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

G63, G52 and G30 are done. What follows is the plan through 0.1.79, and it puts
the language before the library on purpose: every new stdlib module widens the
surface where a type Glyph has lost leaks, so the leak is worth closing first.

Every entry below was reproduced against the shipped compiler before it was
written down, which changed two of them. G39 turned out to be far narrower than
its gap entry reads, and the four language items turned out to differ in kind
rather than only in topic: one is a silent wrong answer, one is an uncatchable
crash, and two are ergonomics. They are ordered by that, not by the order they
were listed in.

### 0.1.71 — Shipped · The `any` the manifesto forbids

Published 2026-08-10 and smoke-tested from a clean npx cache in an isolated
HOME. The published binary reports `E0200` on the `match string.index_of` with
no `None` arm, which is the shape that built with `0 error(s)` and threw one
release earlier. First release to go through the pull-request workflow end to
end, and the required checks earned it: they blocked two merges on a temp-file
race that reproduced roughly never locally, and held a dependency bump
(`ariadne` 0.6) that would not compile.

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

**Shipped, and it is the six rather than the nine.** `FnParam` gained an
`optional` flag and the arity check now reads a minimum and a maximum, so the six
trailing-optional functions are modeled and the silent-green case is closed: a
`match` on `string.index_of` with no `None` arm is `E0200` at compile time. All
124 modules in the examples tree still build, which is the thing that mattered:
modeling these could have produced a false arity error on every call that omits
the last argument, and did not.

`map`/`flat_map`/`zip` were attempted and reverted. A callback modeled as
`fn(T) -> U` rejects `array.map(items, async fn(n: number) -> number { ... })`,
which is legitimate and appears in the examples, because assignability compares
`is_async` (D40). Closing them needs a callback type that admits a sync and an
async function alike, which is a decision about colorless async through a
callback, not more table entries. It moves to 0.1.72 beside the other
diagnostic-quality work.

### 0.1.72 — a typo answers in Glyph's own voice

**G27**: `string.repeeat(...)` is checked against the resolver seed the way a
named import already is, so the same typo stops giving two different experiences
depending on import style. **Done**: recorded during resolution and checked
against the same export list, which also required a gate keeping that list in
step with the runtime, since it is now the authority for both spellings. **G79**: the remaining half is that a descriptor does
not synthesize a check for a type it has no descriptor for, which is its
documented soundness limit and now interacts with E0304.

#### 0.1.73 — Shipped · A project changes compiler on purpose

**Published 2026-08-14.** Found by reading `glyph-hello`, the outside app above. Its
`package.json` said `"@glyphlang/glyph": "^0.1.72"`, which our own `glyph init`
wrote, and the caret is wrong in a way that took an outside project to make
obvious. On a `0.x` version npm's caret still floats the patch, so that range
accepts every later 0.1.x. Set that beside what `docs/stability.md` promises out
loud, that a 0.1.x release "may reject code that previously compiled (that is
usually the point)", and the scaffold we ship was arranging for a stranger's
green build to go red on an `npm install` run for an unrelated reason.

He also had no committed lockfile, because nothing told him to commit one, and
no way to learn a release had happened, because nothing in the CLI has ever
mentioned one. The three failures are the same failure: **there was no path from
"a new Glyph exists" to "this project is on it."** Fixing four of the five links
would have left the chain broken, so all five landed together.

- **`glyph init` pins exactly.** `SCAFFOLD_GLYPH` drops the `^`. A test asserts
  the pin is exact *and* that it is not a range, because the second is the thing
  that regresses silently.
- **`glyph doctor` reports this compiler against the registry**, with the release
  notes URL when a newer one exists. Three rules hold it in place: an available
  release never changes the exit code (doctor runs in CI, and a publish ten
  minutes ago is not a broken toolchain), only commands that exist to answer
  questions may look (`doctor` and `upgrade`, never `build`/`run`/`check`), and
  not reaching the registry is reported, not failed.
- **`glyph upgrade`** rewrites the one pin, runs `npm install`, and prints what to
  read. `--dry-run`, `--to <version>` (including backwards), `--no-install`. It
  reads a caret as well as an exact pin, so every project scaffolded before this
  release can be moved onto an exact one, which is the population that needs it.
- **`glyph init` says to commit the lockfile.** An exact pin buys a reproducible
  build only if the lockfile is committed too, and the scaffold's `.gitignore`
  not listing it is not a hint anyone reads.
- **The release notes are reachable from the CLI**, from both `doctor` and
  `upgrade`, rather than from a website you have to already know about.

**No HTTP client was added, and that was the design constraint.** The compiler
has no network dependency at all and this release does not give it one: npm is
already required (it is how Glyph is installed, and `gen` already shells out to
`npm root -g`), so `npm view` answers the only registry question we have without
pulling a TLS stack into a compiler that had none. `--fetch-retries=0` and a
3-second fetch timeout matter more than they look: npm's defaults retry with
backoff, so an offline `doctor` would have sat for most of a minute before
admitting it could not connect. Measured: npm absent reports in 9ms, an
unreachable registry in 0.93s, both exit 0.

Verified on the app that prompted it. `glyph upgrade` moved `glyph-hello` from
`^0.1.72` to an exact pin as a **one-line diff**, `npm install` wrote the
lockfile it never had, and the pinned compiler builds it green. Rewriting the
pin textually rather than re-serializing the manifest is what keeps that diff to
one line: `package.json` belongs to the developer, and a formatter's opinion is
not ours to impose during an upgrade.

Docs updated in the same pass: `docs/stability.md` gains the section this is
really about, `getting-started` gains both commands, `AGENTS.md` and both
`llms.txt` mirrors tell an agent not to widen a pin to make an install resolve,
and `web/answers/upgrades/` is answer 22, "Will a new Glyph release break my
build?"

**One defect the release cut found in the release's own feature.** Because the
repo version had always equalled npm's `latest`, `doctor`'s update branch had
only ever been unit-tested; the live path had returned `Current` every time it
had ever run. Exercising it properly (a binary stamped 0.1.71 against the real
registry) confirmed the branch renders and the JSON is well-formed, and turned
up the neighbouring case: a build *ahead* of the registry was also classified
`Current`, so it printed "the newest published release" while being newer than
anything published. That is every dev build, and every release between its
version bump and its publish. There is now an `Ahead` state that says which
version the registry actually has, and the classifier is a pure function with
all four orderings pinned by a test that was watched failing before it passed.

#### 0.1.74 — Shipped · Three ways the compiler reported green and was wrong

**Published 2026-08-14.** Three pieces of work, shipped as one release because none of them
can break a build that was green and correct, so separate version numbers
would have bought bisection points nobody would use at the cost of three
irreversible publishes. The three stories are kept apart below, since the
record of what was found and why is worth more than the publish cadence.

##### The generator reported green for a file that fails

**Both halves** of `gen dts` that round 28 found, and they had the same
shape: exit 0, a success line, and output that could not compile. `gen dts` is
the answer the interop story names, so a generator that lies about its own
output is worse there than anywhere else.

**G104: a relative specifier carrying a file extension resolved to nothing.**
`resolveModuleFile` had a candidate commented "spec already carried an
extension" that only matched when the file literally existed under that name,
which for a types-only package it never does. So `export * from "./a.js"`
resolved to nothing and a barrel materialized **zero** types, reporting it with
the OpenAPI generator's message about `components.schemas`. The extension is
mandatory under `moduleResolution: nodenext`, so every ESM-authored typed
package was in that class. Fixed by mapping the runtime extension to the
declaration file that carries its types, as a lookup rather than a strip: the
mapping is not uniform, since `.mjs` takes its types from `.d.mts`. Measured on
the package that produced the finding, **`glyph gen dts date-fns` went from 0
types to 280.**

**G103: two source types written under one Glyph name.** `sanitize_type` drops
every non-alphanumeric character, so it is many-to-one, and it runs *after* the
`.d.ts` reader's own uniqueness check, which sees the still-distinct dotted
names. The check was not too weak, it was upstream of the only step that can
create a duplicate, and in a different language and process from it. First read
as a namespace bug and it is not: `tokens_list` and `TokensList` collide with no
namespace in sight.

The check now runs on the emitted names, lists every colliding source so one run
resolves the file, and writes nothing. `--rename Source=GlyphName` resolves it
and is recorded in the generated header, so `glyph regen` replays the choice
instead of failing on the collision it was already told how to resolve.

**Why erroring rather than renaming automatically, in pillar terms.** Three
options, and only one keeps the wedge intact. First-wins with a warning is what
the existing cross-file path does, and it is the worst of the three: with
`marked`, a field typed `Tokens.List` would bind to `TokensList`
(`Token[] & { links }`), so a descriptor would validate the wrong shape at a
boundary, quietly. That is the failure verifiability exists to prevent, and it
is worse than the crash it replaces. Auto-renaming to `TokensList2` keeps
verifiability but spends greppability, since that name appears in no source, and
risks diff stability if the numbering follows declaration order. Erroring keeps
both wedge pillars and spends abstraction, which is the polish tier, and the
manifesto's tiebreak is that the wedge wins. The developer names it once and the
name means something.

**What this does not fix, recorded rather than glossed.** `glyph gen dts marked`
now writes 46 types and exits 0, and the file still fails to build with 14
`[E0103] unresolved name`. The difference is that **all of them were disclosed
in `gen`'s own notes**, nine of them naming exactly the names that then failed.
The reader materializes interfaces and type aliases, and marked's surface is
classes (`Lexer`, `Parser`, `Renderer`), TypeScript utility types (`Omit`,
`Pick`), and host types (`RegExp`, `Promise`). That is **G108**, and it is the
next thing standing between `gen dts` and a package a working engineer would
call usable.

##### A loop index that was a string

Round 31 ran four apps at once. The finding that mattered was not the
one that looked worst.

**G109: a `for k, v` could compute the wrong number in a green build.** An
array's pairs bind a **number** index (`it.entries()`); a record's bind a
**string** key (`Object.entries(it)`). The emitter chose by the iterand's static
type and, per its own comment, defaulted "to a record when it is unknown". So

    match Wire.parse<number>(raw) {
      Ok(w) => { for index, key in w.keys { io.println("next=${index + 1}") } },
    }

printed `next=01` and `next=11` instead of `1` and `2`, out of a build reporting
`no diagnostics` and `tsc --strict passed`. The identical loop over the identical
declared `Array<string>` emitted `.entries()` when the value came from a
*non-generic* `parse`, so two spellings of one idiom disagreed at run time. This
is the silent-miscompile class, and it is the one the whole project exists to
remove.

**The fix is to stop guessing.** `iter_shape` answers Array, Record or Unknown as
three cases rather than two, and Unknown emits `__glyph_pairs(it)`, a bootstrap
helper that reads `Array.isArray` where the compiler could not. A settled type
keeps its direct emit, so no typed loop pays anything for the one that could not
be typed. `Ty::Imported` is treated as unsettled, because a type crossing a
module boundary carries no shape with it. The regression test was watched failing
against the old behaviour before it was kept, and a second test pins that known
arrays and records still emit directly.

**G111: a stdlib type imported by name lost two checks.** `import std/http {
HttpError }` and `import std/http` + `http.HttpError` are both legal and they
disagreed: under the named spelling a bogus field produced no Glyph diagnostic at
all (only a `tsc` TS2339), and a `match` covering all three of `kind`'s literals
was `E0218 non-exhaustive`, whose advice is to add a catch-all, which is advice
to switch the check off. `stdlib_type_path` keys the field tables on a
two-segment path and a named import has one, so the D30 string-literal union that
makes the match exhaustive was never found.

This is the class CLAUDE.md records as already settled twice, in 0.1.56 and
0.1.57, and calls wrong on arrival, and `lower.rs` states the rule in a doc
comment three functions above the bug. The `ImportNamed` arm now consults
`stdlib_modeled_type` before falling through, so both spellings lower to the same
`Ty`.

**Found and not fixed, each recorded with a reproduction.** **G110**, the cause
behind G109: a generic record's `parse` returns a payload the checker cannot see
into, so a field typo on it is a `tsc` error mapped to the whole function where
the non-generic path gives E0210 at the field. Typing it means modelling the
per-parameter checker arity the emitter already writes. **G112**, the widest
interop gap yet: Glyph has no default-import form, so a CommonJS `export =`
*callable* package cannot be called at all, which is express, lodash, debug,
chalk@4, commander and most of the pre-ESM registry; a *named* export through the
same namespace works, so the gap is exactly the default binding, and closing it
is a D15 decision. **G113**: `Intl` is a host global with no route, so CLDR
plural data is unreachable, though `value.toLocaleString(...)` and
`a.localeCompare(...)` pass through and are type-checked today, which is
undocumented.

Four apps landed and the whole examples tree builds: `sitegen` (marked +
gray-matter, the first app in the tree on real npm dependencies), `resilient`
(retry, backoff, circuit breaker, concurrency limiting, 51 examples), `collections`
(generic `Heap`/`Cache`/`Trie` plus a fallible pipeline, 23 examples), and `i18n`
(CLDR plurals, fallback chains, locale formatting, 25 examples).

##### The three round 31 left open

Round 31 recorded three findings it deliberately did not fix. All
three are closed here, and one of them changes the spec.

**G112: no default-import form (D15 now names four).** A CommonJS package whose
export *is* a function had nothing Glyph could import. All three existing forms
emit a named or namespace import, which `tsc` answers with TS2595 or leaves
uncallable, so express, lodash, debug, chalk@4, commander and `gray-matter`'s own
`matter(text)` were unreachable. The new form is
`import express { default as app }`.

`as` is legal **only** after `default`, never for an arbitrary imported name, so
this does not open general renaming: a name in the file still matches the name at
its source, and `grep 'default as'` finds every default import in a tree.
Binding the default through the *aliased* form was the tempting alternative and
was rejected, because it gives one spelling two meanings depending on the
package's module format, which is precisely the class G111 was fixed to remove
one release earlier. Verified end to end against gray-matter, whose documented
entry point now runs.

**G110: a generic record's `parse` had no type.** The member still has none, and
that part of the original reasoning was right: a generic descriptor takes one
runtime checker per type parameter, so `T.parse` has no single signature to give.
The instantiation is read from the **call**, where the explicit type arguments
are, so `Wire.parse<number>(raw)` types as `Result<Wire<number>, Array<Issue>>`
and a field typo is `[E0210]` at the field instead of a `tsc` TS2339 pointed at
the whole enclosing function. Explicit arguments are required: `parse` takes an
`unknown`, so there is nothing to infer them from, and a guessed instantiation
would put a wrong shape behind a boundary check, which is worse than staying
opaque.

That also closes the previous release's miscompile at its source rather than at
the fallback. With the shape known, the loop emits `.entries()` directly and
never reaches the run-time helper. The helper stays for iterands that are still
genuinely unknown, and the test for it was rewritten to use one, since the case
it originally used is typed now.

**G113: `Intl` had no route, so CLDR plural data was unreachable.** Closed the
way this repo already documents for a host global the stdlib does not wrap:
`std/intl` wraps it, as timers and WebSocket were. Twelve functions across
plurals, ordinals, numbers, fixed decimals, currency, percent, lists, relative
time, dates, collation and locale negotiation.

The wrapping earns its keep instead of forwarding: `plural_category` returns the
**string-literal union** of the six CLDR categories, so a match over it is
exhaustive with no catch-all and a missing one is named
(`[E0200] ... missing variants "zero"`). Handed back as a bare `string` it would
have been E0218, whose advice is to add an `else`, and an `else` over a plural
category is how a locale's `few` silently renders as `other`. An app branching on
`n == 1` is wrong in most of the world; the host has the ~200 correct tables and
now Glyph can reach them. Verified against real data: Polish 1/3/5 select
one/few/many.

Two gates caught mistakes on the way in, which is them working: a new emitter
global routed as a shadowable JS one, and a new stdlib module whose exports were
not yet in `docs/reference/stdlib.md`.

**Still open from round 31.** G108 grows rather than shrinks: a package whose
declarations reference `Intl.*` cannot be materialized by `gen dts` either, so
the list of things the reader cannot follow is now classes, TypeScript utility
types, and host globals. The direct-import path needs no generation and is
unaffected, so this bites only boundary validation.

#### 0.1.75 — Shipped · The emitted imports survive having their types stripped

**Published 2026-08-14.** Round 32: the outside app came back with 3,377 lines of Glyph, a Web
Worker AI, 198 passing examples, and a **487-line build tool** it had to write to
get that output into a browser. Every step in that file is something the compiler
did not do. Three of them are ours to fix.

**G114 is the one that is a defect rather than a gap.** The emitter puts
type-only names in a value import list, so `import { Option, Some, None } from
"std/option"` reaches a runtime where `Option` has no binding. `tsc` elides such
names, which is why every build is green; a type-stripper does not, and the
result is a hard ESM link error. It is also `[TS1484]` under
`verbatimModuleSyntax`, against our own `runtime/std/*.ts` as well as emitted
code. Emitting `import type` for a name the source module exports as a type is
the fix. **Done**, and it took two passes: the standard library's 25 type-only
names across 16 modules are tabled and reconciled against the runtime by a gate
that fails in both directions, and a Glyph **plain alias** (`type Board =
Array<Cell>`, which emits no descriptor `const` where a record or union does) was
missed until the fix was re-tested against the application instead of the reduced
case. The runtime's own sources carried the same defect and are fixed, so
`verbatimModuleSyntax` is clean over the whole emitted tree.

**G115 is not in this release.** Pruning the standard library out of an output
and emitting relative-specifier ESM is a `--target browser`, which is a feature
rather than a fix, and it does not belong bolted onto one.

**G115 is the deployability half.** A program importing five std modules gets 31
in its output, `sqlite` and `http` and `fs` among them, which a browser worker
must not carry; and the runtime lands in `.glyph-runtime`, a path component most
static hosts hide. Tree-shaking and a rename answer both **if** a bundler is in
the pipeline. The app deliberately took the no-npm-dependency path, so it wrote
the graph walk and the rename itself. A `--target browser` emitting pruned,
relative-specifier ESM is what removes that file.

#### 0.1.76 — Shipped · Tell the agent what the answer is

**Published 2026-08-14.** From reading the outside author's session log (round 33), which is the
first record of what writing Glyph feels like in real time rather than what the
result looks like. Eleven diagnostics across 3,377 lines is the headline; these
two are what is left.

**G116: `E0105` withholds the answer it is holding.** Eight of the session's
eleven diagnostics are one agent hunting a single function in `std/random` —
`int`, `next`, `float`, `number`, `range`, `int_range`, `between`, `shuffle` —
where the module exports exactly one name, `seeded`. Every message is correct and
none is actionable: "check the spelling" cannot be acted on without the list, and
the resolver has the list, because producing the error is what proves it. Naming
the exports, or the nearest matches, turns eight build cycles into one. For a
human this is an annoyance solved by opening the docs; for an agent, which is who
this language is for, each guess costs a whole build.

**G117: the most-recommended loop idiom is the slowest one.** Measured over a
scanning shape: `for c in cells` 40 ms, `array.filter` with a closure 72 ms,
`for i in array.range(n)` with `cells[i]` **168 ms**. The session's own benchmark
recommended the third. `array.range(n)` allocates an n-element array per call and
indexing goes through a bounds-checking helper, so the index loop pays both.
Lowering `for x in array.range(a)` to a counting `for` removes the allocation and
makes the idiom what every reader already assumes it is. **Done**, both: the direct
`array.range` / `range_from` call lowers to a counting `for` with its bounds
hoisted (a range bound to a `let` stays an array, since something else may hold
it), and 168 ms becomes 61. `performance.md` gains the table it did not carry.
Direct iteration stays fastest at 33 ms, because indexing still costs the bounds
check that turns an off-the-end read into an error, and the guide now says which
to reach for instead of leaving it to a benchmark harness.

#### 0.1.77 — Shipped · A mutation that loses an update

**Published 2026-08-14.** All four language items below were reproduced on 2026-08-09 and
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

**Shipped as D43 / E0225, and the narrowing was found by running it.** The first
draft fired on any place read before an `await` and written after, which flagged
`mut failures = failures + ...` and `mut rounds = rounds + 1` in
`examples/apps/jobq` — ordinary local accumulators in an async function that
nothing else can reach. The rule now requires the write to go *through a
parameter* and to touch a field, since a parameter is the only thing a caller can
also have handed to another task, and rebinding a parameter whole changes this
function's copy rather than the caller's record. With that, the whole examples
tree (145 modules) is clean and the failing case still fails. Six tests pin both
directions, including the two false positives verbatim.

The pass needed a traversal, and rather than hand-write a fourth copy of the
19-variant `Expr` match (`owned.rs` has one, and the file itself notes a third
copy of a similar walk as a cleanup worth doing), it went into `glyph-ast` as a
shared `visit` module with no wildcard arm, so a new AST variant forces a
decision in one place instead of being silently missed by one pass.

#### 0.1.78 — Shipped · Bytes

`std/bytes`, and the three boundaries that had no way to carry octets. Closes
**G102**, which two apps in one round stopped dead on: a PNG reader that could
not read a file whose first byte is `0x89`, and an RFC 6238 authenticator that
could not form either argument to an HMAC. Both are now written as compiler
tests.

`Bytes` is an immutable sequence of octets, a `Uint8Array` at run time so it
hands to a host API with no unwrapping. Twenty-one functions: the sequence
operations named after their peers in `std/array` and `std/string` (`len`, `get`,
`slice`, `concat`, `join`, `equals`, `index_of`, `starts_with`), the UTF-8 bridge,
and hex, base64, base64url and base32 codecs. `std/fs` gained
`read_bytes`/`write_bytes`/`append_bytes`; `std/crypto` gained a `_bytes` form of
every digest and HMAC, SHA-1 in both forms, `hmac_sha512`, `random_bytes` and
`timing_safe_equal`.

**The design call that cost the most and mattered the most: every decode returns
a `Result` that names the position.** node's `Buffer` is silent on malformed
input. `Buffer.from("zz", "hex")` returns an empty buffer and reports success;
base64 decoding skips any character outside the alphabet, so a base64url string
decodes under the standard alphabet to quietly wrong bytes; `toString("utf8")`
substitutes U+FFFD for a malformed sequence and reports success, which turns a
truncated read into plausible-looking text. Delegating to `Buffer` would have
been four lines per codec. Each is written out instead, refuses all three, and
reports `index`; `to_text` scans on the error path so the answer is "not valid
UTF-8 at 2" rather than "not valid UTF-8". `from_array` rejects anything outside
0..255 for the same reason: a silent `& 0xff` turns 256 into 0.

Writing the codecs rather than delegating bought a second thing that was not the
motive: the module touches no host API at all, so a bundle that reaches only for
`std/bytes` runs in a Web Worker. That is the property round 27's author wanted
and assumed he did not have.

**base32 was not in the plan and is in the release.** Round 28 proved it writable
in ordinary Glyph, so it was left out on that basis. Then the `std/crypto`
documentation's TOTP example opened with `bytes.from_base32(secret)`, because
`otpauth://` URIs carry the shared key in base32 and every authenticator starts
by decoding one. The example could not be written, which is what showed the
surface was incomplete.

**A note on what a published vector proves.** Deliberately breaking
`hmac_sha1_bytes` to route its key through a string left the RFC 6238 assertion
passing, because that vector's secret is ASCII and survives the trip unchanged.
The test now also pins an HMAC over a key containing `0xff`, where the string
route gives `4ab779f0…` instead of `c543ef42…`. A vector is evidence only for
what its inputs exercise.

Still open and deliberately not in this release: hex literals (`0xff` is still
`[E0002]`), which makes a CRC32 table written in decimal unreadable, and integer
codecs (`u32_be` and friends), which are ordinary arithmetic over `bytes.get`
and which round 28 verified against published vectors in plain Glyph.

#### 0.1.79 — Shipped · The host boundary

Two halves of one theme: give the stdlib the host calls an app currently makes
raw, and then deliberately step outside the stdlib to find what is still
missing.

**`std/net`, `std/dns`, `std/tls`, `std/url`.** The driver is
concrete: `chat/daemon.glyph` imports node's `net` directly, the only raw host
call in the entire examples tree.

**`std/net` has landed and the daemon is ported.** Events are individual
functions, as in `std/websocket`, and `serve` is async and resolves when the
server *stops*, as in `std/http`, so a port already in use is an `Err` the caller
matches rather than a throw at a handler that may not exist. The daemon's own
listener-error branch is now that match.

The design call worth recording is `on_text` against `on_data`. TCP is a stream
of octets with no message boundaries, so a multi-byte character can be split
across two packets, and decoding each chunk on its own turns one `e`-acute into
two replacement characters. It is a bug that appears only under load or with
non-ASCII input, which is the worst pair to debug. `on_text` holds a
`StringDecoder` per socket and emits whole characters only; `on_data` gives the
octets untouched. An integration test sends `0xC3` at the end of one write and
`0xA9` at the start of the next and asserts the server saw the character whole;
reverting `on_text` to a per-chunk decode makes it fail with a replacement
character in both reads.

**Two things this did not close, recorded so neither is assumed.**

The E0304 framing above was wrong and is corrected here. E0304 is about `parse`
/`is` on a record holding a field with no runtime check, and the daemon's `Conn`
record still holds a `socket`, which is still an opaque host handle. Wrapping the
import in a stdlib module does not give the handle a runtime check and was never
going to. What the port actually buys is typed events, callbacks that need no
narrowing, correct UTF-8 across packet boundaries, and a bind failure as a value.

And **a server cannot be stopped once started**, in `std/net` or in `std/http`.
Both `serve` functions resolve `Ok(void)` on close, and neither module exposes
anything that closes one, so the `Ok` branch is unreachable unless the peer
process does it. That makes graceful shutdown unwritable today, which sharpens
the "signals and graceful shutdown" item already parked below: it needs a server
handle, and the shape of that handle is the open question, since `serve`'s
awaitable return is what makes the bind error a value in the first place.

**Already landed on main and shipping with this release:** the `std/bytes`
codec rewrite. Benchmarking 0.1.78 after the fact showed the hand-written
codecs were 40x to 100x off `Buffer` on a megabyte, from growing a string a few
characters at a time and from an `indexOf` scan per input character rather than
from the algorithm or the validation. They now build into a typed array and
convert once, and read through a precomputed reverse table: `to_hex` 160 ms to
5.3, `to_base64` 135 to 9.7, `to_base32` 170 to 9.4, `from_base64` 40 to 11.
A delegating fast path was considered and rejected, because it would have made
a guarantee depend on which host the code ran under. `scripts/check_bytes_codecs.mjs`
runs 168k differential checks against `Buffer` in CI. Not worth its own release
on the evidence that no application has hit the slow path, so it ships here.

**`std/bytes` was in that list, was pulled out of it, and shipped first as
0.1.78.** Round 28 showed it was not a peer of the other four: they each wrap one
host boundary, while that one was a missing *type*, and without it binary formats
and real cryptography were both unwritable whatever else shipped. The entry above
records what landed.

**The dogfood round ran, and the interop gate passes.**
`examples/apps/feeds` reads an RSS feed with `fast-xml-parser`, an ordinary
typed npm dependency: imported by name, constructed with `new` (D37), returning
an `any` that `Document.parse` turns into a checked value, with no adapter, no
hand-written `.d.ts` and no `extern_ts`. It is the first application in the tree
to use a real npm package, so the 1.0 interop gate has an app behind it rather
than only a guide.

**Five of this round's stated blockers were already gone**, which is worth
recording because a session nearly implemented all of them. The apps gate
already skipped `node_modules` (`check_apps_are_glyph.py`), `.gitignore` already
covered `examples/apps/*/node_modules/`, CI already installed dependencies for
any app whose `package.json` declares them, and both `gen dts` prerequisites,
G103 and G104, were already fixed. The "owner's call" this plan said gated the
round had been made and shipped. Re-check before implementing.

**Two findings, both recorded rather than worked around.** A client cannot say
"this response body is text": `Response.body` is an `unknown`, the client keeps
the raw string when the body is not JSON, and `string.from` is the identity on
it, but the same line renders `[object Object]` for a JSON body and reports
nothing. `http.raw` is the server-side counterpart; the missing piece is
`http.text(response)` (G118). And `url.join`'s `Err` branch is nearly
unreachable, because against a valid base anything that is not a URL is treated
as a relative path, so only an invalid base fails (G119).

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

#### 0.1.80 — Shipped · A server is a resource

**A Linus review of the server-lifetime design ran before this shipped, and
changed most of it.** The verdict was SHIP WITH CHANGES, nine of them, and four
claims were verified against the compiler and node before any were acted on. The
review is at `feedbacks/linus/04-server-lifetime-and-std-net.md` (gitignored).

The one that mattered most was not the thing being reviewed. **A socket the
server handed you, with a `data` listener and no `error` listener, ended the
process on a peer reset** (`UNCAUGHT: ECONNRESET`, reproduced). The example in
our own reference was exactly that shape, so the documented way to write a
server was a remote kill switch. `listen` now attaches a default error handler
to every accepted connection before the caller's handler sees it.

Three more, all reproduced first: `stop` called twice fired `on_stop` twice
(every other teardown in the stdlib promises the opposite); `on_close(Socket |
Server)` is not writable in Glyph at all, because a bare path in union position
becomes a *variant*, so `type Target = Socket | Server` silently declares a
two-variant tagged union; and the docs drift guard was already red.

**`serve` is deleted rather than kept as a convenience.** Its `Ok` branch could
never run, and because Glyph has no `if` and a `match` on a `Result` must be
exhaustive, every caller was *forced* to write an arm that could not execute:
`daemon.glyph` printed a line that would never print, `jobq` returned an exit
code that could not happen. Six callers ported, two more than the review found.
`std/http` moved in the same change rather than a release later, because it had
the worse version of the bug (a post-bind failure resolved `Err` while the
server was still listening and still answering) and because `resilient/main.glyph`
was already working around the missing bind signal with `let _ = http.serve(...)`
followed by a 150ms sleep.

The bind error is structured (`kind` is `in_use`/`denied`/`unavailable`/`other`)
rather than a string to scrape, `port(server)` exists so `listen(host, 0, ...)`
is usable, and `host` has no default: a standard library that will not ship a
switch for turning off certificate checking should not quietly bind every
interface either.

**One piece of taste debt paid off on the way.** Five separate gates were each
scanning `runtime/std/*.ts` for exports with their own prefix lists, and they
have to agree about what counts as surface. They now share
`glyph_cli::runtime::exported_items`, which also honours an `@internal` marker
for the one export `std/http` borrows from `std/net`. A sixth scanner lives in
another crate that cannot reach the helper, and says so where its exclusion is
spelled.

**Still open from the review, recorded rather than fixed here:** `http.read_request`
has no body size cap and never settles if a client disconnects mid-body, which
is a memory leak with no log line (G120); and network `Bytes` are zero-copy views
onto node's pooled 8 KiB buffers, so retaining a small frame per connection pins
8 KiB each (G121).

WebSocket binary messages, a WebSocket server, connection options and
subprotocols, and WebSocket integration tests all shipped. **`std/sse` did
not, and moves to 0.1.81.** It is the least specified of the five: Node 26
has no `EventSource` global, so there is nothing to wrap the way
`std/websocket` wraps one, and the runtime has no streaming primitive, so
the module has to carry its own `fetch` plus reader. That is a design worth
doing rather than rushing into a cut, and holding the release for it would
have left an unauthenticated remote process kill in the published package.

*Reviewed against 0.1.79.* All four claims re-checked against the built
compiler: `websocket.ts:18` still says binary frames are decoded as UTF-8 text,
there is no server export, there is no `std/sse`, and the integration suite
mentions WebSocket zero times.

**Was blocked in part, and is not any more.** "WebSocket binary messages" needs a
byte representation, and there was none: `runtime/std/websocket.ts` says so in
its own header ("Binary frames are decoded as UTF-8 text ... a program that needs
the bytes is not served by this module yet"). 0.1.78 shipped `std/bytes`, so the
work is now `on_binary` alongside `on_message` and a `send_bytes`, over a type
that exists. The header comment has to come out in the same change.

**Two things `std/net` taught that apply here, one of them a trap.** The trap is
that WebSocket is *not* TCP: frames carry their own boundaries, so a message
arrives whole and `on_message` needs none of the per-socket `StringDecoder` that
`net.on_text` holds. Copying that machinery across would be cargo-culting a fix
for a problem this protocol does not have. What does carry over is the split
itself: separate handlers per payload kind, rather than one handler and a
narrowing.

And a WebSocket **server** will meet the problem `std/net` and `std/http` already
have, that a server cannot be stopped once started, so it should not ship before
that shape is decided or it adds a third copy of the same hole.

#### 0.1.81 — Four gaps, and the streaming primitive underneath two of them

G99, G115, G105 and G108, taken together after an adversarial review of all four
(`feedbacks/linus/05-four-gaps-g99-g105-g108-g115.md`, gitignored). Six of its
claims were verified against the code before any were acted on, and two of them
dissolved premises this roadmap had been carrying for releases.

**G99 was never a design question, and a lying comment is why it looked like
one.** The thunk `async fn() -> T` already ships (`resilient/main.glyph:43`) and
all of `std/task` is typed for it, so D40 answered "what is the type of a value
that is not here yet" when it shipped. The blocker was `par.all`, a prelude
global typed `Array<T | Promise<T>>`, which is the one shape D40 refuses to name,
used in exactly one example and two compiler tests. Meanwhile
`assign.rs:1680` carries twenty lines of doc comment describing `map`/`flat_map`/
`zip` as modeled **and describing this very bug in the past tense** — none of the
three is implemented. Anyone who looked read the comment and stopped looking.
That is the mechanism by which a gap survives eight releases, and it is worth
more attention than the gap.

Land the three signatures the comment already specifies, delete `par` rather than
deprecate it, and move `all_ok` to `std/result` where `Array<Result<T, E>> ->
Result<Array<T>, E>` belongs. Breaking, and taken deliberately: doing the right
thing for the people writing Glyph outranks the cost of the change.

**G105's naming constraint was fake.** `std/stream` has **zero** `.glyph`
importers anywhere in the tree; it is a 22-line property-testing sampler whose
only consumer is `std/test`. Three reproductions treated "the obvious name is
taken" as a constraint on the design. It is a squatter, and it moves to
`std/sample`.

The primitive is a handle over the `io.read_line` pattern, whose state is a
module-level singleton today. Two decisions on top of the review's shape:

- **`next` answers a three-variant union, not an `Option`.** The review proposed
  `async next(s) -> Option<T>` on the `read_line` precedent, and that precedent
  has a flaw the review did not mention: `io.ts:88` treats *every* read failure
  as EOF, so a closed descriptor is indistinguishable from clean end of input.
  Defensible for stdin, documented there, and wrong for a file: a k-way merge
  would silently emit a short result on a disk error. `StreamItem<T>` is
  `Item(T) | End | Failed(e)`, so D9 forces the failure arm to be handled and a
  failure cannot be read as success. That is the whole first pillar, on the one
  API where the alternative is a silent wrong answer.
- **No concession on verifiability, so `Stream<T>` is a real resource.** The
  review's honest cost was that this would be the first stdlib type where two
  reads give different answers, checked by nothing but documentation. That is
  not acceptable. D25 exists for exactly this and the case it was written for is
  a handle that must be closed exactly once. Two pieces of machinery are missing
  and are part of this release: `stdlib_types.rs` has no way to mark a stdlib
  type `resource`, and `owned.rs:24` supports **free-function consumers only**,
  so `stream.close(s)` does not currently count as a consume. Both get built.
  This also retires G16's complaint that D25 is under-exercised.

**G115 is two defects, and `--target browser` is neither of them.** The emitter
materializes a runtime graph it never prunes (36 modules for a program importing
three), and it emits **bare** `std/*` specifiers resolvable only through the
generated tsconfig's `paths`, so pruning alone still leaves output no browser can
load. Prune always, emit relative specifiers always, rename `.glyph-runtime` to
`glyph` (the dot hides it from static hosts and buys nothing; the *name* is what
avoids the collision). No flag: the moment `--target browser` exists, `node`,
`deno` and `workers` follow and the stdlib grows a matrix. A diagnostic naming
the host module a program reached is useful on every target instead.

**This one does not count as closed until a real no-bundler browser load runs.**
`moduleResolution: nodenext` requires `.js` in a relative specifier while the
emitted files are `.ts`, so the specifier form has to be proven rather than
reasoned about. That check is the release gate for G115, not the prune.

**G108 is cut to its honest half.** `check_ref_integrity` (`gen.rs:766`) already
detects every dangling `$ref`, writes the file anyway and exits 0, with a note
saying `glyph build` will report it later. Make it fail and name the three
groups. Then classes as opaque types, which is five of the eight names and has a
clean answer under D37 `new`. Utility types and host types are deferred: the
first means evaluating TypeScript's type system, the second has no honest Glyph
spelling, and one package is asking.

Order: G99, then G115, then G105, then G108. The review ranked G115 first on the
grounds that it is decaying and mechanical; it is decaying, but it carries two
breaking layout changes and an unproven specifier form, so "mechanical" is too
confident. G99 goes first because it is small, it is a wrong answer in the wild,
and it deletes the comment that hid it.

*Reviewed against 0.1.80.*

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

**Also in 0.1.72.** The callback parameters of `array.filter`, `find`, `any`,
`sort`, and `fold` are modeled, so passing an `async fn` where a predicate goes
is `E0211` pointing at the callback instead of a TS2322 about
`Promise<boolean>` pointing at the whole statement.

`map`, `flat_map`, and `zip` were written the same way and then parked, because
the model that closes them also rejects `par.all(array.map(items, async fn ...))`,
which is Glyph's own concurrency idiom and has an integration test covering it.
That idiom passes an `Array<Promise<T>>`, and Glyph has no spelling for a
pending value: `await e` types as `e` and D40 leaves `Promise<T>` deliberately
invisible. The consequence is recorded as **G99**: with those three unmodeled,
`array.map(xs, some_async_fn)` compiles, passes `tsc --strict`, and prints
`[object Promise]`. Closing it is a decision about whether a pending value gets
a type, not a table entry, so it is the owner's call and not scheduled here.

## glyph-hello — the first application written outside the project

Read on 2026-08-13, against 0.1.72. `github.com/canpolatoral/glyph-hello` is a
tic-tac-toe game by an author with no connection to this project. Every dogfood
round in this file so far was ours, which means every round's findings were found
by someone who already knew where the walls are. This one was not, and that is
what makes it worth its own section.

The shape is the shape we have been arguing for: the entire engine in Glyph
(rules, minimax, position evaluation, board rendering), an HTTP server and a
terminal client in Glyph on top of that one engine, and the DOM in 301 lines of
hand-written vanilla JS. 1,485 lines, one commit, and **no TypeScript in the
repository at all**. `src/.types/` contains the scaffold README and nothing else,
so the rule that an app needing a `.d.ts` proves Glyph unnecessary held without
anyone enforcing it.

It builds green, and green was not the check. Bundling the engine and searching
it exhaustively: the `Perfect` level as O against every legal line is 593
terminal positions and zero losses, as X it is 94 and zero, and across all 5,478
reachable positions none of the three difficulty levels ever returned an illegal
move. An outside author got a provably correct game engine out of this language
on the first commit. That is the strongest evidence the project has that the
thing works for somebody who is not us.

Three findings, all recorded in `docs/dogfooding-gaps.md` under Round 27.

- **G100: `std/array` has no `max`, `min`, `max_by`, `min_by` or `sum`.** The app
  hand-writes the same fold five times, because argmax over an array is what
  every search does and we make people spell it out in four lines of `match acc`.
  `max_by`/`min_by` taking a key function is the shape that closes it. Small, and
  the clearest single ergonomic win an outside reader has pointed at.
- **G101: `array.fold` cannot stop early.** Their spec asks for alpha-beta
  pruning. Alpha-beta *is* writable today, verified: mutually recursive functions
  threading an index compile, pass `tsc --strict`, and return the right answer.
  But it takes four functions and an explicit cursor to say what a `fold_while`
  says in one call, and the version that reads naturally evaluates every branch,
  which in a search is the difference between pruning and not.
- **A deployment-guide gap, no code involved.** Their spec records as settled
  that "the emitted code uses bare `std/*` specifiers that a build step must
  rewrite". It is not true: `dist/tsconfig.json` carries the `paths` map, and
  `esbuild dist/xox.ts --bundle --format=esm` yields 20.8 kb of ESM with no
  `node:` imports and no `process` references, which runs in a bare realm. An
  engine in a Web Worker works today. `docs/guide/deployment.md` only says a
  front-end build "via React interop" bundles like any other TypeScript, so an
  outside developer read the emitted imports, concluded the pessimistic thing,
  and wrote it into his requirements as a decision. A worked browser and worker
  example naming the tsconfig and the esbuild line is the fix.

One measurement to carry forward without acting on it: 31 of the app's 70 `match`
expressions are `match <bool> { true => ..., false => ... }`, 44%. First number
we have for what D9 costs at the keyboard from someone who did not choose the
restriction. The pillar case for a single branching construct is unchanged; the
number is worth knowing when the next person raises it.

Nothing here carries the **Next** marker. G100 and G101 are stdlib additions
small enough to ride along with whichever release is open when they get picked
up; the guide fix is documentation and needs no release at all.

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

## glyph-kanban — the second outside application

Read on 2026-08-23, against 0.1.80. `github.com/yildizadem/glyph-kanban` is a
React Kanban board by a second outside author. Where glyph-hello was Glyph all
the way down, this one is the hybrid the docs have never written down: four
Glyph modules (models, an RBAC permission matrix, analytics formulas, status
transitions; 292 lines) compiled into `src/generated/` and imported by
hand-written React/TSX under Vite. The author published three evaluation docs
alongside the app, and the verdict lands on the pillars unprompted: exhaustive
`match` forcing every new role and status to be handled everywhere before it
compiles, `grep mut` as a complete mutation audit, no casts for an agent to
reach for, and a closing recommendation to use Glyph for "mission-critical
business rules edited autonomously by AI agents" inside an otherwise-TypeScript
app. That is the manifesto's wedge, rediscovered independently by someone who
found the walls himself.

The caveat that frames every negative finding: they pinned `@glyphlang/glyph`
0.1.3 and never upgraded, so the retrospective they published on 2026-08-22
evaluates a compiler 62 releases old. Four of its five findings were re-verified
and are already closed (the binary execute bit, the `--no-test` flag,
`std/math`/`std/time`/`std/decimal`, and strict-mode narrowing: their modules
rebuilt with 0.1.80 pass a host `tsc --strict` clean). What survives is the seam
their whole architecture stands on, recorded as Round 34 in
`docs/dogfooding-gaps.md`:

- **G122.** Generated output cannot be dropped into a host TypeScript project
  without hand-wiring `std/*` aliases in two places (host tsconfig `paths` and
  `vite.config.ts` `resolve.alias`), and no guide documents the wiring. Vite
  does not read tsconfig `paths`, so the generated tsconfig that answers the
  esbuild case does not cover the most common React toolchain. Reproduced both
  ways: 25+ host `tsc` errors without the wiring, clean with it.
- **G123.** No watch mode. The hybrid dev loop is `glyph build` by hand per
  domain change while the UI half of the same app gets HMR on save.

Both are scheduled into the React track below as the Vite embedding seam: a
serious React language meets its users inside a Vite project, and today that
embedding must be reverse-engineered from the emitted imports. The design fork
(relative specifiers, a documented recipe, an init template, a Vite plugin) is
recorded in the entry and stays a decision for that track.

Two carries. 15 of the app's 29 `match` expressions are boolean two-arm, 52%,
the second number for D9's keyboard cost beside glyph-hello's 44%. And the
staleness is a finding of its own: nothing tells a user a pinned version has
fallen 62 releases behind, so a public evaluation shipped describing gaps that
had been closed for weeks. An update notice is a network call from a compiler
and therefore a policy question; it is deliberately not decided here, only
named, in the rolling lane.

Also in this release, from watching 0.1.82 publish: the release smoke test
retried on npm's exit code, and npm drops an `optionalDependencies` entry it
cannot fetch while still exiting 0. Seconds after a publish the launcher
installed alone and the install reported success. Windows logged "added 1
package" where macOS logged "added 2", and the only thing that noticed was the
launcher failing to resolve its binary. The retry now keys on the platform
package being on disk.

### 0.1.82 — Shipped · The download runs, and the package carries its license

Published 2026-08-24.

Three defects in what we hand people, none of them in the compiler. Every
GitHub Release archive since the feature shipped carried `glyph` at mode 0644,
confirmed against v0.1.10, v0.1.80 and v0.1.81. Anyone following the
instructions printed on the release got `permission denied`. GitHub's artifact
upload strips the Unix mode and only the npm half of the pipeline restored it,
so the npm packages were always right, which is why the release smoke test never
saw it: that test only exercises npm.

The fix puts the mode inside a tar in the build job, where the round trip cannot
touch it, and asserts it again on the extracted contents of the finished archive
rather than repairing it. All six npm packages also now ship the license text
they had been declaring and omitting, and the launcher's reinstall hint names
`@glyphlang/glyph` instead of `glyph`, which is an unrelated static site
generator on npm.

The release workflow itself was rebuilt around the one fact that shapes it: npm
versions are immutable. Everything checkable now happens before the first
irreversible step. The tag must match every version string, the commit must be
on `main` and have passed CI, each binary is executed on its own platform and
asked for its version, and all six packages are dry-run before any is published.
A version that already exists stops the release instead of half-completing it,
and `skip_npm` gives a half-failed publish a way to still produce the archives.

Known and deliberately left: the x86-64 macOS binary is executed by nothing. The
runner is arm64 and running it needs Rosetta, which the image does not
guarantee. Its mode is checked; nothing runs it.

### 0.1.81 — Shipped · Generated output drops into a host project as-is

Published 2026-08-23 and smoke-tested from a clean npx cache in an isolated
HOME: `--version`, the execute bit, `glyph init`, `npm install`, `glyph run`,
and the headline feature itself (the published binary emits no bare `std/*`
specifier).

G122, the alias half of the Vite embedding seam. Every `std/*` specifier in
emitted code is now **relative** to the bundled runtime
(`./.glyph-runtime/std/result`, with the same parent-hop rule the bootstrap
import has always used), in all four places the emitter writes one: a written
`import std/io`, the injected `?` machinery, the injected schema factory
import, and the auto-imported prelude constructors. The runtime's own
`schema.ts` import of `std/result` went relative with them, and
`glyph-bootstrap.ts` now carries a `/// <reference>` to `glyph-prelude.d.ts`,
so the ambient `Issue`/`Schema` types travel wherever the emitted code is
compiled; `schema.ts` and `json.ts`, which use those types, carry the same
reference for the case where a host includes them without an emitted module.

The fork recorded in Round 34 was decided for relative specifiers over a
documented alias recipe or a Vite plugin, on the bootstrap's own precedent: the
bootstrap import went relative when a Vite build failed to load it, and the
diff-stability cost is not new in kind, because the bootstrap line already
varied with module depth. A relative path is the one specifier every host
toolchain resolves the same way with no configuration; the alias recipe would
have documented friction instead of removing it, and a plugin is a second
artifact to install and version for what one emitter rule fixes outright. The
generated tsconfig keeps its `paths` map so a hand-written `extern/*.ts` that
imports `std/*` bare still compiles, and old host projects wired with aliases
keep working (the alias simply goes unused).

Verified against the kanban app's own modules with **zero host config**: a
stock-Vite-shaped `tsc --strict` passes (25+ errors before), a real
`vite build` bundles the generated output untouched, and `tsx` executes it
directly with correct runtime behavior. The conformance corpus re-pinned with
only specifier lines changing, which is the diff the corpus exists to make a
human sign off on. The deployment guide gained the hybrid-embedding section
(layout, the `pub` requirement, the `@types/node` caveat for Node-flavored
std modules), and the interop guide stopped claiming `std/*` is
tsconfig-mapped.

Still on the seam after this: G123 (no watch mode; the rebuild half of a
future Vite plugin) and G124 (a no-`pub` library module builds green and
exports nothing; the candidate fix is a build-time diagnostic naming `pub`,
parked in the rolling lane).

Pillar: verifiability at the boundary. The emitted code was only compilable
inside the compiler's own tsconfig; now the guarantee travels with the files.

### 0.1.83 — Shipped · The runtime compiles against `@types/node`

Published 2026-08-24 and smoke-tested from a clean npx cache in an isolated
HOME: `--version`, the execute bit, `glyph init`, `npm install`, `glyph run`,
and the headline feature itself.

G125, found straight after 0.1.81 by an app that followed the external-imports
guide. Installing `@types/node`, which that guide tells you to install for the
full node builtin surface, failed every build inside the compiler's own runtime,
with no user code involved. A bare `pub fn main` produced four errors against
`@types/node` 26.2.0: three in `std/net.ts` and one in `std/process.ts`.

One cause behind both. The bundled shim declared two node APIs more narrowly
than node has them, and runtime code was written against the narrow types.
`process.exitCode` was `number | undefined` where node accepts a numeric string
and coerces it, so `exit_code()`'s `?? 0` widened to `string | number`. A
socket's `data` chunk was a buffer where node delivers text on a socket someone
has called `setEncoding` on, so `on_data`'s `chunk.buffer` stopped resolving.

Fixed in both places each time rather than only where tsc pointed: the shim now
declares each field the way `@types/node` does, `exit_code()` reads through
`Number`, and `on_data` encodes a text chunk back to UTF-8 rather than dropping
it.

The first attempt shipped a guard that built a stand-in `@types/node` by copying
the shim. That is a check that the shim agrees with itself, and it passed
against the very release of the package that was failing, which is how the
`net.ts` half stayed open. The guard is now
`scripts/check_runtime_against_types_node.py`: it installs the real package at
`latest`, builds a bare `main`, and fails if the bundled shim was written at all
(a green result would otherwise prove nothing). It runs in CI beside the
`std/bytes` codec differential check, and needs the network for the same reason
that one does.

What this does not do is compare the whole shim against `@types/node`
declaration by declaration. It catches a divergence that breaks a build, which
is every divergence the runtime actually depends on today, and it will catch the
next one on the release of `@types/node` that introduces it rather than on the
app that runs into it. A full conformance check between the shim and the real
typings stays in the rolling lane.

Nothing here carries the **Next** marker.

### 0.1.84 — Shipped · A subprocess you can watch while it runs

Published 2026-08-24 and smoke-tested from a clean npx cache in an isolated
HOME: `--version`, the execute bit, `glyph init`, `npm install`, `glyph run`,
and the headline feature itself.

G126, from the same app that found G125 and for the same reason: the bundled
node shim had `spawnSync` and `execFileSync` and not the async `spawn`. Both of
the two it had block until the child exits and return the whole output at once,
so a dev-loop tool that reports a long-running command's output while it runs
could not be written at all without installing `@types/node`.

The failure named the wrong thing twice over. `TS2305` said `spawn` is not
exported, and then two `TS7006`s said the `data` and `close` handlers had
implicit `any` parameters, because with no `ChildProcess` type behind the value
there was nothing to contextually type them from.

`spawn` is declared now, with the overload pairs `@types/node` uses, and a
`stream` module carrying the `Readable`/`Writable` subset a child's pipes need
(that is where the real typings export them from; declaring them inside
`child_process` would make `import child_process { Readable }` compile until the
real package arrived). A plain `spawn(cmd, args)` returns
`ChildProcessWithoutNullStreams`, whose pipes are non-null; anything that can
turn a pipe off returns the named `ChildProcess`, whose pipes are nullable.

The pipes live on the base interface, not only on the return type, and that is
the whole point of the shape. The first attempt put them only on the return
type, which made the guarantee change with the spelling: `let c = spawn(...)`
reported `TS18047` "possibly null" while `fn f(c: ChildProcess)` reported
`TS2339` "property does not exist", a different story about the same value.
Checked against `@types/node` 26.2.0, the shim now reports the same code as the
real typings on both, and `setEncoding` takes `BufferEncoding` rather than
`string` so it reports the same `TS2345` too. `string` there would have been
G125 again: wider than node, green here, red the moment the package is
installed.

Stream and process event names stay literal types, which is stricter than
`@types/node` (it keeps the `EventEmitter` string fallback, so `on("datum", ...)`
compiles there). That matches the `net` block's existing choice and errs the safe
way: the shim no longer accepts a *name* the real typings reject. That is
narrower than it sounds. A duck-typed `Readable` still compiles here and fails
`TS2345` under `@types/node`, because node's is a class and the shim's an
interface, so shape parity is a separate and harder problem than name parity.

The signal names sit in a global `declare namespace NodeJS`, not inside
`declare module "child_process"`, and that placement is load-bearing rather than
tidy. A type declared in an ambient module is exported from it whether or not it
says `export`, so the first attempt's private-looking `type Signals` made
`import child_process { spawn, Signals }` compile with nothing installed and
fail `TS2305` once `@types/node` was there. `@types/node` keeps `Signals` in the
`NodeJS` namespace, so the shim does too, and a test builds that import and
requires `tsc` to reject it.

Still open on this seam, and found while checking this change: five other shim
declarations take `string` where node takes `BufferEncoding`
(`net.Socket.setEncoding`, `new StringDecoder(...)`, `Buffer.from(s, enc)`,
`Buffer.byteLength(s, enc)`, `buf.toString(enc)`). Same bug as G125 and G126,
never swept. Narrowing a declaration people already build on is a guarantee
change, so it is in the rolling lane with the reproduction rather than folded in
here. Behind it: nothing compares the shim against `@types/node` declaration by
declaration, and `check_runtime_against_types_node.py` only covers what the
compiler's own runtime touches.

### 0.1.90 — Shipped · An object pattern's field takes a pattern

Published 2026-08-26 and smoke-tested from a clean npx cache in an isolated
HOME: `--version`, the execute bit, `glyph init`, `npm install`, `glyph run`,
and the headline feature itself.

G137. `ObjectPatternField` carried an `Option<Ident>`, so a field could name a
binding and nothing else. Every arm that wanted to look more than one level into
a record payload was unwritable: a variant tag in a field was E0009, and a nested
constructor, a nested destructure, a literal or an array pattern fell off the
field parser as a plain E0002. That is the shape D8 forces on any multi-field
payload, so the Okasaki red-black `balance` (four rotation cases, each named by a
two-level shape) had no spelling in Glyph at all.

A field now holds a `Pattern` and matches the field's value, prelude
`Option`/`Result` variants included (`Wrapped({ inner: Some(n) })` reads
`.value`; a user variant's record payload is spread flat and reads its fields
directly). Which of the two a payload uses is answered by
`variant_payload_is_record` against the matched type, the same question the rest
of the emitter already asks, so a union declared in a sibling module matches
exactly as a local one does. Descending is the checker's: it records the type of
every pattern node as it walks an arm and the emitter reads them back. Three things came with it. An object pattern is refutable when a field of it is
(`Pattern::is_refutable`), and one refutable arm routes the whole match through
`emit_pattern_chain`, an exclusive `if`/`else if` over a conjunction of field
tests plus the arm's binds, in the same family as the existing `is`- and
array-pattern chains: a `switch` cannot express it, because two arms can share
the outer tag and the second has to run when the first's field test fails.
Exhaustiveness takes the conservative side: a refutable object pattern no longer
covers its variant, so an arm that tests a field needs a sibling or an `else`
behind it. And E0009 retires, since the construct it named is the feature;
`--explain` keeps the entry and says so.

A `true`, `false` or `void` after the colon is a literal, not a binding named
after the keyword. It used to be the latter, which matched every value; that was
harmless while a field could only bind and wrong as soon as it could test.

Exhaustiveness reaches the scrutinees that have no variants too. A record
scrutinee is declined by every one of the union, array, bool and value-domain
checks, so once the field pattern above became writable, `match p { { x: 0, y:
y, } => .. }` compiled with no diagnostics and a clean `tsc --strict` and threw
on the first call. No shipped release had that hole: through 0.1.89 the pattern
is `E0002` and cannot be written at all. It is `E0226` now, a sibling of
E0218: every arm can fail, no arm is a catch-all, and no usefulness algorithm is
needed to see it.

One rejection here lands on code that used to build. The array exhaustiveness
predicate counted a bare `Ident` element as irrefutable whatever its case, so
`[Black]` covered length 1 and `match xs { [] => .., [Black] => .., [a, b,
...rest] => .. }` certified as exhaustive. It also miscompiled: the arm emitted a
length test and no tag test, so `[Red]` took it. The predicate is
`Pattern::is_refutable` now, the same reading D9 gets everywhere else, and that
match is `E0208` on arrays of length 1. Add the arm the tag test needs, or bind
the element and match it:

```glyph
module colors

type Color =
  | Red
  | Black

pub fn describe(xs: Array<Color>) -> string {
  return match xs {
    [] => "empty",
    [only] => match only {
      Black => "one black",
      Red => "one red",
    },
    [a, b, ...rest] => "many",
  }
}
```

The lowering underneath is untouched and stays open as G138, so this stops one
spelling from being certified on the strength of the disagreement rather than
closing it.

What is deliberately not in it: usefulness over a product of fields. Proving
`Node({ color: Red, .. })` and `Node({ color: Black, .. })` together cover `Node`
is a decision-tree question, not a tag-set one. When it lands it accepts strictly
more programs and rejects none that compile after this release.

G139 rides with it, and it is the half of the cross-module work that only a
*generic* union reaches. The registry answers a payload's storage across a module
boundary, but the type handed to it was the type as written at the match, and for
`Tree<Payload>` imported from a sibling module that is `Ty::App { base:
Ty::Imported }`: the application, not the union. Every arm fell past all four
proofs `payload_shape` has and came back undecidable, which is refused rather
than guessed, so a match over an imported generic union was rejected whole even
though the checker had resolved it. Neither half showed it alone: a local generic
union's base is a declaration this module can read, and an imported non-generic
one is a bare `Ty::Imported`, which proves boxed by itself. `payload_shape` now
unwraps the application first, the way its two neighbours already do. The unwrap
is load-bearing for any imported generic union under a nested pattern, recursive
or not: a two-variant non-recursive control declared in a sibling module fails
with the same E0300 on the same arm without it. It shipped in `04bf826` with
G137, which is also the commit that made the reproducing spelling (a pattern in
an object pattern's field) writable, so no released compiler ever failed this.
What 0.1.90 adds for G139 is the coverage: delete the unwrap and exactly one of
the 199 `glyph-cli` integration tests notices.

### 0.1.91 — Shipped · A generic union is checked like its bare twin

Published 2026-08-27 and smoke-tested from a clean npx cache in an isolated
HOME: `--version`, the execute bit, `glyph init`, `npm install`, `glyph run`,
and the headline feature itself.

G141 and G142. A type parameter on a union no longer makes a nested field
pattern unmatchable, and no longer switches exhaustiveness checking off for a
union the matching module declares. The red-black rotation arm 0.1.90 made
writable compiled over `type Tree = | Leaf | Node({ left: Tree, ... })` and was
`E0300` over the same union carrying a key type. G142 turned up while reviewing
that fix, on the caller two lines away: a `match` over a generic union could
omit a variant, build clean, pass `tsc --strict`, and throw at run time, where
the same match on the non-generic union is `E0200`. One missing unwrap was
behind both, so it moved into the resolver they share.

G141 is the first half of G140, found while pinning the 0.1.90 work. A field
holding a constructor that carries a payload needs the compiler to know how that
payload is stored, and there were two spellings it could not work that out for.
The first is a union generic over its own parameter, which refused a nested arm
the same union without the parameter accepts. The second is the namespace import
spelling, which is what is left of G140 and sits in the 0.1.96 plan below.

```glyph
module tree

type Tree<K> =
  | Leaf
  | Node({ left: Tree<K>, key: K, right: Tree<K> })

pub fn shape(t: Tree<string>) -> string {
  return match t {
    Node({ left: Node({ key: lk }), key: k, right: r }) => "deep:" + lk + "/" + k,
    other => "leaf",
  }
}
```

That was `E0300` on the first arm. `variant_payload` resolved a variant's payload
through `resolve_named_union`, which wants a bare `Ty::Named` and got a
`Ty::App`, so nothing under the payload got a recorded type and the emitter,
which reads those types back to decide flat-versus-boxed, had nothing to decide
from. It refused rather than guessing, which was the right direction and the
wrong answer.

`resolve_named_union` now unwraps an application to its base and reports the
arguments alongside the declaration, and `union_variant_payload` substitutes the
declaration's parameters into the payload, the way `record_fields_of` already
sends one to `named_record_fields`. Chasing what a newly visible payload would do
to nested exhaustiveness is what turned up G142, and the answer was not "nothing":
the recursion was unreachable. `check_patterns_exhaustive` needs
`required_variants` to answer before it consults a payload, `required_variants`
reached a module-local union through the same `resolve_named_union`, and a
`Ty::App` scrutinee produced no variant set at all. So the missing unwrap was
also switching exhaustiveness off for every module-local generic union: a
`match` on `Tree<K>` could omit `Leaf` and still build, pass `tsc --strict`, and
throw at run time, where the same program on a non-generic `Tree` is `E0200`.
Module-local is the limit of what the unwrap reaches; the imported spelling of
that same program is untouched by it and stays open under G142. That is why the unwrap
lives in `resolve_named_union` rather than in the payload lookup: both callers
need it, and one of them is the exhaustiveness check. With it in, a generic union
whose payload is itself a union reports the missing inner variant, which is the
recursion observed running rather than assumed harmless. Unwrapping the
application also puts a prelude `Result<T, E>` in front of `resolve_named_union`
for the first time, so that function picked up the prelude/module symbol-id
collision guard its two neighbours already carried.

G142 ships half closed, and the half that is left is worth stating plainly
because it is the same sentence with one word changed. Move the union into a
sibling module and `match t { Node({ key: k }) => k, }` on a `Tree<string>` still
builds clean, still passes `tsc --strict`, and still throws `non-exhaustive
match` at run time; delete the `<K>` and it is `E0200`. That spelling survived
this fix because it is a different mechanism, not a third caller: an imported
generic scrutinee arrives as `Ty::App { base: Ty::Imported, .. }`, and the
cross-module branch in `check_match_exhaustiveness` is gated on
`Ty::Unknown | Ty::Imported` and does not match the applied form, so
`check_imported_union_coverage` never runs. Widening that gate is its own risk
slice and is parked in the rolling lane below with G143, not claimed here. This
release closes the module-local half of G142 and nothing wider.

Also here: `scripts/check_doc_claims.py`. Across the 0.1.89 cut five release-audit
agents spent seventy-seven minutes between them, and every blocking finding they
returned was a stale number or a stale version reference in a markdown file: a
test count in the transpiler roadmap that no longer matched the suite, a
sentence promising a feature in a version that had already shipped. That is a
script, and review time spent on it is review time not spent on the diff. It
checks three things. A live status doc's test count has to equal what the suite
reported, read out of a suite log that has to be newer than every crate source,
so it cannot pass on a number nobody measured. A sentence promising something in
0.1.N does not survive 0.1.N shipping, and the pattern excludes the past tense so
that "added in 0.1.76" stays out of it: a gate people learn to ignore is worse
than no gate. And exactly one release section carries the Next marker, at a
version ahead of what has shipped. Frozen history is exempt by name: `archive/`,
the implementation plan, the open questions, and the per-release entries in this
file, where "690 tests green" is a fact about that release rather than a claim
about now. CI runs it alongside the other gates.

### 0.1.92 — Shipped · The derived-type cast reaches every return

Published 2026-08-27 and smoke-tested from a clean npx cache in an isolated
HOME: `--version`, the execute bit on the resolved platform binary, `glyph
init`, `npm install`, `glyph run`, and the headline feature itself. The
combinator below builds clean and passes `tsc --strict` under 0.1.92; the same
program under 0.1.91 fails `tsc` with `Record<string, unknown>` not assignable
to `__GlyphInferOutput<Shape>`, which is the emitted difference the fix makes.
The linux-x64 tarball on the GitHub Release extracts to a mode-0755 binary and
matches its `SHA256SUMS` entry.

G144. D28 gives a combinator whose declared return type mentions
`infer_output<Shape>` one compiler-inserted boundary cast, because the body
assembles a value the type system cannot prove carries the shape-derived type.
The cast lived in `Emitter::emit_return`, and two lowerings write a `return`
without going through it: a `match` in return position, which becomes a `switch`
whose arms carry their own `return`, and a tail `E?`, which returns the
unwrapped payload directly. Either one dropped the cast, so a combinator that
compiled as `return { name: "object", parse: ... }` stopped compiling the moment
a `match` sat between the `return` and the value. The failure surfaced as a
`TS2322` against the emitted TypeScript, not as a Glyph diagnostic, which is the
worst place for it: the program is well-formed Glyph and the compiler said so.

The reproduction is `examples/corpus/infer_output.glyph`'s combinator with one
`match` sitting between its `return` and the value it already returned:

```glyph
module schema

import std/result { Result, Ok, Err }

type Schema<T> = {
  name: string,
  parse: fn(input: unknown) -> Result<T, string>,
}

fn number_schema() -> Schema<number> {
  return { name: "number", parse: fn(input) {
    match input {
      is number => Ok(input),
      else => Err("expected number"),
    }
  } }
}

fn object_schema<Shape: Record<string, Schema<unknown>>>(shape: Shape, strict: bool) -> Schema<infer_output<Shape>> {
  return match strict {
    else => { name: "object", parse: fn(input) { Err("unimplemented") } },
  }
}

type Point = {
  x: number,
  y: number,
}

pub const point_schema: Schema<Point> = object_schema({ x: number_schema(), y: number_schema() }, true)
```

Both arm-return sites in `glyph-emit` call `emit_return` now instead of writing
`return {v};` themselves, so the cast follows the return rather than the
spelling. Two lines, no new mechanism. The coverage is three tests:
`infer_output_cast_reaches_every_return_a_match_lowers_to` and
`infer_output_cast_reaches_a_tail_try_return` pin the emitted TypeScript in
`glyph-emit`, and `infer_output_cast_survives_a_match_in_return_position` in the
CLI integration suite builds the corpus combinator with one `match` around its
body and runs `tsc --strict` over the output, so the end-to-end failure this
started as is the thing being watched.

The rest of the 0.1.92 plan did not ship. G130 went out in 0.1.93 below; G138
and G140's namespace half moved on to 0.1.96, unchanged.

*Found by an app round. Reproduced against 0.1.91 and fixed in the same round.*

## What Glyph checks that tsc does not, measured rather than asserted

An outside analysis proposed twenty checks Glyph performs that `tsc --strict`
does not, and a positioning line built on them. Each was written as a real
program and run against the shipped 0.1.98 binary, with the equivalent
TypeScript run through `tsc --strict` for comparison. Five hold. Nine are false.

| | count | rows |
|---|---|---|
| Rejected at compile time by a Glyph diagnostic | 5 | 1, 3, 4, 10, 19 |
| Glyph accepts it, so the claim is false | 9 | 2, 5, 6, 7, 8, 13, 14, 18, 20 |
| TypeScript catches it too, so Glyph adds nothing | 3 | 9, 11, 17 |
| Not expressible in the language at all | 2 | 15, 16 |
| Checked at a runtime boundary, not at compile time | 1 | 12 |

**What is real is one theme, and it is the theme of the last three releases.**
Glyph knows every case of a closed union and refuses a `match` that mishandles
them. `E0200` for a missing variant, `E0305` for an arm that can never run, and
it holds in all four spellings tested: a named import, a namespace import, a
nested payload union, and the nested case through a namespace. The equivalent
TypeScript exits 0 and prints `undefined` for the omitted branch. The
comparison is also stronger than it first looks: `tsc --strict` catches a
missing case in a *return-typed* switch as a side effect of return analysis,
and misses it entirely in the void, side-effect-only switch that most real code
is.

`E0220` on a constructor-shaped pattern over a record is genuine and
Glyph-native, but there is no TypeScript construct that mirrors that syntax, so
it is Glyph catching a mistake in Glyph-only spelling rather than being stricter
on shared ground. Worth documenting, wrong to headline.

**Three findings the audit produced, which matter more than the table.**

*Nominal record identity is inconsistent, and the inconsistency is one line
wide.* Two structurally identical named records are not interchangeable at a
call site or a return: `ship(d)` where `d: Draft` and `ship` takes `Paid` is
`E0211`, and `tsc --strict` accepts the identical TypeScript. But `let p: Paid =
d` compiles clean, and so does passing the value through a union constructor. So
the guarantee holds at some sites and evaporates at others, and the compiler
does not steer anyone toward the encoding that works. Filed as G156.

*`docs/language/spec.md` D44 still describes G143 as open.* It closed in 0.1.97.
A reader who takes the marketing claim and then checks the spec finds the two
contradicting each other. Filed as G157.

*One finding from the audit did not survive re-checking, and is recorded here so
it is not re-adopted.* The assessment reported that a union's generated
descriptor accepts an unexpected key, which would have undercut the one runtime
guarantee worth marketing. It does not: `json.parse<Shape>` over a payload
carrying a surplus field returns `Err`, and so does the record case. Both were
re-run before filing. The audit's own conclusion, that a claim gets written as a
program before it is written as a sentence, applies to the audit.

**The positioning line does not survive.** "TypeScript checks what your values
are, Glyph checks what your program is allowed to do with them" promises
permitted operations, legal transitions, and invariants that survive use. Every
row that would carry it came back false or inexpressible: state transitions,
invariants after mutation, facts carried across a call, impossible states in a
record, relationships between fields or between variables, whole-structure
invariants, unreachable paths. A reader disproves it with `let p: Paid = d` in
under a minute, on a green build.

What the evidence supports:

> Add a variant, and Glyph shows you every match that no longer handles it.
> TypeScript compiles it and you find out in production.

Every clause there is a reproduced `E0200` against a confirmed `tsc --strict`
exit 0.

**What must not be claimed, in any form.** That Glyph tracks invariants, proves
a branch unreachable from a value fact, verifies a state transition, or carries
a refinement through a function call. Four were tested and are false, and three
of them are recorded non-goals in this file rather than unbuilt features:
interprocedural dataflow is unsound under first-class functions, refinement
facts beyond `where` are not expressible, and `requires`/`ensures` clauses are
on the manifesto's abandoned list. A function-level precondition does not even
parse.

This is the second time a proposed claim has outrun the compiler. The first
showed a diagnostic proving a branch unreachable from a balance guarantee, which
`where` refinements do not do, because D39 is boundary-validated rather than
statically tracked. The rule this establishes: a public claim about what the
compiler refuses gets written as a program and run before it is written as a
sentence.

## Road to 0.1.105

The committed sequence. Each release is cut and published as soon as it is
ready rather than batched, and each gets its own `###` entry with a review
stamp when it is cut, because a plan written ten releases ahead is a claim
about a compiler that has not stopped moving.

Ordering rule, in force for the whole sequence: a program that compiles clean
and misbehaves at run time outranks a missing diagnostic, which outranks
anything an agent or a person merely finds annoying, which outranks
infrastructure, which outranks polish. Dependencies are respected where the
entries below name them.

**0.1.96 — Shipped · The variants of a union the emitter did not declare**
- Hand the emitter an imported union's variant names so a lowercase nested arm stops failing E0305 (G147, half: fixed for a prelude scrutinee, an imported outer union still needs a payload-type registry)
- Deepen the shallow imported-union coverage check off that same variant list (G143)
- Report a constructor-shaped pattern over a non-union payload as an E0220-class Glyph error (G146)
- Route the top-level array-pattern chain through `pattern_conditions` so `[Black]` tests instead of binds (G138)

**0.1.97 — Shipped · One identity for a function across a module boundary**
- Add an `exported_fn` signature query so a call into another module stops typing as Unknown (G133)
- Decide that query's shape: full `Ty::Fn` or return type only, and the emitter's answer on an empty registry (G133)
- Record an imported union's payload type under the namespace spelling so `tree.Node` stops being E0300 (G140)
- Type a match scrutinee bound by an inferred `let` so the arm stops falling back to `.value` (G132)

**0.1.98 — Shipped · The two-binding for loop knows its element type**
- Bind an element type for `Stmt::For` so `array.slice`/`map` iterands stop lowering to `Object.entries` (G37)
- Keep D30 string-union exhaustiveness alive inside a loop; E0218 stops suggesting the `else` (G67)
- Diagnose a module that declares no `pub` and emits zero exports, before the host toolchain does (G124)
- Count `@example` references in the unused-import lint so E0106 stops firing on live imports (G106)

**Correction, made when 0.1.98 was triaged.** This release was scheduled around
a live miscompile: a two-binding `for` over `array.slice` binding a string
index, printing `01:` for `1:`. That is fixed and has been for several
releases, closed by real stdlib signatures on `slice`/`map` and by
`__glyph_pairs` dispatching the iteration protocol at run time. The
`examples/apps/expenses` annotation cited as load-bearing was deletable
independently of anything here, and is deleted.

What remains is real but narrower, and it is a diagnostic downgrade rather than
a wrong answer: the typechecker types a loop element only for the
single-binding form, so a two-binding `for i, c in xs` leaves both names
unknown and a match on a string-literal-union field read through `c` reports
`E0218` where the single-binding form reports nothing at all. Closing it is a
`ForStmt` per-binding-span change, which the parser already has the span for
and discards.

The scheduling was not wrong to include this; the justification was stale. A
plan written ten releases ahead makes claims about a compiler that keeps
moving, which is why triage re-runs every premise before anyone writes code.

**0.1.99 — Shipped · The front end answers instead of tsc**
- Check an annotated `let` against its initializer, so `let x: string = 42` is a Glyph error rather than TS2322 (G149)
- Make `glyph fix` act on an unused item inside a named import list, not only on a whole unused import (G152)
- Put the escape table in the bootstrap and make E0001's help name `\u{HEX}` rather than the category (G153)
- Give `std/random` its signatures in the bootstrap; `seeded` and `Rng` appear zero times today (G154)
- Delete or implement `Rng.bool`'s promised 0.5 default; the comment describes a signature that does not exist (G155)
- Reject member access, call arity, and argument types against a receiver that is still Unknown (G39)
- Resolve E0010 and E0300's opposite claims about positional variant patterns into one rule (G135)
- Make an unresolved import a resolver diagnostic naming the modules that exist, not TS2307 (Q46)
- Report an `is`-narrowed value used outside its binding as a Glyph diagnostic rather than a tsc one (G98)

**A measurement worth keeping.** Between 0.1.93 and 0.1.95 the agent bootstrap
changed by exactly one line, and `u{` still appears in it zero times. Over those
five releases the compiler got materially more correct, and the documentation
surface that actually misleads a first-time reader did not move. That is not a
complaint about prioritisation, since the union work was the right thing to fix
first. It is the reason four of the five items above are documentation-shaped:
a reader starting today hits the same walls in the same order, including
rebuilding an eleven-line workaround for an escape that has always worked.

**0.1.100 — Shipped · A client can read a response as text**
- Decide and ship the http deadline: required on `get`/`post`, or `request` stops reading 0 as skip-the-timer (G128)
- Give `Response` a text accessor so a JSON body stops printing as `[object Object]` (G118)
- Decide how `Option<T>` reads ordinary JSON: loosen `.parse`, or add a distinct nullable boundary type (G91)

**0.1.101 — Shipped · A note on an example line no longer verifies a false claim**
- `glyph build --watch`, so the Glyph half stops being compile-by-hand next to Vite's HMR (G123)
- A browser target that stops materializing the seven host modules under `.glyph-runtime` (G115)
- Make `glyph fmt` reach a fixed point in one pass so `--check` stops failing on fmt's own output (G151)
- Stop `fmt` reprinting `=> ({})` as `=> {}`; the AST needs a grouping node (G60)
- Format the repo's own Glyph and gate it, so the corpus matches the formatter (G158)
- Make the benchmark fixtures compile, and gate that they keep compiling (G159)
- Remap a tsc error onto the right project's module in a multi-project build (G107)
- Close the false green: a comment must not eat an `@example` assertion (G160)
- Stop `fmt` reattaching a comment to the wrong annotation (G161)

**0.1.102 — Shipped · salsa 0.28, and the pipeline's own gaps**
- Migrate the query layer to salsa 0.28's moved `Update` trait, alone, with no feature work beside it
- Stop `fmt` emitting a bare literal statement and a parenthesized one that reparse as a call (G162)
- Decide what a file's address is, since its `module` header and its path-derived key can disagree (G172)
- Escalate an unknown PascalCase head over an imported union to E0220, as the local path does (G169)
- Make the playground emit what `glyph build` emits, or say plainly on the page that it does not (G170)
- Measure what fraction of diagnostics land inside a function body, which settles whether locals stay excluded (G171)
- Give the `std/net` integration test a deadline so a hang fails instead of leaking a process tree (G166)
- Execute darwin-x64 somewhere in CI (macos-13 is the honest fix)
- Run `check_binary_fresh.py` from the release workflow instead of by hand
- Detect musl and diagnose it rather than handing Alpine a glibc binary

**0.1.103 — Shipped · MCP answers from the compiler's own memo**
- One semantic query boundary shared by LSP and MCP over `resolve`, `module_symbols`, `type_map`, `decl_ty`, `module_exports`
- Stop `glyph_references` running 175 full analyses of the examples tree to answer one question
- Write the projection constraint into the spec: the semantic view derives from the compiler and never re-derives (Q45)
- Measure the agent success rate, first-compile fraction and cycles to green, from what Thor already produces (Q46)

**0.1.104 — Shipped · An entity keeps its name when the file around it changes**
- R1: `ModuleId` and a `module::name` declaration key; inserting a declaration renumbers nothing
- R1: the MCP tools answer about an entity, not about a line and column that moved
- R6: an `@example` failure names the function it is about, not `example #1`
- Bound the release smoke jobs so a queued runner cannot block the next publish

Deferred with reasons recorded above, not dropped: `ScopeId` and locals (no
question this line of work exists to answer is about a `let`), `TypeId` as a
separate identity (a type is a declaration; a second name for one thing is what
0.1.56 and 0.1.57 cost), arm identity (R4's foundation, and production code has
none today), and the boundary node kinds (the origin question belongs where the
generator already knows the answer).

**0.1.105 — Shipped · One answer to "does this match have a catch-all"**
- Unify the seven catch-all predicates into one, so the graph does not inherit a disagreement
- Acceptance test is a diagnostic diff across all 31 apps: no change means it was free, any change is a finding
- Nothing else ships beside it, because it is the one item here that can change what existing code reports

**0.1.106 — Shipped · The exhaustiveness relation, and the tool reading it**
- Give a match site an identity, since `walk_decl` takes a bare `&Decl` and nothing names the declaration a site sits in
- Thread a `DeclKey` out of `required_variants`, which returns a display string today and the corpus holds 11 unrelated `Command`s
- Make `check_patterns_exhaustive` form the per-arm relation it currently discards
- Retain it: a site record with four states, `mentions` edges, the catch-all arm's ordinal
- `match_coverage(db, file)` plus a project fold, so the value backdates for its consumers; the query itself still re-executes on any edit
- `glyph_variants`: for a type, every match site and which variants each one names, with unindexable sites named rather than counted

**0.1.107 — Shipped · The autofix stops reporting success over a file it broke, and the tool says how to update itself**
- G173: `glyph fix` welded a pruned import onto the next line and exited 0; live since 0.1.99, not the 0.1.106 regression it was reported as
- Both prune paths share one `decl_text_end`, and `fix` re-parses every file before writing it, so a broken rewrite fails loudly instead of quietly
- The MCP server sends an `instructions` string for the first time: an agent learns `glyph --update` (the tool) and `glyph upgrade` (a project's pin) without opening a file
- G174: the two routes for discovering an undocumented module's signatures are undocumented, and G179's pruning narrowed one of them
- G175: the bootstrap prints `rng.bool(probability: number)` with no `?`, so the default 0.1.106 added is invisible to a reader
- G176: the bootstrap's two statements about `mut` contradict, and the wrong reading costs an architecture rather than a probe
- G177: E0101's help implies a project module needs a package prefix; a bare sibling import is the real spelling
- G178: twelve modules sit under "not detailed", `std/random` among them despite now having a signature block
- G179: 0.1.106 prunes the emitted runtime and nothing says so, which is the mechanism behind G174's narrowing
- Evidence that docs alone do not carry this: `glyph --update` was added to the bootstrap and the next agent to need it still reached for npm
- `glyph_variants` dropped a payload site whose file carries the G172 module/path mismatch from all three of its lists, on the layout `glyph init` generates
- A diagnostic carries the declaration it sits in as `module::name`, on `glyph check --json` and `glyph_diagnostics`, so an error leads into the graph without a re-parse
- Not the forward-impact query: that stays 0.1.108, which is what this release's prerequisites were clearing the way for

**0.1.108 — Shipped · One identity, one spelling**
- G180: three surfaces derive a declaration's module half from three different roots, and two of them disagree on the layout `glyph init` writes
- One derivation and one attribution function, rather than the two byte-identical copies and three roots shipped in 0.1.107
- G181: attribute a diagnostic from the declaration the checker is standing in, since an annotation's span sits in the gap before a decl's span starts
- The same for `tsc`-remapped diagnostics, whose Glyph source is already in `ModuleMap`, and for `@example` failures, which carry no structure at all
- G182 is closed: the release workflow named a runner retired nine months earlier, so `darwin-x64` was executed nowhere and every Release run concluded `cancelled`
- Ahead of the impact query on purpose: that query names this field as a prerequisite, and an ambiguous identity makes it confidently wrong rather than incomplete
- G183: `glyph check --json` ran no `@example` gate at all, so the agent-facing surface reported a clean tree on a program the text path fails
- G184: absence of an entity was spelled two ways, omitted on one surface and explicit null on the other
- G185: the example gate leaked a copy of the project into the temp directory on every run, which was also manufacturing a flaky test through pid reuse

**Impact before the edit — Delivered across 0.1.106 to 0.1.108, no version of its own**
- Reading the edges forward shipped in 0.1.106; diagnostics carrying the declaration shipped in 0.1.107 and were made consistent in 0.1.108
- The one part nothing had measured, the nested-payload shape, is measured and holds: see the 0.1.109 section below for the numbers
- No release was cut for it because no compiler behaviour changed; a version that ships an identical binary is a version number, not a release

**0.1.109 — Landed on main · The folds you write by hand**
- G100: `std/array` has no `max`, `min`, `max_by`, `min_by` or `sum`, so argmax is a four-line `match acc` fold at every search, ranking and scheduler
- Re-reproduced against 0.1.108 rather than carried forward: every name is still `[E0105] not exported by std/array`
- Not a soundness fix: the hand-written forms are correct, just long, and long is what an agent gets wrong
- G101 is deliberately NOT here. An early-exit fold needs a continue-or-stop type, and whether that is a new `Step<A>` export or a reuse of `Result` is a surface commitment, not a mechanical addition. It goes to review first.

**The target, stated as one sentence.** An agent can make a semantic change, ask
Glyph what that change affects, and get an exact, actionable impact set without
grep. Everything from 0.1.104 onward is that sentence taken apart: an identity
that survives an edit, a relation retained rather than recomputed, one spelling
per identity, diagnostics that name the entity they concern. What is missing is
the traversal itself, and the relations it would traverse.

**One primitive, not another bespoke query.** The shape:

```
glyph_impact { entity: "payments::PaymentResult" }
```

Addressed by entity, which 0.1.108 made possible by giving a declaration one
spelling. It returns nodes and labelled edges rather than a shape invented per
question: each affected entity, the relation that connects it, and a verdict.
The verdict is the part that carries the value, and `glyph_variants` already
proved the distinction is real: a site that fails to compile is a task, and a
site that keeps compiling while silently absorbing the change is a bug nobody is
going to be told about. An impact set that does not separate those two is a
list of files.

Three properties it must hold, all inherited rather than new:

- **Exact or named.** Anything the graph could not resolve is listed, never
  dropped and never counted. A short list presented as complete is worse than no
  list, because an agent edits believing it has seen the whole surface.
- **One identity.** Entities in the answer are addressable by the same
  `module::name` the diagnostics carry, so following a thread does not re-derive
  the world between hops.
- **Relation-labelled.** "Affected" is not a fact. Affected *how* is.

**Why it is not next.** The primitive is only as honest as the relations under
it, and they are not ready:

- G186: `glyph_references` silently drops a namespace-qualified call. The same
  symbol answers differently depending on how the consumer spelled its import,
  which is the shape this project has twice decided is wrong on arrival.
- There is no `CALLS` relation distinct from `REFERENCES`, so "what calls this"
  and "what mentions this" are the same answer today.
- A record field is not addressable at all. `glyph_references` handles top-level
  names, so "rename `User.email`, what breaks" has no answer and an agent has to
  grep, which is the exact thing the sentence above forbids.

Building the *general* traversal on those would produce an impact set that looks
exact and is not, which is worse than having no traversal.

But one change kind does not wait, because its relation is already whole. "What
breaks if I add this variant" rides entirely on the coverage relation retained in
0.1.106: cross-module, four states, and sites it cannot key named rather than
counted. None of the three gaps above touch it. So that case ships first, as the
first instance of the primitive rather than as another bespoke tool, and the
generalization follows once the remaining relations can carry the same promise.

**0.1.110 — Landed on main · "What breaks if I add this variant?"**
- **The flagship, and larger than the releases around it on purpose.** It carries nine entries, and an earlier reading of this file called that overloaded. It is not: they are one thing said in four parts. The capability (ask as a change, answer with a consequence), the invariant that keeps the answer honest, the tests that prove the loop closes, and the artifact that lets someone else run it. Ship any three without the fourth and the release is a claim rather than a demonstration
- The order within it is capability, gate, tests, artifact. The gate comes second rather than last because an answer that manufactures a relation is worse than no answer, and the tests come before the artifact because a demo of an untested loop is a transcript
- G187: the question can only be asked as a lookup today. `glyph_impact { entity: "payments::PaymentResult", add_variant: "Pending" }` asks it as the change it is
- G188: the answer states a consequence, not a state: FAILS for a site that stops compiling, SILENTLY ABSORBS `Pending` for a site whose catch-all takes it, and the union's own variants so the caller can render what it is changing
- Rejects a proposed name that already exists, which the lookup form cannot check because the name never reaches it
- **It ships ahead of the other relations on purpose.** This one rides on the coverage relation, which is already whole: cross-module, four states, unindexable sites named rather than counted. Nothing here waits on G186 or on `CALLS`
- The first instance of the general primitive, on the one relation that can carry it honestly today. Building it settles the answer shape before the shape has to serve four relations at once

**0.1.110 also carries the rule's own enforcement.**
- A hostile corpus that deliberately builds five degenerate conditions and asserts no impact edge is manufactured for any of them: missing identity, ambiguous identity, unsupported entity, malformed module, unresolved import
- Probed against 0.1.108 first rather than written from the design: three already answer correctly, and two do not
- G190: asking about a record returns `{"sites": []}`, shaped identically to a union with no matches, so "the question does not apply" is spelled the same as "nothing is affected". An empty list is a claim
- G189: `glyph check --no-tsc` says "no diagnostics" on a program importing a module that does not exist, while the impact surface asked about the same file correctly answers `scrutinee_unresolved`. One unresolvable name, two surfaces, one saying it does not know and one saying everything is fine
- The corpus is a gate, not a document. The rule survives only if something fails when it is broken

**The repair loop, closed and tested.** The second half of the architectural
claim. The first half asks what a change would affect; this half is what happens
after the change lands.

```
agent edits -> glyph check -> diagnostic -> graph query -> exact target -> agent fixes -> check -> green
```

*Walked by hand against 0.1.108, and it closes.* An agent adds `Pending` to a
three-site union, `glyph check --json` returns two `E0200`s each naming the
declaration it is in, `glyph_variants` called with the file the diagnostic
already named returns all three sites, and the third is `billing::audit`, which
keeps compiling and silently routes `Pending` into its catch-all. The diagnostic
never mentions that site. The query does. That difference is the whole argument
for the graph, and it holds.

**It needs no grep except at the hop that starts it, and there it needs a regex
over a sentence.** To make the query, the agent needs the union's name. A
diagnostic carries `code`, `entity`, `file`, `help`, `message`, `note`, `range`,
`severity` and `stage`, and the union's name is in none of them. It is in the
message text, in backticks, next to the missing variant:

```
"non-exhaustive match on `PaymentResult`: missing variants `Pending`"
```

- G195: the diagnostic carries the entities it concerns as structure rather than describing them in prose. This is the 0.1.107 `entity` argument one field along. Pinning a machine contract to a sentence makes every improvement to that sentence a silent breaking change
- G196: the loop has been walked once, by hand, on one union. Nothing tests that it closes. `benchmarks/impact-before-edit/` covers the half before the edit and the half after it has no counterpart
- **The test asserts something narrower than "the loop works" and harder to fake:** at every hop, that what the next call needed was a *field* in the previous answer. A loop that closes because the harness knew the type name in advance proves nothing, and that is exactly how this test gets written by accident
- Same instrument discipline as the impact benchmark: defined before the result is seen, the losing run published, and no guarantee demonstrated that the compiler does not make

**Three levels, and the one that is missing is the one that matters.** The graph
internals are well covered and the surface an agent talks to is not.

*Measured against 0.1.108.*

| level | what it covers | state |
|---|---|---|
| Unit | edge creation, identity, state calculation | 196 tests in `assign.rs`, 53 in `glyph-db`, 44 in `mcp.rs` |
| Integration | project, compiler, graph, MCP response shape | present, in process through `handle()` |
| Process | `glyph mcp` over stdio, as a client speaks to it | **nothing** |
| Golden scenario | edit, ask, fix the returned sites, check green | **nothing** |

- G197: no test starts the server and exchanges a line with it. Forty-five integration tests spawn the binary for other commands and none spawns it for this one, so framing, the handshake as an ordered exchange, and behaviour on a malformed line are all unexercised
- The sharpest instance: the `instructions` string 0.1.107 added appears twice in `mcp.rs`, in a doc comment and in the value, and in no assertion. Delete it and the suite stays green. It is a field that only ever mattered on the wire, and it was checked once, by hand, by piping JSON into the published binary
- G196: the golden scenario. Add a variant, ask what is affected, edit the sites that came back, run check, require green. Nothing like it exists
- **The golden scenario is worth more than the other two together, and is the easiest to write dishonestly.** It has to assert that each hop's input came out of the previous hop's answer as a field. A test that closes the loop while the harness supplies the type name from its own fixture proves that the fixture is consistent with itself
- These levels arrive with the work rather than after it: a relation added without a case at all three is a relation nobody has checked from the outside

**The demonstration, and it already runs.** One example that is hard to wave
away, on a real application rather than a fixture built to flatter the answer.

*Run against 0.1.108 on `examples/apps/csvql`, an eleven-file CSV query engine
already in this repository, by adding one variant to the `Value` union at its
centre.*

```
Agent: add `Blob` to Value.

Glyph, before the edit:
  10 match sites across 4 files
   8 will fail compilation
   2 contain catch-alls and will silently absorb it

     FAILS    value.glyph:18   value::render
     FAILS    value.glyph:32   value::key
     FAILS    bind.glyph:148   bind::literal_kind
     ABSORBS  exec.glyph:130   exec::total
     ABSORBS  render.glyph:128 render::literal_text
     ...

Agent adds the variant.

Compiler: 8 E0200 failures. The same eight.
          The two catch-all sites report nothing at all.
```

**The prediction was exact. Eight of eight, and two of two silent.** The two
sites the compiler will never mention are the reason this is not an LSP feature:
`exec::total` and `render::literal_text` keep compiling and route a `Blob`
wherever their `else` points, and no build, no test and no type error will say
so. An agent that fixes the eight and stops has a green tree and two live bugs.

What is missing is not the capability. It is everything around it:

- G199: this exists as a transcript of one session. It is not committed, not scripted, not on the website and not in the suite, so it is a claim about a run rather than a property of the compiler
- G198: the three summary lines were computed by a throwaway script from a flat list. Glyph returns ten entries with states and takes no position on the totals, so two callers can tally the same reply differently and neither is wrong
- The third category the demo wants, a site already exhaustive through an existing constructor and therefore genuinely unaffected, needs the hypothetical input (G187): "unaffected by adding `Blob`" is a fact about a proposed change, not about the source text
- **Any total states what it could not count.** A summary that silently omits sites the graph could not key is the partial-list-as-complete failure in its most persuasive form, because a number reads as authoritative in a way a list does not

**The site sells the wrong thing.** Two outside readings of this project, one
commercial and one engineering, arrive at the same instruction from opposite
directions: stop describing the language, demonstrate the capability.

The commercial reading: the interesting asset is not a nicer dialect, it is a
compiler that already knows what an agent would otherwise rediscover by
grepping, exposed as an API. Transpiling to TypeScript matters because it means
nobody has to replace node, npm, tsc or their deployment to try it.

The engineering reading: syntax is not evidence. Show a bug, show the same bug
passing `tsc --strict`, show Glyph rejecting it, show an agent repairing it,
and show that it was not contrived.

Both are already answerable and the site answers neither:

- G200: the homepage opens with a side-by-side of Glyph and its emitted TypeScript, which asks a reader to compare spellings
- `catches/` holds seven paired demos, each a `ts.ts` that passes `tsc --strict` beside a `glyph.glyph` that fails with a named code, and `check_catches.py` fails the build if either half stops behaving. Seven of seven verified both ways. The homepage mentions this once, in passing
- The impact and repair capability appears zero times on the homepage. Not the ten sites across four files, not the eight that fail against the two that silently absorb, not that the prediction was exact on a real application in this repository
- The rebuild leads with the demonstration and keeps the language comparison as supporting material, because "here is a bug your current toolchain ships" is an argument and "here is different syntax" is a preference

**0.1.111 — Landed on main · The proof, and what the relations still owe**
- G186: one answer for a symbol's references whichever way the consumer spelled its import
- `CALLS` distinct from `REFERENCES`, each answer saying whether the compiler proved it or a `.d.ts` asserted it
- Record fields become addressable entities, so a field rename has an impact set rather than a grep. **Landed on main.** `glyph_references` takes `Record.field` (`User.email`, the record named the way the querying file names it) and answers `{ entity, sites, unkeyed, not_indexed }` from a new field-use relation the member-access check writes while it types each file, alongside the match-coverage one. `sites` holds the field's own declaration, every member read, every write (`mut u.email = v`), and a `@redact fields: [...]` name, each keyed to the record the checker resolved the access against and ranged over the field's name alone so a rename can write from it. Two records in one module with the same field name are two entities; a read through a cross-module alias keys to the record the chain ends at, not the name the use site reached it by; a read of a payload binding (`Loaded(u) => u.email`) is a proven site. `tests/exact-or-absent/field-entity/` gates it
- Every relation states what it could not index, per entity, never as a count

**The field relation's two open edges, both recorded rather than papered over.**
A construction site (`return { name: "a", email: "b", }` where a `User` is
expected) breaks when the field is renamed and is not in the relation, because
`Expr::Object` gets no expected type from the checker: an object literal types as
`Unknown`, so no key in one is ever joined to a field of a declaration and `tsc`
is what reports the mistake. An object pattern destructuring a named record
(`Loaded({ email })`) is the same shape: the pattern binder resolves fields
against a structural payload record only, so nothing is resolved through a named
one, and a misspelled key there produces no Glyph diagnostic at all. Both classes
are named in every field answer's `not_indexed`, so a caller is not handed a list
of member accesses that reads as the whole impact set. Closing either one means
giving `Expr::Object` and the object-pattern binder a bidirectional expectation,
which is a language change with a new diagnostic behind it and a breaking release
to carry it, not a graph change.

**Traversal is bounded and explicit, or it is a dump.** "Everything connected to
X" stops being useful at the second hop. The traversal takes named relations, a
depth, and returns, for every entity in the answer, the path that put it there.

The vocabulary is small and closed, and fixing it comes before anything reads
it: `mentions`, `declarations`, `match_sites`, `dependents`, `variants`,
`declared_in`. A relation cannot be selected in a request or named in a reply
until the set exists (G193).

```
glyph_impact {
  entity:    "payments::PaymentResult",
  relations: ["mentions", "declared_in"],
  depth:     2,
}
```

and every entity in the answer carries its own provenance:

```
payments::PaymentResult
  --mentions-->    billing::settle   (match site, line 6)
  --declared_in--> src/billing.glyph
```

- G191: the relation is never named on the wire. `mentions` was chosen over `covers` in 0.1.106 as that release's central distinction, and the word reaches no reply. Position in `sites` / `nested` / `unkeyed` carries it instead, and position cannot carry three relations
- G192: no depth, no relation selection. `depth` is already computed and already reported per nested arm, so the traversal knows how far it went and offers no way to say how far to go. That is a report, not a parameter
- G193: no vocabulary to select from, which is why the other two cannot be built first
- **Exact-or-absent binds per relation, not per answer.** A traversal that resolved `mentions` completely and `dependents` partially and returned one list would be a partial list presented as complete, which is the failure the rule exists to prevent. Each relation states its own coverage, and `tests/exact-or-absent/` grows a case per relation as each lands

**Every result answers "why did you give me this" on its own.** Not the boundary
and origin provenance of R3, which stays where it is planned. This is the
narrower obligation that every element of a traversal result stands up without
its envelope:

```
{ "entity":   "payments::PaymentResult",
  "relation": "mentions",
  "target":   "payments::handle",
  "source":   "payments.glyph:42",
  "state":    "fails_on_new_variant" }
```

- G194: three of those five are on an entry today. `entity` and `relation` are missing, and they are exactly the two that let a result stand alone. Today the subject appears once, in the envelope, so an entry lifted out of its reply can no longer say what it is about
- It is survivable while one reply answers one question about one entity, and stops being survivable the moment relations merge, since entries from `mentions` and from `dependents` are the same shape and only position tells them apart
- `state` becomes a consequence rather than a state (G188): `fails_on_new_variant` is a fact about the change being asked about, where `has_catch_all` is a fact about the source text that the caller has to interpret
- **`source` stays a relocation hint and never becomes a key.** 0.1.106 settled that a match site gets no durable identity, since an ordinal moves when a match is inserted above it and a spelling changes when the scrutinee is edited. A result says where the site was when asked, so an agent goes and looks rather than dereferencing an address that may have gone stale. Provenance that has to be re-derivable follows from that decision rather than working around it
- The test for all of this is not that the field is present. It is that an agent handed one result, with no reply around it and no memory of the question, can still say why it is holding that result

**0.1.112 — Landed on main · `glyph_impact`, generalized over every relation**

**The two questions are decided, and this is the answer.**

*Graph reachability is not impact.* `A -> B -> C` says a relationship exists. It
does not say the consequence travels along it. `PaymentResult` gains a variant,
`billing::settle` matches on it exhaustively and stops compiling, and
`api::checkout` calls `settle`. Whether `checkout` is impacted does not depend on
the edge between them. It depends on whether `settle`'s signature changed, which
is a fact about the change and not about the graph.

**Transitivity does not depend on the change kind either.** That was the first
answer and the compiler contradicted it. Seven change kinds were probed through
the language server with the buffer supplied in `didOpen`, so nothing touched
disk: add a variant, rename a variant, rename a type, rename a field, rename a
function, add a parameter, change a return type. Every diagnostic in every probe
landed at a site that lexically names the changed entity. Not one landed at
depth 2, including on a three-deep call chain.

The reason is structural and it is a language property rather than a graph one.
**Glyph never infers a declaration's type from its body.** A `fn` with no `->`
types as `void` and its caller gets E0204; an unannotated lambda behaves the
same. A callee's type therefore cannot reach a caller's signature, so a change to
X can only invalidate expressions that name X. The signature is a firewall, and
that is what bounds propagation, not the kind of edit.

What looked like transitivity is direct membership. `render::literal_text` is in
the impact set because it matches on `Value` at line 128, not because it calls
`value.render` at line 131.

**So each change kind names one carrier relation, exact at hop 1 and empty at
hop 2:**

| change | carrier | diagnostic | coverage |
|---|---|---|---|
| add union variant | `match_sites` | E0200 | exact where the scrutinee resolved; `ABSORBS` is a verdict, not a gap |
| rename union variant | `match_sites` + `references` | E0103, E0220, E0200 | exact |
| rename record field | `field_access` | E0210 | exact for member accesses; record literals and object patterns `NOT_INDEXED` by name |
| rename declaration | `references` + `calls` | E0103 | exact |
| change arity | `references` + `calls` | E0213 | exact |
| change a signature type | `references` + `calls` | none when a named type is on either side | **`NOT_INDEXED`, never `SAFE`** |

That last row is the one worth reading twice. `takes_string(r)` with `r` a named
union produces no diagnostic, where `takes_string(b)` with `b: bool` produces
E0211. The checker reports only a provable mismatch and is silent on an
undecidable one, so an impact answer cannot call those call sites safe. It has to
say it did not check them (G201).

**The measured cost of getting this wrong.** Expanding `MENTIONS` from
`value::Value` in csvql, against the 8 declarations the compiler proves break:
depth 1 reaches 28 declarations (29% precision), depth 2 reaches 38 (21%), depth
3 reaches 48 (17%) and includes `main::main`. A `mentions` traversal three hops
from a cell type reaches the program entry point, and the compiler has proved
that 40 of those 48 do not break. That is what "everything connected to X" costs.

**`A -> B -> C` therefore gets no answer at all.** An impact answer is rooted at
one entity and one named change, and every edge runs from that root to a site
naming it. Whether `checkout` needs changing after `settle` is repaired is a
different question about a different root, and it does not exist until someone
decides how to repair `settle`. Widening the first answer to cover it would
report a consequence of an edit nobody has made as compiler-proven.

#### Six decisions the design phase surfaced, settled

**1. The request names the change, not just the entity.** `glyph_impact { entity,
relations, depth }` cannot assign `WILL_FAIL`, `ABSORBS` or `SAFE` to anything,
because those are consequences of an edit and no edit is named. Every entry would
come back `REFERENCES` and the release would be a lookup wearing a new name. The
shape is `glyph_impact { entity, change, relations, depth }`, with `change`
required for any answer carrying a consequence, and the change kinds closed from
the start: `add_variant`, `remove_variant`, `rename`, `change_arity`,
`change_signature_type`, `remove`.

**2. Verdicts are about consequence only.** `REFERENCES` was both a relation and
a verdict, so an entry could read `{"relation": "CALLS", "verdict":
"REFERENCES"}`, which is two facts wearing one word. `REFERENCES` and
`GENERATED_FROM` leave the verdict set; they were relations all along. The verdict
vocabulary is now `WILL_FAIL`, `ABSORBS`, `SAFE`, `UNDETERMINED`, `NOT_INDEXED`,
and the relation is a separate field. This also keeps relation names identical
across tools, which is what 0.1.113 is for.

**3. G201 is not fixed here.** Making `definitely_incompatible` decide named
against primitive would make the signature-type change exact and `SAFE` sayable.
It is also a new diagnostic on programs that compile today, so it is a breaking
change, and it turns this release into a checker release. The class ships
`NOT_INDEXED` and the gap is filed, the same way the object-literal class was
handled in 0.1.111. One change kind ships partial and says so.

**4. `mentions` with a depth does not ship.** Measured at 29% precision at hop 1,
21% at hop 2 and 17% at hop 3, where hop 3 reaches `main::main`. A relation that
returns candidates rather than consequences is a dump, and no caller has asked
for one. It waits for a caller who wants candidates and knows it.

**5. A hop-2 request is answered, not rejected.** The answer carries a
`next_query` naming the question that would be exact, rather than raising a tool
error. An error is louder and a field is composable, and the consumer here is a
program deciding what to ask next rather than a person reading a message.

**6. The relation vocabulary is reconciled before any of this is built.** G193
says a relation cannot be named in a reply until the set exists, and there are
currently two non-overlapping sets in the tree. One set, one spelling, fixed
first.

#### The verdicts, and why `UNDETERMINED` is the one to guard

The strongest property of this whole direction is not the size of the graph. It
is that **Glyph knows the boundary between what it knows and what it does not**.
Every verdict below is a statement about that boundary, and each means one thing:

| verdict | what Glyph is claiming |
|---|---|
| `WILL_FAIL` | it has enough semantic knowledge to prove this site stops compiling |
| `ABSORBS` | it can prove the change is absorbed here, silently |
| `SAFE` | it can prove this site remains correct under the change |
| `UNDETERMINED` | it indexed the relationship, looked, and cannot establish the consequence |
| `NOT_INDEXED` | it does not index the required semantic class at all |

`UNDETERMINED` and `NOT_INDEXED` are not two shades of "don't know". The first
means the compiler reached the site and could not decide. The second means the
question was never askable, because the class of fact it needs is absent from the
model. Collapsing them loses the only information a caller can act on: whether
looking harder would help.

**The failure to guard against is `UNDETERMINED` becoming a dumping ground.** It
is the comfortable answer for anything awkward, and once a few sites land there
for convenience an agent learns to read it as "probably safe", which is the exact
inversion of what it means. A verdict that cannot be trusted to be rare is not a
verdict.

So it is gated rather than documented. `tests/exact-or-absent/` carries a case
per verdict, each asserting the verdict is reached for its own stated reason and
not reachable for another's. A site that could be decided and comes back
`UNDETERMINED` fails the gate, in the same way a known-failing case that starts
passing fails it today.

**Both are hard gates.** If the propagation table cannot be answered precisely
with concrete programs, `glyph_impact` does not generalize yet.
- One entity in, nodes and labelled edges out, with a verdict per node from a closed set, never a similarity score and never "possibly related"
- **This is the release most likely to go wrong, and the failure has a shape: a specific semantic question turns into generic graph infrastructure, "impact" comes to mean everything, and Glyph becomes a graph database with a compiler attached.** The defence is that every edge stays compiler-derived. If the checker did not already compute it, it is not an edge
- **Transitivity is a hard gate, settled and written down before any code.** `A -> B -> C` has to mean one thing. Direct semantic consequence, transitive dependency, compilation consequence and runtime consequence are four different relations, and "potentially affected" is not a member of the set at all. A caller of a function that stopped compiling is impacted only if that function's signature changed, so transitivity is a property of the change kind rather than of the graph
- The verdict vocabulary is closed and each member means one thing:

  | verdict | meaning |
  |---|---|
  | `WILL_FAIL` | this site stops compiling when the change lands |
  | `ABSORBS` | this site keeps compiling and silently takes the new case |
  | `SAFE` | reached, examined, and unaffected by this specific change |
  | `REFERENCES` | names the entity without a compilation consequence |
  | `GENERATED_FROM` | derived artifact, with the source and content hash |
  | `NOT_INDEXED` | the compiler could not key this, named rather than counted |

- Measured against search alone on a real codebase, same agent and prompt on both sides, the losing run published

**0.1.113 — Shipped 2026-09-05 · Provenance, and the boundary as a node kind**
- G202: the playground's own build instructions pin `wasm-bindgen-cli 0.2.125` where the manifest pins `=0.2.127`, so the page cannot be rebuilt from its documented steps and a change to it is reviewed by reading rather than by running
- **G170 is the non-negotiable one and it is worse than a playground bug.** Two surfaces that emit TypeScript from the same Glyph must emit the same TypeScript. Today the CLI and the WASM playground assemble the front end separately and the playground emits with an empty `EmitContext`, so a program with a cross-module import can compile to different output depending on which one you asked. For a language that is not a rendering difference, it is two languages with one name
- R3: `glyph` / `extern` / `opaque-ts` as a node attribute, never part of the key, so exact-or-absent survives the first npm import
- `CALLS` distinct from `REFERENCES`, each answer saying whether the compiler proved it or a `.d.ts` asserted it
- R5 generated-from edges with path and content hash; R7 sorted, line-oriented, byte-identical serialization
- G170: **landed, as the second of the two.** The scan behind the six emit tables moved to `glyph_emit::ProjectTables`, so `glyph build` and the playground build their context with one implementation instead of two. That does not make the output identical and cannot: every table is keyed by the source module of an imported name, so all six answer questions about sibling `.glyph` files, and the playground holds one source string. The blocker is the missing files rather than salsa; the scan is a pass over parsed ASTs and takes no database. So the playground names the divergence where it exists (every non-`std`, non-`extern` import comes back in `assumed_external_imports`, and the page says what `glyph build` writes instead), and stays silent when there is none. The diagnostic half of the gap is fixed outright: `module_lints` runs, and severity is read off each diagnostic rather than assumed. Closing the emit half means a multi-file playground page, which is a product decision rather than a compiler one; `ProjectTables::from_modules` already accepts many modules

**0.1.114 — Shipped 2026-09-05 · The incremental debt, in the order it actually binds**
- **Each item earns its place against the question at the top of this file or it waits until after 1.0.** This is the release where engineering elegance is most likely to be mistaken for product value, and "this architecture would be cleaner" is not a reason to ship anything
- `type_map`'s span keying: **investigated and deliberately not done.** The premise written here was that every relation the agent work depends on sits downstream of `type_map`. That is not true of the current code, so rekeying it moves nothing an agent can hold and would have been the tidiness this release exists to refuse. The claim stays on the record as wrong rather than being quietly dropped
- **The key is `(SourceFile, Ident)`, not a literal `DeclKey`, and that is ratified rather than a shortcut.** `unsafe impl salsa::SalsaValue for DeclKey` is rejected by the orphan rule (E0117, verified by compiling it). More importantly a `ModuleId` is issued by the project-level interner, so a key carrying one would make every per-declaration memo depend on the project file list and go cold whenever a file is added, which is the exact staleness hazard `MatchCoverage`'s own doc comment describes. Forcing literal `DeclKey` would mean putting salsa back into `glyph-resolver` and undoing G164, for a coupling that makes memoization worse
- Rekeying the five position-keyed per-declaration queries to `DeclKey`: **earns it, barely.** No user-visible effect on its own, and it is the precondition for an identity that survives an edit, which is the whole first clause of the question
- The content-difference guard behind `set_file_text`: **earns it.** Raw `set_text` being public is how a byte-identical write forces re-execution, which is a correctness edge in the incremental layer rather than a tidiness one
- G171: measure what fraction of diagnostics land inside a function body, before committing to a local-level graph. Measuring first is the point, and if the fraction is small the local work does not happen at all

**0.1.115 — Shipped 2026-09-06 · The language server stops running a second compiler**
- **What this is not: routing the language server through the semantic graph.** The graph and the language server are peers, each a projection of the compiler's model, and neither is built on the other. The graph reads disk truth and answers an agent; the server reads buffer truth and answers an editor. Collapsing them would make an editor's unsaved keystroke a question for a disk-backed database, which is the wrong abstraction in the most expensive possible place
- What it is: the incremental analysis path for the server, with an overlay between disk state and unsaved editor state, so the server stops re-running an analysis the compiler already did
- Re-measured against 0.1.113: **17.78ms** per keystroke for diagnostics alone and **70.37ms** for an editor burst on `examples/apps/minilang/main.glyph` (2,205 lines), growing at **n^1.58**. The earlier figures were 15.4ms and 61.2ms at roughly the same exponent
- **The argument rests on the exponent and the structure, not on the 15% rise.** A wall-clock number measured in a different session on a different machine load is not comparable; the exponent is a ratio taken within one session and reproduced to within 0.05 on both segments. And the structure is verifiable by reading: `analyze` and `analyze_full` take a bare `&str` and re-run parse, symbol collection, prelude construction, resolution and type assignment on every call, so a hover costs the same as a fresh diagnostics run. `glyph_db` appears zero times in `glyph-lsp/src/lib.rs` and 21 times in `mcp.rs`. The second compiler this release exists to remove is still literally there
- **The method, so the next re-measurement is a command rather than an excavation.** Three sweep files at 535, 1,652 and 2,205 lines (`examples/corpus/json_parser.glyph`, `examples/apps/sheet/main.glyph`, `examples/apps/minilang/main.glyph`); a client speaking real LSP framing over stdio to `glyph lsp`; ten warm-up keystrokes then 100 measured across three server processes; a keystroke timed from `didChange` to the `publishDiagnostics` carrying the matching version. The burst's composition was not recorded with the original numbers and is reconstructed by ratio (61.2/15.4 = 3.97, four full-pipeline runs), which is inference and is marked as such
- Database lifetime is the real question underneath it: `glyph build` throws its database away and only the MCP server keeps one
- **That dependency was wrong twice over, and it was my own renumbering that made it stale.** The line was written in the same commit as the incremental-debt lane, when that lane was numbered 0.1.109. The lane was renumbered 109 to 110 to 111 to 114 across later revisions and this line was never touched, so it now points at the `std/array` folds release, which contains no keying work at all. `git log -S` on its text returns exactly one commit
- The real referent was refused in 0.1.114 on a finding this release inherits, and it is false a second time for this release's own reason: **`type_map` is not on the editor path.** It has one non-test caller in the workspace, in `glyph-cli/src/build.rs`. `glyph_db` appears zero times in `glyph-lsp/src/lib.rs` and 21 times in `mcp.rs`, and the editor's document store is a plain `Mutex<HashMap<Url, String>>`
- The overlay is strictly upstream of any keying: it decides what the `SourceFile` text input holds, and every memo hangs below that input

The overlay was built and first measured with a keystroke regression: 15.60ms rose to 33.39ms. The cause traced to the per-declaration fill layer `typed_file` drives, `decl_ast` cloning every declaration and copying its source bytes, `resolved_decl` cloning a sliced `ResolvedModule` per declaration. That layer pays for itself in a build, where its memos are read across files and revisions, but on a keystroke it was paid in full to save the 4.2ms of assignment it wraps.

The fill cost was fixed, `resolved_decl` fell from about 15ms per call to 0.29ms, and the overlay shipped unchanged. Measured back to back over the real protocol with `benchmarks/lsp-latency/measure.py`: the minilang keystroke went from 15.96ms to 13.78ms, the editor burst from 64.38ms to 16.97ms, and a repeated workspace-wide references from 13.96ms to 1.44ms. The full per-file table is in `benchmarks/lsp-latency/README.md`; json_parser's burst fell from 6.86ms to 1.93ms and sheet's from 38.45ms to 10.33ms.

The one number that moved the wrong way is the keystroke's growth exponent, which steepened from n^1.49 to n^1.64, because the smallest file gains 30% and the largest gains 14%. Every measured size is faster than before. Whether the exponent matters past 2,205 lines is not something this sweep can say.

- The server owns its own `CompilerDb` and never shares the MCP server's. Sharing would let an unsaved keystroke rewrite the input the agent's disk-truth answers are computed from, which is the collapse this lane refuses arriving through the back door
- The benchmark harness ships under `benchmarks/` rather than living in a scratchpad. This lane already records that the last measurement took an excavation to reproduce, and leaving the harness uncommitted is how that happens again
- A keystroke can be timed without any harness at all: `cargo test --release -p glyph-db --lib -- --ignored --nocapture timing_instrument_minilang` breaks a `type_map` fill into parse, resolve, per-declaration fill and assignment. It is `#[ignore]`d so CI never runs it, and it exists because the alternative is another excavation

**0.1.116 — Shipped 2026-09-06 · The checker accepts things it should not**
- The whole group is one shape: the compiler is silent where it could speak, and every one of them costs an agent the guarantee the language is sold on
- G201: **fixed.** `assign_incompatible` now flags a value of a declared union or record where a `string`, `number` or `bool` is expected; `takes_string(r)` with `r` a union is `E0211` where it used to be silent. The reverse direction, a primitive where a locally declared union or record is expected, is flagged too, excluding only the zero-field record `type T = { }` that TypeScript lets a string satisfy. Breaking, 0.1.114 exit 0 to exit 1
- G39: **narrowed, not closed.** The one defect position, an annotated module-level `const` lowering to `Ty::Unknown`, is fixed and breaking the same way (exit 0 to exit 1 under `check --no-tsc`, the LSP and the playground). The entry's headline surface, member access and calls on `string`, `number`, `bool` and `Array<T>`, is not a checker hole a patch closes: Glyph deliberately permits raw JS members there and every example compiles and passes `tsc --strict`. It is left to `tsc` for now (option C); modelling the member set per primitive is the stdlib-from-`.d.ts` question, Q21/Q40, and moves to the Deferred bucket below with that reason
- G169: **fixed.** A bare PascalCase head naming no variant of an imported union now escalates to `E0220`, the same as the module-local check. Breaking, 0.1.114 exit 0 to exit 1
- G135: **fixed.** The parser rejects a positional variant pattern on the rule (D8), sharing `E0010` with the declaration spelling, so the emitter's `E0300` can no longer disagree with it about whether the tuple form is coming. Not breaking: the program was rejected before and is rejected now
- `change_signature_type` in `glyph_impact` is decided per call site, as the payoff of G201. Each argument paired with a parameter is one cell of a table keyed on (argument kind, parameter kind), and a site is the weakest of its arguments: `WILL_FAIL` with `E0211` where the checker compares the pairing (a primitive against a primitive, a local union or record against a primitive, a primitive against a local union or a record with at least one field, a local named type against a local named type), and `UNDETERMINED` with a reason naming the missing rule where it does not (a type declared in another module, an argument the checker holds no type for, the zero-field record `type T = { }` that TypeScript lets a string satisfy). A function read as a value stays `NOT_INDEXED`, and the `CALLS` search names the class the relation does not hold, a return-type change reaching the position a call's result flows into
- G204: **fixed.** A module-level `const`'s initializer is checked against its annotation with the same `E0204` an annotated `let` gets. The corpus was swept first, 50 annotated consts, none flagged. Breaking, 0.1.114 exit 0 to exit 1
- G172: **decided. The path is authoritative and the header is not a key.** The decision has been owed since 0.1.106 and three releases worked around it

  It is also worse than the entry recorded. Two files claiming one header, in a project that compiles clean, make `glyph_variants` return a match site from the wrong file over an unrelated type as a **proven** edge, with `unkeyed` empty. That is exact-or-absent broken in shipped code, on the surface a field rename is driven from. Every gate case so far tests a site the compiler could not key; none tested one keyed to the wrong entity, which is why 23 invariants missed it

  **The fix needs no migration.** The compiler already treats the path as the key everywhere it builds and emits: a header that disagrees changes no emitted byte, is not a resolution target, and only anchors a diagnostic. Local type ends take the path key, which is the rule `exported_type` and `exported_fn` already document. Nothing that compiles today stops compiling, and all three wrong answers become correct

  **A disagreeing header becomes a warning, not an error, and only inside a marked project.** A hard error was refused on four verified false positives, and refusing it is the right call: the path key is invocation-dependent outside a marked project, not every path key is spellable as a module name (`module 01_validator` is E0001 and five such files exist here), the playground has no path at all, and the documentation gate would go 29 for 29 red because its harness names every snippet the same thing. Scoped to a marked project the warning fires on two files, both deliberate fixtures

  **A file with no `module` line defaults to the path key rather than erroring.** The parser makes it optional, the resolver's own comment calls it legal, and a headerless project builds clean. Making it an error is a separate convention decision and does not ride along here
- **Breaking, and it should be.** Each of these starts catching something, so somebody's green build goes red. That is the fix working, and the release marks it

**0.1.117 — Shipped 2026-09-06 · The seam with npm**
- This is the 1.0 gate rather than a feature: the recorded diagnosis is that Glyph is safe on code it owns and leaky at the seam with npm, and real projects are all seam
- **The design calls were put to an adversarial review before any code, and two of the three proposals did not survive it.** The review is on file; what follows is what was decided after it
- G91: **decided, twice.** Loosening `Option<T>` to accept `null` and bare values was rejected in review: `@open type Meta = { id: int }` with `{"m": {"tag": "None", "id": 7}}` decodes two ways, both valid against the declared types, and `type Tagged = { tag: string, value: int }` under `Option<Tagged>` has a valid document only a backtracking decoder accepts; the wire format of a field would depend on a property of a type declared in another file, which greppability forbids. The review's replacement, a `Nullable<T>` that is the same Glyph type as `Option<T>` and emits as `T | null`, then failed contact with the emitter: `Option` is a tagged object at runtime (`switch (x.tag)`), the checker cannot tell two spellings of one type apart, and `parse` validates and casts rather than decoding, so "into `Some`" has nothing to hang on. Making `parse` a decoder would split `is` from `parse` and stop a Glyph value round-tripping through its own `parse`; making the two types implicitly convertible is a type-directed emitter change nobody reviewed. **What ships:** `Nullable<T>` is its own prelude type, runtime `T | null`, emitted as `T | null`, with a validation-only check (`null` or `T`); `T` may not be `Nullable` or `Option`; the bridge to `Option` is explicit, `std/nullable` with `to_option`, `from_option` and `is_null`, one greppable call at the point null enters the program. `Option<T>` is untouched and the release is not breaking on this account. `gen dts` and `gen openapi` route a `nullable` schema field to it instead of collapsing it to an optional field. The entry's premise is narrowed too: an optional field of the base type has accepted `null` since 0.1.68, so what was broken is `null` tolerance and a matchable value in one field
- G108: **decided, against the first proposal.** A class or host type (`RegExp`, `Date`, `Map`) materializes as `type X = extern_ts("import('pkg').X")`, which resolves the name, lets `tsc` check every method call against the real class, and keeps `X.parse` absent so `E0304` still fires on a wire record carrying one. A presence-only descriptor was refused: it would turn a shipped `E0304` into `Ok` on a value nobody validated, which is the one thing a boundary must not do. Its `origin` is `extern`, per the definition `glyph_impact` already ships, not `opaque-ts`. `Promise` stays unmaterializable under D40 with a note naming the field. Evaluating `Omit` and `Pick` is deferred: the reader is syntactic and has no heritage-clause handling, so `Omit<Options, "silent">` over an interface that `extends` another materializes as an empty record whose strict descriptor rejects every real value, a clean build over a boundary that lies. It stays a note by name
- G208: every method signature `gen dts` drops gets a note naming the member. This is the silent half of G108 and the cheaper one
- G27: **closed on a re-checked premise.** All three import spellings of a stdlib member typo (`string.repeeat`, `import std/string { repeeat }`, `s.repeeat` through an alias) are `E0105` with a Glyph code, identical under 0.1.115 and this tree. What still leaks is a method on a value, `t.toUpperCasee()` on a `string` and `r.unwrapp()` on a `Result`, which is G39's member set and stays deferred with it
- G147: **fixed, and the lane's premise was wrong.** The `(module, union)` variant-name registry has existed since 0.1.96 and the entry's own program already built; what remained, recorded in the entry's 0.1.96 note, was an imported outer union whose variant carries a payload (`B(alpha)` over `tree.G`), which stopped at `E0305`. A second registry, `(module, union, variant)` to the payload's type, resolved in the declaring module's namespace, closes it in the CLI, the playground and the MCP surface at once since all three build `ProjectTables::from_modules`. Not breaking: 0.1.115 exit 1, this tree exit 0
- G203 and G206: **fixed, by ruling (D46).** A `type` whose body is a bare path resolving to another `type` declaration is a second name for it: `local_type_decl`, `named_record_fields` and `has_descriptor` follow the chain, so a `B` passes where an `A` is declared (0.1.116 exit 1, now 0), `b.naem` is `E0210` in one module as it already was across two (0.1.116 exit 0, now 1), and `B.parse` is `A.parse` at run time through an emitted `const B = A`. A body of any other form is not followed and decides no new incompatibility; the corpus holds zero bare-path aliases and six `Record<string, T>` aliases. Two edges stay open: a cross-module alias of a descriptor type still falls to the presence floor for a field typed by it, and an alias of a union is still not exhaustiveness-checked. The `change_signature_type` description in `mcp.rs` now says "compared by the declaration each name resolves to". "Loosening only" was wrong in one direction: the field check turns silence into `E0210`, and G201's rule now reads the declaration an alias names, so an alias of a union passed as a `string` goes 0 to 1 under `check --no-tsc`; both are recorded
- G207: **fixed.** A `where` over an alias that resolves to a record or union is `E0300` like the direct spelling; the emitter reads the shape the base resolves to. Breaking: `type Positive = Rec where value.x > 0` with `Rec` a record is 0.1.116 exit 0, now exit 1. `type PosCents = Cents where value > 0` with `Cents = int` is unchanged
- G205: **fixed.** `glyph_db::exported_const`, the sibling of `exported_fn`, reads an annotated `pub const`'s annotation across the module boundary, so `ORIGIN.rowz` and `lib.ORIGIN.rowz` are `E0210` naming the record. Breaking: 0.1.116 exit 0, now exit 1. The negative-test runner accepts a directory of modules now, since a two-module case cannot be one file
- G209: **fixed**, found while closing G108. Every declaration `gen dts`, `gen zod` and `gen openapi` write is `pub`, so the guide's `import types/api { Doc }` flow works for the first time; against 0.1.115 it was `E0105`
- G210: **found and recorded, not fixed.** `E0304` fires only in the module that declares the record; `Doc.parse(v)` from a sibling that imports `Doc` reports nothing
- G208: **fixed.** Every interface member `gen dts` drops gets a note naming the owner, the member and the reason
- **Breaking, and it should be.** G205, G206, G207 and the alias hop under G201 each turn a silent program red; G91 and G147 only accept more. The release marks it
- Measured on a real dependency rather than a fixture, because every one of these was found that way

**0.1.118 — Shipped 2026-09-06 · The stdlib gaps real programs hit**
- **The three design calls went to an adversarial review before any code, and all three changed.** The review is on file. Three facts it established decide most of it: no stdlib type is tracked by the `owned` check (a stdlib type carries a sentinel symbol the resource lookup cannot find, and a namespaced call is never a consume, which is why `examples/apps/discord/gateway.glyph` declares a local `resource type Wire = Socket`); the checker has no generic stdlib type (`Option`, `Result` and `Nullable` are prelude names with dedicated builders); and the emitter's union registries are filled only by scanning project Glyph, never the hand-written stdlib
- G101: **fixed, and decided against `Step<A>`.** A new generic stdlib union would be the first of its kind and the checker has no generic stdlib type: `match s { Continue(a) => a }` over a `Step<int>` would be a two-variant match with one arm and no diagnostic, which is G143 and G147 reintroduced through the stdlib door. What shipped: `array.fold_while(xs, init, f, done)`, stopping before the next element when `done(acc)` is true, and `array.try_fold(xs, init, f: (acc, x) -> Result<A, E>) -> Result<A, E>` over the prelude `Result`. Both are typed through the existing signature table, so a `done` returning the wrong type and a `try_fold` result matched without an `Err` arm are diagnosed. The alpha-beta test prunes: 4 of 6 nodes evaluated where `fold` visits all 6. A `Step` type, if still wanted, is a prelude proposal with its own review. Not breaking
- G105: **fixed, with two signatures changed and one claim struck.** `std/fs` gained `open_lines(path) -> Result<LineReader, FsError>`, `next_line(r) -> Result<Option<string>, FsError>` and `close_lines(r)`. `Option<string>` alone would have reported a disc error at line 400,000 as end of input, a silent truncation at a boundary, and the mutation that made it do so was caught by opening a directory: `Ok(None)` where `Err(IsADirectory)` was due. `next_line` closes the descriptor on `Ok(None)` and on `Err`; `close_lines` is idempotent and a closed reader reads as ended. Synchronous, chunked `readSync` into a `StringDecoder` per handle, the way `io.read_line` already works on fd 0; the module header says it blocks and is not for a program that also serves. The shim gained `openSync` and `closeSync`. The proposal's sentence "an `owned` resource so a dropped reader is E0206 like a socket" was false and is struck: no stdlib handle is tracked, `Socket` included. No new iteration construct: D21 has two loops and `for await` is a third, a spec amendment, and the streamed HTTP body has the same limit untouched. Not breaking
- G128: **fixed, option (b) with three corrections and one ruling the review did not foresee.** Every HTTP verb goes through `fetch_of`, which sets a 30 000 ms deadline, so all six (`get`, `post`, `put`, `patch`, `del`, `head`) are bounded and the number is written once; a source-scan test fails if a verb stops going through it or a `timeout_ms: 0` literal reappears. An explicit `timeout_ms: 0` in a `Fetch` sent through `send` keeps meaning no timeout, the one greppable opt-out. `http` gained the `MAX_TIMEOUT_MS` guard `tls` has, so a deadline past `setTimeout`'s clamp is refused rather than aborting in one millisecond and blaming a deadline that never elapsed. The guard's `Err` raised a fork: `HttpErrorKind` was the closed union `"timeout" | "network" | "status"`, and filing a rejected argument as a network failure would be the confident wrong answer the guard exists to remove. Ruling: a fourth kind, `"argument"`, the request was never issued because an argument was invalid. That is breaking under D30: a `match e.kind` over the three old kinds with no `else` is 0.1.117 exit 0, this tree exit 1 with `E0200`. Two corpus sites gained the arm (`linkcheck`, `resilient`). G127's "a bound nobody passes is a bound nobody has" argued against optional; a default is a bound somebody has and nobody chose, visible and recoverable as `kind: "timeout"`, where the absent bound hangs
- **Breaking, established by running both binaries.** The `"argument"` kind turns an exhaustive match over `HttpErrorKind` red (0 to 1). G212 accepts more, G101, G105 and G211 change no verdict. The release marks it
- G212: **fixed**, found by the review. Resource-ness follows the alias chain: `direct_type_decl`, `alias_target` and `resolve_alias_chain` became crate-level functions so `named_is_resource` in `owned.rs` reads through the same walker the rest of the checker uses, and there is one symbol-table read site. `let owned b: H` with `type H = Handle` is accepted and tracked, a drop through the alias is `E0206`, and an alias of a non-resource is still `E0205`. 0.1.116 exit 1, this tree exit 0; accepting direction only
- G211: **fixed.** The emitter recorded a source-map checkpoint at the top of every declaration and statement except the last statement of a block, which `emit_tail_stmt` emitted without one, so a tsc error there matched the previous statement's checkpoint. `emit_tail_stmt` checkpoints now, through one helper that drops an identical consecutive pair so the map stays strictly increasing. The ledger program reports at `main:8:3`, the call, rather than `main:7:3`, the `let`; a tail expression in a `match` arm maps the same way. Not breaking: the position changes, the verdict does not
- G119: `url.join`'s `Err` branch is nearly unreachable and nothing says so; the fix is documentation
- G178: eleven modules are shipped and importable with no documented signatures

**0.1.119 — Shipped 2026-09-08 · The house we live in**
- Four entries about this repository rather than the language, and they share a failure: a thing that cannot be run is reviewed by reading
- G202: **fixed.** The manifest is the one source: the README says 0.2.127, `build.sh` reads the pin out of `Cargo.toml` and refuses with the exact install line when the installed cli differs, and `check_playground_pin.py` fails if the README, the script or the Pages workflow drifts from the manifest
- G159: **fixed, and the premise was narrower than recorded.** `slugify` and `load_feed` did not compile as stated, and `parse_user` compiled only without tsc. All three now pass `glyph check` with tsc and `check_benchmark_fixtures.py` keeps it so. The numbers moved: Glyph is 436 tokens to TypeScript's 478 on the three functions, not 341, about 9% fewer rather than 29%, and level with Python; `benchmarks/FINDINGS.md` and the README say so, because a density claim measured on programs the compiler rejects is not a measurement
- G158: **fixed.** 80 files under `examples/` reformatted, every app and corpus file checked before and after with identical exit codes and diagnostic counts, a second pass changing nothing, and `check_fmt.py` in the release gates and CI so the corpus stays what `glyph fmt` produces. No formatter bug surfaced
- G166: **fixed, with the mechanism.** The test program's exit condition counted reads, and TCP has no message boundaries, which is the very fact the test checks: under load the two halves of the split character arrived in one read on one side or the other, `seen` stopped at 1, and the program waited on an idle socket forever. Reproduced both ways (a 0 ms gap hangs every time; eight copies under a 32-process load hung 5 of 8) and fixed by making the client send the second half only after the first echo is back. Every `glyph run` the integration suite spawns, 24 sites, now runs in its own process group under a 120 s deadline that kills the whole tree and reports the test, the command and the output so far; 8 of 8 complete under the same load
- G177: **fixed.** E0101's help names the three true spellings: a stdlib module by its `std/` path, a sibling by its bare name, a subdirectory file by its path from the root. Closing it found G223: no program reaches E0101, because a relative import stops in the parser as `E0002` with a generic help; scheduled with the diagnostics work

**Deferred, with the reason rather than by omission**
- G18, G19, G20, G98: language ergonomics. `T?` sugar, nested strings inside `${...}`, `is` narrowing, formatter layout. Each is a design decision and each is forward-compatible by construction, so deferring costs nothing that adding them later would not also cost
- G78: a multi-module app cannot be built as part of an enclosing tree. Waits on the same question G172 asks, which is what a file's address is
- G170: the emit half needs a multi-file playground page, which is a product decision rather than a compiler one. The compiler side is done and `ProjectTables::from_modules` already takes many modules
- G171: measured in 0.1.114 and the conclusion held on a corrected argument. It stays listed only because nothing has been built on it; there is no work owed
- G39 (member set): member access and calls on `string`, `number`, `bool` and `Array<T>` are unchecked against `Ty::Unknown` in `glyph check --no-tsc`, the LSP, the MCP server and the playground. Not a checker hole `tsc` lets through; the four surfaces just don't have `tsc` in the loop. Modelling the member set per primitive is the stdlib-from-`.d.ts` question, Q21/Q40, and waits on that architecture decision rather than growing the hand-written table further

**0.1.120 — Shipped 2026-09-08 · The emitter consumes facts, it does not rediscover them**
- **The principle, stated once.** The emitter must consume semantic facts computed by the resolver and the typechecker; it must not independently rediscover language semantics. The rule is recorded in `04-transpiler.md` beside the decisions it refines. Three releases in a row found the cost of breaking it: G147, where the emitter's own variant-list rule could not see an imported payload the checker knew; G207, where the `where` refusal grew a second alias walker inside the emitter instead of reading the checker's; and G212, where a pass read a type name the rest of the checker had already resolved through an alias. Each was one spelling with two answers, and each was found by an outside reading rather than by a test, because a test written against the emitter's own reading passes
- **What the audit found (G214), and what shipped.** 49 rediscovery sites in six families. The three changes landed in order and each one uncovered a hole of the same family in the checker, which is the argument for the principle in one line:
  1. **`decls: &dyn DeclTyResolver` on `EmitContext`.** `build.rs` passes the `SalsaDeclTy` the checker already uses; the playground and the test harness pass a unit resolver, which is the previous single-module behaviour. Four AST-only registries and the spelling-based `ModuleTypeNames` are gone. Landing it found G224: a match over a local alias of an imported union was not exhaustiveness-checked at all
  2. **One alias chain, and fields decided from the resolved type.** The checker's `direct_type_decl`, `alias_target`, `resolve_alias_chain` and `split_type_app` are public and the emitter reads them; four private walkers, six `direct_type_decl` copies and six open-coded `Ty::App` unwraps are gone; `field_check` and `is_check` decide from the lowered `Ty` and the resolution rather than a name's last segment, which closes G213, a user `type Option<T>` receiving the prelude's runtime check. Crossing the module boundary in the chain lost one true diagnostic (G226) and exposed G225, a namespace import shadowing the prelude `string` so `let x: string = 5` went silent
  3. **The checker's arm classification, recorded and read.** `TypeMap` carries, per `Pattern::Ident` node, whether the checker read it as a variant reference or a binding; the emitter asks and refuses a node the checker never classified rather than guessing from capitalisation. The four `is_variant_shaped` rules are gone and the name has no use left in the emitter. The synthesized grouping binding is the one span the checker never sees and is emitter-owned, with the soundness argument at the rewrite
- **G215, G226 and G225 are closed here, not deferred.** `assign_incompatible` canonicalizes both sides and decides an imported declaration the way it decides a local one: an imported union or record against a primitive, a primitive against an imported union or fielded record, and identity between declarations, with every reading the local rule leaves undetermined left undetermined. A namespace binding in type position lowers to the prelude type it spells. The two `signature_type_cell` arms that shipped `UNDETERMINED` citing G201 now answer, verified by running each of eleven programs through the checker before the cell was written
- **The evidence that no program changed.** Every app at its own root plus the corpus emitted before and after each change, 684 TypeScript files across 32 roots, `diff -r` empty every time
- **What it does not do.** No emitted TypeScript changes for a program that compiles today, and the suite's emission snapshots are the gate for that. This is not the IR decision reopened: the visitor stays a visitor, it stops guessing

**0.1.121 — Shipped 2026-09-11 · One call describes a symbol, and the compiler stops passing what it should refuse**
- **What the release is.** The first of the three agent-surface priorities, measured by the audit's own probes re-run on the three-module project it used: `glyph_hover` answered two of fourteen positions on the 0.1.120 binary and answers thirteen here, `glyph_variants` handed back variant names and now hands back each payload with the syntax that constructs it, `glyph_symbols` handed back a name, a kind and a range and now carries the identity, `pub`, the kind in `glyph_symbol`'s vocabulary and a one-line signature. Beside that, three programs the checker used to pass are refused
- **G216, a prelude container where a primitive is declared.** `prelude_container_incompatible` sits in `assign_incompatible`, beside the imported rule and on the same canonical types, so the `let`, the `const`, the return and the argument checks all reach it. Forward, `Option`, `Result`, `Array`, `Record` and `Nullable` are never a `string`, a `number` or a `bool`, whatever their arguments; backward, four of the five are decided the same way and `Nullable<T>` is decided against `T`, so `let v: Nullable<int> = 3` stays correct and `let v: Nullable<int> = "three"` is `E0204`. Two binaries on the ledger's own programs: `let o: Option<int> = Some(3)` then `let x: string = o` exits 0 under the published 0.1.120 and 1 here with `E0204 expected string, found Option<number>`; `let v: Nullable<int> = 3` then `takes_int(v)` exits 0 under 0.1.120 and 1 here with `E0211 expected number, found Nullable<number>`. The second is the one tsc cannot make, because the emitted `const v: number | null = 3` narrows back to `number` before the call is checked, and `catches/nullable-into-int` runs both halves to show it. `ty_display` prints an application's arguments now, because "expected `number`, found `Nullable`" names the container and drops the thing the reader has to check. The readings the rule declines are enumerated with a test each rather than left to be rediscovered: `Schema<T>`, `Component<P>` and `Issue`, whose shape comes from a library; `infer_output<S>`, a type-level operator whose result may be a `string`; a user's own generic, whose body may be its argument; a container against `void`; and an alias of a container, since `alias_target` does not follow a body that is a generic application
- **G217, a type's name where a value is expected, is `E0228`.** `return Order { id: "a", total: 1 }` is the TypeScript-adjacent guess for constructing a record, and what the compiler said about it was `E0108 lint: unreachable code` on an exit-0 build. It now reads `` `Order` is a type, not a value; a value is the record literal `{ id: ..., total: ... }`, with the type on the annotation ``. The message is read off the lowered body, so a record lists its fields, a tagged union its variants with their payload shapes and a string-literal union its literals. It is raised for a local type, a named import the sibling declares as a type and a prelude type name; `Order.parse` and `Order.is` stay legal, exempted by a `member_object` span set the way `assign_target` is. The unreachable-code warning went away without code: the lint tier runs only for a module that produced no errors, so raising the error takes the warning off. `docs/error-codes.md` and `glyph --explain` carry the code
- **G218, `glyph_symbol`.** One call, addressed by `module::name` or by a position, answering with the symbol's kind, its identity, whether it is exported, its generics, a record's fields with types, a tagged union's variants each with its payload and construction syntax (`Paid({ transaction_id: string })`), a string-literal union's values, a callable's parameters and return, an interface's members, whether a `match` over it must be exhaustive, its `@example` text, and its origin. Every fact is a pair: a null carries a `<key>_absent` sentence saying why, so a caller never has to tell an omission from a symbol that has none. Two of those sentences are the point of the rule rather than decoration. An unannotated `const` reports no type, because the checker lowers its declaration to `Ty::Unknown` on purpose (G39) and printing the initializer's type would say the uses are checked against it when nothing checks them; `exhaustive_match` has three answers rather than two, because `false` on an alias would be a caller dropping an arm believing nothing stops it. `hover_at` reads a declaration name, a parameter, a written annotation, a variant and a binding's definition site beside the checker's expression table, and the language server gets the same. `glyph_definition` carries the `module::name` so an answer chains into `glyph_impact` without re-deriving it, and `glyph_symbols` answers from the project database rather than from a parse-only walk, with an `interface` reported as `interface` rather than as `type`
- **G219, `glyph_diagnostics` reads the project the file lives in.** The tool ran the single-file front end on the text it read, so anything needing another module was invisible: on the audit's two projects the published 0.1.120 answers `[]` for a wrong field on an imported record, and for a non-exhaustive match over an imported union it answers worse than nothing, two unused-import warnings naming the two variants the missing arms would have named. It now reads the same project database `glyph_impact` and `glyph_variants` use and answers with `glyph check --json`'s own `Diagnostic`: the type moved to `glyph_lsp::diagnostic`, the lowest crate both surfaces reach, and `glyph-cli` re-exports it, so there is one Rust type and no second assembly of the JSON. The gate is `one_diagnostic_has_one_shape_on_both_surfaces` in `glyph-cli/tests/agent_loop.rs`: six projects, one diagnostic each from parse, import, lint, typecheck twice and emit, compared as serialized JSON key for key. The reply is an envelope, `{path, module, project_root, member, member_detail, diagnostics, unindexed, not_run}`, because what the check could not read is part of the answer. An MCP client that parsed the old bare array reads the `diagnostics` key now
- **G221, the assignability half.** `glyph_assignable(path, from, to)` takes each side as a `module::name` or as a Glyph type spelling and answers three ways. `WILL_FAIL` carries the codes the pairing draws, `E0204` at an annotated `let` or `const` and at a `return`, `E0211` at a call argument. `COMPATIBLE` is claimed only where a total rule accepts: equal primitives, one declaration against itself, two applications compared by arity and base and argument, width subtyping on records, and a scalar against `Nullable<T>` decided against `T`. `UNDETERMINED` is no rule covering the pairing, with the reason naming what is missing. `glyph_dependencies` and `glyph_exports`, the other two questions in G221, are 0.1.122
- **The two `glyph_impact` cells the container rule made answerable.** A container argument against a scalar parameter and a scalar argument against a container parameter both fell to `signature_type_cell`'s final arm, whose reason said the checker has no rule, and that sentence had stopped being true. Both are `WILL_FAIL` with `E0211` now, with `Nullable<T>` decided one level in against `T`. `void` stays `UNDETERMINED`, as every shape rule in the checker leaves it. Six programs were run through the checker before the cells were written
- **`glyph query <tool>`.** Nine CLI verbs, `symbol`, `diagnostics`, `hover`, `definition`, `references`, `variants`, `impact`, `symbols` and `assignable`, each taking its tool's arguments as flags and printing the tool's JSON on stdout, exit 2 with the reason on stderr on a refusal. They route through one entry point in `glyph_lsp`, the same `call_tool` an MCP client reaches, and the integration test asks both surfaces the same question and compares the answers as text, so a verb that reformatted or summarized would fail it. The server serves nine tools
- **What it does not do.** No write tier: nothing here renames, applies an edit or fixes anything. `glyph_dependencies` and `glyph_exports` are not built. The diagnostics tool does not run `tsc` and does not raise `E0104`, and it says so in `not_run` rather than answering as though it had. Hover on a field read from an imported record answers null (G227) and the impact table still disagrees with `glyph_assignable` on a container against a container (G228); both are scheduled for 0.1.122 rather than absorbed here
- **Why `COMPATIBLE` and not `SAFE`.** `glyph_impact`'s verdict vocabulary already has `SAFE`, and it means "this site stays correct after the edit you named". `glyph_assignable` is asked about two types with no site and no edit in the question, so reusing the word would have made one vocabulary answer two different questions. The relation underneath answers a bool whose `false` means "not refused" rather than "accepted", which is the second reason the word had to be new: `glyph_typechecker::assignability` enumerates the seven rules that accept in `accepting_rule`, and every pairing outside them is `UNDETERMINED` however permissive the relation was. Two function types are the shape of it, since the relation compares the returns and says nothing about the parameters; `tests/exact-or-absent/assignable` holds that case
- **Why a type spelling is resolved by writing a file rather than by reading it.** Both sides of `glyph_assignable` are resolved in the scope of the file at `path`, by writing that file's text plus two type aliases into a scratch database input and running the real parser, resolver and lowerer over it. A resolver written in `mcp.rs` would be a second reading of what `Nullable<int>`, a second name and a namespace-qualified path mean, and the two would disagree the first time either was extended, which is the 0.1.120 principle applied to the agent surface instead of the emitter. The probe is never a project member, so it resolves imports as the anchor file does and answers nobody else's, and a test asserts the anchor file's own diagnostics and the project's symbol list are unchanged by a call
- **`glyph-lsp` depends on `glyph-emit` now.** `project_file_diagnostics` runs the compiler's stages in the compiler's order, and the emitter is one of them, so an `E0305` duplicate match case is reported by the tool the way `glyph check` reports it. Without the dependency the tool would have answered clean on a program `glyph build` refuses, which is the disagreement G219 exists to remove
- **Why `E0104` is named rather than raised.** Telling a mistyped sibling import from an installed npm package needs the build's view of `node_modules` and `package.json`; with no such view the resolver's own answer is `Unknown`, and `Unknown` reports nothing. Three ways out, and the third shipped: give the server that view, which puts package resolution in a surface that reads a project without writing one; report every import the project walk cannot place, which is wrong on every installed package and so trains a caller to ignore the code; or name the check in `not_run` and let `glyph check` be the surface that runs it. The first is worth reopening when the server needs `node_modules` for something else, and not before
- **The measurement.** Every item above was re-measured by running the audit's probes against the 0.1.120 binary and this one in the same worktree, and the answers are in the ledger entries rather than in this summary. The suite is 1625 tests, from 1542; the exact-or-absent corpus is 31 invariants, from 28
- **Breaking**, in the checking direction. The three programs under G216 and G217 exit 0 under the published 0.1.120 and 1 under this tree, each established by running both binaries rather than by reading the diff

**0.1.122 — Shipped 2026-09-15 · Diagnostics as a repair protocol**
- **What the release is.** A Glyph diagnostic used to be a sentence with a code on it. It is an object now: the two types, the symbol at fault, the finite list of things that could legally stand where the wrong thing stands, the union's whole variant list, the file, and the pointer at the code's own explanation, on every diagnostic that has them and as an explicit `null` on every diagnostic that does not. On top of that object sit three things an agent uses in order: `glyph check --agent` says what an edit repairing the diagnostic must not break, `glyph --explain <CODE> --json` hands over the code's explanation and a compiled wrong program that draws it, and `glyph fix` writes the repair where the compiler settles it on its own. Beside the protocol, three checker and parser holes close and two more tools ship
- **G220, the fields.** `expected`, `actual`, `cause`, `alternatives`, `related`, `module`, `file` and `explain` are populated from the error variants that already carried them by name rather than re-derived: E0200 gets `cause` and `related`, E0202, E0205 and E0304 get `actual`, E0203, E0204 and E0211 get `expected` and `actual` with `alternatives` on the last two, E0210 gets `cause` and `alternatives`, E0220 and E0105 get `alternatives`. `file` is a path relative to the resolution root, which is the root the module keys are counted from, and `check --json` and `check --agent` name that root in `project_root`, so the string opens with no second question about what it is relative to. It did not, until a review pointed out that the flagship agent command was the one surface handing back a relative path with nothing to join it to while `glyph query diagnostics` carried the root. `project_roots` names them all when a tree holds several projects (D41) and `project_root` is `null` there. `cause` on E0211 is `null`, and that is a fork left open rather than an omission: the symbol at fault at a call argument is a parameter, `module::fn.param`, and the identity vocabulary has no spelling for one. Writing a key nothing else accepts would be worse than the null, since `cause` is what an agent feeds back to `glyph_symbol`
- **`glyph --explain <CODE> --json`.** The code, its title, the long explanation, the `help`, the docs URL, and a counter-example taken from `tests/negative/`, `tests/negative/nesting_too_deep/` among them, because the one code nobody had seen before was the one with no wrong program behind it: the wrong program's source, and the diagnostic this compiler draws for it, built when the question is asked rather than stored. `corrected` is `null` beside a sentence saying why: the corpus pairs no repaired program with a case, and a fix invented at answer time would compile nowhere and be checked by nothing. A code the corpus does not cover gets `counter_example: null` and the matching sentence, so the corpus claims nothing about the codes it does not hold
- **One root, on every surface.** On a `src/` tree with no `package.json` the compiler contradicted itself inside one invocation: `glyph check <dir>` counted modules from the directory and reported `src/main`, while `glyph check <dir>/src`, `glyph check <file>` and every tool counted from `src` and reported `main`. So `symbols` was empty on every diagnostic of that layout, with `glyph_symbol` explaining that the project holds `main` about the file the same process had just keyed `src/main`, and `glyph fix` declined every repair on the shape `glyph init` scaffolds. An unmarked directory gets the same source-directory rule a marked one gets now: `src/` when it exists, else the directory itself
- **`glyph check --agent`.** The `--json` object with two keys added per diagnostic. `constraints` are the invariants an edit repairing this diagnostic has to keep, and they are stated only where the compiler holds one: E0200 says to preserve exhaustiveness and names the missing cases, and says not to write an `else` when the union is declared in this project, because a catch-all forfeits the guarantee that adding a variant later forces this match to be updated. E0204 and E0211 say not to cast and name the type the position requires, and where the compiler holds a form a value is written in they name that too, taken from `glyph_symbol`: the `construct` spelling for a tagged union (`Green({ hex: string })`, since a variant with a payload is a constructor and not a value) and the quoted literal for a string-literal union (`"read"`, not `read`). An earlier cut printed the bare `alternatives` list for both and an agent following it wrote `takesColor(Green)`, which is `TS2345`; where neither form is available the sentence is not written at all; E0210 says the record's fields are closed and lists them; the `owned` codes say to keep the consume. A code with no such invariant gets an empty list rather than a sentence written to fill it. `symbols` is the `glyph_symbol` answer for every symbol the diagnostic names, and `symbols_absent` carries the tool's own reason for each one it refuses, so "we did not look" and "we looked and it is not there" stay apart
- **`glyph fix` grows three repairs, and every refusal says why.** E0200 writes one arm per missing case, with the pattern taken from `glyph_symbol` (a record payload destructured by field name) and a block body that prints the case and leaves through `process.exit(1)`, under a `// TODO(glyph fix)` line naming the case, importing `std/process` when the module does not already reach it. E0220 takes the checker's suggestion when there is exactly one and it names a case the match is actually missing. E0210 takes the one declared field within edit distance one of the typo, and declines when two are. The arms compile and are written to be replaced
- **Why the arm body leaves through `process.exit`.** It is the one body that is legal whether the `match` owes a value or not. `process.exit` returns `never` (D47), which contributes nothing to the arm join, so the same text works in a statement `match` and in a value-position one and the rule needs no guess about what the arm should produce. The obvious alternative was measured rather than reasoned about: the same arm written `assert(false, "unhandled")` in a value-position match returning `string` fails `tsc` with `TS2322: Type 'void' is not assignable to type 'string'`, so it would have been a repair that turns one error into another
- **G221, the rest.** `glyph_dependencies(entity)` answers what one declaration depends on, every declaration its own source names split by `CALLS`, `REFERENCES` and `FIELD_ACCESS`, each edge with both ends and its provenance. `glyph_exports(module)` answers what a module makes visible, from `glyph_db::module_exports`, every `pub` declaration and every variant a `pub` union hoists, each as a `glyph_symbols` entry. Eleven tools, and `glyph query dependencies` and `glyph query exports` beside the other nine verbs
- **The provenance bug the two tools found.** `symbol_provenance` answered `PROVED` for an edge whose other end is in a module that does not parse, which is exact-or-absent broken: nothing was proved, the module was never read. It answers `UNDETERMINED` now with the reason, and `glyph_references` and `glyph_impact` were reading the same function, so both were wrong the same way. Found by writing `glyph_dependencies` against a project with a broken sibling, which is the argument for building the mirror of a relation rather than only one end of it
- **G228, the two impact cells, decided by the same call `glyph_assignable` makes.** A container argument against a container parameter and a structural record against a structural record both fell to `signature_type_cell`'s catch-all, whose reason said the checker has no rule, and the checker had one in each case. Neither cell is a sentence written into the table: both ask `glyph_typechecker::assignability` on the two types the site holds, which is the call the assignability tool makes, so the two tools cannot answer differently about one pairing. `ParamKind` gains a `Structural` member so the record cell is reachable
- **Why the word is `SAFE` and not `COMPATIBLE`.** The impact table answers about an edit at a site, and `SAFE` is its existing word for "this site stays correct after the edit you named". `glyph_assignable` answers about two types with no site and no edit in the question, and `COMPATIBLE` is its word for that. The two cells carry a tension worth naming rather than hiding: every other cell in the table describes a hypothetical replacement type, and these two describe the pairing actually in hand, decided by a rule that is total over the shape, so a replacement the rule does not accept is `E0211` at that site. The JSON names that rather than leaving it to be reconciled: a `WILL_FAIL` site carries its own sentence, and each pairing's `SAFE` reading stays in `arguments` where it is about the types in hand
- **G227, hover answers fourteen of the audit's fourteen positions**, from thirteen. The predicted mechanism was wrong and worth recording: the entry said the member-access expression on an imported record was missing from the `TypeMap`, and it was not. The tool was reading the file without its project. Building the project is what answers all four of the positions the 0.1.121 note listed, not just the field read. The editor's own hover is not fixed by this and is a new finding (G235): `glyph lsp` registers the buffers an editor opened rather than a project, so `textDocument/hover` on an imported name still answers `null` where the tool answers
- **G229, a nesting-depth limit of 64 in the parser.** Recursive descent with no guard, so 2,000 nested brackets ended the process with `fatal runtime error: stack overflow, aborting` and exit 134 on `glyph check`, `glyph fmt`, and by the same file walk on `glyph lsp` and `glyph mcp`, where one pathological file killed the server for the whole workspace. Both nightly fuzz targets had failed on it since 2026-09-10; both ran clean for two minutes afterwards. The `Cursor` carries a depth and every recursion entry point that can nest goes through one helper, in expressions, types, patterns, blocks and JSX children alike, so past the limit the parser returns `E0011` at the token that would have opened the next level
- **Why 64, measured rather than guessed.** One level of the costliest entry point, a nested array literal, costs 7,792 bytes of stack in release and 8,810 in debug on x86-64 macOS, found by binary-searching the smallest thread stack a given depth parses in. The thinnest stack the parser runs on is a spawned thread's 2 MiB, which is what the language server's tokio workers get. At 64 levels the debug build spends about 580 KiB, under a third of that. The deepest nesting across the 343 `.glyph` files in this repository is 16
- **G230, a string-literal-union annotation refused nothing.** `type Mode = "read" | "write"` declares a finite set of strings (D30), and the checker compared a value against it by nothing at all, so a number, a `bool`, a record and an out-of-set literal all passed `check --no-tsc` while `tsc` refused every one. The hole was Glyph's own relation, which is the only thing `--no-tsc`, `glyph lsp`, `glyph mcp` and the playground have. The rule reads the union as what it is at run time, a `string`, and asks the relation the corresponding `string` question in each direction rather than re-deriving which values are records and which are containers. Two literal sets are compared by set, an alias chain is followed on both sides (D46), an imported union is decided like a local one, and `Nullable<T>` is descended with the union intact. The rule stops at the generic-argument boundary, and stopping there is the fix an adversarial review forced before the tag: the first cut recursed into `Ty::App` arguments and refused `fn arr() -> Array<Mode> { return ["read", "write"] }`, which `tsc --strict` compiles, because D30 types every string literal `string` while TypeScript reads the same expression as `Array<"read" | "write">`. A bare `string` inside a generic argument is left to `tsc` now, and so is a call to a generic function that returns its own type parameter; a `number`, a `bool`, a record, a tagged union and an out-of-set literal are refused there exactly as they are against `Mode`. 93 app and corpus entries were checked at their own roots before and after with identical exit codes and identical diagnostic-code lists, and because that sweep can only find newly *caught* programs, thirteen correct programs that write a string-literal union in every position were run under the published 0.1.121 with `tsc` in the loop and are now a corpus in the suite (G237 is the widening that would decide the fenced shapes instead)
- **Two smaller things the same review found.** `glyph fix` writes a module-level `import std/process` for the arm bodies and an import list it extended, and reported neither; both are in the `Applied` line now, because the command's own output is what a CI log or a review captures. And `glyph fmt` printed a Rust `Debug` dump for a file that does not parse (`NestingTooDeep { construct: "array literal", limit: 64, span: Span { start: 87, end: 88 } }`) where `glyph check` prints `[E0011]` with the caret and the help; it prints the same report `glyph check` does
- **What it does not do.** `glyph fix` does not guess, and the list of what it therefore declines is longer than the list of what it repairs. It cannot repair a match over a prelude union, because no identity keys `Result` or `Option` and the rule needs the payload shapes from `glyph_symbol` (G231). It repairs a match over a union declared in another module, which it could not when the notes were first written: the rule verified its own work by resolving the candidate file alone, so every imported variant read as an unresolved name (G236). The self-check runs against the project now, and the rule writes the variant imports the arms need, or the namespace spelling when the module is imported as one. `corrected` on `--explain --json` is null. `cause` on E0211 is null. An object literal still has no type, so `let g: string = { x: 1 }` is still silent under `--no-tsc` (G232), and the impact table still understates the checker for a string-literal union on either side (G234)
- **The measurement.** The suite is 1714 tests, from 1625; the exact-or-absent corpus is 33 invariants, from 31; the gap ledger is 237 entries
- **Breaking**, in the checking direction, established by running the published 0.1.121 and this build on the same programs rather than by reading the diff. G230's four-statement program exits 0 under 0.1.121 with four unused-variable lints and 1 under this build with `E0204` twice, `E0211` and `E0204`, which is the four `tsc` reported, at the same four statements, without running `tsc`. The 2,000-deep program aborts under 0.1.121 with exit 134 and reports `E0011` at 1:75 under this build with exit 1, so no program that used to compile is lost there

**0.1.123 — Shipped 2026-09-18 · A knowledge surface generated from the compiler, and a literal that keeps its type**
- **What the release is.** Two things that turned out to be one thing. Everything an agent has to know about Glyph now comes out of the compiler's own tables rather than out of prose somebody maintained by hand: the diagnostic catalogue, the prelude, the stdlib surface, the D-decision index and the tool manual, all in one `glyph llms --json` document, with a gate that fails when a hand-written copy drifts from it. And the two things that document could not say about the compiler's own vocabulary are now sayable: the prelude `Result` has an identity (`std/result::Result`), and a written string literal has the type it spells rather than a bare `string`
- **G222, the surface.** `glyph llms --json` emits six sections and nothing in it is written by hand. `diagnostics`: 61 codes, each with the catalogue sentence, the one-line fix, the whole `--explain` text, and the help and note read off a real diagnostic; 38 of the 61 also carry a wrong program from `tests/negative/`, compiled at the moment you ask, and the other 23 say in a `counter_example_absent` line that the corpus holds no case for the code. `prelude`: 24 names out of `build_prelude`. `stdlib`: 36 modules and 327 exports, split three ways and never two: 24 with a signature the checker models in every position, 76 whose signature carries a `?` where a parameter type belongs, counted `partially_modeled` with a `signature_partial` line saying how many positions are left, and 227 saying in a `signature_absent` line that the tables model none. A `?` is `unknown` rendered, so counting those 76 as modeled published a complete-looking signature nobody can read a parameter type out of. `decisions`: 47 parsed out of the spec's own list. `tools`: the 11 `tool_specs()` serves, each with its `glyph query` verb and its full manual. The whole document builds in 0.09 seconds, measured on three runs, and is byte-identical across them
- **One table, and what lifting it found.** The code catalogue lives in `CODES` in `explain.rs` now, and `docs/error-codes.md` and the diagnostic table in `AGENTS.md` are written from it by `scripts/check_llms_sync.py`, which fails on drift without `--write` and re-mirrors `llms.txt` and `web/llms.txt` with it. Merging the three hand-maintained copies turned up `E0304`, which had `--explain` prose and no place in the code list, and an `E0111` row whose escaped pipe had gone unnoticed. A test in `glyph-cli` reads `tool_specs()` and the document together: every tool served is named, every `glyph_*` name in the document is served, and the count sentence agrees with the list. Both halves were broken on purpose to check that they fail. This is what "exposes five tools" needed to survive two releases against a server that served seven
- **Every fence compiles, including the wrong ones.** `AGENTS.md` was 19 fences with 2 compiled and 17 skipped as fragments; it is 24 fences, 20 compiled and 4 marked, none skipped: one is a `fragment-of` a named complete example that is itself compiled, and three are `needs-deps`, which imports an npm package or declares a `component` and is a different claim. `docs/reference/stdlib.md` was 8 with 4 compiled and is 8 of 8. `check_docs_compile.py` gains `example=NAME` and `fragment-of=NAME`, whose every line has to appear in the named example, holds those documents to zero unmarked fragments, and requires an `expect-error` fence to name the code it draws. Five wrong forms the bootstrap warns about are programs paired with their diagnostic now (E0228, E0008, E0006, E0002, E0103), starting with the `Order { ... }` construction G217 is about. Compiling the documents found two things nobody was going to find by reading: an inline triple backtick in `AGENTS.md` that read as a fence opener, so 250 lines counted as one snippet, and a worked server in the stdlib reference calling `http.serve`, which `std/http` does not export. The function is `listen`
- **`tools/list` stops costing what it costs.** Measured over `glyph mcp` stdio: eleven tools, 31,446 bytes of description and a 45,509-byte result before; the same eleven tools, 8,627 and 20,386 after, which is 73% off the descriptions and 55% off the reply every session pays for on connect. The `instructions` string went 1,295 to 1,153. Nothing was deleted: `tool_manual()` holds the previous text verbatim, keyed by tool name, and `glyph llms --json` publishes it, so the contract an agent needs at the moment of the call is in the description and the rest is one command away rather than in every session's first reply
- **The negative corpus becomes an answer.** `glyph llms --negative E0200` prints the `tests/negative/` and `catches/` cases that draw the code, each compiled here, the `catches/` ones carrying the TypeScript a `tsc --strict` project accepts beside the Glyph that refuses it. `glyph llms --negative` with no code lists 69 cases over 40 of the 61 codes, and names the 21 codes that have no case. That is a different count from the 38 above: `--negative` reads `tests/negative/` and `catches/` together, and the counter-example on a code reads `tests/negative/` alone. Both corpora are embedded by `build.rs` from the directories themselves, so a case added to either is in the answer with no second edit to remember
- **G231, decided 2026-09-15: a prelude declaration is keyed by the stdlib module that declares it.** `Result` is `std/result::Result`, `Option` is `std/option::Option`, `Nullable` is `std/nullable::Nullable`, and the same rule gives `fs.ErrorKind` its address, `std/fs::ErrorKind`. The key was not invented, it was being discarded: the resolver registers `std/result` with `Result`, `Ok` and `Err`; the emitter writes `import { Ok, type Result } from "./.glyph-runtime/std/result"` for a program that never imported it; and `import std/result { Result }` resolves today. One layer, the agent-facing one, threw it away, under a comment saying a key for `Result` would name a module no project has. That comment was false. `UnionRef::Builtin` and `DiagnosticDecl::Builtin` carry the module key, so an `E0200` over `Result` reports `cause: "std/result::Result"` where it reported `null`; `check --agent` describes it under `symbols` with `Ok(T)` and `Err(E)`; `glyph fix` repairs `match r { Ok(v) => v, }` and the result passes `tsc --strict`; `glyph query symbol --entity std/result::Result`, `glyph query variants` and `glyph query exports --module std/result` all answer instead of refusing, with `path` and `range` absent and the reason attached, since the stdlib is TypeScript the compiler stages rather than Glyph source. A project that declares its own `pub type Result` still keys as `orders::Result`
- **Why not a reserved segment, and why not a kind beside the name.** A new segment such as `<prelude>::Result` is collision-proof and nothing else. It would give one declaration two identities, the agent's and the emitter's, which is the G75 rule broken on purpose, and it would add a fourth module-path grammar beside project paths, npm-scoped paths and `extern/`. A `kind` beside a bare name was already shipped and is the thing that was broken: the union entity has carried `kind: "builtin"` beside `name: "Result"` all along and it buys an agent no next call. The axis is not binary either, because `fs.ErrorKind` is stdlib and not prelude, so a discriminator would have had to be a taxonomy where a slash does the work. And `kind` is taken twice on the same objects, by `glyph_symbol`'s declaration kind and `glyph_impact`'s change kind. The deciding argument is narrower than any of those: the whole surface rests on forwarding one flat string from a diagnostic into the next query, and three tools take an entity argument, 27 emission sites in `mcp.rs` emit one, and the exact-or-absent gate and the committed generation records key on it. A string whose meaning depends on a sibling field cannot be forwarded. `union.kind` stays `builtin`, because it still answers a real question: whether this project can add a variant. The residue that no stdlib module declares (`Array`, `Record`, `Schema`, `Component`, `Issue`, `par`, `print`, `assert`, `infer_output`) stays unkeyed under `NOT_A_DECLARATION` with a reason each, rather than getting an invented `std/prelude`. The one thing that reopens the decision: the prelude `Result` ever becoming a declaration distinct from `std/result`'s. The consultation is Linus 67
- **G238, E0113, the collision guard the identities rest on.** A project file could declare `module std/io`, and it compiled clean and was silently unreachable: an import of that path resolves to the stdlib stub and the project file is never read. The resolver reserved `std/` and `extern/` for imports only and nothing rejected the declaration. A `module` declaration whose first segment is `std` or `extern` is an error now, matched as a whole segment, so `standard`, `externals/io`, `app/std` and `app/extern/helper` are untouched and an import under either prefix still resolves. D15 is corrected to say both prefixes are reserved for declarations as well as imports. It is a dead-code hole on its own account and it landed before the identities did
- **G237, literal-typed expressions.** D30 put the literal set on the declaration and nothing on the value, so `"read"` was a `string` and `["read", "write"]` an `Array<string>`, which made two programs indistinguishable: `return ["read", "write"]` against an `Array<Mode>`, which `tsc` compiles, and `return xs` for an `xs: Array<string>`, which it refuses. 0.1.122 declined the pairing rather than get it wrong, and that bought the right program by giving up the wrong one. A written string literal now carries the one-literal union it spells, an array literal's element type is the join of its elements' (two literal sets join to their union, a literal and a `string` join to `string`), and a record literal's fields carry their literals. The fence is gone with it: a written array or object literal is checked element by element and field by field against the declared type, so a refusal names the offending literal and underlines it rather than the whole literal. `["read", "nope"]` against `Array<Mode>` and `{ mode: "nope" }` against `{ mode: Mode }` are refused; `["read", "write"]` and `{ mode: "read" }` are accepted
- **The widening rule, established against `tsc` row by row rather than reasoned about.** A fresh literal widens to `string` at a binding that infers its type from the value: a `let` with no annotation, an array bound to such a `let`, and a `for` element binding, all three of which widen in the emitted TypeScript too. A `const` keeps its literal, because a Glyph `const` emits a TypeScript `const` and `takes_mode(CM)` compiles, so widening it would refuse a program `tsc` accepts. An argument, a `return`, an annotated binding, a written array element and a written object field all keep the literal. A literal type that arrived through a name is not fresh and is not widened, which is TypeScript's own freshness rule: freshness is read off the written form, not off the type. Each row was settled by writing the equivalent TypeScript and running `tsc --strict` on it, which is why the `const` row and the `for` row point in opposite directions
- **Why the record field is decided by contextual typing and not by a record type for the literal.** The field check reads the declared type at the field and asks the relation about the literal written there. It does not synthesize a record type for `{ mode: "read" }`. That is deliberate, and it is why a record literal bound to an unannotated `let` and returned is still undecided: nothing types the binding. Synthesizing the type reaches the record recursion in `definitely_incompatible`, every width-subtyping site, and the inference the language deliberately does not do at a `const` (G39), so it is a type-system change with a blast radius and it wants its own release and its own review rather than a paragraph in this one. It is G232, in the rolling lane
- **What G237 leaves undetermined, each with a reason.** The key argument of a `Record` against another `Record`'s: TypeScript writes `Record<string, V>` as an index signature covering every key a `Record<Mode, V>` declares and accepts the pairing in both directions, so refusing it would refuse a program `tsc` compiles. A literal inside a prelude constructor's payload (`Some("nope")` against an `Option<Mode>`): the walk records `Ty::Unknown` for `Ok`, `Err` and `Some` pending use-site generic instantiation, so it is the constructor's missing type rather than the literal's that keeps Glyph silent. A record literal bound to an unannotated `let`, above. A concatenation (`"re" + "ad"`): a binary expression has no type here at all
- **G233, one declaration, one spelling.** An imported string-literal union lowered straight to its literal set, so the same declaration printed as ``expected `"read" | "write"` `` through a named import and ``expected `Alias` `` through an alias, and neither matched what the same declaration printed locally. It lowers to the `Ty::Imported` every other imported declaration lowers to now, under the named and the namespace spelling alike, and the literal set is read from the declaration on demand through one accessor that assignability, match exhaustiveness, `glyph_symbol`, `glyph_assignable` and the emitter's narrowing assertion all go through. ``expected `Mode` `` under both import spellings. The emitted TypeScript is unchanged. One thing the fix removed rather than added: the lowerer reaches across no module boundary at all now, so `Lowerer::with_imports` and the per-shape query behind it are gone
- **G234, and a common shape that got more accurate.** `glyph_impact`'s `change_signature_type` table sent a string-literal union to `Other` on both sides, so six cells said the checker has no rule for a pairing G230 had given it one for, while `glyph_assignable` on the same pairing answered `COMPATIBLE`. The union is its own kind on both sides now, however it is spelled, and its cells run the same `assignability` call `glyph_assignable` makes, the way the container and record cells have since G228, so the two tools cannot answer differently about one pairing. Six cells are asserted in both verdict directions against the tools' own answers rather than against fixed strings. The first cut of that reported an accepted pairing `SAFE`, which is the design call the review refused before the tag. `change_signature_type` names no replacement type, so nothing about an accepted pairing proves the site survives the edit: in a project that compiles every pairing is accepted, and `SAFE` there was true of every argument of every call. It cost something on a common shape, because `f("x")` and `f(s)` against one `f(s: string)` came back `SAFE` and `WILL_FAIL` under one edit and both break identically, so an agent planning `string` to `number` would repair one site of two. One meaning runs across the table now: an argument whose pairing the relation reads in full is `WILL_FAIL`, whether the cell states the rule or runs it, and an argument with no rule is `UNDETERMINED`. That reverts the value-decided `SAFE` for a literal argument and the two cells 0.1.122 added for a container against a container and a record against a record. `glyph_assignable` keeps `COMPATIBLE` for the acceptance, because it is asked about two types with no edit in the question
- **What it does not do.** No retrieval layer, no embeddings, no chat surface, and no documentation written by hand about the compiler where the compiler can be asked. `glyph llms --json` is a document you fetch once, not a service. 21 of the 61 codes have no wrong program in either corpus, and the answer says so rather than inventing one. 227 of the 327 stdlib exports are in neither of the compiler's tables and still answer `kind: null` with the reason, and 76 of the 100 that do have a signature model only the arity and the return. An object literal still has no type of its own, so `let g: string = { x: 1 }` is silent under `--no-tsc` (G232), and the editor's hover still answers single-file where the tool answers from the project (G235)
- **The measurement.** The suite is 1748 tests, from 1714; the exact-or-absent corpus is 34 invariants, from 33; the gap ledger is 240 entries, 216 fixed and 6 open. The positive string-literal-union corpus is eighteen programs, from thirteen, each run under the published 0.1.122 with `tsc` in the loop before it was added; the nineteenth was a JSX attribute that no rule reads, and it is G240 rather than a corpus entry now. The 31 apps at their own roots and `examples/` answer with identical exit codes and identical diagnostic-code lists under both binaries
- **Breaking**, in the checking direction, established by running the published 0.1.122 and this build on the same programs rather than by reading the diff. `fn bad() -> Array<Mode> { return ["read", "nope"] }` is `1 module(s) checked, no diagnostics.` and exit 0 under 0.1.122, and ``[E0204] type mismatch: expected `Mode`, found `"nope"` `` with the span under `"nope"` and exit 1 under this build. Eight programs `tsc --strict` refuses and 0.1.122 accepted are `E0204` or `E0211` now, so what changes is which compiler tells you. E0113 is breaking in the same direction and loses nothing: a two-file project declaring `module std/io` is `2 module(s) checked, no diagnostics.` and exit 0 under 0.1.122 and `[E0113]` with exit 1 here, and no importer could ever reach that file's code
- **One spec matter left for the owner, not decided here.** The spec numbers two different decisions D43, the `mut` across an `await` rule and `never` as the bottom type. `glyph llms --json` reports it in `duplicate_numbers` rather than deduplicating it silently, which is how it was found; renumbering one of them is a decision about a stable public index and it is the owner's.
- **G239, an imported union named like a prelude type, and the tool that reported zero sites over it.** `module orders` declares `pub type Result = | Won | Lost`; `module main` imports it and matches with one arm. Nothing was reported, the emitted TypeScript carried `default: throw new Error("non-exhaustive match")`, and the program threw at run time on `Lost`. The lowerer read the imported name before the import path, so a prelude container's spelling beat the binding that says which declaration the module means, and a bare prelude `Result` is not an application, so no variant list was ever required. `Option` and `Nullable` collided the same way; `Outcome` did not, and a local `type Result` did not. `glyph_variants --name Result` resolved `orders::Result`, read both variants, walked the project and answered `"0 match sites across 0 files"` with `not_counted: []`, which is a manufactured absence over a file that holds the site. One rule fixes both halves: a named import lowers to the prelude container only when its path starts `std`, so an explicit import binding shadows a prelude name, which is what a local `type Result` already did (G213) and what `import std/string` already did for a namespace binding (G225). `import std/result { Result }` is unchanged, because the prelude is a curated re-export of the stdlib and the two are one declaration
- **The knowledge surface answers what the checker holds, which it did not at first.** `glyph_symbol` on `std/array::filter` reported no kind and a sentence saying Glyph's checker does not read the stdlib, while the same binary published `fn(Array<T>, fn(T) -> bool) -> Array<T>` for it in this release's own document and raised `E0211` on `array.filter(xs, 3)`. That was true of all 100 modeled exports, on the tool an agent is told to call first. `describe_stdlib_symbol` calls `stdlib_signature`, the entry point `glyph llms --json` already used, and fills `kind`, `type`, `parameters`, `returns`, `async` and `generics` from it; `glyph_exports` carries the signature beside the kind on every export. An export the tables hold nothing for keeps `kind: null` and says what is missing rather than what the checker cannot do. `glyph_impact` and `glyph_dependencies` on a stdlib entity stop answering "no file of this project is module `std/result`", the sentence G231 made obsolete, and refuse with the reason: this project cannot change a union the compiler declares
- **G240, and a corpus entry that proved nothing.** A JSX attribute reaches no rule in the Glyph front end, so `variant="danger"`, `variant="nope"` and `variant={42}` against a `variant: Variant` prop all pass `glyph check --no-tsc` with nothing. The string-literal-union corpus held a `jsx_attribute` case as one of the nineteen programs the rule must not refuse, and it could not be refused by any rule, so it was a dead instrument that would pass under any widening. The case is removed, the corpus is eighteen, and the gap is recorded and scheduled in the rolling lane rather than left in the corpus looking like coverage

**Later · Recorded from the 0.1.126 round, not yet in a release**
- G275: a `<case>` binding typed by the checker from the variant's payload, and a decision on what `bind` means on a positional payload, which the emitter reads as a record field today and binds `undefined`
- G245: a structural record displayed by its fields in a diagnostic, so an inline props record reads as its shape rather than as `record`
- G248: a gate project with React and its types installed compiles every `needs-deps` fence with `tsc` in the loop; the cheap half first, `glyph check --no-tsc` on every `needs-deps` fence that declares a `component`
- G257: values that come from React (a state setter, a hook result) are typed by Glyph's checker from React's declarations, so a wrong setter argument is a Glyph diagnostic at the argument; today `tsc` reports it across the whole line
- G259, G260, G261: go-to-definition on an import binding and on a declaration's own name, hover on a namespace base segment, and a same-file definition range, each answering what the other spellings and positions answer
- G262, G263, G264: an arity check on the prelude `Component`, generic bounds resolved like any name, and the elements of a bounded generic component checked against the bound
- G265: the prelude `Component` modeled in the relation, so JSX is a `Component` and a `Component` is not a `string`; decided in principle, its own change
- G266: a declared `children` type compared with what is written between the tags
- G267: `default as` on a Glyph module with no default export refused by the Glyph stages
- The watch-mode test `build_watch_rebuilds_when_an_edit_lands_during_a_build` lost its race on CI on 2026-10-17, on a one-line docs change: the first build over 1500 filler modules finished while the edit was being written, and the test's own premise guard failed it. The guard is right to refuse a vacuous pass; the test needs a build that cannot finish before the write, rather than a filler count tuned to today's runner speed
- G272: `==` on an opaque host handle (`Timer`, a socket, `dom.Element`) refused by the checker, since `__glyph_eq` has no cycle guard
- G378: `==` on two deeply nested values overflows the stack (record 116). An iterative `__glyph_eq`; its own release once G272 has decided how a cyclic value reaches `==`, since an iterative walk with no visited set hangs on a cycle where the recursive one throws
- Tests the 0.1.125 review found missing: the `tsc` messages `missing_fields.rs` records are compared in a test with React types installed, and G252's concurrent checks get a regression test
- Carried: the remaining stdlib signatures and the unifier change that widens a bound name (its own change, split by risk); editor go-to-definition across modules in an unmarked `src/` tree (`project_root_for`), which waits on the owner's scope call; and the enterprise review's items from the rolling lane as they are decided

**0.1.131 — Shipped 2026-11-06 · What 0.1.130's checks found next to them**
- **G300 and G301: a keyword is a name after a dot.** `type Event = { type: string }` can be read as `e.type`, and `Sized.is(v)` can be called, where both were E0002; any keyword after `.` or `?.` is a member name, read from the lexer's `KEYWORDS` table. A misspelling is the ordinary E0210. A keyword in type position (`fn f(x: match)`) takes the reserved-word message. An import item and a type-path segment still refuse a keyword
- **G292: explicit type arguments on a user generic bind.** `id<number>("x")` is E0211 at the argument and E0204 at the return, for a local function and a sibling module's under both import spellings. A count that is not the declaration's binds nothing and is left to `tsc`
- **G293: a declared record where `Array`, `Option` or `Result` is declared is E0204**, the reverse pairing D49 names; a `Nullable<T>` is compared against `T`. `Record<K, V>` is left undecided and pinned by a test
- **G297 and G298, half each: `std/json` at the boundary.** `json.parse<Doc>` is E0304 where `Doc.parse` is, with `Doc` or `w.Doc` as the type argument and inside `Array<..>`, and so is `Doc.schema`; an `Option`, `Nullable` or `Record` around the type argument still casts; `json.parse<w.Doc>` under `import w` validates where it cast. `json.parse_with` is in the checker's stdlib table. The named-import `parse<Doc>` and the 165 stdlib exports with no signature are 0.1.132's
- **G294: the stdlib's string-literal unions are modeled.** `http.RedirectPolicy`, `http.HttpErrorKind`, `log.Level` and `net.ServerErrorKind` are read from a table a bootstrap test holds against the runtime, so a full match needs no `else`, a missing value is E0200 naming it, and a misspelled literal is E0204. `HttpError.kind` and `ServerError.kind` carry those names
- **G295: the editor shows the emitter's errors.** The language server publishes the list `glyph check --no-tsc --no-test` reports, E0302 to E0305 included (not `tsc`, the `@example` gate or E0104), in two phases: the front end on every keystroke, the whole list once typing pauses for 300 ms, and the whole list at once on open and save. A run that is no longer current is dropped before it publishes. Lint findings are published as warnings, as `glyph check` reports them
- **G296: a second name for an imported record has a descriptor.** `pub type Page = Doc` with `Doc` imported binds `export const Page = Doc;`, so `Page.parse` works and a third module checks a `Page` field under any import spelling; E0304 follows the alias
- **G299: a `tsc` diagnostic in `--json` carries the terminal's help**, picked by one function both renderers call
- **Members after G288.** A misspelled member on a `number` is E0210 with "did you mean", checked against the pinned TypeScript like arrays and strings; a misspelled record field suggests the nearest field; and the diagnostic JSON marks a built-in value's list `"members"`, a record's `"fields"`. Typing a member that exists is the owner's decision (0.1.132)
- **`tsc`-error fences on the answer pages.** The finance and imports pages, the verifiability page's extra-field case and the imports guide's missing shim are fences the docs gate compiles with their `tsc` code; the early-errors page's codes describe older releases and are left as prose
- **The suite's wall clock, measured.** The 0.1.130 Release run was 12m42s, 5m15s of it waiting for macOS runners; Smoke spends about 2m50s waiting on npm; the CI cargo cache has never been restored; and most of the local suite's time is between test binaries rather than in them. The changes are 0.1.132's
- **The measurement.** The suite is 2014 tests, from 1980. The gap ledger is 305 entries: 266 fixed, 9 partly fixed, 11 decided or resolved and 19 open, with G292 to G296, G299, G300 and G301 closed here, G297 and G298 half closed, and four opened (G302 to G305), each reproduced on this build
- **What it does not do.** A second name spelled through a namespace is not followed (G302); `print` of a `bool`, `?.` on a `Nullable` and a match arm for a literal outside its union are still left to `tsc` (G303 to G305); `parse<Doc>` through a named import still casts
- **Breaking**, established by running the published 0.1.130 and this build on the same programs. At run time, `json.parse<w.Fine>(body)` under `import w` prints `ok` in 0.1.130 and `err` here for a body with an undeclared key: it validated nothing and now validates a closed record, with no exit code to say so (G297; `@open` keeps the old acceptance). Under `check`, exit 0 in 0.1.130 and 1 here: `json.parse<Doc>`, `json.parse<w.Doc>` and `json.parse_with(body, Doc.schema)` over a record with an `extern_ts` field (G297), and `let o: Option<number> = m` where `m` is a user union `| None | Some(number)`, which `tsc` accepts (G293, D49). Under `--no-tsc` only, each already a `tsc` error: a misspelled `number` member, `id<number>("x")`, a `User` where a `Result` is declared, `return "folow"` from `-> http.RedirectPolicy`, an `HttpErrorKind` returned as a `ServerErrorKind`, and `RawPage.parse` over an aliased imported record with an `extern_ts` field (G296). A full match over `http.RedirectPolicy` goes the other way, from 1 to 0

**0.1.132 — Shipped 2026-11-08 · What 0.1.131's fixes turned up** (decided in Linus records 79 to 81)
- **G302: a second name is a second name however it is spelled.** D46 follows `pub type Page = a.Doc`, `aa.Doc` and `http.RedirectPolicy` as it follows a bare name, in the checker and the emitter (`export const Page = a.Doc;`). The named-import spelling had three of the same defects and they closed with it: a field read through a second name is checked (E0210), a match over a second name of a string-literal union is held to its values, and a `where` over an imported record is E0300 as the local one is. E0210 through a second name names both: ``type `Page` (a second name for `a.Doc`) has no field `titel` ``
- **G304: a read through a `Nullable` is checked, and its `undefined` cannot escape.** `n?.f` is checked against the inner record's fields; `n.f` is the new E0235; and an optional chain over a `Nullable` may only be the left of `??` or an operand of `==` or `!=`, because anywhere else it holds an `undefined` Glyph has no type for. `n?.name ?? "none"` is a `string`. An alias of a `Nullable` is read the same way
- **D49 decides `Record<string, V>`.** A declared record is compared field by field against `V`; an `@open` record is refused unless `V` is `unknown`; a tagged union is refused; an optional field and a non-`string` key are left to `tsc`. A generic record (`Box<string>`) is compared the same way, where 0.1.131 refused it
- **E0233, a wrong number of type arguments**, on a call (`id<number, bool>(1)`), a stdlib call, a type application (`Box<number, string>`) and a generic type written with none (`let b: Box`, `fn f(a: Array)`). Type arguments on a function value are 0.1.133's
- **`json.parse<T>` validates in both import spellings.** `parse<Doc>` through `import std/json { parse }` is rewritten to validate as `json.parse<Doc>` is, and an `Option`, `Nullable`, `Record`, an `Array` alias or a primitive `T` is checked the way a record field of that type is. A type parameter, an `extern_ts` type, a package type, a container of `unknown` and an imported `Array` alias still cast (G297, 0.1.133)
- **G303 and G305.** `print`, `assert`, `number.to_string` and `number.parse` check their arguments (E0211). A match arm for a literal outside its union is E0220, and E0220 suggests only a case no other arm covers
- **`length` is a `number`** on an array and a string, so `let a: string = xs.length` is E0204
- **Keywords as type-path segments.** `kinds.type` works in type position as it does in value position; `import kinds { type }` is still refused and its help points at the namespace form
- **The editor.** The emitter's errors are carried through each keystroke instead of dropping out until the pause, a change to one module republishes the others that are open, and closed documents are forgotten. `glyph/applyEdit` runs the same list `glyph check` does (G306) and refuses only what an edit introduces, warnings included; what the file already had comes back under `preexisting`. It still checks only the edited file (G310), and while the file has an error the lint tier and the emitter do not run, so an edit can add a warning to such a file and be accepted (G313)
- **The stdlib against its runtime.** `check_stdlib_signatures.py` holds every complete stdlib signature against the runtime's TypeScript in both directions, with each array parameter read as `ReadonlyArray`, so "a stdlib function never mutates an array it is passed" is checked. It found G307 and G308
- **CI.** Both darwin targets build in one job, Smoke polls every 10 seconds against a deadline, and the cargo cache is keyed on the toolchain and the lockfile, restored by prefix on pull requests and exactly on `main`
- **The measurement.** The suite is 2106 tests, from 2014. The gap ledger is 315 entries: 273 fixed, 9 partly fixed, 11 decided or resolved and 22 open, with G262, G302 to G306 and G309 closed here, and G307, G308 and G310 to G315 opened, each reproduced on this build
- **What it does not do.** `applyEdit` checks only the edited file (G310); `?.` on an `Option` or a `Result` is not checked (G311); a built-in method read without a call is accepted (G312); the applyEdit gate does not run the lint tier or the emitter on a file that already has an error (G313); `json.parse<T>` still casts for a type parameter, an `extern_ts` type, a package type, a container of `unknown` and an imported `Array` alias; a type parameter nested in a type is not compared (G314); a second name for `unknown`, a type parameter or a `Nullable` is refused where the plain spelling is accepted (G315); type arguments on a function value are ignored
- **Breaking**, established by running the published 0.1.131 and this build on the same programs. At run time, `parse<Doc>("{}")` through `import std/json { parse }` prints `ok` in 0.1.131 and `err` here. Under `check`, exit 0 in 0.1.131 and 1 here, each accepted by `tsc`: a match over `type L = a.Shape` missing a variant (E0200); `type T = a.Doc where ...` (E0300, also under `build`); an `@open` record or a tagged union where `Record<string, string>` is declared (E0204); `let x = n?.name` and `"${n?.name}"` on a `Nullable` (E0235); the same chain as a JSX child `{n?.name}` (E0235; `{n?.name ?? ""}` is the form); and the named-import `parse<Doc>` over a record with an `extern_ts` field (E0304). In the editor protocol, `glyph/applyEdit` answers `ok: true` for an edit that introduces nothing to a file that already had a diagnostic, where 0.1.131 answered `ok: false`, and a refusal's `diagnostics` holds only what the edit introduced, with the rest under `preexisting`: `ok: true` no longer means the file is clean. Under `--no-tsc` only, each already a `tsc` error: `p.titel` through a second name, a record where `Record<string, string>` is declared with a non-string field, `id<number, bool>(1)`, `fn f(b: Box)` with `type Box<T>`, `let a: string = xs.length`, `print` of anything but a `string` and `assert` of anything but a `bool` (`print(42)`), a literal arm outside its union, and `return n?.name` from `-> Nullable<string>`. These go the other way, from 1 to 0: `let p: Page = d` with `Page = a.Doc`, a generic `Box<string>` where `Record<string, string>` is declared, a `Box<T>` where `Record<string, unknown>` or `Record<string, T>` is declared, and a full match over an alias of `http.RedirectPolicy`, each accepted by `tsc` too; and, under `--no-tsc` only, a `Box<T>` whose field is an `Array<T>` or an optional `T` where a `Record<string, ...>` is declared, which `tsc` still refuses (G314)

**Planned · what agents get wrong in TypeScript (research, 2026-11-19).** Two passes ran on 0.1.149: one checked 40 TypeScript mistakes against Glyph, the other checked published studies of agent-written code (Tambon et al. 2024, Wang et al. ICSE 2025, Hora and Robbes MSR 2026, Pearce et al. S&P 2022, the GitClear and CodeRabbit reports) against Glyph's checks. The items below are where both found Glyph no better than `tsc --strict`. They are numbered into the plan below (decided 2026-11-20), ahead of most of the earlier queue where they fix a bug in the boundary check or a common agent mistake. Logic errors, the largest class in every study, are outside what a type checker sees and are not on this list.

**0.1.150 — Shipped 2026-11-20 · G369, a check refuses a value nested more than 1000 deep instead of overflowing the stack** (records 111 and 116; breaking)
- **G369 and D50.** Every generated check (`is T`, `T.is`, `T.parse`, `T.schema` and its `array()`, `json.parse<T>` in both import spellings, `json.parse_with`) takes the depth of its value as a parameter and compares it once per object or array it enters against `MAX_DEPTH` (1000), defined once in the internal runtime module the emitted code imports and re-exported by `std/schema` as the public limit; the emitted code never writes it as a number. Past it, `is` and `T.is` answer `false`, and a `parse` answers one issue, `code: "depth"`, at the path a `type` issue for that part would carry. A private sentinel carries the refusal to where the answer's path is known, a public `is` or a `parse` field, and lives with the helpers that throw and catch it in an internal runtime module no Glyph program can import, so Glyph code cannot throw it or forge a refusal (record 122); `MAX_DEPTH` is the one public name, in `std/schema`. A `RangeError` is never caught. `Issue.code` gains `"depth"`. A cyclic value is refused the same way where it used to throw. `JSON.parse` keeps no limit; the untyped read decodes any depth
- **Measured.** 29 paths and shapes (records, a union, `Option`, `Nullable`, a generic record, `Record` values and literal-union keys, a refined field and a refined container, an inline record, sibling and namespaced descriptors, `is` over `Array` and `Record`) pass at depth 1000 and are refused at the first depth past it, identically under node, `glyph run`'s tsx and both at `--stack-size=492`; none throws at 3000 or 20000, where 0.1.149 throws `RangeError` on 21 of the 29 at 3000 levels with half the stack and on all 29 at 20000 with the default. A 10,000,000-deep document through `json.parse<Tree>` is the depth `Err`. On a wide 960k-node tree (three passes, median of seven runs) `T.is` takes 117 ms against 126 ms on 0.1.149 and `is Tree` 115 against 123, because the walk over an array's elements or a map's values hands `every_at` the descriptor's own `__is` (`Tree.__is`, `__is_T`) once per container instead of the closure 0.1.149 built on every call; `T.parse` takes 119 ms against 113 (5%), within 2 ms of this build's `T.is`, since both run the same walk; `json.parse<Tree>` of the same document is 1156 against 1142, and record 111's 3,000,000-element `is Array<int>` is 122 against 123. No guard was dropped for any of it (record 122)
- **Breaking.** At run time only: `json.parse<Tree>` of a 600-node `Tree` chain (1202 levels) exits 0 under 0.1.149 and 1 here, with `[{"path":["kids"],"message":"nested more than 1000 objects and arrays deep","code":"depth"}]`. Any value between 1001 levels and the depth at which 0.1.149 overflowed (about 4300 to 18000 by shape) is refused where it was accepted. `check` is unchanged: record 116 expected a `match` on `issue.code` listing the four old codes to become E0200, but Glyph's checker types `code` as `string`, so that match was already E0218 without a catch-all and still is; a test pins it. The emitted TypeScript of every descriptor changes (`__is`, `__parse`, and an import of `.glyph-runtime/internal/depth`, which every build now writes), with no change to Glyph sources
- **What it does not do.** `==` on two deeply nested values still overflows (G378). A host checker passed to a generic descriptor ignores the depth and restarts the count inside host code

**0.1.151 — Shipped 2026-11-20 · G377, a `where` cannot refine a string-literal union** (records 115 and 119; breaking). Decided in record 119, option (d), and reordered ahead of G373 by the same record: it is a declaration-site refusal in one emitter function with no corpus hits, and it takes refined finite keys out of the language before G373 has to define key-domain equality for them.
- **The refusal.** A `where` whose base resolves to a string-literal union, a single literal included, is E0300 at the declaration. The base is judged by what it resolves to: inline, a local alias, an alias chain, imported by name, through a namespace, a local second name of an import, a second name a sibling module declares, and a chain across three modules. D39 always said this; `refused_refinement_base` had matched only tagged unions and records. It removes all three defects record 119 found in `type T2 = Tier where value != "c"`: `tsc` read `T2` as all of `Tier` while the run-time check refused `"c"`, so `json.parse<Record<T2, int>>` had no document both accepted; `let x: T2 = "c"` compiled; and a `match` on a `T2` lost D30 exhaustiveness
- **The diagnostic is the program's own** (record 120). The D39 base refusal is its own error (still E0300, stage `emit`) carrying the declaration, its base as written, what the base resolved to, the members of a literal base and the predicate as written (printed by the formatter, never the emitted TypeScript). The headline states the rule instead of "not implemented yet": ``` `Paid` refines `Plan`, a string-literal union, and a `where` refines only `string`, `int`, `number`, `bool` and `bigint` (D39) ```. The help lists every member the base resolves to, gathered from whichever module declares them, and quotes the predicate: "Declare `Paid` as the members of `Plan` it keeps, written as a union of their own. `Plan` is `"free" | "pro" | "team"`, and `Paid` is meant to hold the ones that pass `value != "free"`. ..." It computes no subset and offers nothing to paste. The record and tagged-union refusals use the same error with their own headline and help (a record's ends "a check across a record's fields is a planned extension"); every other E0300 keeps the old wording. `EmitError::help` now returns `Option<Cow<'static, str>>`, as the parser's and checker's do
- **Two-hop record and tagged-union bases.** The walk that follows a sibling's own second names reaches a record or a tagged union too, so `type T = Rec2 where value.x > 0` with `Rec2` a sibling's second name for a third module's record is now E0300, as is the same chain over a tagged union (record 120, probes `d` and `g`). D39 refuses those bases however they are spelled (G207); 0.1.149 stopped one hop into the sibling
- **Unchanged.** A refined `string`, `int`, `number`, `bool` or `bigint`, including `string where value == "a" || value == "b"`, keeps its descriptor and the 0.1.148 key rule. No predicate is evaluated at compile time and no refined type is emitted narrowed
- **The canonical numeric keys, recorded.** A numeric `Record` key is accepted exactly when `m[n] = v` can create it for some `n` of the key type: `NaN`, `Infinity` and `-Infinity` for `number` only, `1e+21` for both, and `1e21`, `-0`, `01` and `9007199254740993` for neither. D30 and D31 say so, and one test pins the whole table and the round trip of a map Glyph wrote. No behaviour changed
- **Docs.** D39, D30 and D31 in the spec; the E0300 catalogue entry and `glyph --explain E0300`, which now opens on both meanings of E0300; `AGENTS.md`; the typed-APIs guide; the typed-APIs answer page (its translations show the stale notice until they are redone)
- **Breaking**, established by running the published 0.1.149 and this build on each program; every one exits 0 under both `check --no-tsc` and full `check` on 0.1.149 and 1 under both here:
  - `type Plan = "free" | "pro" | "team"` with `pub type Paid = Plan where value != "free"` (record 120, probe `a`), and every other spelling of a `where` over a literal union;
  - `pub type T = Rec2 where value.x > 0`, where `r2` declares `pub type Rec2 = Rec` over `r1`'s record `Rec` (probe `d`);
  - `pub type T = Sh2 where true`, where `u2` declares `pub type Sh2 = Sh` over `u1`'s tagged union `Sh` (probe `g`).
  No tracked `.glyph` file and none of the 32 apps under `examples/apps/` changed exit code
- **New gaps from record 119**, filed here: G393 (the checker reads an optional field as its type; rolling lane), G394 (a widened container alias admits writes; rolling lane, with G380) and G395 (`int` erased to `number`; the G373b entry below)
- **New gap from record 120**, filed here: G396 (a refinement's run-time issue prints the emitted TypeScript instead of the predicate; rolling lane, beside G375)

**0.1.152 — Shipped 2026-11-21 · G391, an optional field is an absent key or a `T`, and a `null` in it is refused** (record 119; breaking; supersedes 0.1.68's "null is absence")
- **G391 and D45.** `f?: T` is an absent own key or a `T`. One presence helper in the emitter writes "present" as `Object.prototype.hasOwnProperty.call(v, "f")`, and every optional-field site uses it: a record's `parse` and `is`, a union's inline record payload, an anonymous inline record field and a generic record, so `json.parse<T>` in both import spellings, `T.parse`, `T.is`, `is T`, `T.schema` and `json.parse_with` give one answer. A key holding `null` is a `type` issue unless `T` is `Nullable`, with ``field `timeout_ms` is null; declare it `timeout_ms?: Nullable<number>` to accept null`` (a field that is an `Option<X>`, written inline or through any chain of aliases under either import spelling, is told `Nullable<X>`, since `Nullable` of an `Option` under any name is E0227; when `X` is itself an `Option` or a `Nullable` the issue says ``field `t` is null, and its type has no spelling that accepts null``, and when the chain names a sibling's own type the suggestion is left out, ``field `t` is null, and its type does not accept null``; a test compiles every suggestion the tests print, record 124). A key holding `undefined`, which only a host value carries, is refused always: ``field `timeout_ms` is undefined; omit the key``. Nothing is copied or mutated, and `Nullable`, `Option`'s encoding, required fields, `json.stringify` and the `gen` mappings are unchanged
- **Why it was wrong.** 0.1.68 read a `null` in an optional field as absence, but the check returns the input by identity, so the `null` stayed in the value: `{ timeout_ms: 5000, ...opts }` gave `null`, `cfg.timeout_ms * 2` gave `0`, and `t?: Option<number>` holding `null` decoded `Ok` and then threw `TypeError` in a `match`. Both premises of 0.1.68 are gone: no generator writes a `?` field expecting null, and `f?: Nullable<T>` (D45) is the spelling for absent, null or a value
- **Measured.** A `null`, a host `undefined`, an absent key and `5`, through 15 routes (both `json.parse` spellings, `T.parse`, `T.is`, `is T`, `parse_with`, a field, an array element, a `Record` value, a generic argument, an optional field of the record's type, a union payload, an `Option` payload, a sibling's record by name and by namespace): `Err Err Ok Ok` on every route, where 0.1.149 gave `Ok Ok Ok Ok` on all 15. `s?: Nullable<int>` gives `Ok Err Ok Ok`. On the corpus, against the 0.1.150 branch tip: `check --no-tsc` and `fmt --check` give identical output on all 517 tracked `.glyph` files; 7 of 538 emitted `.ts` files change (354 lines, the presence test and the new issue branches); the 32 apps give the same `check` exit (with `tsc` and their `@example`s) and the same `run` result; and no recorded fixture or wire sample in the repo goes from `Ok` to `Err`, since none holds a `null` in an optional field (the Discord frames' `"s": null` is in keys their `@open` records do not declare). The G391 catch case is a `boundary` case now (`null-in-optional-field`), off the page's gap list. The suite is 2355 tests, from 2351
- **Breaking.** At run time only; no exit code of `check` changes. `json.parse<Opts>("{\"timeout_ms\": null}")` with `type Opts = { timeout_ms?: number }` is `Ok` under 0.1.149 and `Err` here with path `["timeout_ms"]`: the smallest program that returns 0 on `Ok` exits 0 under 0.1.149 and 1 on this build, and `check` and `check --no-tsc` exit 0 on both. `T.is` and `is T` go from `true` to `false` on the same input, and every route above goes from `Ok` to `Err` for a `null` or `undefined` in a `?` field whose type is not `Nullable`
- **Outside apps.** glyph-kanban and glyph-hello give the same `check` result and byte-identical emitted TypeScript under 0.1.151 and this build; neither declares an optional field, so no payload they decode changes. glyph-hello's one error on both, E0105 for `http.serve`, predates this release
- **What it does not do.** Glyph's own checker still reads an optional field as its type without `tsc` (G393, filed for its own release); this release changes only what the run-time check accepts

**0.1.153 — Shipped 2026-11-21 · G373, a `Record`'s key type is invariant** (records 113 and 119; breaking). Decided in record 119, option (3). Before it, `fn narrow(r: Record<string, int>) -> Record<Tier, int> { return r }` passed both checkers, and so did the reverse: `let wide: Record<string, int> = r` followed by `mut wide["zzz"] = 3` wrote a stray key into the caller's `Record<Tier, int>` (record 119, probe `c/t`), so refusing only the narrowing would not have closed it.
- **The rule (D30 rewritten).** `Record<K1, V1>` is assignable to `Record<K2, V2>` only when K1 and K2 are the same key domain and V1 is assignable to V2; the value stays covariant, like `Array` (G394). Same domain: `string` and every D46 second name of it; two string-literal unions with equal member sets (a single literal is a one-member set); a refined type with itself only, through D46 hops and across a file edge (an imported declaration now carries whether it is refined); a type parameter with itself inside its declaring body, compared after inference at a call. A key the checker cannot decide (an `extern_ts` or package type) refuses nothing. `Position::RecordKey` is now the invariance rule, and the rule applies however either map is spelled, written out or through a `type` whose body is one (`type Scores = Record<Tier, int>`, local or a sibling's)
- **`std/record`.** `get(r: Record<K, V>, key: K)`, `has(r, key: string)`, `keys` returning `Array<K>` (an `Array<string>` for an `int` or `number` key, or a refinement of one), `values`, `set(r, key: K, value) -> Record<K, V>` and `remove(r, key: K) -> Record<K, V>`. The call binds `K` from the record argument alone, through aliases, so a key argument is checked against it and never widens it. `record.remove` on a `Record` whose `K` is a string-literal union, by any spelling, is E0211 at the record: ``record.remove takes a Record whose key type has no fixed member set; `Record<Tier, int>` holds every `Tier`, so removing one leaves a value of no declared type``, with the help ``For a partial map over `Tier`, declare a record with optional fields (`{ a?: int, b?: int }`); to replace a value, use `record.set`.`` The record in the help is built from the key's members and the value type as written, offered only when every member is a field name, and a test compiles every one the tests print. The runtime `record.ts` is generic in `K extends PropertyKey`, with `V` first and `K` defaulting to `string` so a call written with one type argument (`record.set<Message>(...)`, which the `i18n` app has) still compiles, and with the key argument typed `KeyOf<K>` (`[K][K extends unknown ? 0 : never]`, declared in the file) so `tsc` binds `K` from the record as Glyph does. The first cut used `NoInfer<K>`, a TypeScript 5.4 intrinsic, which broke every project on TypeScript 4.9 to 5.3 that reached `std/record`, below the 4.9 floor the getting-started guide documents (record 126). A new gate, `scripts/check_runtime_ts_floor.py`, compiles the whole runtime under `typescript@4.9.5` (`--strict --lib es2022,dom`) in CI and in `check_release.py`, so the floor is checked rather than claimed. It fails on the `NoInfer` version with three TS2304 and one TS7053 in `std/record.ts`, and passes on the fixed one; record 126's `use.ts` probe compiles clean under 4.9.5, 5.3.3 and 6.0.2. The gate writes its own tsconfig and models both configurations `glyph build` generates: without `@types/node` (the runtime's shims, `"types": []`) and with it (`"types": ["node"]`, using the `@types/node` DefinitelyTyped publishes for TypeScript 4.9, pinned in the CI lockfile), with `types` and `typeRoots` set so an `@types/node` elsewhere on the machine cannot make it pass (record 127). `set`'s value slot stays `unknown` in the checker, as its row already explained. Emitted call sites do not change
- **A fix, not a break: `std/http` compiles on TypeScript 5.x without `@types/node`.** `query` and `form` iterated a `URLSearchParams` with `for...of`, which needs `dom.iterable` on TypeScript before 6, so a project without `@types/node` on TypeScript 4.9 to 5.x could not build a program that reached `std/http` (TS2488). Both use `URLSearchParams.forEach` now, with the same order and the same last-value-wins result, and the generated tsconfig's `lib` is unchanged. Found by the floor gate's first pass (record 127)
- **Errors.** E0204 at a `let`, `const` or `return`, E0211 at an argument, with a note and help computed from the program: for `narrow` above, the note ``a `Record`'s key type must match exactly. A `Record` keyed by `string` may hold keys outside `Tier` or lack one of its members, and a `Record` keyed by `Tier` passed on as one keyed by `string` can be written a key outside `Tier`.`` and the help ``Check it with `is Record<Tier, int>` or `json.parse<Record<Tier, int>>`.``; for the widening, ``Keep the key `Tier`: declare this position `Record<Tier, int>`, or make the function generic over its key (`<K: string>`) and key the `Record` by `K`.``; for two literal unions, the members one has and the other lacks. The declared type in a suggestion is the annotation as written (a `let`, a `const`, a return type, a local function's parameter), so it says `int` where the headline still says `number`. A key argument outside `K` keeps the existing literal-union E0211 with its accepted values. No new code. A type parameter in a refusal is printed by its name (`Record<K, number>`) inside its own body
- **Measured.** Every breaking program below exits 0 on the published 0.1.152, the previous release, and 1 on this build, under `check --no-tsc` and full `check` alike, except `Tier` into `Tier3`, which `tsc` already refused (full `check` 1 on both). Record 119's probes agree: `c/t` and `c/w` go from 0 to 1, and `c/u`, the `int`/`number` pair, stays 0. On the corpus against the 0.1.152 branch tip: `check --no-tsc` gives byte-identical output on all 519 tracked `.glyph` files, and the 32 apps give the same `check` exit (with `tsc` and their `@example`s; four fail identically on both for reasons of their own) and byte-identical emitted TypeScript, the copied runtime `std/record.ts` being the only file that differs. No tracked program is refused, as record 113's count of string-keyed uses predicted. The `i18n` app writes `record.set<Message>(...)` and `record.get<Catalogue>(...)`, which is why the runtime keeps `V` first
- **Not in it.** Value variance; lifting E0224 (needs G372 first); any run-time check change; an undecidable key as a refusal; the `int`/`number` pair, which is still accepted and still printed as `number` until G373b; and `record.set`'s value slot, still `unknown` to the checker and refused only by `tsc` (G402)
- **Breaking.** Exit 0 becomes exit 1 for: `Record<string, V>` into `Record<Tier, V>` and back; the same against a refined-string key, both ways; `Record<Tier, V>` into `Record<Tier3, V>` (under `--no-tsc`; `tsc` refused it already); a type-parameter key widened to `string` inside a generic; a `type Scores = Record<Tier, int>` passed on as `Record<string, int>`; `record.set` or `record.get` with a key outside `K`; and `record.remove` on a literal-union `K`

**Planned, after 0.1.153 · G373b, `int` keeps its identity in the checker** (record 119; breaking; G373 itself landed without it, and `Record<int, V>` against `Record<number, V>` is still accepted). G395: the lowerer erases `int` to `Primitive::Number`, so `Record<number, int>` passes where `Record<int, int>` is declared and the map can hold `{"1.5":7}`, which the validator refuses, and diagnostics print `int` as `number`. Give `int` an identity in `Ty` (a `Primitive::Int` assignable to and from `number` everywhere except a `Record` key, or a named prelude type), make `int` and `number` two key domains under G373's invariance rule, and show `int` as `int` in every diagnostic. This touches every match on `Ty`, the G75 pattern, which is why it is its own release. The key refusal does not ship before the identity does, or its message would read "expected `Record<number, number>`, found `Record<number, number>`".

**0.1.154 — Shipped 2026-11-21 · G384, a dropped `async` call is refused (the pending type)** (record 129, decision (b); breaking). New D51 and E0238. The `forEach` half is split out to its own release, below.
- **The pending type.** A call to a callee Glyph's checker knows is async (an `async fn`, an async lambda, a value of type `async fn(...) -> T`, a stdlib function modeled async such as `http.get` or `time.sleep`) is a new `Ty::Pending(T)`, displayed as "a pending `T`", and `await` unwraps it. Before this the call typed as its result, so `let n: number = load()` passed every Glyph stage, `let a: string = load()` was E0204 with the wrong reason, and `match try_save(2)` was checked against a `Result` the program never held. The call an `await` applies to is read off the emitter's own rule (the head of the chain in `await load(p).map_err(f)`), so the documented Result idiom is unchanged. Every match on `Ty` in the three crates that hold one handles the variant: the relation does not judge it (E0238 does), a refused argument binds no type parameter, `try_save(1)?` is not also E0202, and `change_signature_type` reports a pending argument as failing whatever the parameter becomes.
- **Where it may stand.** The operand of `await`; `return` or the tail value of an async callable; the initializer of a named, unannotated `let` that is read later; an argument to a callee with no Glyph signature. Everywhere else is E0238: a statement (the block's final statement included, unlike E0217), `let _ =`, a never-read `let` (a `_`-led name included), a typed `let`, a `match` scrutinee, an operand, `${}`, a field, an element, a member read, a Glyph-typed argument, and `return` from a sync function. A pending `Result` is E0238 and not E0217. The message names the call and callee as written; the help is `await ...` in an async function, `${await load()}` in an interpolation, `let _ = await ...` for a discarded `Result`, and in a sync one ``Make it `async fn sync_caller` and write `await save(10)`, or start it on purpose with `task.detach(async fn() -> void { await save(10) })` ``.
- **`task.detach(work: async fn() -> void) -> void`** in `std/task`, with a checker row. It starts the work and returns; a rejection is printed as `std/task: a detached task failed: ...` on stderr, pinned by a runtime test. `task.detach(save(1))` is E0238 with a help that passes the work as a function.
- **Apps.** Two latent bugs refused and fixed: `examples/apps/chat/main.glyph` is `async fn main` and awaits `daemon.serve(port)`, so `main` no longer returns before the bind succeeds or fails; `examples/apps/jobq/main.glyph`'s timer starts each drain with `task.detach` instead of discarding it with `let _ =`. Both build under `tsc`; jobq drained a posted job through the timer and chat served a client and still exits 1 on a port in use.
- **Measured.** Record 129's `p384/a.glyph` exits 0 with two E0217 warnings on the published 0.1.152 and on 0.1.153, and exits 1 with seven E0238 on this build; the original chat and jobq exit 0 and then 1, with one E0238 each. Across every tracked `.glyph` file and the 32 apps (`check --no-tsc`, 142 units), the only newly refused lines are those two app lines and the new `tests/negative` case. Fuzz seed units are excluded as expected hits: the seeds are lexer, parser and formatter inputs, not programs the project claims compile, and `glyph-compiler/fuzz/seeds/examples-03_react_component.glyph:45`, a stale copy of the React example whose `debounce` callback returns an un-awaited call, is refused by the sync lambda rule and left unchanged.
- **Breaking.** Exit 0 becomes exit 1 for any program that calls an async function without awaiting it in a refused position, including `let _ = f()` and `let n: T = f()`.
- **Sync lambdas (record 133).** A sync lambda whose tail or `return` value is pending is E0238 whether or not it writes its return type, so `timers.every(200, fn() { save(3) })`, `task.all([fn() { save(1) }])`, `array.map(xs, fn(x: number) { save(x) })` and `task.detach(fn() { save(1) })` are refused, and the call of such a lambda is not also a false `found void`. The help for the unannotated form is read off the parameter the lambda fills (record 135): `async fn() -> void { ... }` for `task.detach`, `async fn() { ... }` for a parameter of type `async fn() -> T`, a lambda that calls `task.detach` for a sync `void` callback such as `timers.every`, a loop or `task.all` in prose for a sync callback whose result is kept such as `array.map` (no `async fn(` there, which would build an array of promises, G407), and for a lambda that is not an argument, or one passed where the checker has no function type (`task.all`), the rewrite or `task.detach`. Each applied help passes full `check`, `tsc` included. The help for a call that settles to a `Result` says to handle it (`match`, `?`, or `let _ = await ...`), and a pending binding's inlay hint reads `: pending number`.
- **Not in it.** An async callback into a sync callback slot (`forEach`, a timer), split out below; a promise from a callee the checker types `Unknown` (G404). Filed with this release and not yet given one: G403, E0217's exemption of a block's final statement; G404, the npm or builtin promise; G405, the prelude `Issue`'s untyped fields, which G385's sitegen case needs before Glyph's checker can see it. G406, an unannotated sync lambda typed as returning `void` whatever its tail (record 133), is filed with it too; it removes a false refusal and is not breaking.

**0.1.155 — Shipped 2026-11-21 · G387, a SQL query's text is a constant** (agent research, record 129; breaking). Decided in record 129, option (c): a check on the argument's shape stops nothing, because `let q = "...'${name}'"` then `db.query(q, [])` and `"...'" + name + "'"` both pass and Glyph's own guidance produces the hoist (record 129, finding 6).
- **The rule (D52).** The `sql` parameter of `Db.exec`, `Db.run`, `Db.query` and `Db.query_one` is `sqlite.Sql`. A value passes there, or anywhere else a `sqlite.Sql` is declared (a parameter, a `let`, a `const`, a `return`), when it is a constant string expression or already a `sqlite.Sql`: a literal with no `${}`, a literal whose every `${}` is constant, `a + b` of constants, a module `const`, or a local `let` that no `mut` in the module reassigns, whose initializer is constant. A name is followed to its initializer, so the hoist and the `+` are judged by what they were built from. A parameter, a call result, a field, a `mut`-reassigned local and any interpolation of one is E0239. A constant imported from another module counts only when its module declares it `sqlite.Sql`. As a type, `sqlite.Sql` is `string` both ways: a `number` there is E0211, and the emitted TypeScript writes `string` (the runtime declares `export type Sql = string`)
- **E0239, new.** Computed from the program, names as spelled: ``E0239: `db.query` runs its first argument as SQL, and this one is built from `name` at run time, so a value with a quote in it can change the query``, and for the hoist ``... and `q` is built from `name` at run time ...``. The help rewrites a literal at a `Db` call, or the `let` holding one, to `?` placeholders and the parameter array, keeping values already in the array in placeholder order: ``Pass the value as a parameter: `db.query("SELECT * FROM users WHERE name = ?", [name])`.`` `exec` takes no values, so its help is ``use `db.run(sql, [values])` `` with the rewritten `run` call. The rewrite is offered only where a parameter can stand (record 132): a quoted `'${x}'`, or `${x}` after a comparison, `LIKE`, `GLOB`, `LIMIT` or `OFFSET`, or in a `VALUES` or `IN` list. `FROM ${table}` and `ORDER BY ${col}` get ``... stands where SQL needs a name (a table or a column), which a parameter cannot fill; pick the name with a `match` whose arms are constant strings.``, a several-statement `exec` is not rewritten to one `run` (which prepares only the first), and a value inside a quoted string beside other text (`'%${q}%'`) gets prose (G411). A test runs every offered rewrite on node:sqlite: 0 rows for the payload, and the constant query's count for a benign input A test applies every help on record 129's probe as written and checks that the result passes `glyph check` with `tsc` and that the five built queries return or change 0 rows for `x' OR '1'='1`, where they gave 8
- **A `match` of constants is constant.** A `match` whose every arm value is a constant string expression is one of a fixed set of texts, so the column picked from a list needs no escape; one arm built from a run-time value makes it E0239 (``... built from a `match` whose arm is built from `name` at run time``)
- **One diagnosis per constant, however imported.** `lib.BASE` through a namespace draws the same message and help as `BASE` imported by name
- **`sqlite.Db` is modeled.** Its six methods have signatures read off `runtime/std/sqlite.ts`, `sqlite.open` returns it under both import spellings, and a member it does not have is E0210. `params` stays unjudged and a row stays `unknown`, as before
- **The escape.** `sqlite.from_trusted(t: taint.Trusted<string>) -> sqlite.Sql`, for a structure that must vary (an `IN (?, ?, ?)` list, a column from a fixed list). `taint.Trusted<T>` and `taint.Tainted<T>` are modeled, so a primitive where one is declared is refused at every position (E0211 at an argument, E0204 at a `let`, `const`, `return` or field), with ``Text built at run time goes through `taint.sanitize(…)`, or `taint.trust_unchecked(…)` for text you wrote.``, which at `from_trusted`'s own parameter opens ``Pass a constant query directly;`` (record 137); `from_trusted(name)` is refused without `tsc`. `taint.sanitize` and `taint.trust_unchecked` are the only producers, so every query that is not a constant is one grep away
- **Docs.** D52 in the spec; E0239 in the catalogue, `glyph --explain E0239` and `AGENTS.md`; `std/sqlite` and `sqlite.Sql` in the stdlib reference; a section on constant queries in the databases guide. The G387 catch case is a `refused` case now (`sql-by-interpolation`, E0239), off the page's gap list
- **Breaking.** Every refused program exits 0 on the published 0.1.152 and 1 on this build under `check --no-tsc`, the catch case and record 129's `p387/a.glyph` among them. No tracked `.glyph` file and none of the 32 apps is newly refused: jobq and tasks pass literals and the module `const` `SCHEMA`. One more change is `--no-tsc` only: a `taint.Tainted` value given where a `taint.Trusted` one is declared is now E0211 from Glyph's checker, where full `check` already refused it through `tsc` (TS2345)
- **Known limit (record 137).** An interpolation right after a comparison is taken to be a value. If a run-time `${x}` after `=` was meant as a column name, the `?` rewrite turns it into a value comparison. The original was an injection either way, and a column picked from constants is accepted with no rewrite
- **Cut from this release: "the other stdlib injection sinks follow."** The stdlib has no shell sink. `regex` patterns fit the same rule but need a `regex.escape` first (G409), and `http.html` needs an escaping builder, not a constant rule (G410); both are filed in the rolling lane. `child_process` and npm drivers such as `pg` are outside the stdlib and belong to the banned-import release (0.1.182)

**0.1.156 — Shipped 2026-11-21 · Release A, a lambda parameter's type is the one written** (record 130, F1 and F3; record 131 places it after 0.1.154, G384, and 0.1.155, G387; breaking; D53). Branch `dev-0.1.156`.
- **G412 and D53, the omitted marker.** A lambda parameter is a `LambdaParam` whose annotation is `Option<TypeExpr>`: `None` when omitted. The parser used to synthesize a written `unknown` for the omitted form, so the formatter and the emitter read a written `unknown` as omitted and the checker read omitted as `unknown`. Now the checker lowers an omitted parameter to "not typed here" (judged by no rule) and a written `unknown` to the judged `unknown`; `glyph fmt` keeps `fn(p: unknown) -> number { return p + 1 }` byte for byte; the emitter writes `(p: unknown)`, so full `check` refuses `p + 1` with TS18046, and an omitted annotation still emits `(p)`. No stage matches the name `unknown` or a span to tell the two apart. Hover on an omitted parameter's use now answers `not typed here: the parameter has no annotation (contextual typing is G414)`, where it said `unknown`, and a lambda with one hovers `fn(?) -> number`
- **G413, `is` arms narrow.** Inside an arm whose pattern is `is T`, a scrutinee that is a parameter, a `let` or a module `const` has type `T` in the arm's body, for `T` a primitive or a declared type (a prelude container, `is Record<K, V>` or `is Array<X>`, does not narrow yet: G415); outside the arm it keeps its own type. A name any `mut` reassigns anywhere in its scope, nested lambdas included, is not narrowed (record 134, B1: a lambda called inside the arm could rebind it). The predicate, a binding no `mut` reassigns, is one function (`reassign.rs`) that D52's constant `let` for `sqlite.Sql` now reads too, so constness and narrowing cannot disagree (record 136). `is User => v.nmae` is E0210 without `tsc`; hover shows `number` inside `is number =>` and `unknown` outside. Release B's unknown help depends on this
- **Breaking.** A lambda parameter written `: unknown` is now checked by `tsc` as declared (0 tracked files write one), and the narrowing exposes refusals `tsc` already made, under `--no-tsc` only: `is User => v.nmae`, on a local or on a module `const`, exits 0 under the published 0.1.155's `check --no-tsc` and 1 here; full `check` exits 1 on both. Record 130's `r130/f` goes from full `check` 0 to 1 (`check --no-tsc` 0 on both); `r130/g` exits 0 on both under both modes, since nothing judges `unknown` in `${}` until Release B. A name a lambda reassigns (record 134's `t/c`) is not narrowed and exits as before. E0238's help for an unannotated sync lambda now repeats an omitted parameter bare (`fn(id)`) where it used to print `fn(id: unknown)`
- **Not in it.** E0237 and the F2 `string.from` acceptance (Release B, next); contextual typing for an omitted annotation (G414); container `is` arms (G415)

**0.1.157 — Shipped 2026-11-21 · G385, only a text-like value becomes text** (records 129, 130 and 131; on top of Release A; breaking), so a record or an `Option` cannot print `[object Object]` through `${}`, `+` or `string.from`. The pending value is not in it: an un-awaited call is G384's E0238, and E0237 leaves a pending value to it.
- **The rule (D54, new).** One rule for every implicit conversion to text: each `${}` in a `"..."` or `"""..."""` literal, each operand of `+` whose other operand is a string, and the argument of `string.from`. Text-like is an allowlist (`string`, a string-literal union, `number`, `int`, `bigint`, `bool`, an alias or `where` refinement of these, `never`, and an inline `string | number`), so every other `Ty`, including any variant added later, is refused by default. A value Glyph's checker types as its internal `Unknown` (an arithmetic result until G382, an npm value with no materialized types, `Issue.path`) is not judged, and a test pins that edge. JSX children and the emitted code are unchanged
- **Errors.** New code E0237, "a value with no text form is converted to text". The message names the conversion as written and the type by its name, ``E0237: `${user}` turns a `User` into text, and a record has no text form: it prints `[object Object]` ``, with `+` joins a `User` to a string and `string.from(user)` for the other two spellings. The help is computed per type: the first text-like field of a record (`${user.id}`), a `match` bound with `let` for an `Option` or a `Result`, `string.join` for an array (naming `import std/string` or `std/array` when the module lacks it), `??` for a `Nullable`, `${d.to_string()}` for a `Decimal`, `${greet()}` for a function with no parameters, an `is string` narrowing for `unknown`, and prose for a type parameter, a tagged union (its variants listed) and a map. No help puts a string literal inside `${}`, and a test applies every help with a snippet and passes the full check, `tsc` included. `glyph --explain E0237` carries the per-type table
- **`std/decimal` is modeled.** `decimal.decimal` is `fn(string) -> Result<decimal.Decimal, string>`, `from_int` is `fn(number, number) -> decimal.Decimal`, and a `decimal.Decimal` annotation and the methods of one (`add` to `to_number`) are typed, so a `Decimal` is a type the checker holds rather than `Unknown`. `zero`, a value, stays `Unknown`
- **sitegen.** `examples/apps/sitegen/load.glyph` interpolated `i.path`, an `Array<string | number>`, and printed `items,0,name: ...` by accident. It joins the path with `.` now. Glyph's checker does not type `i.path`, so this line was never refused
- **The catch case** `gap-g385-record-in-template` is a `refused` case now, `record-in-template`, with E0237, and off the page's gap list
- **Record 130.** `string.from` takes a declared `unknown` (F2): it is the explicit renderer and the documented way to read a failed task's error, and `workflow/wire.glyph` and the docs snippets compile unchanged. `${}` and `+` still refuse an `unknown`, with the help ``Say the conversion out loud with `${string.from(v)}`; if `v` may hold a record it still prints `[object Object]`, so `match` it first: `match v { is string => "${v}", is number => "${v}", else => "?", }`.``, both of which compile now that Release A narrows `is` arms. How an `unknown` renders is filed as G416, decision deferred. A lambda parameter written without an annotation is not judged (Release A), so the tracked `Issue.path` sites are unchanged, and `array.map(users, fn(p) -> string { return "${p}" })` is accepted and pinned by a test until G414
- **Record 139.** A `+` chain is judged whole (B1): it is flattened left to right through parentheses, and when any operand is a known string every operand is judged, so `"Hello " + name + ", " + u` and `("a" + name) + u` are E0237 at `u`; an `Unknown` operand stays unjudged, and `+` results are still untyped (G382). Beside a `+`, the `unknown` help writes `string.from(v)` rather than `${string.from(v)}` (B2), and a test applies it there under the full check
- **With G384 and G387 (records 131, 139, 141).** A pending value is E0238's alone: `${load()}`, `"a" + load()`, `string.from(load())` and a `+` chain with a pending operand each give exactly one E0238 and no E0237. `sqlite.Sql` is text-like, so `"${sql} LIMIT 1"` and `"SELECT " + sql` are not refused and stay constant queries (no E0239, record 132 ruling 5). Release A's narrowing makes `match v { is string => "${v}", else => "" }` accepted, which is what the `unknown` help points at. A test pins each
- **Breaking.** Measured with the published 0.1.156 and this build, `check --no-tsc` / full `check`: the catch case `record-in-template` goes from 0/0 to 1/1, record 129's `p385/c.glyph` (`+` and `string.from` on a record, `${}` on a `T`) from 0/0 to 1/1, and record 139's `r139/t/a.glyph` (the `+` chains) from 0/0 to 1/1. Record 129's `p385/a.glyph` exits 1 on both, since 0.1.154 already refuses its pending value; this build adds seven E0237 to it
- **Measured on the corpus** against the published 0.1.156: `check --no-tsc` gives identical output on every tracked `.glyph` file and the 32 apps except the catch case and the new `tests/negative/option_joined_to_a_string` fixture, so no tracked program is newly refused; `fmt --check` is identical. The emitted TypeScript is byte-identical except two lines in `expenses` and `settle`, where a `match` over a `Decimal` method's `bool` result now carries `as boolean`, because the method is typed; both apps pass `tsc --strict` on this build

**Planned, unscheduled · G416, how an `unknown` renders as text** (record 130, F2). `string.from` on an `unknown` holding a record prints `[object Object]`. Decide whether an `Error` renders as its message and an object as JSON, through `string.from` or a new function. Changing `string.from`'s runtime changes the output of the five `Issue.path` sites (G405), so it is its own release.

**0.1.158 — Shipped 2026-11-21 · G414, an omitted lambda annotation takes its type from its position** (records 130, 142 and 143; record 131 places it after Release B and before G325; breaking; D55). Branch `dev-0.1.158`, rebased onto 0.1.157.
- **The rule (D55).** An omitted lambda parameter has the type of the matching parameter of the function type expected where the lambda stands. Six positions supply one: a call argument (the callee's parameter type, with the callee's type parameters bound from the arguments before the lambda and from written type arguments, so `array.map<T, U>` binds `T` from `xs` first); an annotated `let`; an annotated module `const`; a `return` in a callable with a declared return type; a field of a record literal in one of those positions whose declared type is a record; and an attribute of a component element whose props record Glyph knows. The built-in `Array<T>` methods that take a callback (`map`, `forEach`, `flatMap`, `filter`, `find`, `findIndex`, `findLast`, `findLastIndex`, `some`, `every`, `reduce`, `reduceRight`, `sort`, `toSorted`) supply its parameter types from the receiver's element type, for context only (`(T, number, Array<T>)`; the seed's widened type then `T` for `reduce`; `(T, T)` for a comparator), so the method spelling gets the function spelling's answer (record 142, B1); their result typing is the generated row's, unchanged. The function type is read through second names and `type` aliases. A fresh literal seed binds its widened type (`array.fold(xs, "", fn(acc, x) ..)` gives `acc: string`), and each lambda's signature binds what it can for later arguments. A written annotation is untouched
- **What it refuses.** `array.map(users, fn(p) -> string { return p.nmae })` and `users.map(..)` are E0210 under `check --no-tsc`; Release B's pinned hole, `array.map(users, fn(p) -> string { return "${p}" })` with `users: Array<User>`, is E0237 under both modes, and so is the `users.map` spelling. The pinned test is flipped and renamed (`an_omitted_lambda_annotation_typed_from_its_callee_is_judged`). A lambda in `<Card on_pick={fn(x) { .. x.nmae .. }} />` is E0210 like the record-literal form of the same props. G345's `rs.map(fn(r) { r.value })` on an `Array<Result<..>>` is E0210 now; its other forms are unchanged
- **The E0238 help reads the same slot** (record 143). The checker records the built-in method slot it typed a lambda from, and the pending pass reads it: a pending tail in `users.forEach(fn(p) { save(p.id) })` gets the `task.detach` rewrite, and one in `users.map(..)` gets the loop over `users`, where both got the generic `async fn(p)` help. A contextually typed parameter converted to text draws one E0237, and a pending call made with it one E0238, never both
- **Hover.** A use of an omitted parameter with a contextual type hovers that type (`User`), in either call spelling, and a lambda hovers `fn(User) -> string`. One with none keeps the note, reworded now that G414 is closed: `not typed here: the parameter has no annotation, and its position supplies no type`
- **A fix the corpus comparison found: a loop over a declared `unknown` decides its shape at run time and binds `unknown` names.** A two-binding `for` over a value typed `unknown` took the record form, so inside an `is Array<unknown>` arm (which does not narrow yet, G415) `i + 1` printed `01,11` under 0.1.157. It defers to `__glyph_pairs` now, and both bindings are the declared `unknown` to Glyph's checker and to `tsc`: the emitted pair is `(__glyph_pairs(v) as Iterable<[unknown, unknown]>)`, never the helper's `any` (record 146, which found that an `any` pair let `let s: string = i` through both checkers and crash at run time). Linus's `t/b.glyph` is refused by full `check` (TS2322 at `let n: number = i` and `let s: string = i`; TS18046 at `i.toUpperCase()`), where 0.1.157 refused it with TS2769 and TS2322; `t/a.glyph`'s array loops print `1,2,` under `glyph run --no-tsc`, and full `check` refuses its `i + 1` with TS18046. Contextual typing gave `01_validator`'s `parse: fn(input)` the slot's declared `unknown`, which is how this surfaced; the validator reads the index only through `string.from` and passes full `check`. The untyped case keeps the `any` helper and is filed as G419
- **Still not judged.** A lambda with no function type expected at all (`let k = fn(p) { .. }`); a parameter whose slot names a callee type parameter nothing has bound (`apply(fn(x) { .. })` against `fn apply<T>(f: fn(T) -> string)`, record 142, ruling 1, pinned by a test); an attribute lambda on a DOM element (`onClick` on `<button>`), whose type is React's; and any slot whose type the checker does not know, such as the tracked `Issue.path` sites (chat:178, pulse:36, schedule:371, i18n:88, `examples/01_validator.glyph:152` and its fuzz seed), since `Issue.path` is not typed (G405)
- **Measured.** Against the published 0.1.157 (exit codes, previous then this build): the pinned hole 0 then 1 under `check --no-tsc` and under `check` (E0237; 0.1.157's `tsc --strict` passes it); `p.nmae` 0 then 1 under `check --no-tsc`, 1 and 1 under `check` (TS2339 then E0210); `users.map(fn(p) -> string { return "${p}" })` 0 then 1 under both; the JSX attribute typo 0 then 1 under `check --no-tsc`, 1 and 1 under `check` (TS7006 then E0210); `rs.map(fn(r) { r.value })` 0 then 1 under both; the pending `forEach` and `map` tails 1 and 1, with the help changed. On the corpus (all tracked `.glyph` files outside the apps, checked one file at a time, and the 32 apps, the four with npm dependencies installed): `check --no-tsc` and `fmt --check` output is byte-identical on all 440 runs. The emitted TypeScript differs in five sources. In chat's `daemon.glyph`, fridge's `main.glyph` and `examples/corpus/lru.glyph` (and their copies built through the parent directory), nine comparisons of two values now typed `string` emit `===` or `!==` where they emitted `__glyph_eq`, which is the same comparison for two strings; full `check` with `tsc` and the `@example`s passes on all three under both builds. In `01_validator.glyph` and its fuzz seed, the array loop emits `(__glyph_pairs(input) as Iterable<[unknown, unknown]>)` where 0.1.157 emitted `__glyph_pairs(input)`, the fix above; both pass full `check` under both builds
- **Breaking.** Yes: the pinned hole and the method-form text conversion go from exit 0 to exit 1 under full `check`, and every field typo or text conversion inside a lambda passed to a typed slot goes from exit 0 to exit 1 under `check --no-tsc`. No tracked program is newly refused
- **Not in it.** Result typing of the array methods (`users.map(..)` is still untyped where its row is); the two hover bugs found while measuring this, G417 and G418, and the untyped-iterand `any` pair, G419, all in the rolling lane

**0.1.159 — Shipped 2026-11-21 · G384's second half, an async callback into a sync `void` slot is E0211** (records 129, 131 and 142; split out of 0.1.154; breaking). D40 is amended, and G407 and G408 close with it.
- **The rule.** An async function value (an `async fn`, an async lambda, a value of type `async fn(...)`) passed where a sync `fn(...) -> void` is declared is E0211 with its own message, `` `timers.every` calls its callback without waiting, so each call of this `async fn` runs on its own and its errors are lost ``, naming the callee as written (`forEach` alone for an array's method). The help is computed from the program: where the slot is an array's `forEach` and the caller can wait, `for id in ids { await save(id) }`; otherwise a sync callback that starts the work on purpose, `fn(id: number) { task.detach(async fn() -> void { await save(id) }) }`, with `import std/task` named when the module lacks it. A lambda body the help cannot quote whole is written `...` and the help says so. A test applies each help and runs the full `check`, `tsc` included.
- **The relation (D40 amended).** Two functions of different kinds are decided by kind alone, in both copies of the function arm: async where sync is expected is refused unless the expected return is one the checker cannot read; sync where async is expected is refused whatever it returns. So an async function bound to a `fn(number) -> void` is E0204, `array.map(xs, async fn(x: number) { await save(x) })` is E0211 (G407), and `task.detach(tick)` with a sync `fn tick()` and `run_num(ext)` with an `extern_ts` return are E0211 (G408). A lambda E0238 already refused keeps its single diagnostic (record 138).
- **Modeled slots.** `timers.after` and `every` were typed already. The `std/net` handlers (`listen`'s connection handler, `on_stop`, `on_server_error`, `on_connect`, `on_text`, `on_data`, `on_close`, `on_error`) and the `std/websocket` ones (`listen`, `on_open`, `on_message`, `on_binary`, `on_close`, `on_error`, `on_stop`) are, with a `websocket.Socket` left `Unknown`. The built-in `Array<T>` methods' callbacks are read from the slot 0.1.158 models for contextual typing (D55), for typed rows (`forEach`, `map`) and untyped ones (`filter`, `find`, `reduce`, `sort`) alike, so `users.forEach(async fn(p) { await save(p.id) })` types `p` as `User` and is refused from the same slot the E0238 help reads; `map`'s slot is judged as one that keeps a value. `catches/gap-g384-dropped-async-call` is `async-callback-nothing-awaits` now, a refused case, and G384 is fixed.
- **One mistake, one diagnostic.** An argument refused for its kind of function leaves untyped only the part of the call's result it decides, a return that names a type parameter the refused parameter names (record 148). So a `${}`, a `+` or a typed `let` over `array.map(ids, async fn ...)`, whose `Array<U>` the callback binds, adds no E0237 or E0204 for the same callback, while `let s: string = apply(async fn(n) { ... })` with `fn apply<T>(f: fn(T) -> void) -> number` is E0211 at the callback and E0204 at the `let`, as on 0.1.158.
- **Measured.** Published 0.1.158 against this build, exit codes under `check --no-tsc` and under full `check`: the catch (`ids.forEach(async fn ...)` in a sync function), a `forEach` probe (named and lambda), `timers.every`/`after` with async lambdas, a user `fn(number) -> void` slot given `save`, and the G407 `array.map` probe each go 0 to 1 under both, with E0211; `task.detach(tick)` and `run_num(ext)` go 0 to 1 under `check --no-tsc` and stay 1 under full `check`, where 0.1.158's `tsc` already refused them (TS2345) and this build refuses them with E0211 before `tsc` runs. Across every tracked `.glyph` file at 0.1.158 (523) and the 32 apps, 440 units: `check --no-tsc` adds one diagnostic and removes none, the E0211 on the catch unit `catches/gap-g384-dropped-async-call/glyph.glyph` (the case under its 0.1.158 name); `fmt --check` changes on no file; the emitted TypeScript is byte-identical for the 28 apps that build without `npm install` and the 248 standalone files that build alone, and the catch, which built before, is refused now.
- **Breaking.** Exit 0 becomes exit 1 for a program that passes an async function to a sync `void` callback, binds one to a `fn(...) -> void`, gives a sync function where an async one is declared, or gives `array.map` an async lambda with no return type.
- **Helps for the other two refusals (record 144).** A sync function where an async one is declared: `` `task.detach` takes an async function, and `tick` is not async. Pass `async fn() -> void { tick() }`, or declare it `async fn tick()`. ``, with the parameter's return in the wrapper and no wrapper where the function's return cannot be read (`run_num(ext)` gets ``Declare it `async fn ext() -> number`.``); a sync lambda is told ``Write it `async fn() { ... }`, keeping its body.``. An async callback to `array.map`: `` `array.map` does not wait for its callback. Await each call in a loop, `for x in xs { ... await save(x) ... }`, or run them together with `task.all`. ``, with no `async fn(`. Each is applied in a test and passes full `check`.
- **Not in it.** G421, filed with this release for the rolling polish lane: the same pairings at a `let`, `const`, `return` or field, and a component prop, keep the general E0204 or E0211 message and help; `time.debounce`'s callback takes a rest parameter Glyph cannot spell and stays unmodeled; G406 is unchanged.

**Planned, after 0.1.159 · G386, `bool` operands for `||`, `&&` and `!`** (agent research; breaking), so `n || 5` cannot turn a real `0` into `5`. `??` and `==` already cover the intended uses.

**Planned, after 0.1.159 · G359, G360 and G361, `glyph fix` against the diagnostics it reads** (record 106). `glyph fix` skips an unused import whose name an E0200 in the same file lists as a missing variant, instead of deleting it; its decline names E0216 when the catch-all arm comes before the head, rather than claiming a rename would move a case; and after an applied rename completes a match it recomputes or drops the E0200 it read before the rename. One release, since all three are the fix driver reading a diagnostic list it does not refresh

**Planned, after 0.1.159 · G379, two top-level declarations on one line are a parse error** (record 117; breaking). A top-level declaration that begins on the line where the previous item ended is E0013 with a declaration-specific help, so D1's title holds at the top level too and `grep '^pub fn b'` always finds `b`. Decide first whether an import followed by a declaration on the same line moves from E0002 to E0013, so one code means one item per line. No tracked file hits it

**0.1.160 — Shipped 2026-11-21 · G380, a mutating method call needs `mut`** (decision record 150; breaking; D5 amended, reversing Q7). Branch `dev-0.1.160`, cut from 0.1.159.
- **Which methods.** The generated method rows gain `mutates`. On an `Array<T>` it is `push`, `pop`, `shift`, `unshift`, `splice`, `sort`, `reverse`, `fill` and `copyWithin`; every other array method (`toSorted`, `toReversed`, `toSpliced` and `with` included) and every `string` and `number` method is non-mutating. `lib.d.ts` has no marker, so the split is written by hand in `scripts/check_builtin_members.py` as `MUTATING` and `NON_MUTATING` per receiver, and held two ways: generation fails on a method in neither set or both, so a TypeScript bump that adds one fails CI until someone classifies it, and node calls every array method with sample arguments on a fresh `[3, 1, 2]` and checks the receiver changed exactly when the method is in `MUTATING` (moving `sort` to `NON_MUTATING` fails it, checked once on purpose)
- **E0240, a call that changes its receiver outside a `mut` statement.** A mutating call may stand only as the whole call of a `mut` statement. A bare statement gets ``E0240: `xs.push(4)` changes `xs` in place, and a call that changes a value is written as a `mut` statement`` with ``write `mut xs.push(4)`, so `grep "^\s*mut "` finds every change to `xs` ``, and `glyph fix` inserts the `mut` (only there: the result re-checks clean, survives `fmt --check`, and a second fix is a no-op). A call whose value is read (a `return`, a `let`, an argument, an operand, the receiver of another call) gets ``its value is used here`` and a help per method: `array.sort(xs, cmp)` and `array.reverse(xs)` return a copy, `array.get(xs, array.len(xs) - 1)` and `array.get(xs, 0)` before `mut xs.pop()` and `mut xs.shift()`, `array.slice` before `mut xs.splice(i, n)`, `array.len(xs)` after `mut xs.push(v)` or `array.push`/`array.concat` for a new array, and the statement form for `fill` and `copyWithin`; each names `import std/array` when the module lacks it, and none names `toSorted` or `toReversed`. An arm of a statement `match` is told to become a block, `{ mut xs.push(1) }`. Under `mut`, the receiver must be a place by the parser's own `is_place` predicate (moved into `glyph-ast` so both read one definition), so `mut make().push(1)` and `mut ys.filter(f).sort(cmp)` are E0240, ``.sort(cmp)` sorts the new array `ys.filter(...)` returns, and nothing keeps it``. A receiver typed by an array method that returns a new array (`filter` is overloaded, so its row is untyped) still counts as an array here. A parameter, a field path, an index and a module `const` are places
- **E0241, `mut` on a call that changes nothing.** A non-mutating method of an `Array<T>`, a `string` or a `number` under `mut` (`mut xs.concat(ys)`: ``returns a new array and leaves `xs` unchanged; this `mut` marks no change and the result is discarded``, help `mut xs = xs.concat(ys)`), and any function read through a namespace import of a Glyph module, the standard library or a sibling (`mut array.push(xs, 1)`, help `mut xs = array.push(xs, 1)` or `mut xs.push(1)`; `mut io.println(s)` and `mut heap.push(h, 5)`, help ``a function call is not written `mut`; the change it makes is a `mut` statement inside it (D5)``). Decided from what the name resolves to, so a local `result` that shadows `std/result` is a value and `mut result.push(v)` is accepted. Never fixed by `glyph fix`
- **Not judged, in either direction.** A receiver the checker types `Unknown` (an npm value, an `extern_ts` result, an omitted lambda parameter with no contextual type, pinned by a test); an npm namespace's functions; a record field holding a function; and the stdlib `Set`, `Store` and `Deque`, whose receivers the checker does not type yet (G423, filed with this release and scheduled next). Writes to state outside the program's data (`Rng.next`, a database, a socket) are not D5's. `let n = mut xs.pop()` and `mut let` stay parse errors
- **Docs.** D5 in the spec states the rule and the edge in so many words; `05-typechecker.md` records the Q7 reversal; AGENTS.md (and its `llms.txt` mirrors) describes both codes in the `mut` section; `docs/error-codes.md` and `--explain` carry E0240 and E0241; the greppability answer shows E0240 and names the `Set`/`Store` edge in its status row; Q47 in `open-questions.md` asks whether a call site should say it lets a callee change an argument, which this release leaves out on purpose
- **The catch case** `gap-g380-mutation-without-mut` is the refused case `sort-without-mut` now, with E0240 and the `array.sort` help, and off the page's gap list
- **Measured.** Record 150's `a_basic.glyph` (`return xs.sort(cmp)[0]` in a helper, a bare `xs.push(4)`, `let n = xs.pop()`) exits 0 under the published 0.1.159's `check --no-tsc` and 1 under this build. Across every tracked `.glyph` file at 0.1.159 and the 32 apps, built one unit at a time with `build --no-tsc --no-test` under both binaries (394 units: each app, each `tests/exact-or-absent/*/src`, each catch directory, every other file alone), exit codes and diagnostic codes are identical on 393; the one difference is the catch unit, which goes from 0 to 1 with one E0240. Re-run after record 151's fixes: the same, 393 identical and the catch unit the one difference. No tracked `.glyph` program needed migrating: the 230 array writes the corpus already wrote as `mut` statements stay accepted, and no tracked file put `mut` on a pure method or a module function. The workspace suite found one site the corpus cannot see, a Glyph program inside a Rust test (`a_method_callback_is_judged_like_the_function_spelling`) that read `users.sort(..)`'s value; it is a `mut` statement now, and the test still checks the `sort` callback's slot
- **Record 151 (review of this branch).** Where a call stands is now read off the pending-value pass's slots (`pending.rs`), so "is this value used" is decided once. The last expression of a block whose value is used (a callable with a declared or position-given return that is not `void`, or a block arm of a `match` in value position) is a value: `fn f(ys: Array<number>) -> number { ys.push(4) }` is E0240 with ``write `mut ys.push(4)` as a statement, then return the value explicitly, `return array.len(ys)` ``, and a `match` in that position does not offer the statement-arm block. `glyph fix` applies the insertion the checker attaches to a statement E0240 rather than re-deriving the shape, so it no longer turns that function into one `tsc` refuses (F1). A receiver reached through an index of an `Array<E>` is judged as an `E`, so `rows[0].items.sort(cmp)` and `grid[1].push(6)` are refused; the index read keeps its type elsewhere (F2). `mut s.opt?.push(1)` parses, with `?.` on the method link only, and is what the E0240 help and fix write for a write through an optional field; assignment through `?.` stays E0002 (F3, amending record 150). A `defer` of a mutating call gets ``let undo = fn() -> void { mut xs.push(1) }`, then `defer undo()` `` (F4). No help names a comparator the program lacks (a `number` or `string` element gets one written out) or puts `...` in a code span (F5). `glyph fix` counts a file it rewrites twice in one run once (F6). Filed with it: G424 (a non-`void` body ending in a `mut` statement passes without `tsc`), G425 (a method read by index), and a note on G393
- **Breaking.** Yes: a mutating array call written without `mut`, or whose value is used, and `mut` before a call that changes nothing, go from exit 0 to exit 1 under `check --no-tsc` and full `check`
- **Not in it.** G423 (next); G394, which this makes judgeable at the `mut` sites and is its own release after G423; marking the caller side (Q47); the `?.` parse error that points at the next line's token (record 150, a polish item)

**Planned, after 0.1.160 · G424, a non-`void` body ending in a `mut` statement is refused without `tsc`** (record 151, S1). Only `tsc` refuses `fn f(ys: Array<number>) -> number { mut ys.push(4) }` today (TS2355). The checker refuses a non-`void` callable whose body can end without a value. Breaking under `--no-tsc` only.

**Rolling lane · G425, a method read by index** (record 151, S2): `xs["push"](1)` passes both checkers and throws at run time. Refuse a string-literal index naming a built-in method, with E0234's help. Not an audit hole, so not scheduled ahead of G423.

**Rolling lane · G426, the help for a call on an optional receiver keeps its `?.`** (record 152, R1; record 155, F4): `let n = s.opt?.pop()` gets a help naming `mut s.opt.pop()` and `array.get(s.opt, ...)`, which `tsc` refuses. Write `mut s.opt?.pop()` and drop the read-first clause; print the whole assignment in E0241's optional help. The E0242 help for `rows[0].opt?.at(0)` writes `array.get(rows[0].opt, 0)`, the same drop. Pinned by a help-compiles test.

**Rolling lane · G430, a fix decline inside an unannotated lambda names no `?`** (record 156): the E0242 decline says ``the declared return is `?` ``; say the lambda declares no return type.

**Rolling lane · G431, an unknown member on a `boolean` is refused without `tsc`** (found re-reproducing G272): `same.to_string()` on a `boolean` passes `--no-tsc` and is TS2339 under `tsc`. Extend the generated method table to `boolean` so the checker judges its members, as it does for `number`.

**Rolling lane · G437, a literal missing a required field is named by its shape** (record 160): ``expected `Opt`, found `record`: missing field `name` `` prints the category word for the literal's side. Render a structural record by its fields there, with G245, which is the same display on an inline props record.

**Rolling lane · G440, `string.from` without its import is E0103** (found re-reproducing G416): the name `string` resolves to the primitive type in value position, so `string.from(5)` with no `import std/string` passes `--no-tsc` and `tsc` refuses it (TS2693). Refuse it as `array.len` without its import is refused, with help naming the import.

**Rolling lane · G442, `{cond && <el/>}` with a number condition** (found writing the `/from-typescript/` React component): `{props.n && <p>x</p>}` with `n: number` passes `--no-tsc` and `tsc` and renders `0`, while `AGENTS.md` says JSX does not take `{cond && ...}`. One of the two is wrong; deciding which, and whether `<if cond>` must take a `bool`, comes first.

**Rolling lane · G443, a ternary inside JSX is E0013** (same source): `{c ? <a/> : <b/>}` is E0003 "unexpected token: Slash" at the closing tag. Report E0013, with help naming `<if>`/`<else>` or `<match>`.

**Rolling lane · G444, watch mode beside a Vite dev server** (found verifying the `/from-typescript/` hot-reload line): every rebuild rewrites the output's `tsconfig.json` and runtime files, so Vite reloads the page instead of hot-swapping, and the documented `--out dist/gen` sits in Vite's default build directory, which its dev server does not watch. Write only changed files, and move or annotate the documented layout.

**Rolling lane · G445, `glyph fmt` joins `{match ...}` JSX children onto one line** (same source): layout only; `<match>` elements format one child per line.

**Rolling lane · G446, E0240's value help for an array no name holds** (found building G423 step 2): `let n = make().pop()` gets a help that reads `array.get(make(), ...)` and then writes `mut make().pop()`, which E0240 refuses. Give it the bind-first form the statement position already has, as 0.1.167 does for the stdlib `Set`, `Store` and `Deque`.

**Rolling lane · G448, a write through a structural interface evades D5** (record 171, probe i01): a `set.Set` passed where `interface Adder { add: fn(number) -> void }` is declared is written by `a.add(1)` with no `mut`, and an array through a `push` interface the same way. Needs a decision before it is built: refuse passing a type with Mutating members where an interface naming them is declared, or require `mut` on a call through an interface member. Goes to the batch reviewer; D5 lists it as not covered meanwhile.

**Rolling lane · G449, `Some(x)` infers no type** (record 171, probes a04 and a07): `let o = Some(seen)` is untyped, so a wrong annotation and a write inside `match o` go unjudged. Give the `Option` its payload type.

**Rolling lane · G450, an array of array literals joins its inner element types** (found building 0.1.168): `[["read"], ["write"]]` is `Array<Array<?>>` today, so a declared `Array<Array<number>>` is not compared. Refusing what it lets through is breaking, so it gets a corpus run of its own.

**Rolling lane · G451, the index binding of `for i, x` is a `number`** (record 173): `let c: string = i` passes `--no-tsc` and `tsc` refuses it.

**Rolling lane · G452, a nested string in a `"..."` interpolation inside a `"""` gets the generic E0002** (record 174, R1, probe s05): `"""${g("q${g("x")}")}"""` says "expected valid expression ... found expected matching `}`" with "Add the expected token". Give it the hoist-or-triple help G441's refusal gives.

**Rolling lane · G453, a `"""` string cannot hold a literal `${`** (record 174, R2, probe s02): `\${y}` inside `"""` is a backslash and then an interpolation, so `y` is E0103. A D22 question: whether the raw form gets a spelling for a literal `${`. The E0002 escape help also shows a tab or a zero-width space inside a nested literal as the raw character, which compiles but reads wrong in a terminal; re-escape non-printing characters in the shown code (record 177, R1).

**Rolling lane · G455, two recursive types of one shape are compared by name** (record 176). `type F = Array<F>` against `type G = Array<G>` is E0204 and `tsc` accepts it. A recursive transparent declaration keeps its name since 0.1.170; comparing two structurally needs a coinductive relation, a new rule for the whole relation that touches E0204's messages and `glyph_impact`. The same decision covers the silence on the other side (record 178): a recursive declaration is not decided against a type of another shape, so `G` returned as a `number` and `Array<G>` as an `Array<number>` pass `--no-tsc` while `tsc` refuses both. Needs its own decision before it is scheduled into a release.

**Rolling lane · G458, `Nullable<T>` as a map's value type accepts a `T` field** (record 180, N1): `fn j<T>(x: One<T>) -> Record<string, Nullable<T>> { return x }` is E0204 and `tsc` accepts it; `map_field_incompatible` compares the field with `V` by equality and does not apply D45. Fixing it removes a refusal. Fixed by 0.1.172.

**Rolling lane · R1 (record 171), `glyph fix` on `let _ = (q.pop_back())`:** the rule declines with the node-drift reason; accept a parenthesized call, or give the true reason.

**Rolling lane · R2 (record 171), the value help at module scope:** `const N = ITEMS.pop()` and `const N = Q.pop_back()` are told to write a `mut` statement, which module scope cannot hold.

**Rolling lane · R3 (record 171), concrete store-update bodies:** `HITS.get().pop()`, `shift`, `splice` and `fill` get the generic `update(f)` help; give them bodies such as `array.slice(current, 0, array.len(current) - 1)`.

**Rolling lane · R4 (record 171), `HITS.get().<method>` in value position:** inside `${}` it gets the statement-form `update` help, which does not say where the value went.

**Rolling lane · R5 (record 171), the class test skips a type it cannot build:** `every_modeled_stdlib_method_has_exactly_one_class` continues when `stdlib_type_members` answers `None`; assert `Some` for every modeled type but the known exceptions.

**Planned, its own release · G427, the built-in method table follows the library the emitted tsconfig declares** (record 153, finding 4). `builtin_methods.rs` is generated from `esnext` and `glyph build` writes `"lib": ["es2022", "dom"]`, so Glyph's checker types `toSorted`, `toReversed`, `toSpliced`, `with`, `findLast` and `findLastIndex` and `tsc` refuses them (TS2550). Decide whether the table follows the tsconfig or the tsconfig moves to `es2023`; record 150 kept the newer methods out of every help, so nothing shipped depends on them.

**Planned, its own release · G428, string indexing, `charAt` and a one-unit `slice` hand back half a surrogate pair** (record 154). `s[2]`, `s.charAt(2)` and `string.slice(s, 2, 3)` on `"a😀"` all give the lone second half. 0.1.161's `string.char_at` answers `None` there; whether to refuse or redirect the three older spellings is open, and touches every string index in the corpus, so it is not folded into G392.

**Planned, after G423 · G394, a widened container alias admits writes the original's type refuses** (record 119; record 150, section 5). G380 makes every array write a `mut` statement, so the write through a wider alias (`let wide: Array<string> = tiers` then `mut wide.push("zzz")`) can be judged at exactly those sites. A different mechanism from G380, so its own release. With it, awaiting a decision, G472 (record 183, N3): a write through a second, wider name after an `is` check breaks the narrowing (`is User` then `mut r["name"] = 5` through a `Record<string, unknown>` holding the same value crashes at run time; `is Array<string>` with `mut a.unshift(5)` through an `Array<unknown>` the same). The options are refusing the write while a narrowing is held on an alias of the value (G394's direction, which needs alias tracking), dropping every narrowing in scope at a `mut` write to a container-typed name, or documenting it as the hole TypeScript's type guards have. Once G325 lands this is the only way a narrowed value can be wrong, so it is not for the Later lane.

**0.1.161 — Shipped 2026-11-22 · G392, a built-in method that answers absence with `undefined` is refused** (decision records 153 and 154; breaking under full `check` too; D56). Landed on `dev-0.1.161`.
- **The rows.** The generated built-in method rows carry `absent`, true when any overload's TypeScript return holds `undefined` or `null`. It is read off the declarations, not written by hand: `find`, `findLast`, `at`, `pop` and `shift` on an array, `at`, `codePointAt` and `match` on a string, at TypeScript 6.0.3. `scripts/check_builtin_members.py` fails generation on an absent row with no entry in the hand table `ABSENT_REPLACEMENT`, so a TypeScript that adds such a method fails CI until someone names its `Option` form; `pop` and `shift` are entered as E0240's. Deleting the `at` entry fails the script
- **E0242 (new).** A call to an absent row on a receiver Glyph types as an `Array<T>` or a `string`, in any position (a statement, a `let`, an argument, a `${}`, the operand of `??`, the receiver of `.name` or `?.name`), through an index (`rows[0].items.find(f)`) or a `?.` call (`s.opt?.find(f)`) included. The message names the call and the type it returns (``the call to `users.find` returns `User | undefined`; Glyph writes a value that may be missing as an `Option` ``), and the help is computed from the call: `array.find`, `array.find_last`, `array.get` (`array.get(xs, array.len(xs) - 1)` for `at(-1)`, with a note that `array.get` answers `None` for a negative index), `string.char_at`, `string.code_point_at`, or `regex.matches` and `regex.captures` for `match`. A `??` default becomes the whole `match`. No help prints a lambda as `...`. Not judged: a call under `mut` (E0241 alone), `pop` and `shift` (E0240's), and a receiver the checker types `Unknown`. The refused call stays `Unknown`, so a read off it adds nothing
- **Three stdlib functions.** `array.find_last(xs, f) -> Option<T>`, and, per record 154, `string.char_at(s, i) -> Option<string>` and `string.code_point_at(s, i) -> Option<number>`, which return the whole code point starting at a code-unit index and answer `None` when the index is not an integer, is out of range (negative included), or falls on the second half of a surrogate pair or a lone surrogate. On `"a😀"`, `char_at` at 1 is `Some("😀")` and at 2 is `None`. This amends spec line 11 and G50, which said there would be no accessor: the index space stays UTF-16 code units, and no accessor returns half a pair
- **`glyph fix`.** Applies the rewrite the checker attaches: `xs.find(f)` to `array.find(xs, f)`, `xs.findLast(f)` to `array.find_last(xs, f)`, `xs.at(2)` to `array.get(xs, 2)`, `xs.at(-1)` to `array.get(xs, array.len(xs) - 1)` when the receiver is a place, and a `??` default to the `match`, laid out as `glyph fmt` lays it out. The receiver moves into the first argument, so the order things are evaluated in is kept. Per record 155 (F1) it rewrites only where the call's value is used up on the spot: an expression statement, a `??` default, a `match` scrutinee, or a `let`, argument or `return` declared `Option`; anywhere else it declines and names the position, since `let u = xs.find(f)` then `u ?? 0` would otherwise go from printing `v=0` to `v=[object Object]` with every check green. A new general helper adds `import std/array` when the module lacks it and declines when the name is already bound, and the help then says to import it under another name. A type parameter is named in the message and help (`T | undefined`, `Option<T>`, record 155, F3). It declines a computed `at` index, a negative index on a value no name holds, a `?.` call, both string accessors (record 154's reason: the `Option` forms answer `None` where `at` counts from the end or hands back half a pair) and `match`
- **The catch case** `gap-g392-find-method-option` is the refused case `find-method-without-option` now, with E0242 and the `array.find` help, and off the page's gap list
- **Measured.** Record 153's probe d (`xs.at(-1) ?? 0`, `xs.find(f) ?? 0`) exits 0 under the published 0.1.160's full `check` and 1 under this build. Across the 32 app directories and every standalone file under `examples/` and `benchmarks/glyph/` (97 units), built one at a time with `build --no-tsc --no-test` under both binaries, exit codes and diagnostics are identical on all 97; the docs already use the stdlib forms
- **Breaking.** Yes, under full `check` as well as `--no-tsc`: probe d passes `tsc --strict` on 0.1.160
- **Not in it.** G429, `??` on an `Option` or `Result` (record 155, F2, next); G381 and G393 (record 153, section 6); G427, the method table read from `esnext` while the emitted tsconfig asks for `es2022`; G428, `s[i]`, `s.charAt(i)` and `string.slice(s, i, i + 1)` still handing back half a pair

**0.1.162 — Shipped 2026-11-22 · G429, `??` on an `Option` or a `Result` is E0236** (record 155, F2; decision record 157, section 1; breaking under full `check` too; D18). Branch `dev-0.1.162`.
- **The rule.** A `??` is judged when its left operand's type is the prelude `Option` or `Result`, through aliases, with any arguments (a nested `Option<Option<T>>` included). A left operand that cannot be absent (a name, a call, a literal, a `match`, a required field) is refused whatever the default is, because the default never applies and the value is the tag object. An `Array` index read counts as never absent, because Glyph's emitted index throws a `RangeError` out of range (record 159, i02), so `os[5] ?? None` is refused rather than looking handled. One that can be absent (a `?.` chain, or a read whose last link is a field declared `f?:`) is refused unless the default is the container (`None`, `Some(x)`, `Ok(x)`, `Err(e)`, or a value of the container's type); then the `??` is typed as the container, so `match b.o ?? None { Some(v) => v, }` is E0200 where it used to pass unchecked. At an absent left side, a default the checker does not type, or an `Option` of another payload, is not judged (G432's question). Not judged at all: a left operand typed `Unknown` or `unknown`, a type parameter (it may be instantiated as a `Nullable`), a `Nullable` (D45), anything else. The refused node is `Unknown`, so nothing cascades, and an E0242 call on the left draws E0242 only
- **E0236, extended.** The same fact as `?.` on these types, so the same code. ``` `??` on an `Option<number>`: an `Option` is never null or undefined, so `?? 0` never applies and the value is the `Option` itself ```, and for a left side that can be absent ``` `b.o` is an `Option<number>` when present: `?? 0` replaces only its absence, so a present `b.o` hands back the `Option`, not its value ```. The help is computed and compiles: `match u { Some(n) => n, None => 0 }`, `match r { Ok(n) => n, Err(_) => 0 }` (with ``or propagate it with `r?` `` inside a callable that returns a `Result`), `match b.o ?? None { Some(n) => n, None => 0 }`, and for a default that is itself the container, ``Write `u` alone`` or `match u { Some(n) => Some(n), None => Some(4) }`. The `Nullable` pointer stays, except over a nested `Option`. A type parameter is shown by its name, `Option<T>` (record 159, F1). A default whose type the checker does not know gets the value form with the default written out, never the container form (F2). Inside a `${}` the help hoists the `match`: ``Bind it before the string, `let t = match u { Some(n) => n, None => 0 }`, then interpolate `${t}` ``
- **`glyph fix`.** Rewrites `u ?? 0` to the `match` for an `Option` whose payload type is decided when the default is a value of the payload and not an `Option` itself, reusing G392's layout helper, and prints what changes: ``` `u ?? 0` handed back the `Option`; it now gives its value or `0` ```. The expression's type goes from unchecked to the payload's, so the fix writes each rewrite, checks the tree again, and keeps it only when no new error appears by review 88's 4b identity; otherwise it puts the file back and declines naming the error. A revert names the line in the file as the run leaves it (F3). It declines a `Result` (dropping, propagating or handling an `Err` is a decision), an undecided payload, a default that is an `Option` or untyped, a default of another type, a left side that is not a name, a field, a call or an index, and a `??` inside a `${}`
- **`glyph fix` no longer writes into a `${}` (record 159, B1; record 160).** Spans inside an interpolation start one column early (G435), and in 0.1.161 two fixes wrote at the drifted range with every check passing: E0242's rewrite of a `??` default inside a `${}` put its `match` into the string's text (`"t=${xs.find(f) ?? 0}"` became `t=$match array.find({x, ...`), and the E0210 read rename turned `"v=${b.name} w=${b.nikc}"` into `b.nickc`. Both the E0242 and the new E0236 rewrite now decline inside a `${}`, and every diagnostic-driven rule (E0210, E0220, E0240, E0242, E0236) passes a guard before it writes: the range must be the span of a node in the file's parse, and when the text there parses as an expression it must parse to that node. The unused-import rule checks that its span still starts with `import`. A drifted range declines instead of corrupting the file
- **The catch case** `default-that-never-applies`: `array.find(...) ?? 3000` is refused with E0236, and the TypeScript half, using the tagged shape Glyph emits, prints `listening on [object Object]`
- **Filed with it.** G435, spans inside a `${}` start one column early, scheduled as 0.1.164, its own release. G432, a `??` default that disagrees with a `Nullable` or optional left side (`n ?? "x"`, `b.n ?? None`), and a dead `??` on a never-absent value; its own release after this batch
- **Docs.** D18 in the spec covers `??`, names the absent-left forms and the not-judged list; `--explain E0236`, `docs/error-codes.md`, the AGENTS.md row and a paragraph beside the `?` rules (with the `llms.txt` mirrors) say `?.` and `??`; the early-errors answer's E0236 section mentions it
- **Measured.** Record 157's probe a01 exits 0 under the published 0.1.161's `check --no-tsc` and full `check`, and 1 under both on this build. Across 285 units built one at a time with `build --no-tsc --no-test` under both binaries (the 32 app directories, the 65 standalone files under `examples/` and `benchmarks/glyph/`, the 54 catch directories, the 18 `tests/exact-or-absent/*/src` projects and the 116 `tests/negative` programs), exit codes and diagnostics are identical on 284; the one difference is the new catch case, which goes from 0 to 1 with one E0236. No tracked program needed migrating: the tracked `??` sites are an optional prop (with its copy among the fuzz seeds) and an index into an array of a literal union, none of them an `Option` or a `Result`
- **Breaking.** Yes, under full `check` as well as `--no-tsc`: a01 passes `tsc --strict` on 0.1.161
- **Not in it.** G432; G435, the span offset inside a `${}`, which moves every diagnostic range inside a template and gets its own release

**0.1.163 — Shipped 2026-11-22 · G423, step 1: the stdlib Set, Store and Deque are typed** (decision records 157, section 2a, and 158, F-B; breaking under full `check`). Branch `dev-0.1.163`, cut from 0.1.161 and rebased onto 0.1.162.
- **The types.** `set.Set<T>`, `store.Store<T>` and `collections.Deque<T>` are generic stdlib types, an application over the stdlib name as `taint.Tainted<T>` is. A written annotation lowers to them by either spelling (`h.s: store.Store<number>`, or `Set<string>` through `import std/set { Set }`), and a diagnostic names them by their qualified name, `set.Set<string>`, as it names every stdlib type (record 158 corrects record 157's "as written"), never as `record`.
- **Every modeled stdlib type is one declaration** (record 158, F-B). One predicate, keyed on the stdlib path whatever the arguments, covers every stdlib type whose shape the checker models (`decimal.Decimal`, `url.Url`, `fs.FsError`, the three above and the rest; `sqlite.Sql` is a `string` and keeps its own rule). Each is never a `string`, a `number` or a `bool`, in either direction, and it and a different declaration are refused in place of each other, a record the program declares included (record 161 added the second direction): a user `Fake` with `Store`'s three members where a `store.Store<number>` is declared is E0211, and a `store.Store<number>` where a `Fake` is declared is E0204, as `FakeUrl` and `url.Url` already were.
- **The rows.** `set.create<T>(initial?: Array<T>) -> set.Set<T>`, `set.unique<T>(Array<T>) -> Array<T>`, `store.create<T>(initial: T) -> store.Store<T>` and `collections.deque<T>(initial?: Array<T>) -> collections.Deque<T>`. `T` is the written type argument, else what the argument binds, a fresh literal widened as `tsc` widens it (`store.create("a")` holds a `string`), else undecided: `set.create()` is a `set.Set<?>` and refuses nothing about its members' arguments.
- **The member tables,** read off `runtime/std/set.ts`, `store.ts` and `collections.ts`, live in `stdlib.rs` beside every other stdlib type's (moved there from `assign.rs`). An unknown member is E0210; a wrong argument is E0211; `update`'s lambda takes its parameter type from the position (D55). `glyph llms --json` publishes every modeled type's table under `members`.
- **The gate.** `check_stdlib_signatures.py` holds each member table to its runtime type in both directions, inside a function generic over the type's parameters; a table with a `?` (`sqlite.Db`) is compared by member name, and the opaque `dom.Element` is skipped. Broken on purpose five ways, each failing: `values` dropped from the `Set` table, a `clear` added to it, `Store.get` retyped, `close` dropped from `Db`, and `clear` added to the runtime `Set` itself. `set.create` and `collections.deque` join G308's known entries for their optional parameter.
- **Refusals `tsc` does not make.** D54 (E0237) on `${seen}` and the other two types, which printed `[object Object]`. E0200 on a `match` over `pop_front`, `pop_back`, `peek_front` or `peek_back` with no `None` arm, a catch: under 0.1.161 the receiver was `Unknown`, both checkers passed it, and an empty deque bound `undefined`. A lookalike record where a stdlib type is declared, and a stdlib value where a lookalike record is declared, both of which `tsc` accepts structurally and D49 refuses.
- **Not in it.** Marking the writing members for E0240 and E0241 is G423b, step 2; `a_stdlib_set_receiver_is_not_judged_until_g423` still passes, and D5 says the writes are not judged until G423 step 2. An unannotated module `const` stays `Unknown` until G433, so b01's `const HITS = store.create(0)` still reads nothing off `HITS`, and D5's edge sentence names that `const` too. E0211 between two function types still prints `function` for both (G434). `Some(x)` under a declared `Option<T>` still compares nothing for a non-literal `x` (G436).
- **Measured.** Under the published 0.1.161 and this build, and again under the published 0.1.162 after the rebase with the same exit codes: record 157's b01 goes from 0 to 1 under `check --no-tsc` (three E0204 for `tsc`'s TS2322s, one E0211 for TS2345, one E0210 for TS2339) and stays 1 under full `check`. Record 158's r05 (`match q.pop_front()` without `None`, E0200) and r06 (`${seen}`, E0237), a `Fake` record passed where a `store.Store<number>` is declared (E0211), and a `store.Store<number>` bound where a `Fake` is declared (E0204), go from 0 to 1 under both `--no-tsc` and full `check`. A `decimal.Decimal` passed where a `number` is declared goes from 0 to 1 under `--no-tsc` and stays 1 under full `check` (TS2345). Across 285 units built one at a time with `build --no-tsc --no-test` under the published 0.1.162 and this build (each app, each standalone file under `examples/` and `benchmarks/glyph/`, each catch, each `tests/exact-or-absent/*/src`, each `tests/negative` case), exit codes and diagnostics are identical on all 285: no scalar refusal, no lookalike-record refusal, and no D54 or E0200 site.
- **Breaking.** Yes, under full `check` as well as `--no-tsc`: r05, r06, `Fake` as `Store` and `Store` as `Fake` pass full `check` on 0.1.161 and fail here. The scalar refusals are breaking under `--no-tsc` only, since `tsc` already refused them.

**Planned, after G273 · G436, `Some(x)` under a declared `Option<T>` compares `x` with `T`** (record 161). `Some(d)` with a `Decimal`, or `Some(u)` with a user record, where `Option<number>` is declared passes both Glyph checks today and `tsc` refuses it. The non-literal half of G273's `Some`/`Ok`/`Err` positions, built on its machinery.

**Rolling lane · G434, E0211 between two function types names them** (record 158): ``expected `function`, found `function` `` breaks the G75 rule; render the signature. D65 (0.1.176) adds sites that reach it, such as `fn(string) -> unknown` where `fn(string) -> string` is declared (record 186, N2).

**Rolling lane · G492, a built-in's rest parameter is checked** (record 186, N3): `s.concat(5)` and `s.concat(u)` pass `--no-tsc` while `s.startsWith(u)` is refused; check each variadic argument against the rest element type. It sits beside record 183's writes into containers.

**0.1.164 — Shipped 2026-11-22 · G435, spans inside a `${}` point at the text they name** (decision records 159, B1, 160 and 166; not breaking). Branch `dev-0.1.164`.
- **The cause.** Each interpolation was parsed from the string's lexed value behind a run of spaces as long as the opening quote's offset plus the character index inside the value. That left out the quote byte, so every node started one column early, and every escape, `\${` and multi-byte character earlier in the string moved it further: `"a${f(\"x\")} ${zz}"` underlined three bytes before `zz`, and `"é😀 ${b.nik}"` four.
- **The fix.** The lexer reports which source bytes each character of a string's value came from (`string_char_spans`, run by the same scan that produces the token, so the two cannot disagree), and the parser places each interpolation at the byte its first character was written at. Every node inside a `${}` now spans its own text after an earlier interpolation, an escape, `\${`, non-ASCII text or a line break, in a plain or a triple-quoted string. A template's `Text` parts span their run of text and its `Expr` parts span `${` through `}`; both used to carry the whole string's span. JSX attribute strings take no interpolation, so they are untouched.
- **What moves.** E0210 in `"n=${b.nik} end"` underlines `b.nik`, the same range it underlines outside a string, and the read fix renames it to `nick` and the file checks clean. Hover, go to definition and diagnostic ranges inside a template land on the name, and the LSP's UTF-16 columns follow (an emoji earlier on the line counts two units). Record 160's x03 (`"v=${b.name} w=${b.nikc}"`) still declines, now for the reason it declines outside a string: `nikc` is a transposition, not one character away from `nick`. f07's false decline reason (``one character away from `ni` ``, record 162) is gone because f07 now applies.
- **What stays.** The E0236 and E0242 rewrites still decline inside a `${}`: a multi-line `match` inside an interpolation is not a layout `glyph fmt` makes (record 159 (i)), and that has nothing to do with positions. The 0.1.162 range guard stays as a backstop for every fix rule.
- **Measured.** 283 units built one at a time with `build --no-tsc --no-test` and checked with `check --no-tsc --json` under the published 0.1.162 and this build, and again under the published 0.1.163 after the rebase with the same result: the 32 app directories, every standalone file under `examples/` and `examples/corpus/`, each `catches/*` program, `tests/exact-or-absent/*/src` and `tests/negative/*`. Build exit codes, build output, and every diagnostic's code, message, help and other fields are identical. Four diagnostic positions changed, all inside a `${}` and each by the one missing column (E0237 in `record-in-template`, E0235 in `template-prints-undefined` and `nullable_optional_chain_escapes`, E0211 in `typo-in-an-options-object`). The emitted `.ts` files are byte-identical; the `.ts.map` files differ in 3803 segments across 34 units, and every one of them now maps into a `${}`.
- **Breaking.** No. Both binaries accept and refuse the same programs; only positions inside templates move, and `glyph fix` applies a rename it used to decline.
- **A panic it removes.** A diagnostic inside a `${}` after an emoji earlier in the string made the published 0.1.162 and 0.1.163 panic (exit 101, ariadne's ``end byte index ... is not a char boundary``), because the drifted range ended inside the emoji's bytes. This build reports the diagnostic and exits 1 (record 166, probe e01). The program was already wrong, so this is not a new refusal.
- **Not in it.** An escape written inside the `${}` of a `"..."` string (`${f(\"x\") + zz}`) is longer in the source than in the value the expression is parsed from, so the nodes after it in that interpolation still sit early (`zz` two bytes early there). Text before the `${` no longer matters, and a `"""..."""` string is exact. The range guard declines any fix at such a range. G441, ruled by record 166 as a parse error in 0.1.169.

**0.1.165 — Shipped 2026-11-22 · G273, a literal names only declared fields** (decision record 157, section 3; breaking; D57). Branch `dev-0.1.165`.
- **The rule.** A key an object literal writes must be a field of the record type it is written as. One the type does not declare is E0210 at the key, the code and the fact a read of the field gives (`type `Opt` has no field `extra``), with the same "did you mean". Judged wherever a written literal meets a declared type: a `let` or `const` annotation, a `return`, a call argument (a variant payload in every import spelling, and a call through a member function), `mut x = ..` and `mut x.f = ..`, a field of such a literal, an element under a declared `Array`, the argument of `Some`, `Ok` or `Err` under a declared `Option` or `Result`, an arm of a `match` in one of these positions (block-arm tails and nested `match`es included, record 160), and what a lambda with no written return type returns where its position declares one. A lambda tail under `array.map` with no declared return is not judged, as `tsc` does not judge it. The declared type may be a local, imported or generic record, an `interface`, an `@open` record or a modeled stdlib record. A key reached from two positions draws one E0210
- **Values at the same positions** (record 160, item 2). Where nothing compared a written value before, its values are compared now: a `Some`, `Ok` or `Err` argument, a variant payload (E0211), a written arm of such a `match`, and an object or array literal written to a `mut` target (E0204). Only a value written out there is compared, not a name, whose recorded type does not follow an `is` arm's narrowing (the string-literal-union corpus's `is Record<Mode, number> => Some(r)` caught that). `let f: Option<Opt> = Some({ name: 4 })` was silent and is E0204, as `tsc` refuses it (TS2322). A generic payload is not compared against its unbound parameter, and a container refused as a whole is one refusal. This folds in the value gap found while building G273, so it was never scheduled separately
- **Not judged.** Fields that arrive through a spread; a binding holding a wider record (width subtyping, as in `tsc`); a declared `Record<K, V>`, `unknown`, type parameter or unknown type. A call argument is judged against the parameter as declared, so a field typed by the callee's own type parameter is not judged whatever the call binds it to
- **Callee resolution** (record 160, departure 3). A constructor under a declared union is judged by what its name resolves to (a variant declared here, a named import, a namespace member), tested in all three spellings; a local function named like a variant is not read as one
- **Beyond `tsc`.** `Some({ name: "x", extra: 6 })` where `Option<Opt>` is declared is refused; `tsc` lets it through only because it infers `T` from the literal. The same holds for a lambda's returned literal, which `tsc` does not check for excess keys either
- **Help and `glyph fix`.** The help names the three repairs: rename it to the declared field, delete it, or declare it in the type. `glyph fix` applies E0210's existing distance-one rename to a literal key, and declines when the literal already writes that field, saying the rename would write it twice; the message then leaves the suggestion out. The diagnostic carries the literal's keys in-process for that decision (never serialized), and `ObjectField::KeyValue` gained the key's own span so the key alone is underlined
- **The fix guard.** The range guard every fix rule passes (0.1.162, record 159 B1) accepted a range only when it was a node's span, and a literal key carries its own span, so it reads key spans now and anchors a key to the object literal holding it: that object's span must re-parse to the same object, and the range must be the key's position inside it (record 167; a text check alone renamed a variable five bytes before the key when five escapes in the `${}` moved the range, G441's region, and that probe is a decline test). Record 168 found two identical object literals at exactly the drifted distance passing that anchor, so every fix rule now declines a range after an escape inside the `${}` of a `"..."` string, decided from the lexer's own escape positions, with the anchor kept as a second line; d01, d04 and d05 decline with the file byte-identical, until G441 removes the region. With 0.1.164's exact spans inside a `${}` (G435), the key rename applies there too; record 160's x02 and x03 are tests: a distance-one key or field read in a second interpolation is renamed at the right bytes and re-checks clean, and `nikc` (a transposition, no distance-one match) declines with the file byte-identical
- **`glyph_assignable`.** The reason of an accepted record pairing whose value has extra fields no longer says the checker accepts them on a literal: it says a value of that type is accepted and a literal writing them is E0210 (D57), TS2353 under `tsc`
- **Docs.** D57 in the spec; D49's "does not decide" clause and D10's extra-field sentence point at it; `--explain E0210`, `docs/error-codes.md`, the AGENTS.md row and the `llms` mirrors describe the literal case; the verifiability answer shows the refusal and moves the edge to the shipping list
- **Measured.** Record 157's probe c01 exits 0 under the published 0.1.161's `check --no-tsc` and 1 under this build (11 E0210s, `wide` accepted); full `check` exits 1 under both. Its `Some` line alone exits 0 under 0.1.161's full `check` and 1 under this build. Across every tracked `.glyph` file (382 units: the 32 apps, each `tests/exact-or-absent/*/src`, each catch directory, each `tests/negative/*` entry, every other file alone), built one at a time with `build --no-tsc --no-test` under both binaries, exit codes and diagnostic codes are identical on all 382, so no tracked program needed migrating. After record 160's fixes: record 160's g02 (a `match` arm writing `extra`) exits 0 under 0.1.161's `check --no-tsc` and 1 under this build (`tsc` refuses it too), `let f: Option<Opt> = Some({ name: 4 })` alone goes from 0 to 1 under `--no-tsc` (TS2322 under full `check` on both), and the 382-unit run is again identical on all 382. Rebased onto 0.1.164 and measured against the published 0.1.164: 383 units identical; c01, g02 and the `Some({ name: 4 })` probe exit 0 under its `check --no-tsc` and 1 under this build, and c01's `Some` line alone exits 0 under its full `check` and 1 here
- **Breaking.** Yes: under `--no-tsc` at every position `tsc` also refuses, and under full `check` for `Some`, `Ok` and `Err` and a lambda's return

**0.1.166 — Shipped 2026-11-22 · G433, an unannotated const has its initializer's type** (decision record 158, F-A; breaking under full `check` too; D20 and D5 amended). Branch `dev-0.1.166`, cut from 0.1.161 and rebased onto 0.1.165.
- **The rule.** A `const` with no annotation has the type of its initializer, under the widening `tsc` applies to the TypeScript `const` the emitter writes. A string literal standing as the whole value keeps its literal type, and so does a value-position `match` whose every arm is a written string (it emits as a function whose inferred return `tsc` keeps as the union), so `const MODE = "read"` still stands where a `"read" | "write"` is declared. A literal inside an object or an array widens: `const CFG = { n: 1, }` is a `{ n: number }` and `const ITEMS = [1, 2]` an `Array<number>`. An array literal is widened element by element, so `[{ mode: "read", from: m }]` keeps the `Mode` a name gave `from`; this also applies to an unannotated `let`, which read the whole element type before. An initializer the checker cannot type stays `Unknown`, including a `const` whose initializer reads itself back, in its own module or through two siblings that import each other's consts, which answers `Unknown` instead of recursing (the first build of the rule panicked inside salsa on the two-sibling loop; both queries in it now carry a cycle answer, and an integration test builds it)
- **One answer for every reader.** The type is answered by `decl_ty`, the query that answers a `fn`'s signature, through `decl_signature_ty` and `unannotated_const_ty` in the checker (the lowerer has no expression walk, so the G39 comment in `lower.rs` now states the rule and where it is answered). Hover on the name and `glyph_symbol`'s `type` report it, and the MCP manuals no longer say an unannotated `const` has no type. An imported `pub const` reaches the consumer through `exported_const`, rendered on the export view (`ExportLowerer::export_inferred`, which turns a module-local type into the imported declaration), so `import lib { ITEMS }` and `lib.ITEMS` read the same type
- **Measured.** Record 158's probe r04 exits 0 under the published 0.1.161's `check --no-tsc` and 1 under this build, with E0240 on `ITEMS.push(3)` and E0204 on `let x: string = CFG.n`. Rebased onto 0.1.164, whose 0.1.163 types the stdlib `Store`, its `let y: string = S.get()` line with `const S = store.create(0)` is E0204 too, so all three unannotated lines are refused. A program whose only refusal is `ITEMS.push(3)` on an unannotated `const` passes the published 0.1.161's full `check` (`tsc --strict passed.`) and fails this build's, so the release is breaking under full `check` too. Across 284 units (32 app directories, every standalone file under `examples/` and `benchmarks/glyph/` copied into its own directory, `catches/*`, `tests/exact-or-absent/*/src` and `tests/negative/*`), built one at a time with `build --no-tsc --no-test` under both binaries, exit codes, diagnostics and the emitted TypeScript were identical on all 284 before the rebase. Rebased onto 0.1.164 and then 0.1.165 and run again against each published release (285 units), exit codes and diagnostics are identical on all 285 both times; the emitted TypeScript differs in one line of auth_api's `clock.ts`, where `let t = frozen.get()` off `const frozen = store.create<number>(0)` is now a `number` and `t == 0` lowers to `t === 0`, which `tsc --strict` and the app's `@example` checks pass, so no corpus site needed migrating
- **The emit for `==` is unchanged (record 163).** The first build lowered an `==` against a now-typed `const` to `===`, which changed four apps' output (bracket, discord, linkcheck, settle) and exposed `tsc`'s literal `const` types: `N == 6` with `const N = 5` became TS2367 under full `check` on a program Glyph accepts. An operand that names an unannotated `const` (in this module, through a named import, or as `ns.NAME`) keeps `__glyph_eq`, the emit 0.1.161 gave it. Record 163's probe k03 passes full `check`; k04 reports only the `m == "zzz"` TS2367 it reported on 0.1.161, which is G438's. Both are pinned by an integration test that runs `tsc`
- **Breaking.** Under `--no-tsc` and under full `check`: `ITEMS.push(3)` on an unannotated `const` is E0240, a refusal `tsc` does not make. The other new refusals are ones `tsc` makes: E0204 and E0211 at a field read or a constant where its declared type disagrees (`takes(NAME)` with `const NAME = "ann"`, `takes(CFG.mode)`), and `match MODE { "read" => 1, "write" => 2, }` with `const MODE = "read"` moves from E0218 to E0220 (`` `"write"` is not a value of `"read"` ``), where `tsc` gives TS2678 on the emitted `switch` `const FOUND = array.find(...)` is now typed `Option<number>`, so `FOUND ?? 0` goes from exit 0 to exit 1 with E0236, G429 reaching a typed `const` (record 170).
- **With G273 (0.1.165).** D57's refusal of an undeclared key is for a fresh literal, and a `const` read by name is not one: `const BIG = { n: 1, label: "x", extra: true, }` stands where a `{ n: number }` is declared, at the top and one record down, as `tsc` accepts it. A `const` missing a required field is still E0204, as `tsc` refuses it (TS2741). Pinned by a test
- **Tests.** No false refusal from the literal rule: a string-literal `const` passed where a `string` is declared and to `string.len` and `string.upper`, compared with another literal, interpolated, and read for `.length`, and a `match` of literals passed where the union is declared. A generic call (`array.map` over numbers) and an empty array literal, whose later `push` of anything is accepted as `tsc`'s evolving array accepts it. The import test builds both spellings in one project
- **Not in it.** `const` is module-level only (D20), so there is no local `const` to type; a `let` was already typed from its initializer

**0.1.167 — Shipped 2026-11-23 · G423b, the writing members of `Set`, `Store` and `Deque` are marked** (decision record 157, section 2b; breaking under `--no-tsc` and full `check`; D5 amended). Branch `dev-0.1.167`, cut from 0.1.166.
- **Three classes.** Every member of a stdlib type whose members the checker models is Mutating, NonMutating or Outside, keyed by (module, type, member). Mutating: `add` and `remove` on `set.Set`, `set` and `update` on `store.Store`, `push_back`, `push_front`, `pop_back` and `pop_front` on `collections.Deque`. NonMutating: `has`, `size` and `values` on a `Set`, `get` on a `Store`, `peek_front`, `peek_back`, `len` and `values` on a `Deque`, and all fourteen members of `decimal.Decimal`. Outside: all six members of `sqlite.Db`
- **Where the classes live.** Hand sets in `scripts/check_builtin_members.py`, beside G380's array split, generate `stdlib_member_classes.rs` under the script's diff gate. The script fails when a classified type's runtime declaration (read with TypeScript's checker from `runtime/std/*.ts`) has a member in no class, in two, or a class names a member the runtime lacks. A node oracle compiles the runtime modules with TypeScript's transpiler, calls each member of a fresh instance holding `[1, 2, 3]` with samples chosen to cause a change, and checks the deep state changed exactly when the member is Mutating; it also checks that the array `values()` returns is a copy and that no `Decimal` member changes its receiver. Broken on purpose: moving `remove` to NonMutating fails with ``set.Set: `remove` changed its receiver in node and is NonMutating, not Mutating``, and dropping `get` fails with ``store.Store: `get` is in none of Mutating, NonMutating and Outside``. Record 171 (F3) found that moving a reader to Outside passed: the oracle could not tell "left the receiver alone" from Outside. It now refuses Outside on any type it builds an instance of, since Outside is only for writes outside the program's data, and record 171's sweep over every move of every member to every other class (72 moves) is caught on all 72. A Rust unit test walks every modeled stdlib type (`MODELED_STDLIB_TYPES`, which `stdlib_modeled_type` now reads) and fails when a method has no class or two, when a data field has one, or when a class row names no modeled method
- **The rule.** E0240 and E0241 apply to these receivers exactly as to arrays: the same places, positions and `glyph fix` insertion, reached directly, through a field path (`mut c.state.set(x)`), a module `const` (typed since 0.1.166), an alias (`type Seen = set.Set<string>`), or a `pub const` imported under either spelling. `mut d.add(e)` on a `Decimal` is E0241 with help `mut d = d.add(e)`; a reader under `mut` is E0241 with help to drop the `mut` and bind the result. Where the receiver cannot be reassigned (a module `const`, which is E0212, a named import, or `lib.NAME` through a namespace import), the help binds instead, ``let result = ONE.add(ONE)``, for a `Decimal` and an array alike (record 171, F1; the array case dated from 0.1.166's typed consts). A `Db` member is judged in neither direction
- **Help.** A value-position `remove` reads `seen.has(x)` first (``read whether it is there first with `let had = seen.has(1)`, which is the value `remove` returns, then write `mut seen.remove(1)` as a statement``); `pop_front` and `pop_back` read `q.peek_front()` or `q.peek_back()` first; a member that returns nothing gets the statement form only (``` `set` returns nothing, so there is no value to use: write `mut st.set(3)` as a statement ```). A returned or arm tail ends with `return had`/`return front`. A receiver no name holds (`mk().remove(1)`) gets ``bind it with `let` first, `let s = mk()`, then write `mut s.remove(1)` `` in every position. `HITS.get().push(1)` stays E0240 under `mut`, since the receiver is not a place, and its message says the store still holds the array instead of "nothing keeps it"; the help writes the change through the store, `mut HITS.update(fn(current) { array.push(current, 1) })`, with matching forms for `unshift`, `sort` (with or without a comparator) and `reverse`. A set or deque a store holds gets the same treatment (record 171, F2): `ST.get().add(2)` says `ST` still holds the set, and the help writes `mut ST.update(fn(current) {`, then `mut current.add(2)` and `current` on lines of their own, then `})`, never the `let s = ST.get()` alias that is G394's shape. Every spelling compiles under `tsc --strict` and runs (`every_stdlib_help_spelling_compiles_and_runs`)
- **`let _ = <mutating call>`.** A new position: the value is thrown away, so the help writes only `mut <call>` and `glyph fix` replaces the `let _ = ` with `mut `, for arrays too (record 157 b03's `let _ = xs.pop()` used to get the read-first help). The fix checks the replaced text still reads as that `let` and declines otherwise
- **E0237 steps aside.** `${q.pop_front()}` was E0240 and E0237, and E0237's help wrote a `match q.pop_front()` that E0240 refuses; a recorded mutating call is now E0240's alone, as E0242 already leaves `pop` and `shift` to it
- **Migration.** 42 bare statements across auth_api (`clock`, `repo`), csvql (`sql`), depsolve, linkcheck, shortlink, webhook_ingress, workflow (`check`) and `examples/corpus/store.glyph`, plus webhook_ingress's `let _ = ring.buffer.pop_front()`, all by `glyph fix` in the same commit as the rule; nothing suppressed. The test fixtures in `integration.rs`, `stdlib_signatures.rs` and `stdlib_state_types.rs` that wrote bare writes or read a pop's value moved to the new spellings (a pop's `Option` is read with the matching peek). `fuzz/seeds/*` reach only the parser and formatter, so they stay as written. The `set.ts`, `store.ts` and `collections.ts` header comments say each write is a `mut` statement
- **Measured.** Across 285 units (32 app directories, every standalone file under `examples/` and `benchmarks/glyph/` copied into its own directory, `catches/*`, `tests/exact-or-absent/*/src` and `tests/negative/*`), built one at a time with `build --no-tsc --no-test` under the published 0.1.166 and this build: before migration 8 units differed, with exactly the 43 E0240 sites above and nothing else; after it, exit codes and diagnostics are identical on all 285. The seven migrated apps pass full `check` (`tsc --strict passed.`)
- **Breaking.** Yes. A program whose only change is a bare `seen.add("a")` on a `set.create<string>()` (record 157 b01's line) exits 0 under the published 0.1.166's `check --no-tsc` and full `check` (`tsc --strict passed.`), and 1 under this build's, with E0240. Record 157's b02 (`mut d.add(d)` on a `Decimal`) goes from 0 to 1 with E0241
- **Docs.** D5 lists the three classes and drops "until G423 step 2", and names the write through a second name for a store's contents as the remaining edge (G394, now noted there). `--explain E0240`/`E0241`, the code table in `docs/error-codes.md` and AGENTS.md (with the `llms` mirrors), the `std/store`, `std/set`, `std/collections`, `std/decimal` and `std/sqlite` sections of AGENTS.md and `docs/reference/stdlib.md`, the anti-patterns guide, and `05-typechecker.md` say it. The greppability answer's shipping line names the stdlib holders and the npm edge, and its "not checked yet" item is gone, in English and the nine translations. G423 is closed; G446 is filed. From record 171: G447 (a `for` over an array literal of applied types) is 0.1.168, which moved every later release down one; G448 (a write through a structural interface) is in D5's not-covered list and awaits a decision; G449 and R1 to R5 are in the rolling lane
- **Not in it.** E0240's value help for an array no name holds (`let n = make().pop()`) still writes `mut make().pop()`, which is refused itself; that predates this release and is G446 in the rolling lane

**0.1.168 — Shipped 2026-11-23 · G447, a `for` over an array literal of applied types binds the element type** (record 171; fixes a false refusal that 0.1.166 has too, so it goes first; breaking under `--no-tsc` only, for programs `tsc` already refuses). Branch `dev-0.1.168`, cut from 0.1.167.
- **The defect.** The loop widened fresh literals (G237) by pairing the element type with the iterand expression. For a written array that paired an array literal with a type that was not its type, and the widening rebuilt any one-argument applied element around the literal's own elements: `for s in [seen]` with `seen: set.Set<number>` bound `s` as `set.Set<set.Set<number>>`, so `s.has(1)` was E0211 (``expected `set.Set<number>`, found `number` ``), and `for w in [[1]]` bound `w` as `Array<Array<number>>`
- **The fix.** The loop widens the iterand's own type and reads the element off the result, the same type `let xs = [..]` then `for x in xs` gives. One site in `assign.rs`; nothing else read the element this way
- **Tests.** `crates/glyph-cli/tests/for_over_array_literal.rs` states each loop twice, over the literal and over a `let`-bound array, and asserts the two read the same: record 171's a08 lines, nested arrays (two and three levels), stores, `Option`s, records, string literals, a mixed literal (`[seen, set.create([2])]`, `[inner, [2, 3]]`), the index form `for i, s in [seen]`, a wrong argument inside the loop (still E0211, now naming `number` and `string`), and the accepted a08 program under full `check` (`tsc --strict passed.`)
- **Measured.** Across 285 units (32 app directories, every standalone file under `examples/` and `benchmarks/glyph/` copied into its own directory, `catches/*`, `tests/exact-or-absent/*/src` and `tests/negative/*`), built one at a time with `build --no-tsc --no-test` under the published 0.1.167 and this build, exit codes and diagnostics are identical on all 285. Record 171's a08 goes from exit 1 (E0211 at 11, E0204 naming `Array<Array<number>>` at 15) to exit 1 with only the E0204, which now names `Array<number>`; the same two loops with `let n: number = w.length` in place of line 15 go from exit 1 to 0 under `--no-tsc` and full `check`, and `run` prints `true` and `1`
- **Breaking.** Under `--no-tsc` only. The wrong element type also let wrong code through: `s.has(seen)` in `for s in [seen]` and `let bad: Array<Array<number>> = w` in `for w in [[1]]` exit 0 under the published 0.1.167's `check --no-tsc` and 1 under this build's (E0211, E0204). Full `check` exits 1 on both binaries, because `tsc` refuses both lines (TS2345, TS2322). The corpus has no such site
- **Not in it.** `let xs = [["read"], ["write"]]` is typed `Array<Array<?>>` at the `let` itself, since the inner literal types do not join before they are widened, so `let a: Array<Array<number>> = xs` is not refused there. The loop over that literal reads the same open type the `let` does. Filed as G450 for the rolling lane: the fix refuses programs accepted today, so it takes its own release and two-binary run

**0.1.169 — Shipped 2026-11-23 · G441, a backslash inside the `${}` of a `"..."` string is a parse error** (decision record 166; breaking under `--no-tsc` and full `check`; D12 and D22 amended). Branch `dev-0.1.169`, cut from 0.1.167.
- **The rule.** Inside the `${}` of a `"..."` string a backslash is E0002, reported at the escape's own bytes: ``` `\"` is an escape inside the `${}` of a `"..."` string, which Glyph does not allow ```. Every escape form is refused there (`\"`, `\t`, `\\`, `\u{..}`, `\$`), decided from the lexer's record of which source bytes each character came from. The help is the hoist, computed from the code (record 174, F1): for `"a${x + \"\\u{e9}\"}"` it says ``Bind the value with `let` and interpolate the name: `let label = x + "\u{e9}"`, then `"a${label}"`. Or write the whole string as `"""a${x + "\u{e9}"}"""`, which reads a string literal inside `${}` as written.`` The text around the `${}` is kept as written. The `"""` form is spelled out only when that text holds no escape, which a `"""` string would not decode; an expression over several lines or nesting a `${}` gets the help in words A `"..."` string nested inside a `"""` interpolation is a `"..."` string, so its own `${}` is held to the rule, and the error is reported at the inner escape rather than wrapped in the outer one. Triple-quoted strings are unchanged; `"a\n${x}"` and `"\${x}"` are accepted
- **Exact spans by construction.** With no escape inside a `"..."` interpolation, every node inside a template sits on its own bytes. The fix driver's escape-region decline (record 168) is removed, since it is not a backstop for anything: no module that parses has the region, and its one remaining trigger was a backslash inside a `"""` interpolation, whose ranges were always exact, where it declined a correct repair (`"""x${f("a\tb") + b.nik}"""` was declined under 0.1.167 and is renamed to `b.nick` here). The node-shape check and the object-literal anchor stay as the guard against any other drift. Record 169's polish item (a decline in that region giving a reason computed from shifted text) goes away with the region: records 167 and 168's probes d01, d04 and d05 now stop at E0002 and `glyph fix` applies nothing to them, leaving the files byte-identical
- **`glyph fmt`.** A single-line `"""` template that interpolates was reprinted in the `"..."` spelling, which turned `"""x${f("a\tb")}"""` into `"x${f(\"a\\tb\")}"`, the refused form; under 0.1.167 it compiled. It now keeps the `"""` spelling, with its text raw and its `${}` interiors still normalized. D12 and AGENTS.md said the old behaviour and now say this one
- **Measured.** `"a${f(\"x\")}"` exits 0 under the published 0.1.167's `check --no-tsc` and full `check`, and 1 under this build's with E0002, so the release is breaking. Across 283 units (32 app directories, every standalone file under `examples/` copied into its own directory, `catches/*`, `tests/exact-or-absent/*/src` and `tests/negative/*`), built one at a time with `build --no-tsc --no-test` under both binaries, exit codes and diagnostics are identical on all 283, and on the 87 other tracked `.glyph` files (`benchmarks/`, `playground/examples/`, the compiler's test fixtures and the fuzz seeds) each in its own directory. The only backslash inside a `${}` in the 524 tracked `.glyph` files is `examples/apps/sitegen/render.glyph:58`, inside a `"""` string, so no site needed migrating
- **Tests.** Parser: `"a${f(\"x\")}"` refused at the first `\"` with the help, each escape form refused, the two escapes outside `${}` accepted, a `"""` string with `"a\tb"` inside its `${}` accepted with exact spans, and the nested `"..."` case refused at the inner escape. Parser: the help computed for record 174's s03, for text on both sides of the `${}`, for an escape in that text and for a two-line expression. CLI: both spellings the help writes for s03 and for a string with text on both sides, applied, pass full `check` and print the same text; d01, d04 and d05 refused by `check` and untouched by `fix`; a key and a read after a backslash in a `"""` interpolation renamed by `fix`. Formatter: a single-line `"""` template keeps its spelling, reparses, is idempotent and emits the same TypeScript
- **Docs.** D22 states the rule and its pillars, D12 the formatter change, `--explain E0002` and AGENTS.md's template-strings section (with the `llms` mirrors) say both spellings

**0.1.170 — Shipped 2026-11-23 · G315, a second name gives the plain spelling's answer** (record 88, amended by record 157, section 4; breaking under `--no-tsc`; D45, D46 and D49 amended). Branch `dev-0.1.170`, cut from 0.1.167 and rebased onto 0.1.169.
- **The rule.** Before the relation compares two types, a `type` whose body is a primitive, `unknown`, a type parameter or a generic application is read as its body with the application's arguments substituted (`transparent_body` in the checker). `expand_transparent` applies it at the top of `assign_incompatible_at` and `accepting_rule`, so every depth the relation recurses into reads it, and `canonicalize_aliases` applies it at every depth, so the rules that read a canonical type see it too. A record, a union of either kind, an `extern_ts` or `typeof` body, a function type and a body with a `where` keep their declaration. A local declaration, a named import and a namespace import of a sibling's are read by the same code
- **False refusals removed** (all accepted by `tsc --strict`): `Maybe<Note>` against `Nullable<Note>` both ways (`type Maybe<T> = Nullable<T>`), `Id` against `Name` (both `= string`), `L<string>` against `Array<string>` both ways (`type L<T> = Array<T>`). Two more fell out of the same rule: `first(xs)` with `xs: L<string>` into `fn first<T>(xs: Array<T>)` (was E0211), and a `Pair<number>` (`= Record<string, T>`) returned as a `Record<string, number>` (was E0204). One refusal goes away that `tsc` still makes (record 178, probe r): with `type W<X> = Array<X>` and `type G = Array<G>`, a `W<G>` returned as an `Array<number>` exited 1 under 0.1.167's `--no-tsc` and exits 0 now. 0.1.167 refused it only because `W` had its own identity; it now reads as `Array<G>` against `Array<number>`, which neither build decides, since a recursive declaration is not compared against a type of another shape (G455). `tsc` refuses it (TS2322). A `let [a, b] = xs` on an `L<number>` is now E0230's "length is not known" message instead of "this value is `L<number>`"
- **New refusals**, each a `tsc` refusal: `Same<string>` given a `number` (TS2322), `Strs` (`= Array<string>`) given an `Array<number>` (TS2322), `Record<string, Same<number>>` returned as `Record<string, string>` (TS2322), all E0204 at a `return`, a `let` or a `const` and E0211 at an argument. Three pinned silences became refusals for the same reason: an `Option<int>` returned as `type Id = string` (TS2322), a `MaybeInt` (`= Option<int>`) returned as a `string` (TS2322), and an imported `Id` (`= string`) passed where a `number` is declared (TS2345). A component parameter typed `Same<string>` is E0229 as `string` is (TS2769 with an attribute). The diagnostic names each type as the site wrote it
- **`glyph_impact` and `glyph_assignable`.** `glyph_assignable` asks the relation itself, so `number` against a sibling's `pub type Id = string` is now `WILL_FAIL` and `string` against it `COMPATIBLE`. The `change_signature_type` table models the relation, so it reads a transparent declaration through `expand_transparent` before classifying either side: an `Id` argument against a `number` parameter, local or imported, and a `string` argument against an `i: Id` parameter are `WILL_FAIL` (E0211) where they were `UNDETERMINED`. The `agent_loop` fixture's undetermined site now uses an `extern_ts` alias, and the impact answer page shows that reason
- **Sibling modules (D46).** The same eight pairs through an alias declared in a sibling, imported by name and through a namespace, give the local answers in both spellings (`a_second_name_declared_in_a_sibling_gives_the_local_answer_by_every_spelling`). Before this a sibling's alias refused nothing: the three `tsc` refusals passed `--no-tsc` under both spellings
- **Measured.** Across 285 units (32 app directories, every standalone file under `examples/` and `benchmarks/glyph/` copied into its own directory, `catches/*`, `tests/exact-or-absent/*/src` and `tests/negative/*`), built one at a time with `build --no-tsc --no-test` under the published 0.1.167 and this build: no unit's diagnostics or exit code changed, so nothing needed migrating
- **Breaking.** Yes, under `--no-tsc`. Record 157's d03 exits 0 under the published 0.1.167's `check --no-tsc` and 1 under this build; full `check` exits 1 under both. The sibling probes exit 0 under 0.1.167 and 1 under this build. The false-refusal program (`Maybe`/`Nullable`, `Id`/`Name`, `L`/`Array`) goes from 1 to 0, and full `check` passes it
- **With G325.** `type Raw = unknown` now reads as `unknown` everywhere the relation looks, so `fn h11(r: Raw) -> string { return r }` is accepted for the reason the plain `unknown` is: the checker never refuses a found `unknown`. Refusing it is G325's (0.1.176), which no longer has to expand the alias itself
- **Recursive declarations (record 176).** A transparent declaration whose body refers to itself, directly or through other transparent declarations and second names (`type F = Array<F>`, `type A<T> = B<T>` beside `type B<T> = A<T>`), is not expanded and keeps its name, compared by identity as in 0.1.167. The first build expanded it and overflowed the stack (exit 134) on record 176's c1 to c5, b5, b7 and j1. Under `--no-tsc` 0.1.167 accepts four of them and refuses four by name; `tsc` accepts all but j1, which is TS2456. The cycle is found once per declaration with a visited set over the declarations reached, and the depth bound of eight the first build used is gone, so no type is ever half expanded. All eight probes now finish in about 10 ms with 0.1.167's diagnostics, pinned by `a_recursive_transparent_declaration_keeps_its_name`. Comparing two recursive types structurally is G455
- **Not in it.** Expansion of a record or union declaration, or of a generic declaration written bare or with the wrong argument count (E0233), which say nothing about their parameters

**0.1.171 — Shipped 2026-11-23 · G314, step 1: type parameters carry their owner** (record 88, record 157 section 4; not breaking). Branch `dev-0.1.171`, cut from 0.1.167 and rebased onto 0.1.170. Displaced from the 0.1.162 batch by G423b, which closes the audit hole D5 names, and placed after G315 by record 158.
- **Owners.** Lowering reads a type parameter's owner off the resolver's binding: the `fn`, `component`, `type` or `interface` whose `<...>` declares it, as `Callable` or `TypeDecl` with this module's `SymbolRef`. A sibling's declaration and every stdlib row are named by `(module, name)` through two new variants, `ImportedCallable` and `ImportedTypeDecl`, since a `SymbolRef` from another module's table would name an unrelated declaration in the consumer's. Stdlib rows take the callee (the G423a rows for `set.create`, `set.unique`, `store.create` and `collections.deque` included); a stdlib type's member table and the builtin `Result` and `Option` take the type.
- **Comparison.** `map_field_incompatible`, the key domain of a `Record` keyed by a parameter and the display of the body's own parameters compare by owner and name. A field typed by a parameter some callee declared and left unbound is undecided at the call site; one typed by the enclosing function's own parameter is refused as before. The `V` of the map is read through its transparent declarations first (G315), so `Record<string, Same<T>>` with `type Same<T> = T` takes `One<T>` as `Record<string, T>` does, which 0.1.170 refused and `tsc` accepts. The expansion substitutes arguments that keep their owners, so `wrapw([])` through `type W<T> = One<T>` and `type L<T> = Array<T>` is accepted like `wrap([])`, and a field typed by the enclosing `T` is refused through them.
- **The fix.** Record 157's e02, `return wrap([])` where `Record<string, string>` is declared, goes from E0204 (``expected `Record<string, string>`, found `One<?>` ``) to accepted, as `tsc` accepts it. The same holds with `wrap` imported by name or through a namespace, and inside a `fn f4<T>` whose own parameter is also `T` (0.1.167 says ``found `One<T>` `` there, naming the callee's `T` as the body's). `fn f2<T>(x: One<T>)` and `wrap(xs)` inside `fn f3<T>` stay E0204. Tests: the local program, and both import spellings in an integration test.
- **Measured.** Across 285 units (every app directory, every standalone file under `examples/` and `benchmarks/glyph/` copied into its own directory, `catches/*`, `tests/exact-or-absent/*/src` and `tests/negative/*`), run one at a time with `build --no-tsc --no-test` and `check --no-tsc --json`, this build against the published 0.1.169 and against 0.1.170 built from `main` (0.1.170 is not on npm yet): the full output, JSON diagnostics included, is byte-identical on all 285 both times. No corpus program has e02's shape.
- **Breaking.** No. Every change is a refusal removed: e02 exits 1 under 0.1.170's `check --no-tsc` and 0 under this build's, and full `check` passes `tsc --strict`.
- **Filed.** G456: a field typed `Same<T>` (a second name for the enclosing parameter) passes where `Record<string, string>` is declared, and `tsc` refuses it. Refusing it adds a refusal, so it goes with step 2 (0.1.172).

**0.1.172 — Shipped 2026-11-23 · G314, step 2: an enclosing type parameter is decided** (record 88, record 175; D58; breaking under `--no-tsc`). Branch `dev-0.1.172`, rebased onto 0.1.171. G314 closes with it.
- **Entry conditions (record 175).** Landed before the first refusal. A corpus gate, `param_owner_gate.rs`, checks every unit of the two-binary set in one process and requires that no type parameter without an owner reaches the relation; it finds none, and fails on the first app once `param_owner` is broken on purpose. Substitution is keyed by owner and name: a call binds only the parameters it instantiates, so `f(5)` with `f: fn(T) -> T` inside `fn g<T>` no longer binds `g`'s `T`, and a declaration's body read at an application binds that declaration's own owner. Record 175's crossed cases, `pair(y, x)` in `h<T, U>` and `wrap(xs)` with `xs: Array<Array<T>>`, are tests. `enclosing_owner` is the only signal that a parameter is decided, and `is_own_param` no longer matches an unowned parameter by name. `generic_call_with_unknown_argument_leaves_param_open` stands unchanged.
- **The rule (D58).** Inside `fn f<T>`, `T` equals only itself, passes where `unknown` is declared, and is compared against `X` where `Nullable<X>` is declared (D45). Every other type the checker reads is refused in both directions, at every depth: a primitive, a literal union, a record, a function, a tagged union, a prelude container, a modeled stdlib type, an interface, and another of the function's parameters.
- **Bounds (record 175, item 3).** A bounded `T: Named` is decided where it is declared, so a `Named` returned under `-> T` is refused. Its bound is read off the owner's declaration, but the resolver does not resolve a bound (G263), so a bounded parameter as a value keeps 0.1.167's treatment: undecided at a return, and its field refused where `Record<string, V>` is declared unless `V` is `unknown` or that parameter. Passing where `Named` is declared waits for G263.
- **R2.** A `type` declaration's own parameter is never decided where a value is checked. It reaches a value only through a generic declaration written bare (`let b: Box = mk()`), which E0233 already refuses; the second E0204 that followed it is gone.
- **New refusals, each one `tsc` makes.** `return x` with `x: Array<T>` under `-> Array<string>` (E0204, TS2322); a `Box<T>` whose field is `Array<T>` returned where `Record<string, Array<string>>` is declared (E0204, TS2322); a `Record<string, V>` returned as `Record<string, string>` (E0204, TS2322); `return 5` under `-> T` and `let a: Array<T> = [1]` (E0204, TS2322); `return y` with `y: U` under `-> T` (E0204, TS2322); a `T` where `Nullable<string>` is declared (E0204, TS2322); a `T` where an interface is declared and the reverse (E0204, TS2322); `let b: string = x` with `x: U` (E0204, TS2322); `f(5)` with `f: fn(T) -> T` (E0211, TS2345). `record.keys(m)` on a `Record<K, number>` returned as `Array<string>` is E0204 at the return; `tsc` refuses that program at the parameter instead (TS2344, `K` is not a key type).
- **Through second names (G456, G458).** On top of 0.1.170's expansion of transparent declarations, a field typed `Same<T>` is the enclosing `T`: `fn r2<T>(x: One<Same<T>>) -> Record<string, string>` is E0204 (G456, TS2322), and so is a `Same<T>` returned where `string` is declared (TS2322). A `T` field where `Record<string, Nullable<T>>` is declared, directly or through `type N<X> = Nullable<X>`, is compared against `T` (D45) and accepted, where 0.1.170 refuses it and `tsc` accepts it (G458). Both close here.
- **Not reached.** Constructor sites the checker does not type (G449's family): `Some(5)` or `None` returned under `-> T`, `Some(x)` under `-> Option<string>` and `Ok(x)` under `-> Result<string, string>` pass `--no-tsc`, and `tsc` refuses all four. An object literal where a `Record<string, V>` is declared is not compared whatever it holds; that is G454, filed here and independent of type parameters.
- **Measured.** Across 285 units (every app directory, every standalone file under `examples/` and `benchmarks/glyph/` in its own directory, `catches/*`, `tests/exact-or-absent/*/src` and `tests/negative/*`), run one at a time under the published 0.1.170 (0.1.171 is not on npm yet) and this build, rebased onto 0.1.171, with `build --no-tsc --no-test` and `check --no-tsc --json`: the output is byte-identical on all 285, so no corpus site needed migrating. The same run against 0.1.167 before the rebase also differed nowhere.
- **Breaking.** Yes, under `--no-tsc`. `fn g<T>(x: Array<T>) -> Array<string> { return x }` exits 0 under the published 0.1.170's `check --no-tsc` and 1 under this build's; full `check` exits 1 on both, from TS2322. G458's `j` goes the other way, from 1 to 0, and passes `tsc --strict`.

**Planned, after this batch · G438, an `==` whose operands can never be equal is refused without `tsc`** (record 163). A comparison whose decided operands are disjoint string-literal sets (`m == "zzz"` on a `Mode`) is TS2367 under full `check` and passes `--no-tsc`. The checker refuses it; then the emitter may write `===` again for a comparison with an unannotated string `const`, which G433 keeps on `__glyph_eq`. A `number` or `bool` constant keeps the helper until Glyph has literal types for those. Breaking under `--no-tsc` only.

**0.1.173 — Shipped 2026-11-24 · G263, a generic bound is resolved, and a bounded parameter is decided as a value** (record 179, fork (b)). Bound names resolve like any other name: an unresolved bound is E0103 (`tsc` reports TS2304), for functions and for generic components (G264). In the same release the bounded-value rule from record 88 switches on: inside its declaring function a `T: Named` passes where `Named` is declared and is refused elsewhere (`T: Named` returned where `Other` is declared, TS2322). Breaking.
- **The declaring side of a bound (record 182).** This release is everything about a bound inside the declaration that writes it. The calling side, where a bound is instantiated, is 0.1.179 (G264, widened), including the generic component elements the heading above names.
- **Resolved.** The resolver binds a declaration's type parameters, then resolves each bound through the annotation path, on a `fn`, a `component`, a `type` and an `interface`. `fn c<T: Nope>(x: T) -> T` and `component Bad<T: Nope>` are E0103 (TS2304 under `tsc`), and `T: l.Nope` is E0105 (TS2694). A bound may name a sibling parameter on either side of it. An import used only in a bound is a use, so the false E0106 `unused import` on it, whose help broke the program, is gone. The E0103 explanation says a bound is a name like any other.
- **Decided as its bound (D58).** Inside its declaring function and any lambda inside it, a value of `T: B` passes where `T` or `unknown` is declared and is otherwise compared as a `B` by the existing relation, at every depth. Refused, each one `tsc` refuses with TS2322: `T: Named` returned where `Other` is declared, `T: HasLabel` where `Named` is declared, `T: string` where `number` is declared, `Array<T>` where `Array<Other>` is declared, `T: Named` where another parameter `U: Named` is declared, and a `Box<T>` with `T: Person` where `Record<string, string>` is declared. Accepted, each one `tsc` accepts: `T: Named` where `Named`, `{ name: string }`, `Nullable<T>`, `Nullable<Named>` or `unknown` is declared, `T: l.Named` where `Named` is declared and the reverse, `Array<T>` where `Array<Named>` is declared, `U: T` where `T` is declared, and a `Box<T>` with `T: Named` where `Record<string, Named>` is declared, which 0.1.172 refuses. One new refusal is Glyph's own: `T: HasName` for an interface `HasName` where the record `Named` is declared is E0204, as the concrete `HasName` is, and `tsc` accepts it.
- **Members read off the bound.** A member read, a method call, an index read and a `for` over a value of `T: B` are checked against `B`: `x.nmae` on `T: Named` or `T: l.Named` is E0210 with ``did you mean `name`?`` (TS2339), `x.name` returned where `number` is declared is E0204, `a.rnak()` on `T: Ranked` is E0210, and `p.lable` in `component Typo<T: HasLabel>(p: T)` is E0210 (TS2551). The comment in `examples/corpus/ranking.glyph` calling `x.rank()` a checked method call on the type parameter is now true: a misspelt `rank` in `higher` and `lower` is E0210, where 0.1.172 reports only the non-generic sites.
- **Circular bounds (record 184).** A chain of bounds that are bare sibling parameters and loops back (`<T: U, U: T>`, `<T: T>`, `<A: B, B: C, C: A>`) is the new E0114, once per cycle at its first parameter, with the cycle named in the message (`circular bound: \`T: U, U: T\``). `tsc` refuses each (TS2313). An F-bound such as `<T: Cmp<T>>` is not a cycle and stays legal.
- **Not read.** An `extern_ts` or `typeof` bound leaves the parameter as 0.1.172 had it: decided only against another of the function's parameters, with the old map rule for a field of it, and `tsc` judges the rest.
- **Not reached, filed here.** An interface value where a primitive is declared passes the relation in general, so `T: Named` with an interface `Named` passes where `string` is declared (G460, TS2322 under `tsc`). A member read on an unbounded `T` passes (record 182, N4).
- **Measured.** Across 286 units (every app directory, every standalone file under `examples/` and `benchmarks/glyph/` in its own directory, `catches/*`, `tests/exact-or-absent/*/src` and `tests/negative/*`), run one at a time under the published 0.1.172 and this build, rebased onto it, with `build --no-tsc --no-test`: the only unit that differs is the new `tests/negative/circular_bound.glyph` (E0114), so no corpus site needed migrating. The same run against 0.1.170 before the rebase gave the same answer. `ranking.glyph`'s nine `<T: Ranked>` functions, `heap.glyph`'s `of_ranked` and both `object_schema`s pass on both.
- **Breaking.** Yes, under `--no-tsc`. `fn c<T: Nope>(x: T) -> T { return x }` exits 0 under the published 0.1.172's `check --no-tsc` and 1 under this build's, and so do `T: Named` returned where `Other` is declared, `x.nmae` on `T: Named`, and `fn x1<T: U, U: T>(x: T) -> T` (E0114; `tsc` refuses it with TS2313). A `Box<T>` with `T: Named` returned where `Record<string, Named>` is declared goes the other way, from 1 to 0 against 0.1.172, and passes `tsc --strict`.

**0.1.174 — Shipped 2026-11-24 · G457, a block's last expression is compared with the declared return** (record 179, N1; record 182, section 2; breaking under `--no-tsc`). Branch `dev-0.1.174`. G457 closes with it, and so does G470, its per-arm half, filed on `dev-0.1.175` and moved here by record 183: `return match f { true => s, false => 0, }` with `s: string` and `let r: number = match v { is string => v, else => 0, }` under a declared `number` are E0204 at the `string` arm, written after `return` or as the tail, and both are tests.
- **One path.** The value a `return` hands on and a returned tail go through one function, `walk_returned_value`: the lambda context a declared return gives (D55), the option-slot consumer (E0235), the comparison with the declared return (D57's literal fields, D58, the async unwrap, and E0238's pending value left uncompared), and E0223 for a `match` arm with no value. Which tails are returned is one predicate, `tail::returns_tail`, read by the checker and by the pending and mutation walks: a named callable's tail when it declares a return other than `void`, a component's always, a lambda's unless it is written `-> void`. That is the emitter's rule. A test runs the six callable shapes through all three walks.
- **Each arm on its own.** A `match` where a type is declared (a `return`, a returned tail, an annotated `let` or `const`, an argument) is compared arm by arm, into nested matches and the tail of a block arm; an arm that diverges adds nothing. Arms that disagree with each other used to join to a type nothing compared, so `return match b { true => 5, false => "x" }` under `-> string` passed. A refusal now underlines the arm, and names the arm's type rather than the join (`"exec"` where it said `"read" | "exec"`).
- **New refusals, each one `tsc` makes (TS2322 unless noted).** `fn z() -> string { 5 }`; `fn y(n: number) -> string { n }`; `fn() -> string { 5 }`; an async `-> string` whose tail is `load()` with `load: async -> number`; record 179's `tail_s`, `g`, `i`, `j` and `k`; `{ v: x, }` as the tail under `-> Box` with `v: string`; `{ v: "a", w: 1, }` there (E0210, TS2353); a block arm `{ 5 }` in a tail `match` under `-> string`; `return match b { true => 5, false => "x" }`; `let s: string = match b { true => { 5 }, false => "x" }`; a block arm ending in `io.println("x")` under `-> number` (found `void`); a tail `r?` in a lambda whose declared return is not the unwrapped type.
- **Unchanged.** A pending tail in a sync callable is E0238 alone. `fn d() { 5 }` and `fn e() -> void { "x" }` are not compared, nor is a lambda written `-> void`. `fn c(xs) -> number { xs.push(1) }` stays E0240's "its value is returned". A `match` passed where a callee's type parameter is declared binds and compares as before.
- **Not reached.** `Some(5)` and `None` returned under `-> T` still pass `--no-tsc` (G449's family), at a tail as at `return`. A body that ends without a value under a declared return (`fn a() -> number { let x = 1 }`, TS2355) and an unannotated lambda's returned value against the function type its position expects are record 182's N2 and N3.
- **Measured.** Across 384 units (every app directory, every `tests/exact-or-absent` unit and `tests/negative` directory, and every other tracked `.glyph` file in its own directory, `catches/*` included), run one at a time with `build --no-tsc --no-test --json` under the published 0.1.172 and this build, rebased onto 0.1.173: one unit differs, `tests/negative/circular_bound.glyph`, whose E0114 is 0.1.173's, and against the 0.1.173 branch build the two agree on all 384. The tail half and the per-arm half changed no site, so nothing needed migrating. Changing one arm of `status_name` in `jobq` to a number gives E0204 at that arm under this build and exit 0 under the published release. A bounded `T: Named` at a tail passes where `Named` is declared and is E0204 where an unrelated record is, as its `return` spelling is and as `tsc` says (TS2322).
- **Breaking.** Yes, under `--no-tsc`. `fn z() -> string { 5 }` exits 0 under the published 0.1.172's `check --no-tsc` and 1 under this build's; full `check` exits 1 on both (TS2322 under 0.1.172, E0204 here). One program goes the other way: `pub fn a() -> never { return process.exit(1) }` exits 1 under 0.1.172's `check --no-tsc` (E0204, ``expected `never`, found `?` ``) and 0 here, and `tsc` accepts it.
- **`never` and an untyped value (record 189, F1).** A value the checker has no type for was refused where `never` is declared, against the rule that an untyped value is never refused. With tails compared, that reached `fn fail() -> never { process.exit(1) }`, the way a `-> never` helper is written. The relation no longer refuses an untyped value there, at a `return`, a tail or an argument; a decided `5` under `-> never` is still E0204, and an unresolved name under `-> never` is E0103 alone. `process.exit` is untyped because its stdlib row predates `never`; typing it is G480, filed here and scheduled in the rolling lane.

**0.1.175 — Shipped 2026-11-24 · G415, `is Record` and `is Array` arms narrow** (record 134, fork 1; D53 amended; breaking under `--no-tsc`). Before G325, which would refuse the `unknown` an un-narrowed `is Array<string>` arm passes on. G415 closes with it.
- **E0224 and `unknown`.** A key read on a map, `m[k]` or `m.name`, is not E0224 when the map's `V` is `unknown`, written or through an alias of the map (`type Bag = Record<string, unknown>`) or of `unknown` (`Record<string, Raw>`). An absent key yields `undefined`, which `unknown` admits, so the read is sound. It is typed `unknown` rather than left untyped, so `"${m[k]}"` is E0237 and any other use has to narrow it. Every other `V` keeps E0224.
- **Containers narrow.** An `is Record<K, V>` or `is Array<X>` arm narrows its scrutinee under D53's rules, as a primitive or declared target already did: `is Array<string> => v.lenght` is E0210 with the `length` suggestion, `is Array<number> => v.map(..)` types the callback from the element, and `is Record<string, number> => v[k]` is E0224, the same answer a declared map gets. `Option`, `Result` and `Nullable` have no `is` check (E0300), so an arm naming one does not narrow and adds nothing beside the emitter's refusal.
- **Measured.** Across 286 units (every app directory, every standalone file under `examples/` and `benchmarks/glyph/` in its own directory, `catches/*`, `tests/exact-or-absent/*/src` and `tests/negative/*`), run one at a time with `build --no-tsc --no-test` and `check --no-tsc --json`, rebased onto 0.1.174: diagnostics and exit codes are identical to the 0.1.174 branch build on all 286, and to the published 0.1.172 on 285, the other being `tests/negative/circular_bound.glyph`, which is 0.1.173's E0114. So no site needed migrating. The corpus has four container arms (`examples/01_validator.glyph` lines 68 and 98, `examples/corpus/infer_output.glyph:39`, `benchmarks/glyph/parse_user.glyph:9`); all now narrow, and the two that read a key read a `Record<string, unknown>`. One emitted line changed against both: `01_validator`'s `for index, item in input` inside `is Array<unknown>` now loops over `input.entries()` instead of deferring to `__glyph_pairs`, since the arm has settled the shape. Full `check` passes `tsc --strict` on it and `glyph run` prints the same issue path.
- **Record 182.** Confirms record 134 fork 1 against this build, and its checklist holds. `return v` inside an `is Array<string>` arm under `-> number` is E0204 (0 to 1); a closure that reassigns the name stops a container arm narrowing (B1), a module `const` container arm narrows (B2), and the `else` arm keeps `unknown`. The arm-value form, `return match v { is Array<string> => v, else => 0, }` under `-> number`, is E0204 too, through 0.1.174's per-arm comparison (G457, which closed G470, filed while building this release).
- **Every spelling of the map (record 183, F1).** Whether a type is a map, and its `V`, are read through the D46/G315 expansion, so a map written out, a local alias, an alias of that alias, a named import and a namespace import of a sibling's alias, and an `is` arm naming any of them, get one answer: E0237 on `"${m[k]}"` where `V` is `unknown`, E0224 on `m[k]` where it is not. Before the fix only the first two spellings were read as a map, which 0.1.170 shares for E0224; `Bag`, `a.Bag` and `LBag2 = LBag` now draw the E0237, and `SBag`, `a.SBag` and `LS2 = LS` the E0224, which `tsc` does not make. The corpus run above includes it.
- **Not in this release.** E0224's message puts the placeholder `that key` in backticks as if it were code (record 182, n1). Record 182 sends it to the rolling lane. Record 183 filed G471 (a write into a container is not compared with its element type), scheduled beside G325, and G472 (a write through a second name breaks a narrowing), filed with G394 and awaiting a decision.
- **Breaking.** Yes, under `--no-tsc`. Measured against the published 0.1.172 and the 0.1.174 branch build, which agree on every probe here. Under their `check --no-tsc`, `is Array<string> => v.lenght` and `is Record<string, number> => v[k]` exit 0, and under this build 1 (E0210, E0224). Full `check` already refused the first (TS2551) and accepts the second, which is E0224's own rule. `is Record<string, unknown> => "${v[k]}"` goes from 0 to 1 (E0237, record 183, N1), and so does a read through an imported or chained alias of a map: of `unknown` under interpolation (E0237), of any other `V` (E0224). A declared `m: Record<string, unknown>` read `m[k]` goes the other way, from 1 to 0 under both. The G415 reproduction itself exits 1 under every binary (E0210 and E0224 before, two E0210s after).

**0.1.176 — Shipped 2026-11-24 · G325, `unknown` is assignable only to `unknown`** (record 89; breaking). A found `unknown` where any decided type other than `unknown` is declared is refused at every depth the relation recurses into: E0204 at a `let`, `const` or `return`, E0211 at an argument. Today Glyph's checker never refuses `unknown` on the found side, so `let s: string = u` with `u: unknown` passes `glyph check --no-tsc` and the agent gate, and only `tsc` catches it. After G315 (0.1.170), so `type Raw = unknown` expands first; G314 step 2 (0.1.172), so the checker catches a generic wrapper such as `fn read<T>(s: string) -> Result<T, Array<Issue>> { return json.parse(s) }` itself; and G415 (0.1.175), so the `unknown` an `is Array<string>` arm narrows is not refused. Proved by a two-binary corpus run in which every new refusal is one `tsc` makes. Branch `dev-0.1.176`, cut from the approved 0.1.172 work on top of 0.1.171 (record 182, section 4), rebased onto 0.1.173 to 0.1.175 before it was measured.
- **The rule (D65).** Only the written `unknown`, a transparent name for it (`type Raw = unknown`) and `json.parse`'s unchecked read are `unknown` here. One found where any other type the checker decides is declared is refused at every depth the relation reads: E0204 at a `let`, `const`, `return` and record field, E0211 at an argument. Decided means a primitive (`void` included), `never`, a literal union, a record, a function type, a tagged union, an enclosing type parameter, a prelude container (`Nullable<X>` only when `X` is decided, since `Nullable<unknown>` is `unknown | null`), `Issue`, a stdlib type, an interface, and a local or sibling declaration of one of those. A value the checker has no type for, a callee's unbound parameter, and `Schema<T>`, `Component<P>`, `infer_output<S>`, `extern_ts`, `typeof` and package types are not judged.
- **D58 flips.** An `unknown` where an enclosing `T` is declared was undecided; it is E0204 now (`tsc` TS2322).
- **Help.** A refusal that is G325's alone (the pairing passes once every `unknown` in the value's type is read as unknown to the checker) says: "An `unknown` stands only where `unknown` is declared. Check it first: `json.parse<T>(text)` or `T.parse(value)` gives a `Result<T, Array<Issue>>`, and a `match` arm with an `is` pattern (`is string => …`) narrows it." Any other refusal keeps its own help, so `Array<unknown>` where `string` is declared still says to change the value or the type.
- **New refusals, each one `tsc` makes.** Record 182's seven k1 sites: `let s: string = u`, `takes(u)`, `{ name: u }` as a `User`, `type Raw = unknown` returned as `string`, `fn e<T>(s) -> Result<T, Array<Issue>> { return json.parse(s) }`, `[u]` as `Array<string>` and `u` as `Nullable<string>` (TS2322 and TS2345). Also `u` under `-> T`, `-> void` and `-> fn(number) -> string`, an `Array<unknown>` binding returned as `Array<string>`, a `fn(number) -> unknown` where `fn(number) -> string` is declared, and `takes(u)` in the `else` arm of `match u { is string => .. }`. `-> never` was already refused. The pinned `json.parse` test's line `a` flips to E0204.
- **Accepted.** k1's `h`, `i` and `j`, `string.from(u)`, `User.is(u)`, `User.parse(u)`, `io.inspect(u)` (a stdlib parameter typed as nothing), `u` as `Nullable<unknown>` and inside `Record<string, unknown>`, `json.parse(s)` as `Result<unknown, Array<Issue>>`, `u` as an `extern_ts` type, and a `fn(number) -> unknown` where `fn(number) -> void` is declared.
- **`is T` on a type parameter (record 194).** Under D65 the arm value of `match v { is Doc => v, .. }` inside `fn h<Doc>` drew E0204, which stopped the build before the emitter's E0304 named the real mistake, and a second such pattern elsewhere in the file surfaced only after the first was fixed. The checker now reports E0304 itself at every `is` pattern that is or holds a type parameter of the enclosing `fn` or `component`, with the emitter's message, help and note (one shared text), all in one run, and leaves the arm's name untyped so nothing else is reported about it. `is Doc` with `Doc` a declared type still narrows. The emitter keeps its check for the other spellings (`json.parse<T>`, `T.parse`, `Box.parse<T>`). No exit code moves: the program is refused before and after.
- **`glyph_impact`.** An `unknown` argument has a cell of its own in the `change_signature_type` table: it asks the relation, so `takes_local(u)` against a literal union is WILL_FAIL naming D65, where it was UNDETERMINED, and `glyph_assignable` gives the same answer for the two types.
- **After the rebase (record 186).** On 0.1.173 to 0.1.175, the tail `fn n(u: unknown) -> string { u }` is E0204, so is the `else` arm of `match u { is number => u, else => u }` returned where `number` is declared, an `is Array<string> => v` arm value is accepted where `Array<string>` is declared, and the same arm is E0204 when a closure reassigns `v`. The published 0.1.172 and the 0.1.175 branch build report none of the four.
- **Not reached.** `Some(u)` where `Option<string>` is declared passes, because the payload of `Some` is compared only when written out (G436).
- **Measured.** 376 units, one at a time, with `build --no-tsc --no-test --json`: every app directory, every other tracked `.glyph` file in its own directory (`examples/`, `examples/corpus/`, `benchmarks/`, `playground/examples/`, the fuzz seeds and the compiler's fixtures), `catches/*`, `tests/exact-or-absent/*` and `tests/negative/*`. Against the 0.1.175 branch build the output is byte-identical on all 376, so no corpus site needed migrating, which matches record 182's proxy count of 0. Against the published 0.1.172 one unit moves, `tests/negative/circular_bound.glyph` (exit 0 to 1, E0114), which is 0.1.173's. Before the rebase the run against the published 0.1.171 was byte-identical on 375 units.
- **Breaking.** Yes, under `--no-tsc`. `let u: unknown = get()` then `let s: string = u` exits 0 under the published 0.1.172 and the 0.1.175 branch build with `check --no-tsc`, and 1 under this build.
- **Filed.** G490: a value of an `extern_ts` type is refused where a string-literal union is declared, though `tsc` accepts `extern_ts("\"read\"")` returned as `"read" | "write"`. A false refusal in `string_literal_union_verdict` that predates this release; for the orchestrator to place.

**Planned, after 0.1.176 · G491, a function value's parameter types are compared** (record 186, N1; breaking under `--no-tsc`). Parameters are compared contravariantly through the whole relation, so `takes_cb(fn(x: number) -> string { .. })` where `fn(string) -> string` is declared is E0211 (`tsc` TS2345), and D65 then reaches an `unknown` in a parameter (`fn(string) -> void` where `fn(unknown) -> void` is declared). Measure with the two-binary run.

**Planned, its own release after 0.1.170 · G432, a `??` default that disagrees with a `Nullable` or optional left side** (decision record 157, section 1). `n ?? "x"` on a `Nullable<number>` and `b.n ?? None` on a field declared `n?: number` are typed `Unknown`, so a mismatch passes `--no-tsc`; a `??` on a value that can never be absent is dead. Kept out of G429 because index reads make the second a false-refusal risk.

**Planned, beside G325 · G471, a write into a container is compared with its element type** (record 183, N2). On a declared `Array<string>`, `mut v.push(5)` and `mut v[0] = 5` pass `--no-tsc`, and so does `mut m["k"] = 5` on a `Record<string, string>`; full `check` refuses all three (TS2345, TS2322). Inside an `is Array<string>` arm `tsc` misses the two array writes too, since `Array.isArray` hands it an `any[]`, so after G415 the arm says `Array<string>` and lets a `5` in. The write-side counterpart of G325: a value written into a container is related to its element or value type as an argument is to a parameter.

**0.1.177 — Shipped 2026-11-24 · G388, `.sort()` on a non-string array needs a comparator** (agent research, decision record 182 section 5a; E0243; breaking). Branch `dev-0.1.177`, cut from the approved 0.1.172 work on 0.1.171 and replayed onto main after 0.1.176. G388 closes with it.
- **The rule (E0243).** `sort()` and `toSorted()` with no comparator on an `Array<E>` are refused unless `E` is text: a `string`, a string-literal union, a refined `string`, or a second name for one, through any number of hops. Refused for `number`, `int`, `bigint`, `bool`, records, interfaces, tagged unions, `Option`, `Nullable`, arrays, `unknown`, the stdlib's own types and an unbounded enclosing type parameter. A receiver the checker cannot type is not judged. `array.sort` is unchanged; it always takes a comparator.
- **The help keeps the form the call was written in.** `mut xs.sort(fn(a: number, b: number) -> number { return a - b })` for the statement, and `array.sort(xs, ...)` for `toSorted` and where a sort's value is read. An `int` gets the same comparator written with `int`. Anything else gets `cmp` and the type it must have, `fn(a: User, b: User) -> number`, negative when `a` comes first; a `bigint` is one of these, since `a - b` on two of them is not a `number`. `toSorted` gets the copy `array.sort` returns rather than `xs.toSorted(cmp)` (record 187): the tsconfig Glyph emits asks for `es2022`, which has no `toSorted`, so that spelling fails `tsc` (TS2550) with a comparator or without; revisit once G427 raises the target. A `.slice()` before the sort is dropped from the copy, since `array.sort` copies already. A test applies every help for every refused type and runs the full check, `tsc` included. `glyph fix`'s decline names the one reason that applied.
- **E0240 changes with it.** On a bare `xs.sort()` over numbers both fire, and E0240's help writes `mut xs.sort(fn(a: number, b: number) -> number { return a - b })` where it used to write `mut xs.sort()`, the same text-order sort. Its value helps use the same comparator. `glyph fix` no longer writes the `mut` there, since `mut` alone leaves a refused sort, and says why. A sort of the array a `store.Store` hands back gets E0240's store help on both.
- **A bounded type parameter is read as its bound** (record 192). With 0.1.173's bounds underneath, the element test reads an enclosing `T` through its bound: `T: string`, `T: Tier` and `U: string, T: U` sort as text and are accepted, `T: Named` is E0243 with the `fn(a: T, b: T) -> number` help, and an unbounded `T` is refused as before. A bound the checker cannot read is not judged. Each case is a test, and the `T: Named` help is applied and passes the full check.
- **Measured.** Across 286 units (every app directory, every standalone file under `examples/` and `benchmarks/glyph/` in its own directory, `catches/*`, `tests/exact-or-absent/*/src` and `tests/negative/*`), run one at a time with `build --no-tsc --no-test` under the published 0.1.173 and this build, rebased onto `dev-0.1.176`: one unit changed, `catches/numbers-sorted-as-text`, the G388 catch case, which is a refused case under E0243 now. No other site needed migrating. The same run before the rebase, against 0.1.171 over 285 units, also changed only that case.
- **Filed.** G500: a `bigint` is typed as `number` by Glyph's checker (record 187), in the rolling lane.
- **Breaking.** Yes. `mut scores.sort()` on `[100, 9, 25]` exits 0 under the published 0.1.173 (and 0.1.171 and 0.1.170), with `check --no-tsc` and with full `check`, and 1 under this build.

**0.1.178 — Shipped 2026-11-24 · G389, a member TypeScript marks `@deprecated` draws a warning** (agent research; decision record 182, section 5b). Branch `dev-0.1.178`, on main after 0.1.177. G389 closes with it.
- **The rule.** A read or call of a member TypeScript's library marks `@deprecated`, on a receiver the checker types as a `string`, an `Array<T>` or a `number`, is warning E0244, through `?.` and an index read included. A warning, not an error: each one runs in every engine and `tsc --strict` accepts it. The message quotes the tag (`` `s.substr` is deprecated: A legacy feature for browser compatibility ``) and the help is computed from the call: `s.slice(1, 3)` for `s.substr(1, 2)`, `s.slice(i, i + n)` for `s.substr(i, n)` with the condition that both are whole numbers of zero or more (record 185, F1: `substr(0.5, 0.6)` is `""` and `slice(0.5, 1.1)` is one character), the general help with that condition for a negative or fractional literal, which the help never names in a condition, `s.trimStart()` and `s.trimEnd()` for `trimLeft` and `trimRight`, and the template string that builds the same markup for the 13 HTML methods (`"<b>${s}</b>"` for `s.bold()`, with a literal argument written in as the method writes it, `"` as `&quot;`). Each spelling the tests check builds under `tsc` and prints what the deprecated call printed.
- **Where the list comes from.** `check_builtin_members.py` reads each method's tag from the pinned TypeScript 6.0.3 into the generated rows: 16 string members, none on an array or a number. Only the `trimLeft` and `trimRight` tags name a replacement, so the replacement is the hand table `DEPRECATED_REPLACEMENT`. Generation, and so CI, fails when a deprecated method has no entry, an entry names a method TypeScript does not deprecate, a replacement is absent or itself deprecated, a tag's "Use `X`" disagrees with the entry, or node disagrees with an entry (a rename is the same function, an HTML method builds the markup its entry names, `substr(a, n)` equals `slice(a, a + n)` for whole `a` and `n` of zero or more, and differs for `(0.5, 0.6)` and `(0, -1)`, so node pins the condition from both sides). Broken by hand, each direction fails with the member named.
- **Not reached.** A deprecated member of an npm package or a node builtin: the checker does not type those values. That is research item N5 of record 182. `glyph fix` writes nothing for E0244: `slice(a, a + n)` evaluates `a` twice, and the renames are a later polish item.
- **Measured.** Across 286 units (every app directory, every standalone file under `examples/` and `benchmarks/glyph/` in its own directory, `catches/*`, `tests/exact-or-absent/*/src` and `tests/negative/*`), run one at a time with `build --no-tsc --no-test` and `check --no-tsc --json`. Against 0.1.177's code (main at the 0.1.177 release commit, built locally; 0.1.177 is not on npm yet) the output is byte-identical on every unit, warnings included. Against the published 0.1.176 one unit moves, `catches/numbers-sorted-as-text`, from exit 0 to 1 by 0.1.177's E0243. No tracked program uses any of the 16 members.
- **Breaking.** No. A warning changes no exit code: `s.substr(1, 2)` exits 0 under the published 0.1.176's `check --no-tsc`, under 0.1.177's, and under this build's, which prints the warning.

**0.1.179 — Shipped 2026-11-24 · G501, a value written by `mut` is compared with its place** (decision record 196, section 1; breaking). G501 and G512 close with it; G471 is half fixed.
- **The rule.** `mut P = e` compares `e` with P's type the way `let x: T = e` compares its initializer: E0204, arm by arm for a `match` (G457), with D57's rules for a written literal, which is reported once. `mut x = "s"` on a `number` binding, `mut r.n = "x"`, `mut r.tags[0] = 5`, `mut v[0] = 5` on an `Array<string>`, `mut m["k"] = 5` on a `Record<string, string>` and `mut r.mode = "exec"` against `Mode` are E0204; `tsc` refuses each (TS2322). So is `mut u[0] = 5` inside an `is Array<string>` arm, which `tsc` accepts because `Array.isArray` gives it an `any[]`. A name holds its annotation or, under D66, its `let` initializer's type; a field its declared type; `v[i]` the element or value type. A name typed by neither (a `for` binding, an arm's payload, a lambda parameter typed by its slot) and a receiver the checker cannot type are not judged. The note says where the type came from (`` `y` holds a `number`: the type of its initializer at line 9, and a `mut` does not change a binding's type ``) and the help is "Write a value of that type, or bind the new value to a new name." Closes G471's index half; the method half is 0.1.181's.
- **G512, found on the way.** A `let` over an unannotated `const` held the `const`'s literal (`let dir = DEFAULT_DIR` held `"examples/apps/chat"`), where `tsc` widens it to `string`. With G501 that refused `mut dir = ...` in five example apps. The `let` now holds a `string` when it reads an unannotated `const` of string literals (a written string or a `match` of them), local or imported by name or namespace; an annotated `const` keeps its type. D66 states both.
- **G511, not done.** Record 196 assumed E0204's key rule holds the key of an index write to `K`. It does not, and record 201 widened the gap to every index expression, read or write, map or array (`mut m["exec"] = 2` on a `Record<Mode, number>` passes `--no-tsc`, and `tsc` reports TS7053). Record 201 decided it as E0204 and scheduled it in the next batch.
- **Measured.** `scripts/measure_corpus.py` over its 375 units against the published 0.1.178: no unit differs, check exit pairs (0, 0) 195 and (1, 1) 180. Before G512 the same run moved five apps (chat, csvql, discord, jobq, workflow) from 0 to 1, all of them the `const` widening above.
- **Breaking.** Yes. `let x: number = 1` then `mut x = "s"` exits 0 under the published 0.1.178's `check --no-tsc` and 1 under this build; full `check` exits 1 under both (TS2322 before, E0204 now). A `let` over an unannotated `const` holding a string literal is a `string`, so passing it where a string-literal union is declared is now E0211, as `tsc` reports it.

**0.1.180 — Next · G390, E0225 covers index and element writes across an `await`** (agent research; decision record 196, section 2; breaking). Branch `dev-0.1.180`. G390 closes with it.
- **The rule.** A place is a parameter followed by fields and indexes, and E0225 fires on any of them read before an `await` and written after it: `c[0]`, `m["k"]`, `a[0].balance` and `c[i]` join `a.balance`. An index counts only when its key is stable: a number or string literal, or a name no `mut` reassigns anywhere, read from the `Reassigned` predicate `is`-arm narrowing already uses. Two places are the same when every step is equal as written, and a name step also by the binding it resolves to. A read spelled `array.get(c, 0)` or `record.get(m, "k")`, under any import spelling, is a read of that element, since E0224 refuses the bare read on a `Record`; the message then ends ``(read through `array.get(c, 0)`)``. The help is unchanged.
- **Unchanged.** `c[i + 1]`, and `c[j]` where some `mut j = ..` exists, are not tracked. A local array, a `push` and a read after the `await` stay silent, and a write clears the place. Reading `a[0].balance` is not a read of `a[0]`, the same way `c.n` is not a read of `c`. Async lambdas are still not visited.
- **Measured.** Across the 375 units of `scripts/measure_corpus.py`, against the published 0.1.178 (0.1.179 is not merged), one unit moves: the G390 catch case, which exits 1 by E0225 at `glyph.glyph:13` and is now the refused case `catches/element-update-across-await`. No other async function in the corpus reads and writes a parameter's element across an `await`.
- **Breaking.** Yes. The record's `idx` probe exits 0 under the published 0.1.178 and 1 under this build, with `check --no-tsc` and with full `check`.

**0.1.181 · G505, a `mut` method call's arguments are checked** (record 196, section 3; breaking). `mut v.push(5)`, `mut v.fill("x", "a")` and `mut v.splice("a", 1)` on an `Array<string>` are E0211, as the same calls are in value position; a built-in rest parameter checks each argument against its element type (`string.concat`, G492). Closes G471's method half and G492. The sort comparator's parameter types wait for 0.1.183.

**0.1.182 · G460 and G502, an interface and a primitive are compared** (record 196, section 4; breaking). A primitive, a string-literal union or an array where an interface is declared is compared through its apparent members (`"abc"` satisfies `interface HasLen { length: number }`, `5` does not satisfy `HasName`); an interface value where a primitive is declared is refused. `glyph_assignable` and `glyph_impact` follow the relation for these pairings.

**0.1.183 · G500, `bigint` is its own type** (decision record 200, ruling 1; breaking). The checker holds `bigint` as a type of its own (D38) instead of as `number`: `1n` is a `bigint`, a `bigint` and a `number` are not assignable to each other, and `glyph query assignable` never answers COMPATIBLE for a `bigint` against `number` or against an interface only a `number` satisfies. It ships before G491, whose full comparison of method members would otherwise make that false answer reachable.

**0.1.184 · G439, an index read is typed as its element** (decision record 199, ruling 2; breaking). (record 163, probe k08). `let b: string = ys[0]` with `ys = [1, 2]` passes `--no-tsc` and `tsc` refuses it (TS2322). The index read carries the element type into the comparison. Breaking under `--no-tsc` only.

**0.1.185 · G491 and G434, a function value's parameters are compared, and function types are printed** (record 196, section 5; breaking). Parameters are compared contravariantly, a value taking more required parameters than the slot passes is refused, a generic function value is instantiated against the slot before comparison, and every diagnostic between two function types prints both signatures.

**0.1.186 · G506, call sites in an `@example` are in CALLS and REFERENCES** (record 196, section 8; not breaking, answers change). `glyph_impact`, `glyph_references` and `glyph_dependencies` list a call inside an `@example` with `context: "example"`; a `@doc` `glyph @run` fence is indexed or named under `not_indexed`.

**0.1.187 · A project-level banned import** (agent research), set in the `package.json` `"glyph"` key, whose error names the project's own wrapper, so an agent cannot bypass it for the raw library. Specified in record 196, section 9: `glyph.banned_imports` maps an import path to `use`, optional `reason` and `allow_in`; a banned import is E0115 and a malformed `glyph` key is E0116, and the `glyph` key becomes a closed schema.

**0.1.188 · G461, a generic interface's members are checked** (record 184, N1; breaking). A member read and a method call on a value of `Cmp<string>` under `interface Cmp<X> { fn cmp(o: X) -> number  v: X }` read the member set through the application, with `X` substituted: `c.cmp("a")` returned where `string` is declared and `c.v` where `number` is declared are E0204, `c.nope` is E0210 and `c.cmp(5)` is E0211, as a non-generic interface's are today; `tsc` refuses the first three. It is what gives an F-bound such as `T: Cmp<T>` (0.1.173) something to guarantee. Measure the corpus with the two-binary run.

**Next batch · G264, a bound is checked where it is instantiated** (record 182; breaking). The calling side of a bound, split from 0.1.173 by risk: an upgrader's build goes red at a call site here, inside a generic body there. With `fn label<T: HasName>(x: T)`, `label(5)` and `label(o)` with `o: Other` are E0211 (TS2345 under `tsc`). A written type argument (`label<Other>(o)`) and a type application (`b: Box<Other>` under `type Box<T: HasName>`) are refused too (TS2344), by a new code or by E0233 with a "does not satisfy the bound" message; the builder reports which, and why. An element of a bounded generic component has its attributes checked against `B` as for `component S(p: B)`: `<Show />`, `<Show label="x" other="y" />` and `<Show label={5} />` against `component Show<T: HasLabel>(p: T)` are refused, as `tsc` refuses them (TS2769). Corpus exposure is small (`object_schema` called with an object literal, which G454 leaves unjudged, and `of_ranked(jobs)`); measure it with the two-binary run.

**Next batch · G511, an index key is compared with the key type** (record 201; breaking). The key of every index expression, read or write, is compared with the key type: `K` on a `Record<K, V>`, `number` on an `Array<E>`. `mut m["exec"] = 2` on a `Record<Mode, number>` (TS7053), the read `u["exec"]` on a `Record<Mode, unknown>` (TS2345, which E0224 lets through because `V` is `unknown`) and `mut v["a"] = "x"` on an `Array<string>` (TS7015) are E0204 underlined at the key, with the note "`m`'s keys are `Mode`" or "an array is indexed by a number". Not E0224, which means a read may be absent, and no new code. Built right after G439, on the same index machinery, and rebased on it. Ranges G576 to G580 and D79. Measure with the corpus run; `Record<literal union, V>` writes are where any exposure is.

**Next batch · A `change_behavior` kind for `glyph query impact`** (agent research), listing the transitive callers of a function and the `@example`s that cover them, for a change that keeps the signature and alters what the function does. Specified in record 196, section 10, after 0.1.186 indexes example sites.

**Next batch · Find a declaration by signature** (agent research). A `glyph query` verb and MCP tool, so an agent can find the existing helper before writing a second one (copy-paste share of changed lines rose from about 8% to 12% between 2021 and 2024 in GitClear's data). Specified in record 196, section 11; after 0.1.183, without which every function-type pairing is undecided.

**Next batch · G307, `array.concat` takes one element type** (record 88). Both parameters are `Array<T>`, bound by the existing rule (the first binding wins, and a `string` candidate replaces a literal union bound earlier), extended so a `number` candidate replaces a bound `int` (D31). `Array<number>` with `Array<string>` is refused, as `tsc` refuses it; both orders of each pair are tested

**Next batch · G308, an optional trailing parameter is published as incomplete** (record 88). `string.pad_end`, `pad_start`, `string.slice`, `string.index_of`, `array.slice` and `json.stringify` are published in `llms` and AGENTS.md as incomplete rows, where they say today that every parameter is required. A spelling for an optional parameter in a function type is a language decision user code should drive. Changes the `llms` bytes, so it lands before 0.1.186

**Next batch · Stdlib signatures from one source** (record 88; decided in record 79, decision 5)
- **The source is Glyph function headers with no body**, `pub fn concat<T>(a: Array<T>, b: Array<T>) -> Array<T>`, one file per module, embedded in the typechecker crate with `include_str!` (so the playground has it) and parsed once through the parser's own fn-header production. Parameter names come with it for AGENTS.md, and type parameters through existing declaration syntax; a type parameter only the return names is explicit by construction, and a test holds that set equal to today's `explicit_type_params`. A name the file cannot resolve fails a unit test rather than lowering to `Unknown`
- **A row that holds the checker's `?` keeps its Rust form**, and a ratchet test holds the count of Rust-form rows from rising. The checker's table, the AGENTS.md lines and `llms` are built from the source, and the port's gate is a byte-identical `glyph llms --json`

**Next batch · The 166 unsigned stdlib exports**, module by module (one is `std/schema`'s `MAX_DEPTH`, the depth limit 0.1.150 made public), each moving from the Rust table into the Glyph-syntax source once it has a complete row (record 88). `intl.format_list` and `intl.best_locale` move to `ReadonlyArray<string>` when they do, or the 0.1.132 gate fails

**Next batch · G504, a member read on an unbounded type parameter is refused** (record 196, from record 182 N4). `x.name` on `fn n4<T>(x: T)` passes `--no-tsc`; `tsc` refuses it (TS2339).

**Next batch · G503 with G406, an unannotated lambda's returned value is compared with the function type its position declares** (record 196, from record 182 N3). `c(fn(x) { return x })` against `fn(number) -> string` passes `--no-tsc`; `tsc` refuses it. Scheduled with G406, which lowers an unannotated lambda's return to `void`.

**0.1.149 — Shipped 2026-11-19 · G370, two statements on one line are a parse error** (record 112; breaking)
- **G370.** Inside a block, a statement that begins on the line where the previous one ended is E0013, generalizing G351 from `{` to every token that can begin a statement. The message quotes the earlier statement, and the help fits the token: G217's rewrite for `{`, "Glyph has no `as` cast" with `T.parse` for `as`, "adjacent values are not joined" with `"a" + "b"` for a literal, and "start it on its own line" otherwise. A token that cannot begin a statement (`)`, `,`, `=`, a binary operator) keeps its old error. One-line bodies, one-line matches, record literals and trailing comments are unchanged. The G162 fuzz seed was split onto separate lines and parses to the same tree. D1's "statements end at significant newlines" is now literally true inside blocks
- **Breaking.** From exit 0 to 1 under `check --no-tsc` and full `check`, all accepted by `tsc`: `let x = h() g`, `let s = "a" "b"`, `let a = 1 let b = 2`, and a statement after a one-line `for`, `match`, `loop`, lambda or JSX element on the same line. `return h() as R` was already exit 1 (E0103, E0228) and is now E0013 alone; `glyph fmt` refuses these files where it used to split them. Measured against the 0.1.148 code: `check` and `fmt` identical on 464 of 467 tracked files (the two G217 fixtures' message text and `unsafe_cast.glyph`'s new code), 324 builds and 2832 emitted files identical, all 32 apps check the same
- **What it does not do.** Top-level declarations on one line (`pub fn a() {} pub fn b() {}`) still parse (G379, planned)

**0.1.148 — Shipped 2026-11-19 · G367, a Record's key type is checked at run time** (records 110 and 113; breaking)
- **G367.** Every `Record<K, V>` check, on every path that runs one (`json.parse`, a record field, `parse`, `is` and `schema`, an `is` pattern, a container's part, a generic's argument, a union payload, a sibling alias), checks each key against `K`, through one key check that both check builders call. A string-literal union `K`, however spelled, is the exact member set: every key a member and every member present. A `where`-refined string runs its predicate on each key. `int`, `number` and refinements of them need the canonical spelling of a number (`"01"` and `"1.5"` are refused for `int`). `Record<string, V>` is unchanged and its emitted code byte-identical, which is every one of the 177 tracked uses
- **New E0304s.** A `Record` whose key type has no runtime check, and a generic record whose `Record` field is keyed by its own type parameter, applied to anything but `string` (G374 lifts the second)
- **Breaking.** A program that compiled still compiles, but `json.parse`, `T.parse`, `T.is` and `is` return `Err` or fall through for a stray key, a missing member, or a non-numeric or non-canonical key, where they returned `Ok`; a parity test drives those inputs through 15 paths and 15 spellings of `K` to one verdict each. Exit 0 to 1 for the two new E0304s. Measured against the published 0.1.147 (record 115): `check` and `fmt` identical on all 467 tracked files and the 32 apps; the only build differences are a header comment in the copied runtime `json.ts`
- **What it does not do.** Glyph's checker still leaves keys to `tsc` (G372), a `Record<string, V>` can still stand where `Record<Tier, V>` is declared (G373, 0.1.151), the error names the whole map rather than the key (G375), and `glyph gen dts` writes `Record<string, V>` for a TypeScript `Record<K, V>`, so generated code gets no key check

**0.1.147 — Shipped 2026-11-19 · G362, a check on a type parameter is E0304** (records 102, 108 and 111; breaking)
- **G362.** Inside a generic `fn` or `component`, every check that trusts a descriptor and names one of its type parameters, bare or as a part (`Array<T>`, `Record<string, T>`, `Box<T>`, `Option<T>`, `Nullable<T>`, nested), is E0304 as `json.parse<T>` already was: `is` patterns in `match` arms, nested lambdas and `<case is …>`, `json.parse<T>` in both import spellings, a generic descriptor's type argument (`Box.parse<T>(v)`), and `T.parse(v)`, `T.is(v)` and `T.schema` where the value `T` is the parameter. The parameter is found by scope, so inside `fn h<Doc>` none of these runs a module-level `Doc`'s check any more. A bare `is T` with no type of that name moves from E0300 to E0304, and `json.parse<T>`'s E0304 now names the parameter and its function
- **Breaking.** From exit 0 to 1 under `check --no-tsc` and full `check`, beside a module-level type of the parameter's name: every part form of `is`, `is Doc` in a nested lambda and in a generic component's `<case>`, `json.parse<Doc>`, `json.parse<Array<Doc>>`, `Box.parse<Doc>(v)`, `Doc.parse(v)`, `Doc.is(v)` and `json.parse_with(t, Doc.schema)`; `tsc` accepts all of these, and each ran the module type's check. `is Doc` and `Doc.parse(v)` returned as the parameter were already refused by full `check` (TS2719) and crashed under `run --no-tsc`. Measured against 0.1.146: `check` and `fmt` identical on all 467 tracked files, 324 builds and 2832 emitted files identical, all 32 apps check the same
- **What it does not do.** A type parameter inside a function type (`is Array<fn(T) -> int>`) is not refused, since no check looks inside a function

**0.1.146 — Shipped 2026-11-18 · G351, a value split from a block on the same line is a parse error** (record 104; breaking)
- **G351.** A `{` that would begin a statement on the line where the previous statement ended is E0013, a new parser code. The message quotes that statement and the help gives G217's rewrite: there is no `TypeName { … }` form, so write the literal alone and put the type on the annotation, or drop the braces if they are not meant to be the value. Braces on their own line, a literal after `=` or `return`, and the brace a construct opens are unchanged. E0228 stays for a type's name used as a value on its own
- **Breaking.** `return h() { k: 2 }`, `let o = h() { … }`, a bare `h() { … }`, and the same shapes in a block-bodied match arm or a lambda go from exit 0 to 1 under `check --no-tsc` and full `check`, and `tsc` accepted all of them. `return Order { … }` and `let o = Order { … }` were already exit 1 and now report E0013 instead of E0228 and E0108. `glyph fmt` refuses these files where it used to split them across two lines. The G217 test now pins `[E0013]` and the `type_name_as_value` fixture expects E0013. Measured against 0.1.145: `check` and `fmt` identical on 465 of 466 tracked files, the difference being that fixture; 324 builds and 2832 emitted files identical; all 32 apps check the same
- **What it does not do.** Two other statements on one line with nothing between them (`let a = 1 let b = 2`) still parse, as D1 allows

**0.1.145 — Shipped 2026-11-18 · G357, `is Record<K, V>` and `is Array<X>` check their contents** (records 107, 108 and 110; breaking)
- **G357.** `is Record<K, V>` and `is Array<X>` check every value or element with `V`'s or `X`'s check, at any depth, in element position, inside a generic's argument and in `<case is …>`. A part with no runtime check is E0304 (``cannot validate `Array<H>`: its type argument `H` has no runtime check``, or the record's own words for a record part); a namespaced generic part is E0300 (G356). A container of `unknown` keeps its shape check unchanged
- **G366, found on the way** (record 110). A namespaced generic application such as `w.Box<int>` was looked up by its last path segment, so in a module with its own `Box`, `json.parse<w.Box<int>>`, `json.parse<Array<w.Box<int>>>`, a field typed `w.Box<int>`, and through G357's part checks `is Array<Nullable<w.Box<int>>>`, validated against the local `Box`. Each is now E0304, judged and emitted alike, as is one whose bare name is a different generic from another module. An application whose bare name resolves to the same declaration (`m.Page<m.User>` beside `import model { Page, User }`) is still checked, so no correct program is refused
- **Breaking.** From exit 0 to 1, all accepted by `tsc`: `is Array<H>`, `is Record<string, H>`, `is Record<string, Doc>` with `Doc` unchecked, and the same in element position; `is Array<w.Box<int>>` and `is Record<string, w.Box<int>>` (now E0300); and the G366 shapes above, each of which was exit 0 checking the wrong type. `is Array<T>` with `T` a type parameter is E0304 too, as `json.parse<T>` is. Programs that still compile now fall through on a value that does not match, for example `is Record<string, int>` on `{"a":"x"}`, and `is Array<int>`, `is Array<"x" | "y">`, `is Array<Option<int>>`, `is Array<Nullable<int>>`, `is Array<Array<int>>`, `is Array<fn…>`, `is Record<int, string>`, `is Record<string, Doc>` and `is Array<w.P>` on wrong elements; a container of `unknown` nested inside another (`Record<string, Array<unknown>>`) is checked to that depth. One case reaches statically well-typed values: an `int` computed by division (`3 / 2`) is not an integer at run time (D31), so `is Array<int>` over it now falls through Measured against 0.1.144: `check` and `fmt` identical on all 466 tracked files, 324 builds and 2832 emitted files identical, all 32 apps check the same
- **What it does not do.** A `Record`'s keys are not checked, here or by `json.parse` (G367, 0.1.148). Namespaced generics are refused rather than resolved through the namespace (G354, G356)

**0.1.144 — Shipped 2026-11-18 · G350, an imported generic is judged by its declaring module's recorded walk** (record 103; not breaking)
- **G350.** Every generic record's walk is recorded in the project tables, its type parameters counted as checked, and an application, the sibling hop included, is followed by that walk when a use is judged. A field typed by a record the generic's module keeps private is followed into that record, so `json.parse<Page<User>>`, `Page.parse<User>(v)`, record fields, `Option`, `Array` and `Record` wrappers, the hop, generic-call type arguments and `is Page<User>` accept it when the record checks every field, where 0.1.141 to 0.1.143 refused it. A record with an unchecked field is still E0304, now naming that field rather than the record's privacy, and the hop uses the direct path's words. A type-argument refusal names the generic's field
- **Not breaking.** No program goes from exit 0 to 1. Measured against 0.1.143: `check` and `fmt` identical on all 466 tracked files, 324 builds and 2832 emitted files identical, all 32 apps check the same

**0.1.143 — Shipped 2026-11-18 · G349, an `is T` pattern is judged by the walk `T.is` uses** (records 103 and 107; breaking)
- **The judgment.** `match v { is T => … }` and `<case is T>` narrow on `T`'s descriptor check, and are now refused with E0304 wherever `T.is(v)` or `G.parse<X>(v)` is, in the same words: a record or union however it is named, a nested record, `Array<T>` where `T` has a check, a generic applied or bare, and a type argument with an unchecked payload. `is Pair<Shape>`, bare `is Box` and namespaced patterns whose walk refuses go from E0300 to E0304. The emitted code is unchanged
- **A namespaced generic `is` is refused** (record 107). `is w.Box<X>` in a module that also declares its own `Box` lowered to the local `Box.is` and passed `check` and `tsc`, so the arm ran the wrong check. It is now E0300, as `is w.P` already was; lowering both through the namespace is G356
- **Breaking.** Every newly refused shape goes from exit 0 to 1 under `check --no-tsc` and full `check`, and `tsc` accepts all of them, the namespaced `is w.Box<X>` with a local `Box` included. An imported generic whose field is typed by a record its module keeps private is refused as 0.1.141 refuses `Gp.parse<int>`, until G350. Measured against the published 0.1.142 (record 108): `check` output identical on 465 of 466 tracked files, the one difference being the E0304 note that now names `is` patterns; the refused shapes behave the same against 0.1.141
- **What it does not do.** `is Record<K, V>` tests the shape only, whatever `V` is, and so does `is Array<X>` with an unchecked `X` (G357, 0.1.145). A namespaced `is m.T` has no lowering (G356). The guide's "checked for presence only" paragraphs on `.d.ts` fields are corrected: such a use is E0304

**0.1.142 — Shipped 2026-11-18 · G313, lints run on a file with a type error** (records 88, 89 and 104; breaking for the applyEdit protocol)
- **G313.** The lint tier runs whenever the front end (parse, symbols, imports, resolution) found no error, in the language server, `glyph build` and `check`, and the playground; the emitter still runs only on a file with no error in `build`, `check` and the language server (G322). Every applyEdit verdict gains `stages`, the stages that ran on the candidate, and a file read by two projects reports only the stages that ran in both. E0106's `--explain` text points at the arm an E0200 lists
- **G327.** `glyph fix` names the arm that handles a value an E0220 head would move, from the checker's coverage relation carried in the same build (record 89)
- **Breaking.** `glyph check`, `glyph build`, the editor and the playground report more warnings on a file that has a type error (E0106, E0107, E0108, E0112). No exit code changes. An applyEdit on such a file that adds a lint warning, such as an unused binding, goes from `ok: true` to `ok: false`. Adding `_ =>` to a match whose missing arms' imports are unused is no longer falsely refused: those E0106s are `preexisting`. Measured against the published 0.1.141 (record 106): 27 files gain 32 warnings (23 E0112, 5 E0107, 2 E0106, 2 E0108), all on files that already fail; no diagnostic removed, no exit code changed, and no app directory differs
- **What it does not do.** `return h() { k: 2 }` still splits into a value and an unreachable block and exits 0 with only E0108 (G351); the playground still runs the emitter past a type error (G352); an applyEdit that writes one of an E0200's several missing arms is still refused (G353)

**0.1.141 — Shipped 2026-11-18 · G347 and G348, a generic's own fields and a union's variant payloads are walked** (records 101, 102 and 103; breaking)
- **G347.** A generic record's own fields are judged, type parameters counted as checked, on `json.parse<Page<User>>`, `Page.parse<User>(v)`, a record field of generic type at any depth or inside `Option`, `Nullable`, `Array` or `Record`, and the sibling hop. A field with no runtime check is E0304: ``Page`'s field `h` has type `H`, which has no runtime check``
- **G348.** A tagged union's variant payload fields are walked like a record's on the same paths, `T.is` included: ``field `h` of variant `A` has type `H` ``
- **The emitted code is unchanged.** Only the verdict reads the walk; a record holding such a generic or union emits the same descriptor
- **Breaking.** Every refused shape goes from exit 0 to 1 under `check --no-tsc` and full `check`, and `tsc` accepts all of them: the generic parsed directly, through `.parse`, imported by name, as a record field, inside `Option`, `Array` or `Record`, and through a hop; the union parsed directly, through `.parse`, through `U.is(v)`, through its `schema` (a bare `U.schema`, `json.parse_with(text, U.schema)`, `U.schema.array()`), as a record field, imported, and through a hop; and a record or union passed as a generic's type argument (`Page.parse<U>(v)`), local or namespaced. That includes an imported generic record with a field typed by a record its module keeps private: where that record checks every field this is a false refusal, lifted by G350 in 0.1.144, and the workaround is `pub` on the record; the message says so. Measured against 0.1.140: `check` and `fmt` identical on all 466 tracked files, 324 builds and 2832 emitted files identical, all 32 apps check the same
- **What it does not do.** A `match v { is T => … }` pattern is not judged by this walk, for records, unions or generics (G349, 0.1.143). An imported generic is judged through its module's exports rather than that module's recorded walk (G350, 0.1.144). The first commit's message says the walk covers every path that trusts a descriptor; `match … is` (G349) is not one of them yet

**0.1.140 — Shipped 2026-11-18 · G326, `json.parse` follows a sibling's alias into the module that declares its names** (records 88 and 101; not breaking)
- **The hop.** An alias imported from another module is checked against that module's types: a name its body imports from a third project module, by name or through a namespace, and a generic record applied in it, validate against the declaring module through the hidden-namespace import, across any number of hops, and a hop that leads back into the importer uses its own descriptor. 0.1.133 to 0.1.139 refused these with E0304. Nothing is read in the importer's scope, so a document shaped like a same-named `User` in the importing file comes back `Err`
- **A generic hop judges the generic's own fields first** (record 101). A generic with a field no runtime check covers, such as `h: H` with `H` an `extern_ts` type, is E0304 naming the field, since its descriptor would test that field for presence only. The same generic applied locally or imported by name still gets that presence-only check; that is G347, fixed in 0.1.141 with the same walk
- **What it does not do** (record 102). A tagged union whose variant payload has no runtime check (`A({ h: H })`) is tested for presence only on every path, the hop included, so a hop through such a union now returns `Ok` where 0.1.139 refused it, as a one-hop alias already did. That is G348, fixed in 0.1.141 with G347
- **What stays refused.** A body naming a private type, an interface, a generic union, an `extern_ts` type or a `std` or package type stays E0304 with its own cause, and the message for the last now names it
- **Not breaking.** Nothing accepted before is refused now. Measured against the published 0.1.137: `glyph check` and the emitted TypeScript are identical across the tracked corpus (the differences are 0.1.138's two `fmt` seeds and 0.1.139's two refused negatives) and the 32 apps

**0.1.139 — Shipped 2026-11-18 · G311, the member set of an `Option` and a `Result`** (record 87, Q3; breaking)
- **E0210 on a plain-dot member.** A plain-dot read on an `Option`, or on a `Result` other than `map` and `map_err`, is E0210, and the message names the member set: ``type `Result<Note, string>` has no field `value`: a `Result`'s only members are `map` and `map_err` ``, and for an `Option`, that it has none. The help opens the value with `match`, or for a `Result` binds it with `?` first. `o.tag`, `r.tag` and `r.value` are the runtime encoding (D49), not members, and `--explain E0210` says so. A near miss is suggested only for a called name (`r.mapp(f)` gets `map`); a field read like `r.nmae` means the payload's field and gets none
- **One receiver test for E0236 and E0210.** The alias resolution E0236 used is now one shared function, so a generic alias, a sibling's alias and every import spelling reach the same answer, and a user type named `Option` is not one. The refused read is typed `Unknown`, so nothing cascades from it. What `map` and `map_err` return is unchanged; typing it is the next item on this line, since it turns on error-type checks at every `.map_err(f)?`
- **Breaking.** Under `--no-tsc` every refused shape goes from exit 0 to 1. With `tsc`, the newly refused programs are `o.tag`, `r.tag` and `r.value`, which `tsc` accepted; `o.name`, `o.value`, `r.name` and `r.unwrap()` were already refused by `tsc` and are now caught without it. `.map_err(f)?` and `r.map(f)` are unchanged. Measured against the published 0.1.137: `glyph check` is identical on all 464 tracked files, 324 builds and 2832 emitted files are identical, and all 32 apps check the same
- **What it does not reach** (record 100). A value built with `Some(x)` or `Ok(x)` and never annotated is not typed as an `Option` or a `Result` yet, so `let r = Ok(x)` then `r.value` still passes (G344); an index, a spread, an array element and an untyped lambda parameter are not checked either (G345). E0210's help on an `Option` method points at the payload (G346)
- **D18 restated.** It said `result?.field` is a syntax error; it is E0236, and D18 now names the member set too

**0.1.138 — Shipped 2026-11-18 · `glyph fmt` stops changing what a file says** (records 96, 97 and 99; breaking, narrowly)
- **G335.** Inside braces a line starting with `(` continues the line above, and 0.1.137 printed `let a = 1` then `load()?` / `.run()` as `(load()?).run()`, which means `let a = 1(…).run()`. The formatter no longer invents that `(`: before `(` or `[` it prints `e?` bare, before `?` it prints `e? ?` (two postfix `?`, since `??` always takes a right operand), and at the start of an expression statement a chain with a `?` before `.` or `?.` takes the segment layout, `load()?` then an indented `.run()`. Elsewhere `(e?).f` stays. A parser rule refusing a `(`, `[` or `?` that continues the line above is a separate item, measured against the corpus first
- **G338.** A tagged union's `where` refinement prints on its own line after the last variant, at the variants' indent, so the formatted file still draws E0300; 0.1.137 dropped it and turned a refused program into an accepted one
- **The AST oracle.** `format_idempotent` compares the reparsed AST with the source's, spans erased, not only the text, so a first pass that changes meaning fails there; a corpus test runs the same comparison over the seeds, `examples/` and `catches/`. It fails on the 0.1.137 output of both new seeds
- **Two `?` in a row place their comments.** With `e? ?` spellable, a chain with two adjacent `?` breaks one segment per line like any other, and the 0.1.137 refusal for it is gone
- **Found on the way: G343** (record 99). A statement headed by a lambda call, `fn() { return 1 }()` after `let a = 1`, is printed `(fn() { return 1 })()`, and the second pass glues it onto the line above; present in 0.1.137, and the emitted TypeScript of such a source is already refused by `tsc`. The AST oracle catches it
- **Breaking, narrowly.** No program's meaning changes and `check`, `build` and the emitted TypeScript are the same, but `glyph fmt` reformats a few layouts 0.1.137 left alone, so `fmt --check` goes from exit 0 to 1 on them: `(e?)(x)` and `(e?)[i]` anywhere (now `e?(x)` and `e?[i]`), `(e?)?` anywhere (now `e? ?`, so `((e?)?).f` is `(e? ?).f`), a statement starting with `(e?).f`, and a one-line lambda whose body is such a statement. Each was measured under both binaries. Measured against the published 0.1.137 over the 464 tracked `.glyph` files: `check` identical, 324 builds and 2832 emitted files identical, the 32 apps identical, `fmt` different only on the two new seeds, which 0.1.137 got wrong; none of the 436 files 0.1.137 formats to a fixed point is reformatted

**0.1.137 — Shipped 2026-11-17 · `glyph fmt` refuses rather than moves a comment** (records 86, 95, 96 and 97; breaking)
- **G324, the general half.** A comment belongs to the innermost construct whose gap holds it. Where that construct has no one-per-line layout (an index, a unary or `await` operand, a type annotation or generic argument, a pattern, a JSX tag, a chain's head and links, after a chain segment's first argument list or index, a chain with two adjacent `?`) or D1 allows no line break, `glyph fmt` refuses the file with `file:line:col:` and one fix ("comment inside an index expression has no canonical place; move it above the statement"), leaves it unchanged and exits 1. Checked where comments are flushed: each flush owns a gap with a lower bound, and a comment pending from before it, or still pending when its statement, arm, item or declaration ends, is refused. `format_module` returns a `Result`, handled by the CLI, the LSP (its format request returns an error, never empty edits), `glyph canonical` and `glyph gen`. D14 is restated as the general guarantee. One E0500 is reported per file, at the earliest refused comment; the next appears once that one is moved (G341)
- **Warning E0500, a new `E05xx` formatter range** (record 97). The refusal is in the diagnostics `glyph check`, `check --json`, MCP `glyph_diagnostics` and the editor share, so an agent sees it without running `fmt`. A warning, because a comment's place does not change what the program means and an error would hide the stages after it; `check` exits 0. `glyph/applyEdit` refuses an edit that introduces it. No `glyph fix`: choosing the place is the author's
- **The fuzz property.** `format_idempotent` accepts an `Err` only for input holding a comment, and the same input with every comment blanked must format; the formatter's own output is never refused. A test holds `examples/`, `catches/` and the fuzz seeds to never refusing
- **Breaking**, narrowly (record 97). `glyph fmt` in write mode exits 1 and leaves a file unchanged where 0.1.136 exited 0 and moved its comment; the LSP format request errors where it returned edits; `glyph canonical` and `glyph/canonicalView` fail on such a file where they printed the canonical view (record 98); `applyEdit` refuses an edit that introduces E0500. `fmt --check` prints the refusal line, `refused <file> (left unchanged)` and a `, N refused` count where it printed `would reformat <file>`. Unchanged: `fmt --check` exit codes (0.1.136 already failed such a file, since moving the comment changed it), and `check`, `build` and the emitted TypeScript apart from the new warning. A file 0.1.136 already formatted is never refused. Measured against the published 0.1.136: of the 462 tracked files, only the new negative case is refused, and no other file or any of the 32 apps formats differently; over 1896 comment insertions into the corpus, 1090 format byte for byte as before and 806 are refused, and every refused case inspected was a comment 0.1.136 moved
- **Found on the way: G338**, `glyph fmt` deletes a union's `where` refinement, turning an E0300 program into one that checks; present in 0.1.136, scheduled with G335 in 0.1.138. G339 and G340 file the JSX-attribute and object-pattern gaps the refusal now covers, as candidates for a layout

**0.1.136 — Shipped 2026-11-17 · A member chain holds its comments** (records 86 and 95)
- **G324, the common half.** A comment in a gap of a member/call chain breaks it one segment per line, dot leading and indented, with the comment above its segment: `s` / `.trim()` / `// then count` / `.length`. A segment starts at `.` or `?.` and takes the calls, indexes and `?` after it; its placeable gaps are before the dot, between the dot and the name, before `?`, and before its first `<`, `(` or `[`. Keyed on the gap test G323 uses, so a comment inside an argument list or lambda body stays with it and breaks nothing
- **A chain with a comment the layout cannot place keeps its 0.1.135 layout whole** (record 95). Such a comment sits in the head's own links or after a segment's first argument list, index or type-argument list. Breaking the chain around it put it after a later literal, in a position that read as intended and that no later check could see, so the chain falls back even when another of its comments could be placed. The release review widened the fallback to every comment the layout cannot hold (in the head, an index or a type-argument list) and to two `?` in a row, which the segment layout printed as `??`, an unparseable file (record 96). The continuation-line layout that would place it (`[0]` on a line of its own) was refused, because a line opening with `[`, `(` or `?` continues the line above, the hazard D1 exists to remove; the 0.1.137 refusal is the answer for these comments
- **Not breaking.** No program's meaning changes. Measured against the published 0.1.135 over every tracked `.glyph` file in a directory of its own (461 files: `check` identical, `fmt` identical except the new seed, which is non-canonical on purpose, 322 builds and 2819 emitted files identical) and the 32 apps
- **Found on the way: G335.** `glyph fmt` can print a statement starting with `(` after one that reads it as a call, so the formatted file is another program; present in 0.1.135. Scheduled for 0.1.138 (record 96)

**0.1.135 — Shipped 2026-11-17 · JSX text is scanned as text** (records 85 and 93; breaking)
- **A JSX-text scanner the parser drives**, after the model of TypeScript's `reScanJsxToken`. The parser asks for a text run after an opening tag, a child element or an `{expr}`, and the lexer reads raw source up to the next `{` or `<`, newlines included. One text child per run across lines, emitted by `jsx_text_value`'s rule. Comments are a by-product of parsing (`parse_with_comments`), so text can never yield one, and `glyph_lexer::comments` left the public API so a missed caller is a compile error
- **The D6 rules that come with it** (record 93). `'`, `"`, `\`, `/*`, `;`, `#`, a backtick and a lone `&` are text (G317). Four things are refused as `E0012`, each message naming what it refused: `}` and `>`, with help to write `{"}"}` (G319); `//`, stricter than TSX, because 0.1.134 read it as a comment and TSX reads it as text (G318); and an HTML entity, `&` then letters, digits, `_` or `#` then `;`, which TSX decodes and Glyph would emit as written, with a help that names the character the entity stands for and a spelling that compiles: the character itself, a string child for `<`, `>`, `{` and `}`, and a `\u{…}` string child for an invisible one or a numeric entity (record 94). A comment among children is a comment-only child (`{`, `// note` on its own line, `}`), which emits nothing; `{}` stays refused. A `>=` glued to a tag's `>` is split where text follows, so `<p>=x</p>` is the text `=x`; after the outermost element's closing `>` it is still E0002. G320 and G321 close with the scanner. D6 and D14 are amended
- **G330, a non-ASCII character in code crashed `glyph check`** (exit 101). The lexer reported the first byte of a character it does not accept as a `char` of its own (`é` as `Ã`) with a one-byte span that ended inside the character, and the diagnostic renderer panicked slicing there. The whole character is reported now. Found by the release review (record 94), present in 0.1.134 too
- **The fuzzer checks text spans.** The `parse` target asserts that a JSX text child is exactly the source at its span, and that `tokenize` over the source with every text run blanked fails where the parser's lexer does; the G318 to G321 reproductions and two entity cases are seeds, which the formatter tests also read. Measured against the published 0.1.134 over every tracked `.glyph` file in a directory of its own (460 files, 2837 emitted files) and the 32 apps (763 emitted files): no difference on any file that predates the release
- **Breaking**, established by running the published 0.1.134 and this build on the same programs. Exit 0 to 1 under `check --no-tsc`: `}` or `>` in text, `//` in text with the closing tag on a later line, and `//` inside a `"` pair in text (`see "http://x.com" now`), all E0012. Exit 1 to 0: `'`, an unpaired `"`, `\`, `/*`, `;`, `#` or a backtick in text (E0001 before), and `=` right after a tag's `>` (E0002 before, E0003 in a fragment). Exit 101 to 0: any non-ASCII character in JSX text (`é`, `’`, `—`, `日本`), which panicked the diagnostic renderer. An entity was E0001 and is E0012. Programs that compile in both and now emit differently: a whitespace-only run on one line among children (`{first} {last}`, `<b>x</b> <i>y</i>`, `{a} </p>`, `<p> </p>`) is a child holding that whitespace, where it was dropped, so `{first} {last}` renders `John Doe` where it rendered `JohnDoe`; text over several lines in a module-level `const` is one child `"a b"` where it was `"a", "b"`; and a `"` pair across tags gives the elements it spells, where it merged them into one text child. None of these shapes is in the corpus, which is why the sweep above found no difference; the release review found the whitespace class (record 94)

**0.1.134 — Shipped 2026-11-11 · A cursor that lexes on demand** (records 85 and 91)
- **The parser pulls tokens from a resumable lexer** (`Lexer::at(src, offset, state)`, `next_token` public, `tokenize` the same loop), and a lex error is held until the parser reaches it, so lookahead that wanders into a lex error backs off without failing. `peek` takes `&mut self`. Programs that compile are unchanged: emitted TypeScript and `fmt` output are byte-identical against 0.1.133 over every tracked `.glyph` file in a directory of its own (318 of 318 clean files, 2795 emitted files), and the 32 apps check identically and build as projects to identical output (616 emitted files); no exit code changes. The JSX-text scanner in 0.1.135 is built on it
- **Changed: diagnostic ordering** (record 91). The parser reports the first error it reaches. A parse error before the first lex error is now the one reported, where the eager lexer always reported the lex error: `x = 1 # 2` in a body goes from E0001 (`#`) to E0008 (`mut`). A lex error is reported only when the parser arrives at it, with the span and message `tokenize` gives; one met during lookahead is not reported, and the reading the lookahead backs off to can fail on valid code: `let x = f<a, "\q">(x)` reports E0003 at the `,` where 0.1.133 reported the `\q` (record 92), pinned by a parser test and filed as G329. Measured: 1 of 449 corpus files changed, the `unterminated_string` fixture, which held a second error (a body-level `const`, G328) and is corrected to `let`; `parse_error_before_lex_error` pins the rule, and the `parse` fuzz target asserts that the reported error never starts after `tokenize`'s

**0.1.133 — Shipped 2026-11-11 · What 0.1.132's decisions deferred** (decided in Linus records 79 to 81, 84 to 89; split by risk)
- **Type parameters on a function value.** `Ty::Fn` carries its type-parameter list, so `let f = id` keeps it, `f<number>("x")` binds it as G292 does for a declaration, and written type arguments on a function value with none are E0233. About ninety construction sites, so it ships on its own
- **G312, typed methods on arrays and strings, and the uncalled read.** A method table generated from TypeScript's `lib.d.ts`, with overloads, `this` types, predicates and rest parameters left `Unknown` and listed; CI regenerates it and fails on drift. A method read without being called (`let f = s.toUpperCase`) is E0234, because it throws at run time and `tsc` accepts it. A called method is never refused
- **`json.parse<T>` never casts** (records 88, 89). E0304 for a `T` with no runtime check, the note naming the cause: a type parameter, an `extern_ts` or package type, an interface, `Result`, a generic tagged union, an `Option`, `Nullable` or `Record` of one, or a generic record applied to one (`Pair<Shape>`, which the release review caught testing `a` for presence only; `Pair.parse<Shape>` is E0304 and `is Pair<Shape>` E0300 for the same reason, record 90). A container of `unknown` is checked by its shape. An alias from another module validates when its body names only types that module declares and exports, and is E0304 otherwise (G326, 0.1.140); its names are never read in the importer's scope, where they validated against the wrong record. A `json.parse` value takes no type arguments (E0233), and a call with none is emitted as `json.parse<unknown>`, so `tsc` refuses the cast `let a: Result<User, …> = json.parse(body)` that passed 0.1.132 and crashed at run time; Glyph's own checker refuses it in 0.1.158 (G325)
- **G311, `?.` on an `Option` or a `Result` is E0236** (record 87, Q1 and Q2). An `Option` is never null, so `?.` reads off the `Option` itself; on a `Result`, `?.` is optional chaining and does not propagate an `Err` (`bind_agg(...)?.label` in csvql read that way). One E0236 at the link where the chain starts, method calls included (`r?.map(f)`); the help points to `match`, to binding with `?` first, or to dropping the `?` before a method. Breaking: `tsc` accepts `o?.tag`, `r?.tag`, `r?.value`, `r?.map(..)` and `r?.map_err(..)`. The plain-dot member set is 0.1.139
- **`glyph fix` applies a literal-arm suggestion** under the rule it uses for variant heads: only when the E0200 of the same match lists it as missing, and never when two arms would take the same value (record 88). A decline no longer names a lowercase arm as the catch-all, since it may be a variant (G327, 0.1.142)
- **G310, `glyph/applyEdit` over the whole project**, with an atomic multi-document `changes` form: the delta rule from 0.1.132 applied to every module, with each other file's new and pre-existing diagnostics returned under `elsewhere`. A diagnostic's identity is its code, severity, range and subject, the message without its suggestion, so an edit that rewrites another file's "did you mean" is not refused for it (record 88)
- **G316, landed on main: `glyph fmt` keeps a run of edge spaces in JSX text** (record 84). In a child list holding an element, a text with a space at either end is written whole as one string child (`{"(    "}`, `{" and "}`), so formatting leaves the emitted TypeScript unchanged byte for byte. It used to split the space off as one `{" "}` child, which cut a run of spaces to one and added a child. Not breaking: every file the earlier formatter left alone is still a fixed point. The nightly fuzzer had failed on it for fifteen days; its stream now counts JSX text as the text it renders, and a fourth target, `format_preserves_emit`, asserts emitted-TypeScript equality on fuzzed input
- **G322, landed on main: a `match` with no arms no longer panics the emitter.** The playground emits past a type error, so `match x { }` reached a first-arm index and killed the wasm module; it is E0300 now. Found by the first `format_preserves_emit` run
- **G323, landed on main: a comment between the operands of an operator expression stays with its operand.** `glyph fmt` put it below the statement; a comment in a gap between operands now breaks the expression one operand per line, operator leading, with the comment above its operand, and D14 names operands among the constructs it covers. A comment inside an operand (an argument list, a lambda body) breaks nothing, which keeps every 0.1.132 fixed point one: checked over the repo's 429 parseable `.glyph` files formatted by the published 0.1.132 (record 86). Found by `format_idempotent` in fork mode
- **The nightly keeps fuzzing past a crash.** `-fork=2 -ignore_crashes=1` in `fuzz.yml`, so a target with a known failure still runs its full time and saves every crash it finds. The stronger properties found G318 and G319 in their first ten minutes; the nightly stays red on both until 0.1.135, and their seeds land with that fix, because the formatter tests read `fuzz/seeds/` too (record 85)
- **Breaking**, established by running the published 0.1.132 and this build on the same programs. Under `glyph check`, exit 0 to 1 although `tsc` accepts them: a built-in method read without a call, `.bind` included (E0234); type arguments on a `json.parse` value (E0233); `o?.tag`, `r?.tag`, `r?.value`, `r?.map(..)`, `r?.map_err(..)` (E0236); `json.parse<T>` for a type parameter, an interface, `Result`, a generic union, an `Option` of an interface, or a sibling alias naming an imported type (E0304). Under full `check` only: the bare `json.parse` cast into a declared `Result<T, …>`, a generic wrapper included (TS2322/TS2345). Under `--no-tsc` only: type arguments on a function value with none, and `o?.name`. At run time `json.parse<Array<unknown>>("{}")` and `json.parse<Record<string, unknown>>("[1]")` go from `Ok` to `Err`; a record with an `Array<unknown>` field passed to `parse` goes from refused to accepted. Also exit 0 to 1 under `check`: `json.parse<Pair<Shape>>`, `Pair.parse<Shape>(v)` (E0304), `is Pair<Shape>` (E0300), and a method result against a record it does not match, `xs.slice(0)` where `{ length: number }` is declared (E0204, method results being typed). Under `--no-tsc` only: type arguments on an interface member or a function-typed field, and `s.trim()` returned as a literal union. The list is what testing found, not every program that could change. Across the examples, apps, catches and corpora (156 files and 32 apps, each mode), only the two new negative cases change exit code

**0.1.130 — Shipped 2026-11-06 · The announcement, and the checks it leans on**
- **G210: E0304 holds across an import.** `Doc.parse(v)` from a module that imports `Doc` is refused when the declaring module would refuse it, read from that module's verdict by a second pass over the project tables, including the records a field check delegates to. The announcement's E0304 catch rests on it
- **G289: a prelude container against a declared record or union is E0204.** `let user: User = json.parse(body)` is refused by Glyph's own checker, so `glyph check --no-tsc`, the editor and the playground see it; `json.parse` is in the checker's stdlib table, with explicit type arguments on stdlib calls. `catches/unvalidated-boundary-read` is an E0204 case, and the announcement, the preview notes and the hardening page say so
- **G288: a member read on an array or a string is checked against TypeScript's member lists.** `xs.lenght` is E0210 with "did you mean `length`". The lists are read from the pinned TypeScript, and `check_builtin_members.py` asks it again on every CI run, in both directions
- **G291: a literal match over a type the checker cannot see is E0232**, which says the declaration is not visible and what would make it so, where E0218 offered an `else` first
- **G285: a `tsc` error lands on the expression.** The emitter maps every expression it writes, so a wrong argument is underlined at the argument rather than across the `let`, and the `.ts.map` is expression-level. The emitted TypeScript is unchanged byte for byte
- **G286: a reserved word used as a name says it is reserved**, still E0002; `glyph llms --json` has a `keywords` section, and AGENTS.md lists the 32 words, kept in step by `check_llms_sync.py`
- **The playground opens on a refusal.** The picker is generated from `catches/` with each case's code, the page opens on the non-exhaustive match, and a program can be shared by URL fragment. All five catches are refused there, which a `glyph-wasm` test holds
- **The docs gate takes `tsc` codes and npm packages.** `expect-error TSnnnn` fences are compiled through `tsc`, and `needs-npm` fences compile against zod, pg and mongodb installed from a committed lockfile. The `needs-deps` fences in AGENTS.md are whole modules now, the leftpad example with its `.d.ts` shown and used (G248)
- **E0400 to E0404 have pages**, in `glyph --explain` and `docs/error-codes.md`, and a test requires every code in the compiler's sources to be catalogued. `check_scaffold_docs.py` runs `glyph init` and `glyph run` rather than reading a string out of a file. G280's `run` and `@doc @run` paths have npm-import tests of their own
- **`glyph fix` stops calling an array a record** when it declines an E0210 whose alternatives are an array's members
- **Still the owner's:** how the launch drafts describe the way the work was done; the `Cancelled` measurement table, whose Glyph column holds predicted results; and whether `web/versions` stays hand-written with a check or becomes a paragraph and a link
- **The measurement.** The suite is 1980 tests, from 1926. The gap ledger is 301 entries: 258 fixed, 7 partly fixed, 11 decided or resolved and 25 open, with six closed here (G210, G285, G286, G288, G289, G248) plus G291, and ten opened (G292 to G301), each reproduced on this build
- **What it does not do.** Explicit type arguments on a user generic are still ignored (G292); `json.parse<T>` is not refused for a `T` it cannot validate, where `T.parse` is (G297); the editor does not show the emitter's errors, so E0304 across an import appears in `glyph check` and MCP and not in the editor (G295); and a record field named `type` can be declared and not read (G300)
- **Breaking**, established by running the published 0.1.129 and this build on the same programs: `Bag.parse(v)` from a module importing a record with an `Array<unknown>` field exits 0 under `check` in 0.1.129 and 1 here (G210); `let s: Sized = xs` with `type Sized = { length: number }` and `xs: Array<string>` exits 0 under `check` in 0.1.129, with and without `tsc`, and 1 here (G289): a prelude container is now another declaration where a record or union is declared (D49), and `tsc` still accepts this program. `Sized.parse(xs)` already returned an `Err`, so the checker now gives the answer the validator gave. To keep the program, declare `interface Sized { length: number }` (D34), which has no validator and is satisfied by any value with a `length`. G288's refusals are of names TypeScript also lacks, so a build that ran `tsc` was already red; under `--no-tsc` they exit 0 in 0.1.129 and 1 here

**0.1.129 — Shipped 2026-11-02 · Toward a preview announcement**
- **The readiness pass**, decided 2026-10-11 by the owner. Two agents played a React and a Node developer from `npx @glyphlang/glyph init` with only the shipped docs, and both reached a clean `tsc` build; what they stumbled on is below. `docs/launch/` was rewritten from the compiler as it ships, in the first person singular by the owner's choice. The release review decided three launch questions at the owner's request: `glyph fix` and E0200, which `catches/` survive, and the playground
- **G280, a failing `@example` passed in a module that imports an npm package.** The gate built a copy of the project in the temp directory, where `node_modules` was unreachable, so the harness died loading the package before running anything, and the runner read the missing `FAIL` line as a pass on every channel (`build`, `check`, `check --json`, `run`'s pre-run pass, `@doc @run`). The copy resolves packages from the project now, and a harness whose closing tally does not account for every test it was given fails as "N example(s) not run" (E0404 in `--json`). Found by the cold-start Node run, which broke its example on purpose
- **G275 and G279: `<case>` takes the pattern a `match` arm takes, and `bind` is gone.** Decided by the owner on the release review's recommendation, and the original design (`archive/SESSION_1.md`). `<case Loaded(items)>` binds a positional payload, `<case Loaded({ users, total })>` a record's fields with renames and D44 nesting, `<case Loaded(r)>` the whole record. A `<match>` is checked and lowered as the value-level `match` it is, so case bindings are typed and a refutable case alone does not cover its variant (E0200). `bind={x}` is E0231, whose help writes the pattern for the payload's shape. In arms and cases alike, a record pattern naming a field the payload lacks is E0210 listing the record's fields, and an object pattern on a payload that is not a record is E0204. D6 is amended
- **G278, a U+E000 in a string compiled to `$`.** The lexer marked an escaped `\$` with that codepoint alone. The marker now comes in a pair with the character it escapes, and `\u{E000}` is accepted. The formatter copies a string in a type as written, where it reprinted the decoded value and turned `"a\$b"` into `"a$b"`. Found by the scheduled fuzz run, red since 2026-10-28
- **`glyph fix` stops writing arms for E0200.** The arms it wrote printed the case and exited the process, after which the program passed `check`, `tsc` and `build` and `glyph query variants` called the match exhaustive. Each E0200 is declined now with the missing cases named; `glyph check --agent` still gives `missing_variants` and each variant's construction syntax. The E0200 help stops suggesting an `else` arm, which the agent constraints for the same error forbid. `glyph fix --help` lists the three rules that remain, and `glyph mcp --help` lists all eleven tools, held to the server's catalogue by a test
- **`catches/` is five cases, each one program written twice.** The review found four that compared two different programs or that `tsc` also catches, and they were dropped (each one's Glyph behaviour stays under a compiler test). `non-exhaustive-match` is a `void` handler with no cast, `unvalidated-boundary-read` is the faithful `let user: User = json.parse(body)`, refused by `tsc` until G289, and `unverifiable-parse` holds a real `net.Socket` on both sides, with `@types/node` installed by the gate. There is no greppability case, and the gate says so
- **The cold-start documentation gaps.** AGENTS.md shows a whole browser page, two modules, an `index.html` and build then bundle, with both fences compiled together against React (G281). Every inline `std` import in AGENTS.md is compiled by a new CI gate, which fixed an HTTP recipe that imported a `serve` `std/http` does not have (G283). The scaffold's `.types/README.md` no longer sends a newcomer to write `.d.ts` files for typed packages (G284). The getting-started guide opens with `glyph init`, checked by `check_scaffold_docs.py`
- **G282: `glyph run -- args` runs the current project with those arguments**, and the scaffold's `start` is `glyph run --`, so `npm start -- args` reaches `main(argv)`. A project scaffolded earlier needs its `start` script changed
- **G287: `glyph bench` fails when its harness dies** or a benchmark throws, through the check G280 added, where it reported zero benchmarks and exited 0
- **Tests that needed `tsc`, `tsx` or `node` fail under CI instead of passing** when the tool is missing; one helper decides, and a developer machine still skips with a message
- **G290: `glyph gen dts` keeps the caller's `NODE_PATH`.** It replaced it with `npm root -g`, so a TypeScript reachable only through `NODE_PATH` was reported missing. CI's is, and ten `gen dts` tests had been skipping there and counted as passed; the rule above failed them on its first CI run
- **G248, half.** A `needs-react` fence is compiled against `@types/react`; the AGENTS.md component fences and the browser page are covered
- **The todo app saves which items are done**, and the render gate checks a toggled item stays done across a reload
- **Breaking**, established by running the published 0.1.128 and this build on the same programs: a `<case Loaded bind={items}>`, and an arm `Loaded({ totl })` on a record with no `totl`, exit 0 under `check --no-tsc` in 0.1.128 and 1 here; the G280 module, with `date-fns` installed and a failing example, exits 0 under `build` in 0.1.128 and 1 here; a bench module that throws on load exits 0 there and 1 here. `glyph run --no-tsc -- src/other.glyph` ran that file and now passes it as an argument
- **What it does not do.** Values from npm are still typed by `tsc` alone (G257), a `let` annotation against `json.parse`'s result is still left to `tsc` (G289), and a member read on an array is too (G288). The announcement is drafted and has not been posted
- **The measurement.** The suite is 1926 tests, from 1906; the exact-or-absent corpus is 41 invariants, unchanged; the gap ledger is 290 entries, 251 fixed, 8 partly fixed and 20 open

**0.1.128 — Shipped 2026-11-01 · React against real React**
- **The measure.** `examples/apps/todo`, a counter and a todo list on React 18, passes `glyph check` with `tsc` against `@types/react`, bundles with Vite, and renders in a headless browser (Chromium through Playwright): the counter counts, adding a todo clears the input, toggling one marks it done, the "N left" line and the title effect track what is left, each item's text survives a reload through `localStorage` (whether it was done is not saved), and the page logs no error or warning. `scripts/check_todo_renders.py` asserts all of it and CI runs it. The first browser run was by hand, and the release review found the committed source showing "2left": the space had been lost to G276 between that run and the commit, and nothing read the page's text. The gate reads it, and fails four ways with the space removed. The 0.1.126 round wrote the same program and stopped at `document`
- **G254, three host modules.** `std/dom` (`title`, `set_title`, `element_by_id` as an `Option<Element>`, `location` as a `url.Url`), `std/storage` (`get` as an `Option`, `set` as a `Result` whose error `kind` is `"quota" | "unavailable"`, `remove`) and `std/console` (`log`, `warn`, `error`), as the plan designed them. `Element` is opaque in the checker and `HTMLElement` in the TypeScript, so `createRoot(root)` type-checks and `{}` is refused where an `Element` is declared. Under node, importing `std/dom` is safe and a call throws naming the function; storage never throws. None of the three imports a `node:` module, which a test enforces. E0103 on `document`, `window`, `localStorage`, `console`, `setTimeout` and the other host globals names the module that wraps it. `docs/reference/stdlib.md` gains a table of which host each module runs on
- **G255, D48 `let [a, b] = expr`.** A `let` destructures a fixed-length array with names, `_`, nested patterns and a trailing `...rest`; a pattern element that can fail is E0230 at parse, and so are `let owned [..]`, an annotated pattern and `const [..]`. The length is proved: a written literal is counted and its elements typed one by one, an `Array<T>` is E0230 naming the `match` that handles a short one, and anything not an array is E0230. A value only `tsc` can see, such as `useState(0)`, is emitted with `satisfies readonly [unknown, unknown]`, which `tsc` refuses for anything but a pair and which leaves its element inference intact (`let n: string = count` is TS2322); every destructuring `let` carries it, so `tsc` and the checker agree per element on a literal too. Each name is a full `let` binding for references, rename, hover and the unused-binding lint. The new catch `catches/split-into-a-pair` is `const [user, host] = s.split("@")`, which `tsc --strict` accepts
- **G256.** An import of a `std/` module that does not exist is E0104 naming the nearest real one, where it passed every Glyph stage and failed only at `tsc` with TS2307. `std/schema`, documented and never seeded, is seeded so its documented import stays legal
- **G258.** AGENTS.md shows `<for>` and `<match>`, in a program checked with `tsc --strict` against React 18, and gains sections on D48 and the three host modules, including that an effect is written `fn() -> void`
- **G271.** `examples/03_react_component.glyph` uses React's own `useState`, `useEffect` and `useMemo`. The CI stub it builds against now has React's hook shapes, and `scripts/check_react_example.py` checks the example against the real `@types/react` installed for the todo app, in CI
- **G276 and G277, two JSX whitespace bugs found on the way.** `glyph fmt` trimmed every JSX text child, so `<p>{n} left</p>` became `<p>{n}left</p>` and the page read "2left"; the formatter now prints each text child as the text it renders, a rule shared with the emitter in `glyph-ast`, and writes a significant edge space on a line of its own as `{" "}`. The parser kept the indentation of a text run on its own line below a child, `"    and"` where JSX gives `"and"`; no file in `examples/` changes its emitted TypeScript
- **G275, recorded and scheduled.** `<case V bind={x}>` lowers to `const x = __v.x`, which binds `undefined` on a positional payload, and the checker gives the name no type. AGENTS.md teaches the record-payload form that works
- **The measurement.** The suite is 1906 tests, from 1855; the exact-or-absent corpus is 41 invariants, unchanged; the gap ledger is 277 entries, 241 fixed and 18 open
- **Breaking**, in the checking direction, established by running the published 0.1.127 and this build on the same programs. An import of a `std/` module that does not exist exits 0 under 0.1.127 with `--no-tsc` and 1 here (E0104); with `tsc` it fails under both, with TS2307 there. Across the 31 apps that existed in 0.1.127, with and without `tsc`, every exit code and diagnostic-code list is identical under both binaries; `examples/` as a whole and the new todo app differ only because 0.1.127 cannot parse D48 (E0002) or find `std/dom` (E0105)
- **What it does not do.** Glyph's checker still does not type a value that comes from React (G257), so a wrong argument to a state setter is a `tsc` error in the build, not a Glyph one in the editor. `std/dom` has no element setters or event listeners, because React owns the DOM it renders. Object destructuring in `let` is E0005 until it is designed

**0.1.127 — Shipped 2026-10-30 · Records compared the way `tsc` compares them, and impact decides a component**
- **G270, a nested record field is compared through the whole relation.** The structural record arm compared each field with the free `definitely_incompatible`, which has no declaration in hand, so `tx(v)` with `v = { o: { b: "x" } }` passed where `{ o: Other }` was declared, and `glyph_impact` reported a `WILL_FAIL` there that no diagnostic followed (live since G228). Both are E0211 now, as `tsc` refuses them (TS2345)
- **G269, the weak-type rule.** A record whose fields are all optional refuses a value sharing none of them, mirroring TS2559 row by row: an empty record is not weak, a value with no fields is accepted, a function value is refused, and the diagnostic says why (`it has no field in common with ..., whose fields are all optional`)
- **G274, four more positions the same hole reached**, found checking G270's premise: a field the value holds as optional where the declaration requires it, a declared record passed where a structural one is declared, a function whose return does not fit, and an interface member. Each passed `--no-tsc` under 0.1.126 and is TS2345 under `tsc`; each is E0211 now. The same change removed a false refusal: a record that names itself through a member (`{ kids: Array<Node> }`) against an interface that does the same was refused by name one level down, on a program `tsc` accepts. The relation assumes an interface pairing it is already deciding holds, as `tsc` does. A function's parameters are still compared by nothing
- **The accepting face of the record rule.** With the two refusals above, the record arm is total, so `glyph_assignable` and `glyph_impact` answer `COMPATIBLE` or `WILL_FAIL` for a record against a declared record (local, imported, generic or a second name) where they answered "no rule". The reason names the width-subtyping divergence when the value has a field the declaration does not, which `tsc` refuses on a fresh literal (TS2353) and Glyph does not; that is G273, recorded and open
- **G251, impact decides a JSX element.** Under `change_signature_type` an element is the call it lowers to, `createElement(Button, { label: "go" })`, so its props record goes through the cell a call argument written as that record gets, and each attribute is listed against the field the checker compares it with. Under `change_arity` the verdict follows what the checker does after the one edit that keeps the component legal, measured under all three import spellings
- **G268, one pairing, one verdict.** An attribute and a call argument go through one function now, so `number` against `unknown` is the same answer at `take(n)` and at `<Box x={n} />`. `unknown` has its own cell: `WILL_FAIL` for a `string`, `number` or `bool`, whose rules decide every replacement the checker reads, and `UNDETERMINED` with the reason for anything else
- **A type parameter bound from a literal.** `array.fold(xs, "", fn(out: string, ...) -> string { ... })` bound the accumulator to the literal `""` and kept it, which the function-return comparison then refused on two apps `tsc` accepts. A later `string` candidate supersedes an earlier literal one, which is what `tsc` infers from the two
- The 0.1.126 ledger entries G246, G247, G249, G250 and G253 are marked fixed; they shipped without the marker
- **The measurement.** The suite is 1855 tests, from 1844; the exact-or-absent corpus is 41 invariants, from 38; the gap ledger is 274 entries, 234 fixed and 22 open
- **Breaking**, in the checking direction, established by running the published 0.1.126 and this build on the same programs. Six programs exit 0 under 0.1.126 with `--no-tsc` and 1 here, each refused by `tsc --strict` as TS2345 or TS2559: a nested record field of the wrong type, a value sharing no field with an all-optional record, an optional field where a required one is declared, a declared record passed where a structural one is declared, a function whose return does not fit, and an interface member of the wrong type. A self-referencing record against a self-referencing interface goes the other way: E0211 under 0.1.126, accepted here as `tsc` accepts it. Across the 31 apps and `examples/`, with and without `tsc`, every exit code and diagnostic-code list is identical under both binaries
- **Toward a preview announcement**, decided 2026-10-11 by the owner, is not in this release. It is a readiness pass rather than a feature and follows 0.1.128: `docs/launch/` rewritten from the compiler as it is then, outside-user agents running the getting-started path cold, and a Linus review of the announcement itself. The announcement says what Glyph catches today, shown with the `catches/` programs verified both ways, and says that the productivity claim has not been measured

**0.1.126 — Shipped 2026-10-17 · Components emit, check and navigate the same way under every spelling**
- G249: go-to-definition on a namespace member answers the declaration the named import answers, in the editor and in `glyph_definition`
- G247: the prelude `Component` lowers to `React.ReactNode`, and an unannotated component returns it rather than `void`
- G253: a missing required `children` explains that children written between the tags are not part of the props object
- G246, decided 2026-10-11 by the owner: E0229 for an `unknown` or unbounded generic component parameter
- G250: an import line is `SAFE` under an edit that keeps the name, and a namespace-qualified call is a call site under every spelling
- The React answer page states what holds for real React and what does not yet
- **The measurement.** The suite is 1844 tests, from 1820; the exact-or-absent corpus is 38 invariants, from 36; the gap ledger is 272 entries, 224 fixed and 30 open, the 19 new ones from the round that built a React app against real React 18 and from the three agents' own findings
- **Breaking**, in the checking direction, established by running the published 0.1.125 and this build on the same programs. A component whose parameter is `unknown` exits 0 under 0.1.125 and 1 here (E0229), with attributes and without; an unbounded type parameter the same; `?` in a component body exits 0 under 0.1.125, `tsc` included, and handed React the `Err` object to render, and is E0201 here. A program writing the prelude `-> Component` goes the other way: `TS2304` twice under 0.1.125, `tsc --strict` passed here. Across the 31 apps and `examples/`, with and without `tsc`, the only verdict that changes is `examples/` with `tsc`, from failing on `TS2304 Cannot find name 'Component'` to passing

**0.1.125 — Shipped 2026-10-11 · A component is checked, emitted and related under every spelling**
- **What the release is.** Four holes in how Glyph handles a React component, three of which shipped green under 0.1.124 with `tsc` in the loop and were broken at run time: a component reached through `import ui` emitted as a string tag and never ran, an element with no attributes passed its required props as `null`, and an unexported component reached through a namespace was accepted. Each now draws a Glyph diagnostic, and the relations an agent reads (references, dependencies, impact, rename) see a component element under every import spelling
- **G242, a dotted element is a component.** `JsxKind::classify` keyed an element on its first character, so `<ui.Button />` under `import ui` emitted `createElement("ui.Button", ...)`: React rendered an unknown tag, `tsc` accepted a string tag with any props, and the resolver reported `ui` unused. A dotted name is a component whatever the case of its base, which is TypeScript's own JSX rule, and one classifier in `glyph-ast` replaces the copies the resolver and the emitter each kept. The base resolves like any name (an unbound one is E0103), the attribute check reaches the component through a namespace or aliased import, and the element emits `createElement(ui.Button, ...)`. Rendered with `renderToStaticMarkup`, 0.1.124 gives `<u.Button variant="danger" label="go"></u.Button>` and this build `<button>go</button>`
- **G243, E0229.** `component Greeting(name: string)` could never be rendered, because the emitter passes one props object, and nothing said so until `tsc` reported TS2769 at the element. A component's one parameter is its props record now, refused at the parameter for a primitive, a string-literal union, a tagged union, an `Array`, a `Nullable` and a function type, each established as TS2769 under `tsc`. A record, an alias of one, an imported record and an `interface` are props records; `unknown`, a bare generic and `Record<K, V>` draw nothing, because the checker cannot prove they are not (G246 asks whether that is the right reading). Calling a component directly, `Greeting("x")`, is refused with the declaration even though it runs: a `component` is rendered by React, calling one bypasses React and its hooks, and a function called as a function is an `fn`. The bootstrap in `AGENTS.md` taught the refused spelling inside a `needs-deps` fence no gate compiles; it is corrected, and the gate is G248
- **The missing fields, named.** A record refused for a missing required field says which: ``type mismatch: expected `User`, found `record`: missing field `age` ``, with the names in a `missing_fields` list on `--json`, `--agent` and the MCP diagnostics, and an explicit `null` on every other diagnostic. The name is `missing_fields` rather than `missing` because `missing_variants` already sits beside it. "Required" matches `tsc` exactly: a `?` field is optional, and an `Option` or `Nullable` field is required, as `tsc` reports it
- **A component element missing a required prop is E0211**, at the element's name, with `missing_fields`. That includes an element with no attributes at all, which emitted `createElement(Card, null)`: React's types accept `null` props for any component, so `tsc --strict` passed `<Card />` against required props and the component read `undefined` at render. A component element with no attributes emits `{}` now, so `tsc` refuses it too; intrinsics and fragments keep `null`. Every refused shape was established by running `tsc --strict` on the TypeScript the emitter writes for it, which reported TS2769 with the property names the list carries; the suite records that `tsc` message beside each case but runs `--no-tsc`, so the agreement is not re-checked on every build (0.1.126) Required `children` written as children is refused, because the emitter passes children outside the props object, which is what `tsc` says too. An element with a spread is not asked
- **G244, every relation sees an element at its name.** Once `<ui.Button>` was a component, `glyph query references` still listed the use under `import ui { Button }` and under `ui.Button` as a value, and nothing under `<ui.Button />`, so the answer changed with import spelling and `glyph_impact` missed the site. The named spelling was wrong too: it recorded the whole element, so the range covered attributes and children and a rename wrote the new name over all of it, and the closing tag was never recorded. Every element is recorded at its name segment now, opening and closing tag alike, and the member goes through the same export check `ui.X` does, so `<ui.Nope />` and an unexported `<ui.Secret />` are E0105. Impact counts one site per element; references and dependencies list both tags, which is what rename needs. Hover on the `Button` segment of `<ui.Button>` answers the component's type where it answered `null`
- **Two false sentences in impact, corrected without changing a verdict.** An element site under `change_signature_type` or `change_arity` said Glyph does not compare a function value against its use context, which has been false for an element since G240, and an import binding was described as a value read. Each has its own reason now, and a test pins the element, value and import reasons as three different strings. Deciding the element site is G251; the import line's verdict is G250
- **G252, two concurrent checks could fail each other.** `glyph check` staged under a shared temp parent and removed it on the way out, so a concurrent check lost the parent between choosing its path and creating it. 2,000 checks 64 at a time gave 3 failures with `io error reading .../glyph-check-scratch/<pid>-0/: Invalid argument`; each check owns its own top-level scratch directory now, and 4,000 runs gave none. It surfaced as a one-off suite failure while another worktree's suite ran beside it, which is the shape an editor, a CI matrix or a parallel agent produces
- **Publishing moves to npm trusted publishing.** Each of the six packages trusts this repository's `release.yml` on npmjs.com, and the publish job exchanges the run's OIDC token for a short-lived credential per package, with npm 11.21.0 installed on the runner because Node 22 ships npm 10. `NPM_TOKEN`, `npm whoami` and `NODE_AUTH_TOKEN` are gone from the job. The `npm whoami` gate proved authentication before the first irreversible publish, and its replacement proves more: a dry-run performs the same per-package OIDC exchange a publish does, so each of the six dry-runs runs at verbose level and must log `oidc Successfully retrieved and set token`, which npm writes only after the registry returned a token for that package. A missing or mismatched trusted publisher stops the job before anything is published. The first cut of this change kept the plain dry-run, which passes with no trusted publisher at all because npm swallows a failed exchange; the release review read npm's source and caught it
- **What it does not do.** An inline props record still displays as `record` in front of its missing-field list (G245). Impact does not decide an element site (G251). Go-to-definition on a namespace member still answers `null` (G249). `-> Component` still names React's class type, which `tsc` refuses (G247). The remaining stdlib signatures and the editor's unmarked-`src/` definition scope did not land here and are carried into 0.1.126
- **The measurement.** The suite is 1820 tests, from 1799; the exact-or-absent corpus is 36 invariants, from 34; the gap ledger is 253 entries, 224 fixed and 11 open
- **Breaking**, in the checking direction, established by running the published 0.1.124 and this build on the same programs. Three programs pass `glyph check` with `tsc` in the loop under 0.1.124 and are broken at run time; all three exit 1 here: `<Button />` against required props (E0211), `<ui.Button variant="nope" ... />` under `import ui` (E0211), and `<ui.Secret />` for an unexported component (E0105). Two programs pass `--no-tsc` under 0.1.124 and fail only at `tsc`; both are Glyph diagnostics here: `component Greeting(name: string)` (E0229) and `<Button label="go" />` missing a required prop (E0211). A correct program using the named, namespace and aliased spellings exits 0 under both, with and without `tsc`. The emitted TypeScript changes for every component element with no attributes (`null` to `{}`); across the 31 apps and `examples/` every exit code and diagnostic-code list is identical under both binaries, with and without `tsc`

**0.1.124 — Shipped 2026-09-22 · What the benchmark runs on, closed**
- The agent-surface priorities are done; what remains before the 0.2.0 gate is the set of holes the two-agent experiment would find first, each already recorded and each scheduled here rather than left in a lane
- G232: a synthesized record type for an object literal, so `let c = { mode: "read" }; return c` is decided the way the written literal already is, with every new refusal established against tsc one shape at a time (missing required field, wrong field type, record against primitive)
- G235: the editor's own hover answers across a module boundary, which means the language server's overlay registers a project rather than a set of buffers
- G240: a JSX attribute value is checked against the component's prop type, so `variant="nope"` draws the diagnostic `tsc` draws
- The 76 stdlib signatures that model only arity and return, and the 227 exports with no signature, move toward modeled, starting with `std/fs` and `std/array`, since `fs.read_text(42)` silent under `--no-tsc` is the kind of hole the experiment measures
- `alternatives` carries its kind (`variants`, `literals`, `fields`, `exports`) from the error variant, and the nested unmarked root spelling agrees across surfaces (both from the rolling lane)
- The spec's second D43 is renumbered, with every reference moved, so `glyph llms --json` reports no `duplicate_numbers`
- G241: `glyph fmt` prints a run of unary operators as nested parenthesised operands, so a file the parser accepts at 40 levels comes back from the formatter at 80 and draws E0011 on the next pass; the parser counts a parenthesised operand as one level, not two, and the fuzz input becomes a formatter regression test
- The publish workflow was to move to npm trusted publishing before this release's tag. It did not: a trusted publisher is configured on npmjs.com per package by the account owner in a browser, and no one with that login was in the loop when the release was cut, so 0.1.124 publishes on the token path (which `npm whoami` confirmed live before the tag) and the move is the first item of 0.1.125
- What did not close, and where it went: `--no-tsc` still passes a record literal or a component element missing a required field (G232 and G240 stop at ``found `record` ``, since no code names the missing field), and the G240 work found two JSX holes beside itself, G242 (a lowercase namespace alias emits a string tag, a silent miscompile) and G243 (a non-record single-parameter component cannot be rendered); all three are in 0.1.125
- The benchmarks page on the site draws 436 tokens and says about 9% fewer than TypeScript, the figure `benchmarks/FINDINGS.md` and the release notes have carried since 2026-09-06; it had kept the withdrawn 341 and 29% for two weeks, and an outside review quoted them back. `check_doc_claims.py` now reads the newest results file and fails on a bar or a "N% fewer tokens than" sentence that disagrees with it. Landed on main

### 0.1.102 — Shipped · salsa 0.28, and the pipeline's own gaps

A release with no feature work in it. The query layer moves to salsa 0.28, and
that migration travels alone: a dependency bump touching every tracked query is
not something to review beside anything else. Dependabot has been carrying it
since 2026-08-16 (PR #47).

**This plan was wrong about what the bump involves, and reading the crate rather
than the changelog is what found it.** Two breaking changes landed together in
0.28.0, not one.

The first is the one named here. `Update` was not moved, it was deleted, and
replaced by an empty marker trait `SalsaValue` plus a `PartialEq` bound salsa now
uses directly for backdating. That is a same-shape rename of the manual impls in
`glyph-db`.

The second is not mentioned anywhere in this roadmap: `#[salsa::tracked]` now
returns `&Output` by default where 0.26 always returned owned. Twelve tracked
functions are consumed as owned values at about 32 internal call sites and seven
more in `glyph-cli/src/build.rs`, so applying only the first change leaves the
tree failing to compile everywhere.

**Decision: keep owned-value semantics, by pinning each tracked fn to
`returns(clone)`.** Not for being the smaller diff. Every wrapper is an
`Arc`-backed newtype, so `clone` is a refcount bump and adopting `ref` leaves no
performance on the table, which removes the only real argument for it. Against
that, `ref` ties a query result to a `'db` borrow in a compiler whose core loop
mutates the database through `SourceFile::set_text`, and a live borrow held
across a mutation is a conflict at best and a staleness bug at worst. Taking that
risk wants a measured reason and there is none. Adopting the new default stays
open as a later, separately reviewed, per-query change.

The bar for this item is not that it compiles. A migration that quietly stopped
memoizing would compile, pass the suite, and only show up as builds getting
slower. `glyph-db`'s own memoization and backdating tests are the acceptance
criteria, and a test rewritten to accommodate the migration counts as a finding
rather than a pass.

**What it cost, measured rather than assumed.** Nothing outside `glyph-db`
changed: `glyph-cli/src/build.rs` needed no edits, which is the direct evidence
that `returns(clone)` preserved the calling convention. No test was rewritten;
every change inside the test module is a comment. Memoization was checked by
breaking it on purpose, twice and independently: forcing `DeclAst`'s `PartialEq`
to `false` fails exactly the two backdating tests and nothing else, so those
tests are live instruments rather than decoration. Sixteen probes built against
0.26 and 0.28 side by side return identical `WillExecute` counts.

The Glyph stages are about 7% slower, and that is recorded rather than waved
past. It scales with project size, from 5.9 ms on the largest app in the tree to
nothing on the smallest, which is the shape of per-query overhead rather than
lost memoization; lost memoization costs multiples. In the driver a user
actually runs it disappears, because `tsc` is roughly 1.75 s of a 1.8 s check
and the delta is inside the noise. The binary is also 197 KB smaller. Accepted
on that basis.

**One thing the migration turned up that had nothing to do with salsa versions.**
`glyph-resolver` and `glyph-typechecker` both declared salsa and neither used it,
so salsa reached `glyph-wasm` through `glyph-emit` despite a comment in
`glyph-wasm/Cargo.toml` saying it is deliberately kept out because it "would
bloat the wasm binary". Filed and fixed as G164.

Beside it, the items that keep turning up in release verification rather than in
the compiler.

**G162, the formatter bug the fuzzer found the moment it could see.** `fmt`
prints a bare literal statement and then a parenthesized one, and the pair
reparses as a call, so the output means something the input did not. Present in
0.1.100 and earlier, invisible for as long as `format_idempotent` was failing on
its own G151 seed every night. The seed is committed and skipped by name until
this lands.

**darwin-x64 is built and never executed.** Every other platform binary runs
somewhere in CI. The honest fix is a `macos-13` runner, which is x86, rather than
trusting that a binary nobody has run works.

**The gates run by hand, and one of them cannot run where this planned to put
it.** The item as written was "run `check_binary_fresh.py` from the release
workflow", and that is wrong: the gate compares a local `target/release/glyph`
against local sources, and CI builds from the tag in a later job, so a fresh
checkout has nothing to check. Adding it to `release.yml` would fail every
release for a reason unrelated to the release. Verified by running it in a fresh
clone.

What the item was reaching for is real: thirteen commands typed by hand is a
list, not a control, and the failure is not refusing to run one, it is running
twelve at two in the morning without noticing which was skipped. That is how the
0.1.74 cut reached CI with a lint failure. `scripts/check_release.py` runs all
thirteen and reports every result rather than stopping at the first, with
`--suite` to run the test suite into the log `check_doc_claims` reads.

Checking that premise turned up G163, which is worse than the item it came from:
the same helper fell back to a `glyph` on `PATH` when no local build existed, so
the three gates that RUN the compiler could have exercised an installed release
instead of this tree's code and reported a clean result for something never
compiled. Fixed, with `GLYPH_BIN` kept as the deliberate override.

**Alpine gets a glibc binary and a confusing failure.** Detecting musl and saying
so is a diagnostic, not a port. The signal is `process.report`'s
`header.glibcVersionRuntime`, which Node embeds on glibc builds and omits on
musl. Failure to read it is treated as glibc: a wrong musl diagnosis on a real
glibc machine is worse than the `ENOENT` it replaces.

The detection answers for the platform it is given rather than for
`process.platform`, and that is not a detail. The glibc field is Linux-only, so
its absence means nothing on macOS or Windows, and a version that read
`process.platform` internally answered `true` on every Mac. Nothing broke,
because the caller checked the platform first, but the correctness lived in the
caller remembering to. Two independent reads of one fact can disagree, and when
they do the guard silently stops matching the condition it guards.

*Reviewed against 0.1.101.* Every claim above was re-checked rather than carried
forward, and one item was dropped because it is already done: license text ships
in all six npm packages today. `npm/glyph` and each platform package carry
`LICENSE-MIT` and `LICENSE-APACHE` and list both in their `files` field, so
there is nothing to do. The rest hold: `salsa = "0.26"` in the workspace
manifest, no `macos-13` or `darwin-x64` execution in `ci.yml`, no reference to
`check_binary_fresh` in `release.yml`, and no musl detection in the npm
launcher.

### 0.1.101 — Shipped · A note on an example line no longer verifies a false claim

- `glyph build --watch`, so the Glyph half stops being compile-by-hand next to Vite's HMR (G123)
- A browser target that stops materializing the seven host modules under `.glyph-runtime` (G115)
- Make `glyph fmt` reach a fixed point in one pass so `--check` stops failing on output `fmt` just produced (G151, and the nightly fuzz job is red on it every night until this lands)
- Stop `fmt` reprinting `=> ({})` as `=> {}`; the AST needs a grouping node (G60)
- Remap a tsc error onto the right project's module in a multi-project build (G107)

**What shipped, against what was planned.** G160 and G161 were not on the plan
when this release opened; they came out of adversarial review of two rejected
G151 fixes and G160 outranked everything else here. G60 was already fixed and
closed at triage with no code written. G151's two-pass symptom went with G160,
since they were one defect. G158 and G159 moved to the polish lane: this release
was already carrying a verifiability fix and four regressions of its own making,
and a 92-file reformat on top of that would have made the diff unreviewable.

**Three regressions this release introduced and then fixed before shipping.**
The first G151 attempt made `fmt` rewrite the inside of string literals. The
second relocated comments out of bracketed argument lists and left trailing
whitespace on 95 of 200 generated files. Std-module pruning broke `extern/*.ts`
shims, and then, once fixed, left a stale staged copy that failed every later
build after a shim was deleted. Each was found by review rather than by the
suite, which is the argument for the review phase existing.

*Reviewed against 0.1.100.* G123, G115 and G107 were each re-run for the 0.1.97
cut and still reproduce: there is no `--watch`, a one-import program still
materializes 37 std modules, and a two-project build still quotes the wrong
project's source under a `TS2307`. G151 is the fuzzer's own open finding and
G60 is re-checked at triage.

**G160 is the reason this release grew, and it outranks everything else in it.**
Adversarial review of the two rejected G151 fixes found that the same parser
defect silently deletes an `@example` assertion, so the compiler reports a false
claim as verified. That is a verifiability failure in the shipped 0.1.100, not a
formatting papercut, and the ledger entry that called G151 harmless is what let
two fixes be written and judged against the wrong premise. G161 came out of the
same read: `fmt` reattaches a comment to the wrong annotation and the result is a
fixed point, so `--check` passes on the damage forever. G158 makes the release coherent: three formatter
items and the reason to trust the output of all three, which is that 121 of the
284 tracked `.glyph` files currently disagree with `glyph fmt` and nothing
notices.

**Two items carried out of 0.1.100, both decisions rather than patches.** G128,
whether `http.get` and friends take a required deadline the way `tls.connect`
does since G127, or whether `request` stops reading a 0 timeout as permission.
And G91, whether `Option<T>.parse` loosens to accept an explicit `null`, or a
distinct nullable boundary type decodes into `Option<T>` and `Option` stays
strict. Both change the public surface and neither is the agents' to pick;
they reported the fork, which is what they were asked to do.

### 0.1.100 — Shipped · A client can read a response as text

- Decide and ship the http deadline: required on `get`/`post`, or `request` stops reading 0 as skip-the-timer (G128)
- Give `Response` a text accessor so a JSON body stops printing as `[object Object]` (G118)
- Decide how `Option<T>` reads ordinary JSON: loosen `.parse`, or add a distinct nullable boundary type (G91)

*Reviewed against 0.1.99.* G128 and G118 were both re-run for the 0.1.97 cut and
still reproduce: `fetch_of` and `head` still set `timeout_ms: 0` and 0 still
means no timeout, and `std/http` still exposes only the server-side
`text(status, body)` rather than a client accessor. G91 is re-checked at triage.
Two of the three are decisions rather than patches, and the agents are told to
report the fork rather than pick.

### 0.1.99 — Shipped · The front end answers instead of tsc

- Check an annotated `let` against its initializer, so `let x: string = 42` is a Glyph error rather than TS2322 (G149)
- Make `glyph fix` act on an unused item inside a named import list, not only a whole unused import (G152)
- Put the escape table in the bootstrap and make E0001's help name `\u{HEX}` rather than the category (G153)
- Give `std/random` its signatures in the bootstrap; `seeded` and `Rng` appear zero times today (G154)
- Delete or implement `Rng.bool`'s promised 0.5 default; the comment describes a signature that does not exist (G155)

*Reviewed against 0.1.98.* G152 through G155 came from an outside field test and
were each reproduced here before filing. G149 was found while verifying the VS
Code extension and is reproduced in its entry. The remaining items are
re-checked at triage rather than assumed here.

### 0.1.98 — Shipped · The two-binding for loop knows its element type

- Bind an element type for the two-binding `Stmt::For`, so a string-literal union read through the second binding keeps D30 and stops downgrading `E0200` to `E0218` (G37, G67). The miscompile this item used to cite is already fixed; see the note below
- Keep D30 string-union exhaustiveness alive inside a loop, so `E0218` stops suggesting the `else` that forfeits the guarantee (G67)
- Diagnose a module that declares no `pub` and emits zero exports before the host toolchain does (G124)
- Count `@example` references in the unused-import lint, so `E0106` stops firing on an import the examples use (G106)

*Reviewed against 0.1.97.* G106 was re-run for this cut: an import referenced
only from an `@example` still draws `unused import`. G37 and G67 were last
reproduced in the rounds that filed them and are re-checked at triage rather
than assumed here, which is what the triage phase is for.

### 0.1.96 — Shipped · The variants of a union the emitter did not declare

This was the 0.1.95 plan and it did not ship there. 0.1.95 shipped the item that
had already landed alongside it, G148, plus `glyph --update`; the four entries
below are unchanged and move forward together, because they share one address.

The lead item is the hole in the rule 0.1.93 shipped. A nested arm reads a name
the way the typechecker does, which means the payload union's own variant list
answers first; for a union imported from a sibling module there is no list to
read, so the emitter falls back to the name's shape. Shape says "variant" for
`Blank` and "binding" for `blank`, and the lowercase spelling of a valid program
stops the build at `E0305`. That is G147. The shallow imported-union coverage
check (G143) is short the same thing, so handing the emitter an imported union's
variant names once settles both, and that is what this release is. Three more
sit behind it: G146, G138, and what is left of G140.

G146 came out of the same gate. A constructor-shaped payload pattern whose
payload type is known and is not a union, `Ok(Point)` over a record, now emits a
test on a `.tag` the author never wrote, so `tsc` reports it against generated
TypeScript. It wants to be a Glyph diagnostic of the E0220 class, naming the
type and the pattern. Better than the silent binding it used to be, still the
wrong compiler answering.

G138 stays open. It is the same disagreement in the array position.
`[Black]` at the top level of a match lowers to `const Black = __m0[0]`, while
the same element inside an object pattern's field lowers to a tag test as of
0.1.90. Half of it is already closed: the array exhaustiveness predicate no
longer counts a PascalCase element as a binding, so a match that leans on the
miscompile is reported non-exhaustive instead of certified. The lowering is what
is left, and the correct implementation of it already exists a few hundred lines
away in `pattern_conditions`; routing the top-level array chain through it is
the fix. The G145 gate does not reach it: a top-level array pattern routes to
`emit_array_chain` before the degrouping gate is consulted, and
`match xs { [] => .., [Black] => .. }` still emits `const Black = __m0[0]`.

G140's namespace half is what is left of it now that G141 has shipped, and it
matters more than the half that went. The same nested arm over a union declared
in a sibling module compiles under `import tree { Tree, Leaf, Node }` and is
`E0300` under `import tree` with `tree.Node`, whether or not the union is
generic. The emitter's last resort is a
by-name fallback that resolves the variant through the consumer's own
`ImportNamed` symbol, and a namespace spelling never binds that name. Two
spellings of one import giving two answers is what G75 settled we do not do,
which is the reason to fix this at the checker rather than by adding a fifth
lookup to the emitter: record the payload type in both halves and the fallback
stops being load-bearing.

The namespace half is the emitter and is still open. With the payload type
recorded, the arm still needs `variant_payload_is_record` to resolve a variant
reached as `tree.Node`, and its by-name fallback has nothing to look up. Adding a
fifth lookup to the emitter is the wrong place for it; the right one is making
the namespace spelling reach the same recorded type the named spelling does,
which is its own release rather than a ride-along.

*Reviewed against 0.1.93.* Both falsifiable claims above were re-run against the
0.1.93 binary rather than assumed. G138 holds and is worse than a lowering
detail: `match xs { [] => .., [Black] => .. }` emits `const Black = __m0[0]`, so
`f([White])` prints `one-black`. It is a live miscompile, not a missing
diagnostic. The namespace half of G140 also holds, with one refinement this
re-read produced: it bites only when the nested constructor carries a payload.
`tree.Node({ left: tree.Node({ key: k }), key: outer })` is `E0300` under a
namespace import and clean under a named one, while a nested nullary variant
(`tree.Node({ left: tree.Leaf, key: k })`) compiles under both. So the fallback
is load-bearing for payload-carrying constructors specifically, which narrows
what the checker has to record.

G133 is the reason the G132 arm behaved the way it did, and it is worth more
than the arm: the checker has no cross-module function signature at all.
`DeclTyResolver` reaches across modules for types, unions and string-literal
unions, and `glyph_db` exports one declaration query, `exported_type`; a `fn`
has no counterpart, so every call into another module returns `Unknown` and
everything inferred from it is lost. The cost is not confined to match arms:
a field typo on a cross-module call's result is a mapped TS2339 against the
statement where the same typo on an annotated binding is E0210 naming the type
and the field, which is the boundary Glyph's whole diagnostic story is about.

G133 is parked here pending a decision, not scheduled, because adding
`exported_fn` alongside `exported_type` changes what the checker knows about
every multi-module program at once. That is a scope call and a risk call
(new diagnostics would surface across the examples tree on the same commit),
and it wants its own release rather than a ride-along. Two things to settle
with it: whether the query returns a full `Ty::Fn` or only the return type,
and what the emitter should do when the registry answers nothing for a project
module's union, which today is a silent guess at the single-value shape whose
consequence `tsc` reports at a span that is not the arm.

### 0.1.108 — Shipped · One identity, one spelling

0.1.107 gave every diagnostic the declaration it sits in, and gave it three
spellings. `glyph check --json` strips the project src root, `glyph_variants`
resolves through `project_root_for`, and `glyph_diagnostics` uses the server
workspace root. On the layout `glyph init` writes, the first reports `a::f` and
the last reports `src/a::f` for the same declaration. Both doc comments claim
they agree.

**This is 0.1.56 and 0.1.57 arriving from a new direction.** Those two releases
were spent removing a guarantee that changed with how a type was imported, and
the conclusion recorded then was that a design where the spelling can matter is
wrong on arrival. Nothing about that conclusion was specific to imports.

The tests could not have caught it. They write into a bare temp directory with
no `package.json`, where the server root and the project src root are the same
directory and all three derivations agree by accident of the fixture. The
harness for a marked project already exists in the same file and was not pointed
at the new field.

**Why this outranks the impact query it was supposed to enable.** The roadmap
names the entity field as a prerequisite of reading the edges forward, and it
also says a partial list is worse than none. An impact query keyed on an
identity with three spellings does not return a short list, it returns a
confident wrong one, which is the failure the exact-or-absent rule exists to
make impossible. So the query waits one release.

**The second half is a real violation of that rule, not a gap in it.** An
annotation's text sits in the gap between two declaration spans, because
`parse_annotations` runs before the keyword while a `Decl` span begins at it. So
the span walk finds no declaration for an unknown-annotation error, and reports
no entity. But the checker emits that error from inside
`check_annotations(&mut self, decl: &Decl)`, holding the declaration by
reference. The relation was computed and thrown away, and its absence then reads
as "there is no relation here". `tsc`-remapped diagnostics say the same thing
for a similar reason, with the Glyph source they would need already sitting in
`ModuleMap`.

**Also closed here, from the same review.** The release workflow ran a
`Smoke macos-13` job whose label had been retired nine months before it was
written, so it was never assigned a runner on any release and `darwin-x64` was
executed nowhere while a job claiming to execute it was cancelled by hand each
time. It now runs on `macos-15-intel`, and in `verify` rather than only in
`smoke`, so an x64 binary is exercised before the irreversible publish instead
of after it. `macos-14` was deprecated with support ending 2 November 2026 and
would have failed identically; both darwin targets build on `macos-15`.

### 0.1.112 — Shipped 2026-09-05 · `glyph_impact`, generalized over every relation

0.1.110 and 0.1.111 built one relation's answer and proved it closes a loop. This
generalizes it: one entity in, nodes and labelled edges out, over more than one
relation at a time.

**This is the release most likely to go wrong, and the failure has a shape.** A
specific semantic question turns into generic graph infrastructure, "impact"
comes to mean everything, and Glyph becomes a graph database with a compiler
attached. The defence is that every edge stays compiler-derived: if the checker
did not already compute it, it is not an edge.

**Transitivity is settled in writing before any code.** `A -> B -> C` has to mean
one thing, and direct semantic consequence, transitive dependency, compilation
consequence and runtime consequence are four different relations. "Potentially
affected" is not a member of the set. A caller of a function that stopped
compiling is impacted only if that function's signature changed, so transitivity
is a property of the change kind rather than of the graph.

The verdict vocabulary is closed and each member means one thing: `WILL_FAIL`,
`ABSORBS`, `SAFE`, `UNDETERMINED`, `REFERENCES`, `GENERATED_FROM`, `NOT_INDEXED`.

*Reviewed against 0.1.111.* The six tools are `glyph_diagnostics`, `glyph_hover`,
`glyph_definition`, `glyph_references`, `glyph_variants` and `glyph_symbols`.
There is no `glyph_impact` and no query takes more than one relation at a time,
so nothing here has been built and the transitivity question is still owed a
written answer rather than an implementation.

### 0.1.111 — Shipped 2026-09-04 · The proof, and what the relations still owe

0.1.110 shipped the capability and the invariant. It did not ship the proof, and
the distinction is the whole point: a release that can answer the question is a
claim, and a release somebody else can run is a demonstration.

**The tests that were planned for 0.1.110 and never built.** The repair loop has
been walked once, by hand, on one union. Nothing asserts that it closes (G196).
And nothing tests the surface an agent actually talks to: every MCP test calls
the dispatcher in process, so framing, the handshake as an ordered exchange, and
behaviour on a malformed line are unexercised. The `instructions` string 0.1.107
shipped appears in no assertion at all, so deleting it keeps the suite green
(G197).

The test that matters most is also the easiest to write dishonestly. It has to
assert that each hop's input came out of the previous hop's answer as a field. A
loop that closes because the harness supplied the union's name from its own
fixture proves that the fixture agrees with itself.

**The answer takes no position on its own totals** (G198). Ten sites come back
with consequences and no count, so the three lines a reader wants are arithmetic
the caller performs. Any total states what it could not count, because a number
reads as authoritative in a way a list does not.

**The demonstration is a transcript** (G199). `examples/apps/csvql`, eleven
files, one variant added to the `Value` union at its centre: ten match sites
across four files, eight that fail and two that keep compiling and absorb it, and
the compiler afterwards reporting exactly those eight. Every number measured.
None of it committed, scripted or runnable by anyone else.

*Reviewed against 0.1.110.* Each claim re-run against the binary this release
builds. `instructions` appears twice in `mcp.rs`, in a doc comment and in the
value, and in zero assertions. No test walks the repair loop. An answer's
top-level keys are `proposed_variant`, `sites` and `type`, with no count, total
or summary among them. The demonstration is still a transcript.

**What the relations still owe.** 0.1.110 landed one answer per symbol across
import spellings, `CALLS` apart from `REFERENCES`, proved apart from asserted,
and fields addressable as `module::Type.field`. What has not been done is reading
them together: one entity in, a bounded traversal out, over more than one
relation at a time, with the transitivity question settled in writing first.

### 0.1.110 — Shipped 2026-09-04 · "What breaks if I add this variant?"

The flagship. Everything from 0.1.104 onward has been building the parts; this
is the release where they answer a question somebody would actually ask.

**One capability, said four ways, and shipping three of them is a claim rather
than a demonstration.**

*The capability.* The question can be asked as a change rather than a lookup:
`glyph_impact { entity: "payments::PaymentResult", add_variant: "Pending" }`. The
answer states a consequence per site, not a state the caller has to interpret,
and it lists the union's own variants so the caller can render what it is
changing. A proposed name that already exists is rejected, which the lookup form
cannot check because the name never reaches it (G187, G188).

*The invariant.* `tests/exact-or-absent/` already gates five degenerate
conditions and records two known failures. Both close here: a record answered
with an empty site list, shaped identically to a union with no matches (G190),
and `glyph check --no-tsc` reporting no diagnostics on a program importing a
module that does not exist while the impact surface correctly answers
`scrutinee_unresolved` for the same file (G189).

*The tests.* The loop has been walked once, by hand. Nothing asserts that it
closes, and nothing tests the surface an agent actually talks to: every MCP test
calls the dispatcher in process, and the `instructions` string 0.1.107 shipped
appears in no assertion at all, so deleting it keeps the suite green (G196,
G197). The diagnostic also has to carry the entities it concerns as fields, since
today the union's name reaches the next query only through a regex over a
sentence (G195).

*The artifact.* The demonstration runs today and exists as a transcript.
`examples/apps/csvql`, eleven files, one variant added to the `Value` union at
its centre: ten match sites across four files, eight that fail and two that keep
compiling and absorb it, and the compiler afterwards reports exactly those
eight. It is not committed, not scripted, and not runnable by a reader (G198,
G199).

*Reviewed against 0.1.109.* Every claim above re-run against the binary this
release builds, rather than carried forward. `glyph_variants` still accepts only
`path` and `name`, ignores a proposed variant, returns `exhaustive` and
`has_catch_all` rather than consequences, and lists no variants of the union it
was asked about. A diagnostic's fields are still code, entity, file, help,
message, note, range, severity and stage, with the union's name reachable only
out of `non-exhaustive match on \`PaymentResult\`: missing variants \`Pending\``.
The two known cases in `tests/exact-or-absent/` are still failing, which is what
that gate reports when it passes.

**What this release is measured by.** Not gaps closed. The table at the top of
this file: files searched, compiler queries, semantic sites missed, incorrect
edits, repair iterations, time to green, against an ordinary TypeScript agent
doing the same task. Those cells are empty on purpose and stay empty until the
experiment runs.

### 0.1.109 — Landed on main · The folds you write by hand

Two `std/array` gaps that an outside app found and that nothing has closed. Both
were re-reproduced against 0.1.108 rather than carried forward on their old
stamps: every name below is still `[E0105] not exported by std/array`.

**G100, no `max`, `min`, `max_by`, `min_by` or `sum`.** `std/math` has the
scalar `min`/`max` only. Picking the highest-scoring element of an array is the
core operation of every search, every ranking and every scheduler, and the fold
that does it is four lines of `match acc` ceremony each time. One app wrote it
five times in one file.

**G101 is held back on purpose, and this is the reason.** `fold` cannot stop early. Alpha-beta pruning is exactly a fold that
stops when the window closes. It is expressible today, and correct: a pair of
mutually recursive functions threading an index compiles and returns the
textbook answer. But it takes four functions and an explicit cursor to say what
one `fold_while` call says, and the version that reads naturally silently
evaluates every branch, which for a search is the difference between pruning and
not pruning.

Closing it means giving the callback a way to say "stop, and here is the final
value". In a language with tagged unions the natural shape is a new
`Step<A> = Continue(A) | Stop(A)` export; the cheaper shape reuses `Result` and
spells an ordinary early exit as an error, which it is not. That is a permanent
addition to the standard library's surface either way, so it goes to review
before it goes into a release, and 0.1.109 ships the half that has no such
question in it.

*Reviewed against 0.1.108.* Both premises re-run rather than carried forward.
Importing each name and reading the answer: `max`, `min`, `max_by`, `min_by`,
`sum`, `fold_while`, `try_fold` and `fold_until` are every one of them
`[E0105] not exported by std/array`. The export list the error prints is
unchanged from the one G100 recorded, so nothing has been added quietly since.

**Neither is a soundness bug, and that is the argument for doing them.** The
hand-written forms are correct, just long. Long is what an agent gets wrong, and
a four-line fold repeated five times in one file is four opportunities to get
the accumulator's initial value or its comparison direction backwards, none of
which the compiler can catch because every version of them type-checks.

### 0.1.109 — Impact before the edit: delivered, and what the measurement says

0.1.106 retained the relation and put a tool on it. `glyph_variants` answers
"which sites name this variant". This release answers the question an agent
actually asks, which is the same edges read in the other direction: **add
`Suspended` to `UserStatus`, and show me every site that must change.**

**The answer is exhaustive or it says which sites it could not index.** A partial
impact list is worse than no list, because an agent that trusts it edits
believing it has seen the whole surface. 0.1.106 already established the honest
shape by naming unkeyable sites rather than counting them; this release inherits
that obligation and extends it, because a forward query has to be right about
absence in a way a lookup does not.

**Then the loop.** A diagnostic today is text with a span, so an agent's only
route from an error back into the model is to re-parse the file it was just told
about, which is the search this whole line of work exists to delete. Diagnostics
carry the entity IDs they concern, and the queries chain so following a thread
does not re-derive the world between hops.

**Measured against search alone**, same agent and same prompt on both sides, with
the instrument defined before the result is seen and the losing run published.
The bug shape is the one this compiler has hit five times: G139, G141, G142, G143
and G148, each a nested-payload failure, which is why 0.1.106 built the nested
case rather than declining it.

*Reviewed against 0.1.106.* Re-checked rather than carried forward, and this
premise did not hold: `glyph_variants` (0.1.106) already reads the relation
forward for the has_catch_all/exhaustive shape when called by union name, with
a regression test on the exact "add a variant" edit this release describes. A
resolve/typecheck/emit diagnostic that lands inside a top-level declaration now
carries `entity` (the `module::name` identity `glyph_variants` reports for the
same declaration), and `benchmarks/impact-before-edit/` is the committed
fixture and measurement comparing that shape against search alone. Still open:
whether the same forward answer holds for the nested-payload shape this release
names (G139, G141, G142, G143, G148), where the impacted match reaches the
union through a payload rather than as its own scrutinee. `glyph_variants`
already lists those under `nested`; nothing has measured that case end to end
yet.

*Measured against 0.1.108, and it holds.* A union with a direct exhaustive
match, a direct catch-all match, and a match reaching it one level inside a
`Result` payload: the tool names all three before the edit, calling the payload
site `exhaustive` at depth 1. Adding a variant then produces exactly two
`E0200`s, at the direct site and the payload site, each carrying the declaration
the tool had named, and nothing at all for the catch-all, which keeps compiling
and routes the new variant into `else`. The payload case is now part of the
committed fixture rather than a transcript, and the measurement script reads
`nested` alongside `sites`, which it did not before: an impact answer that reads
only the first list is incomplete in precisely the direction this benchmark
exists to measure.

The comparison it reports is narrower than the demo framing suggests, and worth
stating plainly. Search alone *finds* the catch-all site; it just cannot tell
which union a catch-all belongs to, so on this fixture it also returns an
unrelated one. That is precision 0.5 against 1.0, not a miss against a find.

### 0.1.106 — Shipped · The exhaustiveness relation, and the tool reading it

0.1.105 gave the checker one answer to "does this site have a catch-all". This
release keeps the relation that answer is computed from, so an agent can ask
what breaks before it edits rather than after.

**Three prerequisites the original plan did not know it needed**, all confirmed
still true after the unification.

*A match site has no identity.* `walk_decl` takes a bare `&Decl` with no index
and no name, `Assigner` carries no enclosing-declaration field, and no
span-to-declaration helper exists. Arms are site plus ordinal, decided in
0.1.104, so the site itself has to be nameable first.

*`required_variants` returns a display string, not an identity.* Four producers
feed it and the module is in hand inside `imported_union_variants` and dropped
at the return. The corpus holds 11 unrelated declarations named `Command`, so a
bare string is not survivable as the type end of an edge. Threading a `DeclKey`
touches all four producers and every caller.

*The relation is not formed anywhere.* `check_patterns_exhaustive` keeps
`covered: HashSet<Ident>`, variant names with no link to an arm, and the arm
ordinal is destroyed in the recursion. Retaining this is not a save of something
computed; it is computing it.

**What is retained, decided in 0.1.105's design pass.** A site record with four
states, exhaustive, has-a-catch-all, declined, and scrutinee-unresolved, the
last because 128 corpus sites have a scrutinee that resolves to no type and
reporting them absent would assert no relation exists. `mentions` edges, named
that and not `covers`, because 71.6% of top-level edges are the deferred kind
where the checker never concludes full coverage. The catch-all arm's ordinal.
Not an unreachable-arm edge: measured 0 occurrences across 6,516 arms, and
structurally so, since E0216 is an error and the graph is built from code that
compiles.

**Where it lives.** A separate `match_coverage(db, file)` tracked query rather
than a field on `Types`, because `TypeMap` is span-keyed and a coverage set
riding on it would be invalidated by a keystroke that changed no coverage.
A project-level fold beside it, because the by-type index spans files.

**Then the tool.** For a type, every match site over it and which variants each
one names, plus the sites it could not index, named rather than counted.

**One rule that binds it.** The edges are keyed by `DeclKey`, and no query that
produces them may be keyed by a position. The five position-keyed
per-declaration queries are not on this path and nothing outside `glyph-db`
calls them, so the churn does not reach the edge set today; a per-declaration
coverage query added later for incrementality would reintroduce exactly the
renumbering `DeclKey` shipped to remove.

#### The site-identity fork, decided: sites get no durable identity

The review left this open and it decides the release, so it is settled here.

**Neither candidate works.** A bare ordinal within the declaration leaves 1,285 of 2,725 sites
(47.16%) able to move when a match is inserted above them, and it reintroduces at the site level
exactly the failure 0.1.104 shipped `DeclKey` to remove at the declaration level. The scrutinee
spelling looks better at 106 sites (3.89%) needing a disambiguator, until you ask what it is
unstable under: **2,103 of 2,735 scrutinees are compound expressions**, and a spelling key changes
whenever the scrutinee is edited. That trades 47% exposure to insertion for 77% exposure to an
ordinary refactor, and it puts a second normalisation path next to a formatter that has produced
six gaps of its own.

**The exit is that nothing needs a durable site identity.** The review established the fact and
stopped short of the conclusion: nothing in the compiler holds a site identity across revisions,
because storage is per file and re-executes on any edit. The only holder is an agent that received
an opaque "site 3 of `bind`" from a tool call and edited before asking again. So the fix is not a
better key. It is to stop handing out a key that can go stale.

- **Storage** uses a within-file index, valid only inside the computation that produced it. It is
  never published and never compared across revisions, so it is not an identity and the One Rule
  does not bite.
- **The answer** carries a descriptor rather than an id: the declaration's `DeclKey`, the scrutinee
  as written, and the line. "In `csvql::bind`, the match on `col` at line 214, arm 2." An agent
  relocates that rather than dereferencing it, and the spelling appears as prose rather than as a
  key that must be canonical, so no normalisation is owed.
- **The durable ends stay durable.** The declaration and the type ends of an edge are `DeclKey`s,
  which is where identity was always needed and where 0.1.104 already put it.

What this costs is that an agent cannot cache a site reference between calls. Given the query
answers warm in about fifteen milliseconds, re-asking is the cheaper half of that trade, and an
answer that is always current beats one that is addressable and sometimes wrong.

#### The shape, decided: extract the classifier and have the dispatch fill a sink

The review showed the three shapes are not alternatives, and that the plan's own argument did not
reach its conclusion. A site's state is decided by an ordered dispatch, and the match handler
mutates the type map before any of it runs, so a query that re-walked and re-asked would need to
reproduce the dispatch and would become the third copy of the logic this release exists to stop
duplicating.

So: one extracted per-pattern classifier, and the existing dispatch, which already knows the
answer, accumulates into a sink as it goes. One copy of the dispatch, one of the per-pattern
rules, two thin call sites in the duplicated pair. The classifier must read three buckets rather
than two, because a single value-testing object payload is recorded in neither `covered` nor
`nested` and is exactly what the declined state is for.

#### What an adversarial review of this plan found

Review 61, saved locally. It re-measured every figure and found four of ten wrong, two structural
claims wrong in the plan's favour, and one contradiction between this plan and its own rule.
Corrected here rather than left for whoever implements it.

**The numbers were high, systematically.** The corpus has **2,725** match sites and **6,502**
arms, not 2,734 and 6,520. Sites with an ordinal above zero: **1,285 of 2,725 (47.16%)**, not
1,289. Sites at depth one or more: **796**, not 802, with the depth-1 and depth-2 buckets each
wrong. Head-bearing sites: **1,138**, not 1,143. Every error was small and positive, which is the
signature of a tool counting some sites once per visit rather than once per node, and the review
anchored its own count on a token partition that sums with no remainder so the ceiling is provable.

**The review disclosed something worth keeping.** Its own first two walkers reproduced the plan's
figures to the digit, ten of them, and it could not reproduce those runs afterwards. It reported
the partition-anchored numbers because the partition is provable and a walker is not. So two
independently written tools share a defect, which is a better reason to distrust the figures than
any single disagreement.

**Also struck: "corroborating an earlier count to within one site."** The two pairs differ
deterministically, by the JSX `<match>` element and its four `<case>` children, and both pairs
were high. Presenting a deterministic difference as measurement tolerance is the sentence to
delete.

**The figure 792 was used for two different populations and was correct for neither.** Prelude and
stdlib together cover **791 of 1,138** head-bearing sites. The `nested`-only trap covers
**1,058 sites and 1,772 arms**, and it has a third bucket the plan missed: a single payload that is
a value-testing object pattern is recorded in neither `covered` nor `nested`, so an extraction
reading both still drops those arms, and they are exactly what the declined state is for.

**"Two of four producers are mechanical" is wrong: none of them can construct a `DeclKey`.**
`ModuleId` is issued by a project-level interner inside `project_decl_keys`, and `Assigner` holds
no module id at all, verified as zero occurrences of `ModuleId` in `assign.rs`. So the plan's
second bullet cannot be done as written. The work is to return a type reference distinguishing
local, imported and builtin, and mint the key at the `glyph-db` boundary where the interner lives,
which crosses a crate seam the plan did not mention. There is also a fourth caller the plan's
"every caller" omitted, `check_arm_reachability`.

**One claim in the review is itself wrong, and it made the problem look harder than it is.** It
states that 129 of 175 corpus files carry no `module` declaration, so "for three quarters of the
corpus the typechecker cannot name its own module even as a string." Measured: **all 175 files
declare their module.** The field is `Option<ModulePath>` so it can be absent in principle, and
the typechecker never reads it, but the string is available on the `Module` the Assigner already
holds. That makes the type end easier than the review argues while leaving its conclusion intact:
the string comes from the typechecker, the `DeclKey` is minted in `glyph-db`.

**The prelude reason is better than the plan's.** Stdlib and prelude types are not nameless:
`StdlibStubs` publishes `std/result::Result`, `std/option::Option` and `std/fs::ErrorKind`. What
they lack is a project declaration, and minting an id for them is unsafe rather than merely
unhelpful, because `ModuleId` is an index into the interner that issued it and an in-range id from
another interner names the wrong module rather than answering nothing. That is the exact-or-absent
invariant, not a limitation, and it is the argument for a sum type at the type end.

**The site ordinal contradicts this plan's own rule, and that has to be reconciled.** The rule says
no query producing edges may be keyed by a position; the plan then keys the site half of every edge
by an ordinal. The arm ordinal is defensible because order is semantic there and first match wins.
Nothing makes "the third match in `bind`" meaningful, so for a site the ordinal is enumeration
order, which is the shape 0.1.104 was spent removing one level up.

The cost was also mis-stated. 47.16% is the fraction of sites that *could* move, not churn that
happens, and nothing in the compiler holds a site identity across revisions: storage is per-file
and re-executes on any edit anyway. The only holder is an agent that received "site 3 of `bind`"
from a tool call and edited before asking again. So the cost is a stale cross-call reference in a
conversation, which is far smaller than the number sounds.

**And the review measured a scheme this plan never considered.** `DeclKey` plus the scrutinee's
normalised source spelling, with an ordinal only among same-spelling sites in that declaration,
leaves **106 sites (3.89%)** needing a disambiguator instead of 1,285. It is stable under
inserting a match on a different scrutinee, which is the common edit, legible in a tool answer
("the second `match on stack` in `step`"), and fails only on a local rename, which is what
`DeclKey` itself fails on. Not free: 1,939 of 2,724 scrutinees are compound expressions, so the
key is sometimes long. This is a live fork and it is the orchestrator's to settle.

**The ordinal rule is under-specified either way.** "Pre-order" does not pin it, because
`Expr::Match` walks the scrutinee, then checks itself, then walks the arms, so a scrutinee-nested
match is checked before its parent while a pre-order over nodes visits the parent first. Two
implementers would assign opposite ordinals. No corpus site has a match inside a scrutinee, so
nothing decides it today and the first program to write one gets whatever the code happened to do.

**The most consequential finding is about the shape, and it is not the one the plan argues.** A
site's state is not decided by a predicate. It is an ordered dispatch in
`check_match_exhaustiveness`: reachability, prelude-array, bool with literal-arm recovery,
string-literal union, number and string with the same recovery, imported union gated on the union
base, the refutable-object check, and only then `check_patterns_exhaustive`. On top of that the
`Expr::Match` handler mutates the type map before any of it runs, writing a recovered union back at
the scrutinee's span. **So a separate query that re-walks and asks `required_variants` per site
would classify some sites differently from the diagnostics unless it reproduced the whole
dispatch, which would make it a third copy of the logic this release exists to stop duplicating.**
The way out is that the three shapes are not alternatives: extract the classifier and have the
existing dispatch, which already knows the answer, accumulate into a sink.

**Two smaller corrections.** There are six entries into the coverage core, not five, and the
omitted one is the imported side's recursion, which needs the same ordinal threading. The
slice-element change touches nine places, not three, and two of them are in
`check_imported_union_coverage`, which the plan treats as out of scope; threading one and not the
other gives the duplicated pair a second divergence, which is what 0.1.105 was spent removing.
Only two of the three coverage functions duplicate the collection logic; the string-literal one
collects values rather than heads and already receives arm indices for free.

**A better argument for building the nested case than the plan's.** The plan justified it as cheap.
The real argument is that the five bug shapes this instruments, G139, G141, G142, G143 and G148,
are all nested-payload failures, and `examples/apps/zipper/main.glyph:205` is the exact shape of the
class. Declining those thirteen arms would leave the instrument blind precisely where the five bugs
lived.

**Not addressed by the plan, and each of these is something an implementer hits.** `@example`
bodies are compiled in a shadow project with synthesised declarations, so a match inside one is a
site in a second project whose module keys name paths that do not exist, and the corpus already
has one. A site inside a closure has no identity of its own, so 89 of them inherit the enclosing
declaration's key and are told apart only by ordinal. The JSX site cannot produce two of the four
states, bypasses `check_match_exhaustiveness` entirely, and ignores an `<else>` child, which also
means a JSX match over an imported or string-literal union is unchecked today. And `match_span` is
the only handle the coverage functions have on a site, so the identity and the diagnostic span are
two values that must travel together with neither derivable from the other.

**Four figures remain unverified** because they need an instrumented build: the 128
scrutinee-unresolved sites, the 1,699 non-union-shaped sites, the count of string-literal union
sites, and the 71.6% deferred top-level edges. That last one is the naming argument for `mentions`
over `covers`, so it deserves a measurement of its own before the name is defended on it.

*Reviewed against 0.1.105.* Re-checked after the unification changed this code:
`is_catch_all_pattern` is one definition with eight call sites, the last
hand-rolled uppercase test is gone, `glyph-db` retains no coverage of any kind,
and `walk_decl` still takes a bare `&Decl`. The corpus figures are carried from
the design pass and were measured with an instrumented build over 175 files.

### 0.1.105 — Shipped · One answer to "does this match have a catch-all"

The compiler works out which match arms cover which variants on every build, and
then throws the answer away. Retaining it turns "what breaks if I add a variant"
from a search into a lookup, and that is the question this whole line of work
exists to answer.

**R4, the edges themselves.** `check_patterns_exhaustive`, `check_arm_reachability`
and `required_variants` in `assign.rs` each compute part of the relation. None of
it survives the pass. Keep the arm-to-variant edges with a per-site flag for
exhaustive or catch-all, so a consumer can tell a site that covers everything
from one that ends in `else`.

**Arm identity is decided and it is the match site plus the ordinal.** That was
settled during 0.1.104 and the reasoning stands: order is semantic in Glyph,
first match wins, and `check_arm_reachability` enforces it with E0216. A content
key would be stable under insertion and would also lose which arm won, and it is
not even unique, since two arms can cover one variant with different patterns.
The ordinal churns when an arm is inserted above; that is accepted for this one
entity kind, because an identity that cannot express first-match-wins is not an
identity for this construct.

**An arm whose coverage the checker declined to determine is marked, and the mark
sits on the match site.** Three branches of `check_patterns_exhaustive`
deliberately decline: a value-testing object sub-pattern, a non-`Path` `IsType`,
and a top-level literal or object or array pattern over a union. Silence is not
available, because an absent `covers` edge asserts there is no relation and the
checker never claimed that. Flagging the site keeps the cost with the graph
instead of giving every arm a third state that every consumer must handle, and a
coverage question about a flagged site is declined rather than answered
approximately.

**Then the tool that reads it.** For a type, every match site over it and which
variants each one covers. That is the query an agent needs before adding a
variant, and it is the same edges read forward.

**Why this shape and not another.** Adding a variant to a union is the failure
this compiler has hit five times: G139, G141, G142, G143 and G148, each a site
that failed to unwrap `Ty::App` to its base. All five are now fixed, which is
exactly why the instrument matters: the fixes were found one at a time, by
someone noticing. The unwrap surface is 33 sites today.

#### What a design pass measured, and the five decisions it forced

The release description was wrong about its central premise, and the corrections
reshape it. Every number below was measured with an instrumented build over the
whole corpus: 175 files, 37,204 lines, 2,733 match sites, 6,516 arms.

**The checker does not compute what this release intended to retain.** It never
forms a per-arm mapping. `check_patterns_exhaustive` keeps `covered:
HashSet<Ident>`, variant names with no link to an arm, and a set cannot express
first-match-wins. The arm ordinal is destroyed in the recursion, demonstrated on
a constructed program where arms 0 and 2 arrive at depth 2 as indices 0 and 1.
So "keep what is already computed" is not the work. The work is changing those
functions to form the relation at all.

**Three more facts the plan did not have.** There are seven coverage producers,
not three, and they use at least three different catch-all predicates.
`required_variants` returns a display string rather than an identity, and the
corpus holds 11 unrelated declarations named `Command`. Nothing knows which
declaration a match site sits in: `walk_decl` takes a `&Decl` with no index and
no name, and no span-to-declaration helper exists anywhere in the tree.

**Fork 1, decided: the edge is `mentions`, and it is named `mentions`.** What the
checker has is "arm k names variant V at the top level of its pattern". What a
consumer wants is "arm k fully covers V, payload included", and 1,645 of 2,298
module-local top-level edges, 71.6%, are the deferred kind where the checker
never reaches that conclusion. Shipping the mechanical save under the name
`covers` would overclaim on nearly three quarters of the edge set, which is the
exact-or-absent invariant applied to naming rather than to edges. `covers` needs
`check_patterns_exhaustive` to return a per-arm verdict instead of only pushing
errors, and that is a control-flow refactor of the function behind G139, G141,
G142, G143 and G148. Doing it in the same release that first retains the output
means a wrong edge cannot be attributed to either change.

**Fork 2, decided: union-shaped scrutinees only.** The test is not "is this a
match" but "is there a declaration someone could add a case to". A `bool` has no
declaration, nor does a number, a string, or an array, and including them doubles
the site table to answer nothing: 1,699 of 2,733 sites are of those kinds. String
-literal unions are in, all 17 of them, because D30 makes them a declared type
and "what breaks if I add a value" is the same user question.

**Fork 3, decided: a separate `match_coverage(db, file)` query, plus a
project-level fold.** Hanging the index off `Types` costs no extra traversal, and
that is the wrong economy. `TypeMap` is keyed by `(u32, u32)` spans, so any byte
change in a file shifts every span after it and recomputes the whole map; a
coverage set riding on it would be invalidated by a keystroke that changed no
coverage at all. That is the failure 0.1.103 was about, arriving from a new
direction. The extra walk is cheap, the coverage set is 7.8% of `type_map`'s
entry count, and a content-comparable value that backdates on its own is worth
more than one saved traversal. The by-type index spans files, so it needs the
project-level query beside it.

**Fork 4, decided: four states, and the fourth one is named in the answer.**
Exhaustive, has-a-catch-all, declined, and scrutinee-unresolved. 128 sites, 4.7%,
have a scrutinee the checker could not resolve to any type, against 2 that
decline. Reporting those 128 as absent asserts no relation exists, which is
false, and absence meaning "no relation" is the invariant. The tool's answer
carries them as named sites rather than a count: "23 sites must change, and these
4 I could not index" sends a reader somewhere, where a bare number does not.

**Fork 5, decided: unify the predicate, and do it first, alone.** Seven functions
compute "does this site have a catch-all" and at least three rules disagree on
`Pattern::Ident { "Foo" }`. A per-kind flag pushes that disagreement into the
graph where every consumer inherits it, and the point of the graph is that one
question has one answer. It is cheap to fix now and expensive once consumers
depend on it. But it can change diagnostics on code that compiles today, which
makes it the one user-visible change here, so it does not ride along with
anything.

**That splits the release.** 0.1.105 unifies the predicate and nothing else,
with a diagnostic diff across all 31 apps as its acceptance test: if none change,
it was free, and if any change, each is a finding to judge on its own. The
identity plumbing, the retention and the tool follow in 0.1.106, and everything
after shifts by one.

**Not retained, on the evidence:** an unreachable-arm edge. Measured 0
occurrences across 6,516 arms, and structurally so, since E0216 is an error and
the graph is built from code that compiles. An edge whose population is empty in
every well-formed program is a cost with no return.

**One rule to write down before any of it.** The edges are keyed by `DeclKey`,
and no query that produces them may be keyed by a position. The five
position-keyed per-declaration queries are not on this path today and neither the
language server nor the CLI calls them directly, so the churn does not reach the
edge set; adding a per-declaration coverage query for incrementality would
reintroduce exactly the renumbering `DeclKey` shipped to remove.

*Reviewed against 0.1.104.* Re-checked rather than carried forward: the three
coverage functions are still in `assign.rs` and nothing retains their output,
`glyph-db` holds no coverage edge, all five named gaps are marked fixed, and the
`Ty::App` unwrap count is 33 sites out of 71 mentions. That count is the
corrected measure recorded in the 0.1.103 entry, not the older figure which
counted mentions.

### 0.1.104 — Shipped · An entity keeps its name when the file around it changes

The release before this made the MCP tools fast by answering from the compiler's
memo. This one gives the things in that memo names that survive an edit, which is
what every later item in the graph work encodes.

**R1, stable identity.** `SymbolId` is the only id the resolver has, and a match
arm is addressed by its position in a list: `assign.rs` navigates to the
`arm_idx`-th arm and panics if the shape is not what it expected. Inserting a
declaration above another renumbers it, and inserting an arm renumbers every arm
after it, so any edge recorded against those numbers is invalidated by an edit
that had nothing to do with it. `TypeId`, `ModuleId` and `ScopeId` beside
`SymbolId`, and an arm keyed by the variant it covers rather than by where it
sits.

**R3, the boundary as a node kind.** There is no distinction anywhere between a
declaration Glyph owns, one a hand-written `extern` shim asserts, and one that
came out of a `.d.ts` nobody can check. Without that distinction the graph's
central promise fails on the first npm import: an edge either exists or it does
not, and "the analysis stopped at the TypeScript boundary" has to be
representable as something other than absence.

**R6, examples bound to what they document.** `collect_tests` gathers `@example`
blocks by scanning; nothing ties one to the entity it is an example of. An agent
that wants to know how a function is meant to be called cannot ask.

#### What a design pass measured, and where this plan was wrong

The premise holds and is worse than stated, but three of the four items were
described inaccurately, and one of the corrections changes the scoping.

**Positional identity breaks, demonstrated rather than argued.** Inserting
`fn audit` above `charge` moves `charge` from `SymbolId(1)` to `SymbolId(2)`.
The renumbering reaches into lowered type values: with `fn charge(m: Money) ->
Money` byte-identical across the edit, inserting an unrelated `type Tax` above
`type Money` changes the return type from `Named { symbol: SymbolRef(0) }` to
`SymbolRef(1)`, so a `Ty` for an untouched declaration compares unequal. It
costs measured work too, because every per-declaration salsa query is keyed by
`decl_idx`: on a 30-declaration file, appending re-executes 0 of 30 `decl_ty`
queries, inserting in the middle 15, and inserting at the top all 30.

**The externally visible identity is worse than `SymbolId` and fails silently.**
The MCP tools address entities by line and character. Against 0.1.103, asking
for references at line 7 returned `charge` and its one call site; after
inserting `fn audit` above it, the identical request returned `audit`, with no
error and a well-formed answer. That is the graph's invariant broken from the
caller's side: an agent holding a coordinate gets an exact-looking answer about
a different entity.

**Correction one: `collect_tests` is not a text scan.** It iterates
`module.items` and reads each declaration's `annotations`, so the AST already
binds an `@example` to its declaration. What it does is discard that binding by
flattening to a vector of pairs, which is why a failure reports `example #1`
rather than naming `triple`, and why adding an example to an unrelated function
renumbers the label. R6 is not building a binding; it is not throwing one away.

**Correction two, and it changes the scope: production code holds no arm
identity at all.** The `arm_idx` addressing cited above is inside
`#[cfg(test)]`, a test helper. There is no arm id, no arm index, nothing keyed
by arm position outside one emitter special case. So this item is not replacing
a wrong key, it is introducing an identity that does not exist, which is larger
and more open than the plan assumed.

**Correction three, and the most useful finding: the design is already in the
tree three times.** `Ty::Imported { module, name }`, `SymbolTarget::Global
{ module, name }`, and `ModuleExports`'s name-keyed set all identify an entity
by module and name rather than by an id, each discovered independently. The
`Ty::Imported` doc says why: a foreign module's symbol ids index an unrelated
symbol in the consumer's table, so carrying one is a live mis-resolution bug.
R1's job is to name that convention, not to invent one.

**A flat name key is sufficient, and adding `kind` to it would be a mistake.**
Glyph's module namespace is single and flat: `fn Foo` beside `type Foo`, a
variant beside a function of the same name, `const NAME` beside `fn NAME` are
all already rejected as duplicate declarations, hoisted variants included. So
`module::name` is unique across every declaration kind, `kind` buys no
uniqueness, and putting it in the key would make changing `const Limit` to
`fn Limit()` a different entity when it is the same one renamed in shape only.

**The blast radius is smaller than 61 grep hits suggests.** One site creates an
id. Two compare ids, and both compare a carried id against a fresh by-name
prelude lookup, so both already work name-mediated. Three loops depend on ids
being dense and cost nothing if `SymbolId` is preserved alongside the new key.
The rest is pass-through and doc comments.

*Reviewed against 0.1.103.* Every number above was measured against this tree
with harnesses outside the repo, and the MCP demonstration ran through the real
stdio protocol against the published 0.1.103 binary.

#### The six forks, decided

**Locals are not in the graph, so `ScopeId` is not in this release.** A
reference to a local resolves to a byte offset and nothing is interned, so
naming locals means building a scope tree from nothing. The questions this line
of work exists to answer are about declarations: what breaks if I add a variant,
where is this type used, what does this function's contract say. None of them is
about a `let`. Deferred without a date, because the case for it has not been
made rather than because it is expensive.

**`TypeId` is not a separate newtype.** A type is a declaration, the flat
namespace makes `module::name` unique across kinds, and a newtype wrapping the
same content under a second name is precisely the shape that cost 0.1.56 and
0.1.57. If a type ever needs an identity distinct from its declaration it will
be because the runtime descriptor and the emitted brand need to share one, and
that is R2's problem to raise with its own evidence.

**An arm's identity is the pair of its match site and its ordinal, with the
covered variant as an attribute.** Order is semantic in Glyph: first match wins,
and `check_arm_reachability` enforces it with E0216. A content key would be
stable under insertion and would also lose which arm won, and it is not unique,
since two arms can cover one variant with different patterns. So the ordinal
stays, and the churn it causes on insertion is accepted for this one entity
kind. This is the opposite call from the one the requirement suggested, and the
reason is that an identity which cannot express first-match-wins is not an
identity for this construct.

**An arm whose coverage the checker declined to determine is marked, and the
mark is on the match site.** Three branches of `check_patterns_exhaustive`
deliberately decline. Silence is not available: an absent `covers` edge asserts
there is no relation, which the checker never claimed. Flagging the site rather
than giving every arm a third state keeps the cost with the graph instead of
with every consumer, and a question about a flagged site is declined rather than
answered approximately.

**Origin is an attribute of the node, not part of its key.** A declaration's
identity must not change when the same declaration arrives through a different
route, which is the G75 lesson: identity that varies with spelling was worth two
releases of fixes. So `glyph` / `extern` / `opaque-ts` is a field, and the key
stays `module::name`.

**This release ships the key and the module id, and nothing else.** Name the
convention already present three times in the tree, add `ModuleId` and the
declaration key, and stop. Arm identity is R4's foundation and belongs with R4
rather than ahead of it; binding examples to their declaration is small and can
ride along; boundary kinds need the origin question answered where the
generator already knows the answer.

### 0.1.103 — Shipped · MCP answers from the compiler's own memo

#### Decisions taken before the code was written

A read-only design pass measured this release instead of reasoning about it, and
the numbers settled three questions and raised two. Recorded here because the
first of them would otherwise have shipped as a bug.

**What a call actually costs.** Against the 175-file examples tree, through the
real MCP stdio protocol: `glyph_references` 169 ms to return three locations,
`glyph_symbols` 142 ms, `glyph_diagnostics` 0.7 ms, `glyph_hover` 0.6 ms. The
win is concentrated in the two whole-project tools; the single-file ones are
already fast and are not worth caching.

**Freshness is not a tradeoff, because exactness is nearly free.** Walking the
tree costs 4 ms warm, reading every file 5 ms, hashing 7 ms, against a 169 ms
baseline. So the MCP server re-reads and content-compares every file on every
request, unconditionally. No file watcher: a dependency, a thread, and
platform-specific missed-event modes to save 5 ms. No mtime heuristic: one-second
filesystem granularity, same-second rewrites and `git checkout` of an older tree
buy a class of silent wrong answers to save 4 ms. The residual race, a write
landing between the read and the query, exists identically today.

**salsa 0.28 does not backdate input writes, and this is the finding that
matters.** Proved with a throwaway crate at the pinned version: `set_text` with
byte-identical text bumps the revision and forces re-execution. So the obvious
refresh loop, re-reading every file and calling `set_text` on each, would have a
zero percent hit rate forever, paying the full analysis plus new IO and being
slower than the code it replaced while looking like a cache. Calling `set_text`
only on a real content difference is load-bearing, not an optimization, and the
same applies to `set_project`. Two tests are required rather than optional: one
proving a file rewritten behind the server's back is seen, one proving a repeat
call with no change executes zero queries.

**Fork 1, decided: the MCP moves onto the query layer this release; the language
server does not.** They disagree about what is authoritative. The server's truth
is the unsaved editor buffer arriving through `didChange`; the MCP server's is
the bytes on disk, and `SourceFile` has one text field. Letting disk win would
overwrite a dirty buffer with stale bytes, which is actively wrong for the
editor. A dirty-buffer overlay is defensible and is arguably what an agent
working beside a human wants, but it introduces a new semantic, that
`glyph_references` may report a symbol which does not exist on disk, and that
deserves its own decision rather than arriving as a side effect of a caching
change. The free functions are shared now; the database is not. The cost is one
release in which two caching stories coexist.

**Fork 2, decided: one database per project root, and `glyph_symbols` keeps its
current behaviour.** A single database over the whole MCP root would merge every
project's module namespace into one `ProjectFiles`, which is exactly what D41
exists to prevent and what 0.1.56 and 0.1.57 were spent fixing, so it is not on
the table. Per-root databases follow D41. `glyph_symbols` deliberately spans
project boundaries today, and changing a tool's observable behaviour as a side
effect of a caching change is the wrong way to make that call, so it keeps its
uncached whole-root walk this release.

**That makes memory a new cost, and it gets a bound.** Today's server holds
nothing between calls. A database retains parse, resolve and types for a project
for the process lifetime: 39.5 MB peak for 1.1 MB of source, about 35 times the
source size. `glyph-db` has no eviction of any kind, so an agent walking many
projects would accumulate without limit. The live databases are capped at a small
number, least-recently-used, which bounds the worst case and matches how an agent
works.

**A separate 47 ms that has nothing to do with caching.** `tool_references`
calls `analyze_full`, which runs `assign_types`, and the occurrence scan never
reads the type map. That is 47 ms of the 169, computing and allocating 68,425
type entries across the tree and dropping them unread. Splitting the boundary by
what each query needs is what exposes it, and it is worth taking whether or not
anything else here lands.

*Reviewed against 0.1.102.* Every number here was measured against a binary
built from this tree, not carried forward. The timings come from a driver
speaking the real MCP stdio protocol; the phase breakdown from a harness with
path dependencies on the workspace crates; the salsa input-write behaviour from a
throwaway crate pinned to the same 0.28.2 the lockfile carries. Two claims that
were assumptions until checked: `glyph-lsp` gaining a `glyph-db` dependency adds
nothing to the shipped binary, because `glyph-cli` already links salsa through
`glyph-db` and is the only consumer of `glyph-lsp`; and the comment in
`glyph-wasm/Cargo.toml` claiming salsa is excluded is true again as of 0.1.102
and stays true after this change, since `glyph-wasm` does not depend on
`glyph-lsp`.

The compiler resolves every name, types every expression and finds every
reference, because compiling requires it. Almost none of that is reachable from
outside. This release builds the graph over what the compiler already knows and
puts MCP on top of it, so an agent can ask the compiler a question instead of
searching for the answer.

The invariant is the whole feature and it is not negotiable: every edge is exact
or absent, and absence of an edge means absence of a relation, never "analysis
did not reach here." An answer that quietly stops at the TypeScript boundary is
a green build that proves nothing, which this project treats as worse than being
wrong. The requirements are written up under "Semantic graph requirements
(Q45, R1 through R8)" below, and the constraint that outranks every item in them
is that the graph is a projection of the compiler's model and never a second
parser.

**Route MCP through salsa first, because everything else is cheaper afterwards.**
`glyph-lsp/src/mcp.rs` contains no reference to the database. It calls
`analyze_full` on raw file text and walks `workspace_files` per request, so
`glyph_references` re-analyzes every file on every call: 175 full analyses
against the examples tree to answer one question. Every query worth adding is
multi-file, so each one built on the current path is built expensive and has to
be moved later. One semantic query boundary that LSP and MCP share, over the
tracked queries that already exist (`resolve`, `module_symbols`, `type_map`,
`decl_ty`, `module_exports`).

**Then the graph, in the order the requirements set.** Entity identity (R1)
comes first because every later item encodes those IDs, and match arms key by
the variant they cover rather than by position, so inserting an arm does not
renumber the others. Boundary node kinds (R3) come with it, because the frontier
with npm and with hand-written TypeScript has to be a node kind or the invariant
fails on the first import. Then the exhaustiveness relation (R4), which is the
highest value per unit of cost in the list: the checker computes arm-to-variant
edges and throws them away, and retaining them turns "what breaks if I add a
variant" into a lookup. That is the direct instrument for the bug shape this
project has hit five times, G139, G141, G142, G143 and G148, each one a site not
unwrapping `Ty::App` to its base.

*Reviewed against 0.1.101.* Every claim re-run rather than carried forward, and
one number moved in a way worth noting. `mcp.rs` still contains no reference to
the database at all, so the gating finding holds unchanged, and the
`analyze_full`-on-raw-text calls have grown from three sites to five while the
`workspace_files` walk is unchanged at three: the cost this release exists to
remove is still being added to. All five named salsa queries exist in
`glyph-db/src/lib.rs`. The examples tree is 175 `.glyph` files, unchanged.

The `Ty::App` count needs a correction rather than an update. An earlier stamp
said "39 sites unwrap `Ty::App` to its base", counted against 29 before it. The
honest number today is 33 sites that destructure it to reach a base, out of 71
mentions in total, and the two figures are not the same measure, so the earlier
39 may have been counting mentions rather than unwraps. The direction of the
argument for R4 is unchanged either way: this is the shape behind G139, G141,
G142, G143 and G148, and the surface is in the dozens.

**What the whole arc has to add up to.** A nine-capability target arrived for
this line of work, and most of it is what 0.1.103 through 0.1.105 already commit
to, which is worth stating so the arc is judged against the outcome rather than
per release. MCP becomes the boundary between the compiler's knowledge and an
outside agent (this release). The query surface stays deliberately small, four
questions an agent can chain: type, definition, variants and references. Three of
those four ship today and are easy to mistake for done, because they answer about
a position in a file, come from the language server rather than the graph, and
carry no provenance; making them graph-backed is this release and the next.
Variants does not exist in any form and is the one the rest rests on, which is
another way of saying R4 is the item to protect if the release has to shrink.

Two capabilities in that target are not on this arc and are not scheduled, because
each collides with a recorded non-goal for a technical reason rather than a
scheduling one. They are written up against those non-goals below: tracing data
flow through the program, and verifying value-level semantic invariants. A third,
graph-driven refactoring, is a live scheduling question rather than a design
conflict, and is noted with the v1.1 deferral it belongs to.

**The new MCP tools follow from the graph, not the other way round.** The five
that exist today answer about a position in a file. What the graph adds is the
questions an agent actually gets stuck on: every match site over a type and
which variants each one covers, callers and callees as a first-class `CALLS`
relation distinct from `REFERENCES`, and what a declaration was generated from.
Each answer carries its provenance, so an agent can tell a fact the compiler
proved from one asserted by a `.d.ts` it cannot check.

Deliberately not in this release: the wider query surface (R8). The workload has
not been observed yet, and freezing a query API before knowing which questions
get asked is the same mistake as locking codegen defaults before generated files
proliferate. Thor's agents are the instrument for observing it, because they
already issue every search against this repo and the tick ledger already records
them.

How this gets judged. The fix lane is 59.6% of all agent-minutes; one gap took
four review rounds and about six hours. If these queries help, minutes per closed
gap fall. If they do not move, that is worth learning before the later items
rather than after them.

### 0.1.106 — Planned · The loop closes: an agent repairs an error without searching

Everything before this builds the graph and puts MCP on it. This is the release
where an agent uses it for the thing the work exists for: it hits a compiler
error, asks the graph what the error is about, fixes every site the change
touched, and recompiles. No grep anywhere in that loop.

Three pieces, and none of them is another query.

**Diagnostics stop being prose an agent has to re-parse.** `E0218: missing
variant` is a sentence carrying facts the compiler had and threw away. The
structured form carries what is missing, why it is required, and where the
governing definition lives, as data beside the human text. The rendered
diagnostic does not change; this is an added channel. Without it, an agent's only
route from an error back into the model is to re-parse the file it was just told
about, which is the search this whole line of work exists to delete.

**Queries chain without re-derivation.** Following a thread (this type, its
fields, the functions taking it, their callers, the sites using the result) is
several questions, and each answer has to carry the IDs the next one needs. Built
naively each hop re-analyzes the workspace, which is exactly the cost 0.1.103
removes and would reintroduce one level up. This depends on entity identity from
0.1.104 and is cheap once that exists.

**Then the repair itself, on the bug shape that has hit this compiler five
times.** Adding a variant to a union: G139, G141, G142, G143 and G148 were each a
site that failed to unwrap `Ty::App` to its base. The sites doing that unwrapping
numbered 29 when the graph requirements were written and 39 when last counted, so
the surface grew while the instrument for it sat unscheduled. A repair loop that
handles this shape is not built for a demo; it is the thing we have needed five
times.

**How it gets judged, and the constraint that binds the measurement.** The same
task twice: once with these queries, once with the same agent and the same prompt
using search alone. Define the instrument before seeing the result, publish the
losing run, and never demonstrate a guarantee the compiler does not make. If
minutes per closed gap do not move, that is the finding, and it is worth having
before the wider query surface is designed rather than after.

*Reviewed against 0.1.100.* The five shipping MCP tools were read off the source:
`glyph_definition`, `glyph_diagnostics`, `glyph_hover`, `glyph_references`,
`glyph_symbols`. None is graph-backed, none carries provenance, and diagnostics
are rendered text with a span and no machine-readable payload beyond the code.

### 0.1.95 — Shipped · An imported union is checked whether or not it is generic

**G148, the imported union whose arity switched exhaustiveness checking off.**
A `match` over a union imported from a sibling module could omit a variant
entirely and still build clean, pass `tsc --strict`, and throw at run time.
Delete the type parameter from both files and the same two modules are `E0200`.

The gate in `check_match_exhaustiveness` that sends a cross-module scrutinee to
`check_imported_union_coverage` tested it for a bare `Ty::Imported`. An imported
union at a concrete instantiation (`Tree<string>`) arrives as `Ty::App` over
one, so it matched neither alternative, the coverage check never ran, and the
match went uncounted. The gate reads `union_base(scrutinee_ty)` now, which sees
through one application, so a union's arity is invisible to it. The
named-import and namespace-qualified spellings are both covered; both reach the
gate through the same `TypeExpr::Generic` lowering.

This closes the half of G142 that shipped open in 0.1.91 and said so. It was
parked in the rolling lane alongside G143, and it comes out of it now; G143
stays, because that one is about what the coverage check does inside a variant's
payload once it runs, and this one was about it running at all. The two are
independent and this was the small slice.

It is also the third check written against a bare base that quietly stopped
applying the moment a type parameter appeared, after G141 (a variant's payload
type) and G142 (the module-local variant set). The unwrap those two moved into
`resolve_named_union` is now a free `split_type_app(ty) -> (&Ty, &[Ty])` that
`resolve_named_union` and the imported gate both call, so the distinction is
unwrapped in one named place. That is deliberately smaller than the answer the
rolling-lane note on the emitter's own copies asks for: normalizing `Ty::App`
away centrally would touch assignability and inference, and an exhaustiveness
fix is not where that gets decided. The test that pinned the hole open,
`an_imported_generic_union_is_not_yet_exhaustiveness_checked`, was inverted
rather than deleted, and two more cover the namespace spelling and the case
where an exhaustive generic match has to keep building clean.

`glyph --update` also shipped here. `glyph upgrade` moves a project's pinned
version in `package.json`, and there was no way to move the compiler itself;
`doctor`, told a global install was behind, printed the project command, which
edits a manifest the user may not have. The flag settles which is which: flags
act on the tool the way `--version` and `--explain` do, subcommands act on your
code. It classifies `current_exe()` against `npm root -g` and moves only a
global npm install,
printing the command rather than guessing for an npx cache, a build out of a
source tree, or a path it does not recognise. It also declines to claim success,
because npm exits zero having skipped an optionalDependency it could not fetch,
which is how a release once shipped with no platform binary; it names
`glyph --version` as the check instead. `doctor` points at both commands now
rather than at the wrong one.

Two smaller things. `scripts/check_plans_fresh.py` matched `^#### 0.1.NN` and
the roadmap had moved plans to `###`, so it found nothing and reported "0
unshipped plans reviewed within 5 releases" every run, which reads like a pass
and is a silence. It matches both heading levels now and decides "already
shipped" by comparing the section's version against the current one rather than
by reading the title, because the oldest titles predate every status convention.
With it working it named the 0.1.95 plan as seven releases stale, which is how
G138 and G140 got re-run rather than re-stamped. And `examples/apps/zipper`
checked only that the current focus was a directory before `cd`, not that the
named child was one, so `cd` could walk the shell's focus into a leaf and get
stuck; it rejects a file target with the zipper's own `NotADirectory` now.

The rest of the 0.1.95 plan did not ship. G147, G146, G138 and G140's namespace
half moved to 0.1.96 above, unchanged.

*Dated 2026-08-29. G148 was found by review of the G141/G142 fix against 0.1.90
and reproduced verbatim against 0.1.94 before the fix.*

### 0.1.94 — Shipped · A red-black tree, written in Glyph

Published 2026-08-29 and smoke-tested from a clean npx cache in an isolated
HOME: `--version`, the execute bit on the resolved platform binary, `glyph
init`, `npm install`, `glyph run`, and the headline feature itself. The
leaderboard runs under the published binary: three submits report ranks off the
tree, `top`, `rank` and `range` answer from it, and the boundary rejects `-3` as
`expected Score (int where value >= 0)` and `2.5` as `expected Score (int)`. npx
resolved `@glyphlang/darwin-x64`, the one platform CI cannot execute, at mode
0755. The linux-x64 tarball on the GitHub Release extracts to a mode-0755 binary
and matches its `SHA256SUMS` entry.

No compiler change in this one. What it carries is
`examples/apps/leaderboard/main.glyph`, and the reason to cut it is that the app
compiles. The union work in 0.1.90, 0.1.91 and 0.1.93 was found each time by a
program somebody could not finish writing. This is that program, finished.

It is a speedrun leaderboard over an append-only JSON log. Every command
re-reads the log and folds it into a persistent order-statistics red-black tree
keyed by score, then answers with a walk down the tree: a player's rank, the top
N, or how many submissions fall inside a score range. Each of those is O(log n)
where re-reading and re-sorting the log is not. Every node carries its subtree
size, recomputed by the one constructor helper every other function goes
through, so a size cannot drift from its children.

`balance` is the part that could not be written before. Okasaki's four rotation
cases are four match arms, each one nesting a constructor pattern inside another
constructor pattern's own field, two levels down, over `Tree<K, V>`: a union
whose `Node` payload names `Tree<K, V>` again, generic over both parameters the
union is declared with. Every piece of that was a separate gap, closed one
release at a time. G137 gave an object pattern's field a pattern of its own and G139
carried a nested arm across a module boundary (0.1.90); G141 and G142 stopped a
type parameter on a union from making the nested arm unmatchable and the match
unchecked (0.1.91); G130 and G145 stopped a variant name in payload position
from binding where it should test (0.1.93). None of them was found by asking
whether a red-black tree would compile. They came out of apps that stopped, and
out of reviewing the fixes for the apps that stopped. This one did not stop.

The boundary is one line, `type Score = int where value >= 0`, and the parse
against it names the rule that failed: `-3` is `expected Score (int where value
>= 0)` and `2.5` is `expected Score (int)`, rather than a clamp or a truncation.

What the app does not touch, so that nothing is read into its being green: the
union is declared in the module that matches on it. Move it one import away and
spell the import as a namespace and the same arm is `E0300` (G140), and the
coverage check an imported union gets never looks inside a variant's payload
(G143). Those sit in the 0.1.96 plan above, which is where this release's work
was scheduled before the round produced an app instead. The missing-variant
check on an imported generic union was open here too, and is closed in 0.1.95
as G148.

Also here, from the release audit: the bug-report template suggested
`glyph 0.1.93` after this bump. 0.1.93 had rewritten that placeholder into the
shape `glyph --version` prints, which made it one more string a release has to
carry, and no list a person or a gate reads named it. `check_versions.py` now
checks it the way it checks the home page's hero pill, and its pinned-pattern
table takes a label per entry so the next such string is one line.

*An app round that finished its app rather than stopping at a gap. Nothing was
filed against it.*

### 0.1.93 — Shipped · A nested variant matches instead of binding

Published 2026-08-27 and smoke-tested from a clean npx cache in an isolated
HOME: `--version`, the execute bit on the resolved platform binary, `glyph
init`, `npm install`, `glyph run`, and the headline feature itself. A program
matching `Err(Blank)` beside `Err(e)` prints `blank`, `other`, `ok` under
0.1.93; the same program under 0.1.92 prints `blank`, `blank`, `ok`, because it
emits two `case "Err"` labels and JavaScript runs the first one. Both builds
pass `tsc --strict`, which is why the miscompile shipped green. The linux-x64
tarball on the GitHub Release extracts to a mode-0755 ELF binary and matches its
`SHA256SUMS` entry.

G145 and G130 are one fix. G130 was found while reviewing the G129 fix; G145 was
an app writing a CLI parser whose `match parse(line) { Err(Blank) => .., Err(e) => print(..) }`
swallowed every parse error instead of only the blank lines. `Full(Black)` and
`Err(Blank)` are the same arm under different outer variants: both compiled
clean, passed `tsc --strict`, and emitted two `case` blocks on the *outer* tag
whose first bound the payload under the inner variant's own name. Every value of
that outer variant took the first arm whatever its payload carried, and the arms
below it were dead code the emitter still wrote out. The typechecker had the
right reading the whole time: drop the second arm and it reports a non-exhaustive
match on the payload union, so the checker read `Black` as a variant reference
while the emitter read it as a binding.

`degroup_nested_arms` already rewrites an outer variant carrying nested patterns
into one arm whose payload is dispatched by an inner `match`. What was wrong was
the gate that turns it on: `is_nested_variant_arg` accepted a bare ident only
when it named a *prelude* variant, so `Ok(None)` degrouped and `Err(Blank)` did
not. It now decides a bare ident the way the typechecker decides coverage
(`assign.rs::check_patterns_exhaustive`): the payload union's own variant list
answers first, and the name's shape answers only when that list is unknown.
Neither half is enough alone. Shape alone leaves a lowercase variant
miscompiling, and Glyph accepts one, so `Err(blank)` beside `Err(e)` reproduced
the same duplicate-label switch one character away from the spelling that had
just been fixed. The variant list alone cannot answer for a payload union
imported from a sibling module, which is `Ty::Imported` to the emitter with no
variants attached and miscompiled identically. `nested_payload_variants`, the
type-driven lookup G130's entry proposed extending, is extended: it reads a user
union's payload out of the variant's own declaration as well as the prelude's
type argument, and the gate and the rewrite both consult it through one method
so they cannot part on a name's case.

The switch itself is guarded now. Two arms that would write the same `case` label
are `E0305` rather than a switch whose second label can never run. That shape is
how every miscompile in this class shipped green: JavaScript runs a duplicate
label as first-one-wins and `tsc --strict` has nothing to say about it. The guard
is independent of whatever rule decides that a name is a variant, so the next
lowering bug that reaches for one tag twice stops the build instead of picking
the wrong arm at run time. It also refuses two arms on one tag whose payload
patterns are both plain bindings (`Err(e) => .., Err(other) => ..`), which
compiled before and could only ever run its first arm.

**This rejects code that previously compiled.** `Err(e) => .., Err(other) => ..`
built and ran the first arm for every `Err`. It is `E0305` now. Delete the arm
that could never run, or give the two arms patterns that test different values,
which is what the second arm looked like it was doing.

Seven tests: five in `glyph-emit` pin the emitted lowering for the binding-arm
shape, the two-nullary shape, G130's user-defined outer variant, and the
lowercase spelling of both, one pins the duplicate-label refusal, and
`nested_imported_nullary_variant_dispatches_on_the_inner_tag` in the CLI
integration suite builds it across a module boundary.

What is not closed, in two places. A *lowercase* variant of an *imported* payload
union still has no variant list to read, so the shape rule answers "binding" and
the arm pair stops the build at `E0305` rather than dispatching on the payload's
tag. That is G147, it is a loud failure on a valid program rather than a silent
wrong answer, and closing it means giving the emitter an imported union's variant
names, which is the same missing registry the shallow imported-union coverage
check (G143) needs. And a constructor-shaped payload pattern whose payload type is
known and is *not* a union (`Ok(Point)` on a record) is now a `tsc` error naming
a `.tag` the author never wrote, where it should be a Glyph diagnostic of the
E0220 class. That is G146, and it is an improvement on the silent binding it used
to be rather than a regression, but it is the wrong compiler answering. Both are
scheduled in 0.1.96 above, alongside G138 and G140's namespace half, neither of
which this release touched.

*Found by an app round and by the G129 review. Reproduced against 0.1.92 and
fixed in the same round.*

### 0.1.89 — Shipped · A bool binding you can match on

Published 2026-08-26 and smoke-tested from a clean npx cache in an isolated
HOME: `--version`, the execute bit, `glyph init`, `npm install`, `glyph run`,
and the headline feature itself.

G136, the one an application stopped on. A `bool` binding could not be matched:
`let done = false` followed by `match done { true => .., false => .. }` built
clean in Glyph and then failed `tsc --strict` with
`Type 'true' is not comparable to type 'false'`. Glyph does not narrow a binding
by what was last assigned to it; TypeScript does, and its `boolean` is the union
`true | false`, so the emitted `switch` discriminated on a type the author never
wrote. The app that found it was bridging a `std/timers` callback into a value,
which is the only way to turn an event-based stdlib API into one while Glyph has
no `Promise<T>` and `std/task` has no callback-to-promise bridge; the callback
turned out to be incidental, and four lines with no callback reproduce it. D30
string-literal unions fail the same way, in a `let`, in a `mut`, and in an
equality (TS2367 there rather than TS2678). Fixed by re-asserting the value's
own type at the read: the `match` scrutinee temporary, and either operand of a
`==`/`!=`. Both pins are decided by the operand's own type and ignore what sits
next to it, so `done == failed` between two `bool` bindings is covered the same
way `done == true` is. The assertion re-states the type the checker already gave
the value, so `m == "nope"` still errors, and now names `Mode` instead of
`"fast"`; because `as` permits a downcast it is not a no-op, and a model that
has drifted far enough to matter surfaces as TS2352 rather than TS2678.
Asserting at the write instead would have been shorter and was rejected on
purpose: `"nope" as Mode` type-checks where `let m: Mode = "nope"` does not,
and D30 leaves that membership check to `tsc` by design. One spelling is still
uncovered: a `bool` alias read through another module (`pub type Ready = bool`,
then `let r: catalog.Ready = false`), since the emitter walks an alias body
only in the module it is emitting. Cross-module string-literal unions are
covered in all three import spellings, because the checker hands those over as
a literal set.

Also here: `pulse`, an HTTPS uptime monitor, and the first app in the examples
tree to use `std/dns`, `std/tls` and the `std/net` socket event API together. It
resolves each target, dials a certificate-verified connection, writes an
HTTP/1.1 request by hand and reads the status line back off the socket's
callbacks, classifying every check as `DnsFailed`, `TlsFailed`, `TimedOut` or
`Responded` and appending one JSON line per check to a history file. Turning
those callbacks into a value an `async fn` can await is where G136 came from,
and the app still carries the shape that provoked it: a shared
`Option<CheckOutcome>` set by whichever callback fires first and polled with a
short sleep, because Glyph has no `Promise` a program can construct by hand and
`std/task` has no callback-to-promise bridge.

G130 did not ship here. It shipped in 0.1.93 above.

### 0.1.88 — Shipped · A variant's shape comes from where it is declared

Published 2026-08-26 and smoke-tested from a clean npx cache in an isolated
HOME: `--version`, the execute bit, `glyph init`, `npm install`, `glyph run`,
and the headline feature itself.

G132. The emitter decided whether a match arm's variant carried a record payload
by looking the variant name up in the *consumer's* symbol table. Through the
namespace spelling, `outcome.Failed(d)` bound `d` to `__m0.value` on a payload
whose runtime shape is flat, so `.value` never existed. It now asks the
scrutinee's own declaring module, which is the rule G75 settled: a type's
identity comes from where it is declared, never from how the consumer happened
to spell the import. The precise branch runs *before* the by-name heuristic,
because behind it the heuristic still won whenever two modules declare the same
variant name with different payload shapes.

G134. A variant carrying more than one positional field reported a sentence that
permitted one payload and then flagged a variant carrying one, with the span
shrunk to the first field. Worse, a plain typo in the payload tail
(`Node(int, 5, int)`) was reported as the arity error rather than as the syntax
error at the `5`. The tail now propagates the real parse error, so E0010 is
reached only when every field read as a type, and its help is built from the
author's own field types rather than a fixed string that named a different
program's variant.

That E0010 message is the enforcement half of a decision, not a new one.
`docs/manifesto.md` already says "named records over positional tuples" under
the abstraction pillar, and the parser already behaved that way; what was
missing is that it said so badly. A six-position `Node(Black, l, x, b, h, r)` is
the argument-swap bug the language exists to prevent, and position 3 tells a
reader nothing.

Also here: `watchrun`, a dev-loop tool that polls for changes, matches them
against globs from a real npm dependency, debounces, spawns a subprocess, and
streams its output to a log while enforcing a timeout. It blocked twice before,
on G125 and G126, and this is the round where it built with no workaround.

G130 did not ship here. It shipped in 0.1.93 above.

### 0.1.87 — Shipped · The exit code a program recorded

Published 2026-08-25 and smoke-tested from a clean npx cache in an isolated
HOME: `--version`, the execute bit, `glyph init`, `npm install`, `glyph run`,
and the headline feature itself.

G131. `std/process.set_exit_code` records the code the process leaves with, and
its doc comment describes exactly the program that computes a verdict inside
`main` and records it rather than returning it. That program exited 0. The
entrypoint `glyph run` generates finished its success path with
`process.exitCode = typeof code === "number" ? code : 0`, run unconditionally
after `main` returned, so a `main` declared `-> void` had its recorded code
written back to 0 with no diagnostic anywhere: no E-code, no warning, no `tsc`
error. A batch CLI that rejects its input and records 1 reported success to its
caller.

The wrapper now assigns only when `main` returned a number, so a void `main`
leaves the recorded code standing, a numeric `return` still wins over an earlier
`set_exit_code` (the return is the later verdict), and a program that records
nothing still exits 0 because Node reads an unset `exitCode` as 0. Three
run-level tests cover the three outcomes and a unit test pins the generated
line.

### 0.1.86 — Shipped · A variant name in a pattern matches, or the compiler says so

Published 2026-08-25 and smoke-tested from a clean npx cache in an isolated
HOME: `--version`, the execute bit, `glyph init`, `npm install`, `glyph run`,
and the headline feature itself.

G129. A pattern naming a variant inside a record field bound the field to a new
name that shadowed the constructor, so `Full({ color: Black, label: l })` fired
for every `Full` and the arms below it were unreachable, with no diagnostic and
`tsc --strict` passing. `ObjectPatternField.binding` is an `Option<Ident>` and
not a `Pattern`, so there was no slot to lower a nested match into; the fix is
`E0009` naming the field and pointing at the form that works.

**This rejects code that previously compiled.** A pattern like
`Full({ color: Black })` used to build and silently take the wrong arm. It is
now an error. Rewrite it as `Full(f) => match f.color { ... }`, which is what it
was always doing wrong.

Two gates came out of the release ceremony catching what they should have caught
earlier. `check_docs_compile.py` skipped `web/versions/` entirely, which is why
the 0.1.85 notes published a `tls.connect` call that does not exist; it now
checks the entry carrying the `Latest` badge while still skipping the history,
since a 0.1.3 note documents 0.1.3. And `check_versions.py` now checks the home
page's version pill, which had advertised v0.1.72 for thirteen releases, with
`bump.py` taught to update it, which is the half that would have let it drift
again. `check_plans_fresh.py` was also demanding a review stamp on the 0.1.81
plan for a release shipped weeks earlier, because the shipped marker lives in a
different heading; it cross-references now.

G130 did not ship here. It shipped in 0.1.93 above.

### 0.1.85 — Shipped · A TLS dial you can bound

Published 2026-08-25 and smoke-tested from a clean npx cache in an isolated
HOME: `--version`, the execute bit, `glyph init`, `npm install`, `glyph run`,
and the headline feature itself.

G127, from `pulse`, a CLI uptime monitor and the first app to import `std/tls`
or `std/dns` since they shipped in 0.1.79. The program's whole shape is "check
each endpoint under a deadline so one bad host cannot stall the run", and the
deadline did not work against the case it exists for.

`tls.connect` handed back nothing until node's promise settled: no handle, no
`AbortSignal`, no way to reach the socket mid-attempt. A peer that accepts the
TCP connection and then sends nothing never finishes the handshake, so that
promise never settled and there was a third outcome next to `Ok` and `Err`,
which was *never*. The abandoned attempt held a libuv handle, and node exits
when its handles are gone, so a program whose last act was such a dial printed
its final line and then had to be killed.

Racing a `timers.sleep` against it does not bound it. That is the idiom
`examples/apps/resilient/main.glyph` documents, and `task.race` leaves the
loser running, which is survivable when the loser holds a socket you can close.
Here it never produced one. The bound has to live on the dial, because the dial
is the only code that can reach the socket and destroy it.

So `connect` takes a required `timeout_ms`, measured from the call rather than
from the start of the handshake. It is required, and 0 is an `Err` rather than a
way to ask for no bound: every TLS dial crosses a network, and a bound nobody
passes is a bound nobody has. An optional trailing argument was not available in
any case, since that is the one shape the stdlib signature table cannot model
(G52).

Both ends of the range are refused, which the first cut of this got wrong.
`setTimeout` clamps a delay past 2147483647ms to *one millisecond* instead of
rejecting it, so a dial asked to wait 35 days failed after a millisecond and
reported `no TLS handshake within 3000000000ms`. A release named for putting a
bound on something does not get to check one end of the bound, and the wrong
answer delivered confidently is worse than the hang it replaced: `int`
arithmetic reaches the limit as `days * 86400 * 1000`, with no suspicious
literal anywhere. Neither refusal wears the `host: ` prefix that the real
network failures carry, so a caller logging the string cannot mistake a
programming error for an endpoint being down.

**A scalar argument rather than an options record, decided rather than
defaulted.** `std/http` solved the same problem with a `Fetch` record, and its
own comment gives the reason: an optional trailing parameter is the shape the
checker cannot model. TLS will grow options too, ALPN and a CA override and a
client certificate and a minimum version, and each one breaks this signature
again. The scalar wins here anyway because a record weakens the one property
this change exists for: a field can be defaulted by whatever constructs the
record, and nobody may get a dial without a bound. Revisit when the second
option actually arrives.

This is a breaking change to a stdlib signature. It has one caller in the tree
and two lines of reference documentation, and pre-1.0 is when a dial that can
hang forever gets a bound rather than a deprecation. An existing caller sees
`[E0213] wrong number of arguments: expected 3, found 2`, which does not mention
a deadline, because the signature table stores only arity and a return type,
with no parameter names or types to print.

The test that pins it starts a listener that accepts and stays silent, dials it,
and requires the whole `glyph run` process to exit on its own. Removing the
`setTimeout` makes it fail with "the program never exited". Getting there needed
a bounded spawn helper: `glyph run` starts node as a grandchild holding the same
pipes, so killing only the CLI leaves the pipes open and the reader threads
blocked, and the first version of the helper turned a hung program into a hung
test run. The same run asserts both refusals, since a clamped deadline passes
every test that only checks the happy bound.

Not closed by this, and named so it is not assumed: the deadline's process-exit
half is tested for the socket phase only. A dial whose deadline passes while a
name is still resolving has nothing for `destroy` to reach, and no test in the
tree covers it, so the documentation now claims the socket phase rather than the
whole attempt. G128, below, is the sibling finding: `std/http` ships the
permissive default this release argues against.

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
- **The Vite embedding seam** (M). From glyph-kanban (G122, G123). The alias
  half shipped in 0.1.81: emitted `std/*` specifiers are relative, so a stock
  Vite scaffold compiles and bundles generated output with no tsconfig or
  vite-config edits, and the deployment guide documents the hybrid layout.
  What remains is the dev loop: no watch mode (G123), so a domain change still
  means rerunning `glyph build` by hand next to a UI that gets HMR on save.
  The candidate shapes are `glyph build --watch` on the CLI or a Vite plugin
  whose remaining job is rebuild-on-change. *Done:* a stock Vite React scaffold
  sees a `.glyph` domain change without rerunning `glyph build` by hand.
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
- **`examples/apps/tasks/main.glyph`** — a persisted task API: `std/sqlite` for storage,
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

### The gate between 0.1.x and 0.2.0: the agent accuracy benchmark

The 0.1.x line does not become 0.2.0 on a feature. It becomes 0.2.0 when the
manifesto's central claim has been measured, on a controlled experiment, and the
result is published whichever way it comes out. Decided 2026-09-11, after
0.1.121 shipped the first of the three agent-surface priorities: the remaining
0.1.x items are the ones the experiment depends on, so they finish first, and
the experiment runs once, against one pinned release, when they have.

The experiment, in the form it has to take to count:

- **Two agents, one model, one prompt.** A TypeScript agent with `tsc --strict`,
  an editor's language server, grep and file reading; a Glyph agent with the
  compiler, the nine MCP queries and `glyph query`. The tooling is the only
  intended difference. Neither agent is told where anything is.
- **One codebase, written twice.** A realistic multi-module program of 50 to
  150 files and 10k to 30k lines, with records, unions, imported unions,
  aliases, interfaces, `Option`, `Nullable`, JSON boundaries, an opaque
  TypeScript dependency and deliberately non-obvious dependencies. The
  TypeScript side is idiomatic TypeScript written by hand, not our emitter's
  output, and a shared black-box conformance corpus proves the two programs
  equivalent before any task is written.
- **At least 30 tasks in ten categories** (change propagation, signature
  changes, type changes, union payload changes, aliases, cross-module
  refactoring, bug diagnosis, JSON boundary changes, interface changes, dead
  code), three difficulty levels, at least ten at the deepest, plus a set where
  `tsc --strict` accepts the program and Glyph rejects it. Four flagships: add
  a variant to a union the codebase matches on everywhere; change the signature
  at the end of a call chain; rename a variant whose name text search finds in
  the wrong places; fix a bug tsc cannot see.
- **The oracle exists before the first run.** Per task, a hand-written list of
  the sites that must change, a reference patch per side, and hidden black-box
  tests the agents never see. Missed sites, incorrect edits and unnecessary
  edits are computed by diffing an agent's patch against that oracle, by an
  evaluator that does not know which agent produced it.
- **Repeated runs and paired statistics.** Every task runs several times per
  agent (the plan says five; 300 runs), from a clean checkout each time, with
  full transcripts, tool calls and compiler output stored. The report gives
  pass rate, missed sites, incorrect edits, repair iterations and time to green
  with means, quantiles, a paired test and confidence intervals. Query counts
  and token counts are diagnostic, never the headline.
- **The failures are published**, in both directions, with the transcript and
  the patch. If TypeScript wins, that is the finding and it goes out.

What comes first, and why the gate waits on it: 0.1.122 (diagnostics as a
repair protocol, `glyph_dependencies` and `glyph_exports`, G227 and G228) and
0.1.123 (the knowledge surface generated from the compiler) are the rest of
the agent surface the Glyph side runs on, and the ledger's open verifiability
entries are the holes the runs would otherwise find first. Running the
experiment on a surface that is still moving would measure the wrong release.

What the gate does not decide: the model both agents run on, the harness, the
domain of the twin codebase and the budget. Those are set when the run is
scheduled, and the 0.3.0 study below folds into this one.

*Done:* the report exists, with its raw dataset, against a named 0.1.x
release, and answers with data whether an agent that can ask the compiler
makes fewer wrong engineering decisions than one relying on text search and
`tsc`. Then 0.2.0.

### 0.2.x — Prove it (the evidence gate)

One CLI dogfood app (`examples/apps/fridge/main.glyph`) is not enough to bet a project
on.

- **A second real app with persisted data on a real DB client** (L), no wrapper.
- **A persisted React app** (L) exercising Context, a custom hook, and a real form
  library, since React is in scope for 1.0.

*Done:* real apps built and kept on the shipped interop path, at least one backed
by a database and one a real React app.

### Settle the productivity claim (milestone 0.3.0)

- **One honest agent study** (M): folded into the agent accuracy benchmark
  above, which is now the gate to 0.2.0 rather than a 0.3.0 milestone. Either it
  backs the manifesto's claim, or the claim stays a hypothesis and the copy
  says so.

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

**G480, `process.exit` returns `never`** (record 189): the `std/process` row types `exit` as `Ty::Unknown` under a stale comment saying Glyph cannot write `never`. Type it `never` (D47), and measure the corpus first: a `match` with an exiting arm then joins differently, which changes what a let-bound or returned `match` is typed as.

**G490, an `extern_ts` value where a string-literal union is declared** (filed with 0.1.176): `extern_ts("\"read\"")` returned as `"read" | "write"` is refused by `string_literal_union_verdict`, though `tsc` accepts it. A false refusal that predates D65; an `extern_ts` value is not judged anywhere else, so it should not be judged here. Measure with the two-binary run.

**`glyph fix` reads E0240's position from a field, not its message** (record 192): the decline names the one reason that applied by matching words in E0240's message, and a test pins each of the six positions. Carry the `MutationSite` on the diagnostic beside `comparator_missing`, the way `insert_before` already carries the repair, and have `fix` match on it, so a reworded message cannot change what `fix` says.

**G500, a `bigint` is typed as `number`** (record 187): `return 5n` under `-> number` and `let y: number = x` with `x: bigint` pass `check --no-tsc`, and `tsc` refuses both (TS2322). Lower the prelude `bigint` and a bigint literal to a type of their own, distinct from `number` as D38 says, and relate it to nothing but itself, `unknown` and `Nullable<bigint>`. Breaking under `--no-tsc` only; measure the corpus with the two-binary run. E0243's help for a `bigint` array with no written type follows from it.

**G507, a member read on a `bool` is checked** (record 196): `b.nope` on a `bool` passes `--no-tsc` while `n.nope` on a `number` is E0210. After 0.1.182, which adds the `boolean` apparent-member list to the generated table.

**G419, `__glyph_pairs` hands `tsc` `any` bindings for an untyped iterand** (record 146): a two-binding `for` over an `extern_ts` or untyped npm value binds `any` for `tsc` and nothing for Glyph's checker, so `let s: string = i` passes and throws at run time. Type the helper's pairs `unknown`, or bind both names `unknown` the way a declared `unknown` iterand does since 0.1.158, and measure the corpus for new refusals.

**G417, hover inside a template string is one column off** (records 142 and 143): on `"a ${i}"` hover answers `number` at the `{` and the literal's `string` at `i`, so an agent asking through MCP reads the wrong type. Fix the interpolated expression's recorded spans. Fixed by 0.1.164 (G435); confirmed against 0.1.169.

**G418, a scrutinee whose type is not known hovers as an arm's narrowed type** (records 142 and 143): `match q { is string => q, .. }` on an omitted, context-free lambda parameter hovers `string` at the scrutinee, because the `is` narrowing is recorded on the scrutinee's own span. Record it on the arm's uses only.

**G420, hover on a `for` binding inside a `match` arm answers the arm's type** (record 147): hover on `i` in `for i, x in v` inside an arm returns the arm block's type, where a use of `i` hovers `unknown`. Record the binding's own type on its span.

**G422, a refused callback erases a result its argument does not decide** (record 149): `pick<T>(x: T, f: fn(T) -> void) -> T` with an async callback hides the E0204 on its result until the E0211 is fixed. Erase only a type parameter bound solely through the refused argument.

**G397, an `Option` pattern against a scrutinee that is not an `Option` passes Glyph's own checker** (record 122): `match i.code { Some("missing") => .., Some(c) => c, None => .. }` on `Issue.code` passes `--no-tsc` and only `tsc` refuses it, and on a plain `string` the same arms draw only E0218 for the missing catch-all. Same family as G381 and G392: the editor, MCP and the applyEdit gate miss it. Not breaking under full `check`.

**G383, renaming an import says only "expected `}`, found As"** (found 2026-11-19): the message should say Glyph keeps every name as declared and give the two spellings that work.

**G382, `string + number` passes Glyph's own checker** (found 2026-11-19): `input + 1` on a string is accepted where a `number` is declared and only `tsc` refuses it. Refuse the mixed `+` or type it as `string`.

**G396, a refinement's run-time issue prints emitted TypeScript, not the predicate** (record 120): `type NotA = string where value != "a"` rejects `"a"` with `expected NotA (string where !__glyph_eq(value, "a"))`, which greps back to nothing the user wrote. Embed the predicate's formatter printing in the descriptor's message, the way 0.1.152's E0300 help already prints it. Not breaking; it can ride with G375, which also changes `Record` issue text.

**G393, Glyph's checker reads an optional field as its type** (record 119): `o.timeout_ms * 2` on an optional `number` field passes `--no-tsc` and only `tsc` refuses it (TS18048), so the editor, MCP and the applyEdit gate accept it. Same family as G381 and G392; it may share their release. Since 0.1.160 a help computed from an optional field's type inherits the missing optionality (record 151, S3), which raises its priority.

**G409, a `regex` pattern built from a run-time value is accepted** (record 129): `regex.matches(term, text)` with a user's `term` reads it as pattern syntax (`regex.matches("a.c", "abc")` is `true`, and `(a+)+$` is a ReDoS). D52's constant rule fits the pattern parameter, but a pattern that has to hold user text needs a `regex.escape` to exist first, so the two land together. Breaking when it lands.

**G411, E0239 rewrites a quoted literal holding more than a value as one parameter** (record 132, ruling 3): `'%${name}%'` becomes `?` with `"%${name}%"` and `'${first} ${last}'` becomes `?` with `"${first} ${last}"`, where today the help is prose. Not breaking; it rides with any later E0239 change.

**G410, `http.html` sends its body unescaped** (record 129): `http.html(200, "<p>${name}</p>")` with a request's `name` is an XSS. An HTML body is dynamic by nature, so a constant rule is wrong; it needs an escaping builder that interpolates text as text, designed on its own.

**G402, `record.set`'s value is not checked without `tsc`** (record 126): `record.set(r, "a", "oops")` on a `Record<Tier, int>` passes `--no-tsc` and only `tsc` refuses it (TS2345). The slot stays `unknown` until the unifier joins candidates, since binding it to the already-bound `V` would refuse literals `tsc` accepts. Same family as G393; it rides with the release that teaches the unifier to join.

**The TypeScript floor gate installs its `@types/node` by version, not by lockfile bytes** (record 128): `check_runtime_ts_floor.py` reads the `types-node-ts4.9` alias's version from `.github/ci-tools/package-lock.json` and installs it from the registry. In CI, after `npm ci`, point `typeRoots` at `.github/ci-tools/node_modules` with `types: ["types-node-ts4.9"]` so the integrity-checked tree is what pass 2 reads; keep the cache install for local runs.

**G381, a member read on `unknown` passes Glyph's own checker** (found 2026-11-19): `data.usr.name` on a parsed `unknown` is caught only by `tsc`, so the editor, MCP and the applyEdit gate miss it. Pairs with G325 (planned), which refuses assigning an `unknown`; it may belong in the same release.

**G368, the bare import a namespaced generic relies on is reported unused** (record 111): with `import model { Page }` and `import model as m`, `json.parse<m.Page<int>>` validates, but `Page` draws E0106 and `glyph fix` deletes it, after which the program is E0304. G369, the other half of that entry, is 0.1.150.

**G372, G374 and G375, what 0.1.148's key check leaves** (record 113): Glyph's checker does not read a `Record`'s key type (missing, extra and written keys pass `--no-tsc`, breaking under `--no-tsc` only); a generic record keyed by its own type parameter is refused rather than checked (lift it by passing the argument's member set); and a `Record` check reports the whole map rather than the offending entry.

**G398, an inline record field reports `must be an object` for any inner issue** (record 124): `{"t":{"a":"x"}}` against `t?: { a: int, b?: string }` names the object rather than `t.a`, and since 0.1.152 a `null` inside it gets the same message without the `Nullable` hint. Splice the inner issues with their paths as a named record's `__parse` does, beside G375.

**G399, a generic alias of `Option` or `Nullable` has no run-time check** (record 125): a field `t?: Maybe<int>` under `type Maybe<T> = Option<T>` is E0304 at `json.parse` and `is`. Fails safe; when it closes, the null hint's substitution path becomes reachable and joins the compile gate.

**G400, the null hint suggests a spelling the consumer already has** (record 125): `Nullable<s.SRec>` instead of dropping the suggestion for a sibling's type, through the same compile gate, beside G398.

**G401, `record.keys`'s published signature says `Array<K>` for every key** (found landing G373): the checker answers `Array<string>` for a numeric key at the call site and the runtime spells it as a conditional type, but the row, and so `glyph llms --json`, says `Array<K>`. The signature gate carries it as a known mismatch. Rides with G373b, which settles what a numeric key is.

**G376, a lambda parameter typed `unknown` is emitted untyped** (record 114): closed by 0.1.156's G412, which keeps a written `unknown` and emits `(w: unknown)`; re-run against 0.1.157, full `check` passes.

**G371, an expression statement with no effect is silent** (record 112): `{ k: 2 }`, `"dead"` or `42` on a line of its own exit 0 with no diagnostic. Decide between a lint warning and a refusal of a literal, object-literal or bare-name expression statement.

**G364 and G365, what 0.1.144's walk still says or splits** (record 109): E0304 stops at a nested generic application instead of naming the field inside it, and a sibling alias over a private generic is refused where a record field of the same type is accepted.

**G352 and G353, what 0.1.142's lint pass made visible** (record 104): the playground holding back emit on a type error, as `glyph build` does, and an applyEdit that writes one of an E0200's missing arms counted as progress rather than a new E0200. G351, the third, is 0.1.146.

**G363, only the first E0304 in a module is reported** (record 108), the emitter's counterpart of G341. G362 is 0.1.147.

**G356, a namespaced `is m.T` and `is m.G<X>` lower through the namespace** (record 107; not breaking). `w.P.is(v)` compiles while `is w.P` is E0300, so the same check depends on the import's spelling; lower both through the namespace's module, judged by the walk the pattern already gets.

**G358, `glyph fmt` sorts imports.** The manifesto promised it under diff stability and nothing implemented it. Needs a decision on the order (path order, `std/` first or not) and on what happens to a comment above an import, then a formatter change that is breaking only for `fmt --check`.

**G354 and G355, two generic-descriptor spellings that disagree with the plain one** (record 105): `json.parse<m.Box<int>>` through a namespace is falsely refused where `Box` imported by name validates, and `Page.is<U>(v)` passes the checker and fails `tsc` with TS2554. E0304's message for a union payload reached through a record (`A(Doc)`) reads ``field `value.h` `` where the direct form names the variant, and carries the record note; same lane.

**Type what `map` and `map_err` return on a `Result`** (record 87). 0.1.139 made them the only members; their result is still loosely typed, so the error type after `.map_err(f)` is not compared with the enclosing function's `E` until this lands. Its own release, measured against every `.map_err(f)?` in the corpus first.

**G344, G345 and G346, what 0.1.139's member check does not reach yet** (record 100): a constructor result is not typed as an `Option` or a `Result`, an index, spread, array element or untyped lambda parameter is not checked, and E0210's help on an `Option` method leads to a second E0210. G344 first, since it is the common spelling.

**G343, `glyph fmt` starts a statement with `(` around a lambda call**, which the next pass glues onto the line above, the G335 hazard on a lambda head instead of a try; the fix takes the same rule, never print a `(` that opens a statement. Found by record 99.

**G341, only the first refused comment in a file is reported**, so an agent
learns about E0500s one run at a time; the formatter should carry on past a
refusal and report each. **G342, the playground never shows E0500**, since
`glyph-wasm` does not run the formatter. Both found by record 98.

**G339 and G340, a comment between JSX attributes or between object-pattern
fields is refused** (E0500) where a one-per-line layout would place it, as one
already does for a record body and an object literal. Found by record 97.

**G336, a module-qualified call is laid out as a chain step** (`json` /
`.stringify<number>(1)`), which splits one name across two lines; the head of
a chain should take the whole qualified name. Found by record 96.

**G337, a chain broken inside a JSX attribute value glues the closing `}`**,
the next attribute and the tag's `>` onto its last step. Found by record 96.

**G331, an unclosed JSX element reports the enclosing `}` as JSX text** (E0012
with help to write `{"}"}`), where 0.1.134 said the closing tag was missing.
The error should name the element left open. Found by record 94.

**G332, an entity in a JSX attribute string is emitted as written** where TSX
decodes it. The same divergence record 93 refused in text; deciding it needs a
record of its own (refuse, as in text, or decode). Found by record 94.

**G334, an invisible character outside a string is named raw, and a BOM is
refused.** The message should print `U+XXXX` for a character that does not
show; whether a leading BOM is skipped, as `tsc` does, needs deciding. Found by
record 94.

**G333, a `<` in JSX text is E0002 with no spelling offered.** It should be
refused as text that cannot hold `<`, with `{"<"}` as the help. Found by
record 94.

**G329, a lex error inside a generic-call lookahead becomes an E0003 on valid
code.** `let x = f<a, "\q">(x)` reports an unexpected `,` since 0.1.134, where
the invalid escape was reported before. The lookahead backs off from the lex
error and the comparison reading fails first. A fix reports the held lex error
when the reading a lookahead backed off to fails before it, without making
lookahead failure fatal again (record 85). Found by record 92.

**G328, a body-level `const` is E0008 "assignment requires `mut`".** The
headline describes a reassignment; the reader wrote a declaration. A `const`
in a body should get a message of its own naming `let`. Found by record 91.

**E0211 between two unnamed records names no field.** The record rule
0.1.127 completed produces ``expected `record`, found `record` `` for a
nested mismatch, which tells the reader the comparison failed and not where.
The diagnostic should name the first field path that does not fit
(`o.a: expected number, found string`). Found by the 0.1.127 release review;
it is the most visible cost of that release's new refusals.

**A function's parameters are compared by nothing.** Since 0.1.127 the
relation compares a function value's return, and a callback whose parameter
type is wrong still passes `--no-tsc`. TypeScript compares parameters
bivariantly for methods and contravariantly under `strictFunctionTypes`, so
the rule needs deciding against both before it is written.

**The docs gate skipped the newest release notes' code.** `check_docs_compile`
adds the latest `web/versions` entry so its snippets are compiled, but a block
with no `module` header is filed as a fragment and skipped, so the two 0.1.127
samples, which did not parse at top level, passed the gate. The latest entry
should be held to the strict rule: a fragment there is marked or it compiles.
`check_doc_claims.py --tests N` also accepts the count it is meant to verify;
it should require a finished log that agrees.

**G268's `unknown` cell for a non-scalar value has no test.** The scalar
case (`number` against `unknown` is `WILL_FAIL`) is pinned; a record against
`unknown` answering `UNDETERMINED` with its reason is not.

**G273, an undeclared field on a fresh object literal.** `tsc` refuses
`let u: User = { name: "a", extra: 1 }` (TS2353) and Glyph accepts it, because
the record relation is width subtyping and a type cannot say whether it came
from a literal written at that site. The place to decide it is the
written-literal walk, which sees the literal itself. It is a breaking
verifiability change on its own, so it waits for a release with room to run
both versions over every app; 0.1.127 records it and the assignability reason
names the divergence in the meantime.

**An enterprise reviewer's list, 2026-09-20.** A DevSecOps-minded engineer read
the repository, the tour, the roadmap and the benchmarks, rated the design
strong and the ecosystem immature, and would run Glyph as a controlled proof of
concept but not approve it for a regulated production platform. The review is
on disk in the private feedback folder. Six of its points are gaps this lane
did not carry; each is below with what is already true and what is not.

**The VS Code extension is not on the Marketplace, and the name is taken
nearby.** `editors/vscode/PUBLISHING.md` records it: nobody gets highlighting
or the language server without building the extension from source, and a
Marketplace search for "glyph language" returns `GlyphLang.glyphlang`, an
unrelated REST DSL. The reviewer read the missing listing as the maturity
signal, ahead of anything about the language. What has to be decided before
the first `vsce publish` is the publisher id and display name, distinct enough
from the other project that a search does not land on it, and whether Open VSX
(Cursor, VSCodium) ships in the same step. The listing is one command once the
id exists; the id is the owner's call.

**A scanner's finding on the emitted TypeScript has no path back to the Glyph
line.** Every build writes a v3 `.ts.map` beside each module and `glyph build`
remaps `tsc` errors through it, so the bridge exists. What does not exist is
the workflow a security review needs: a SAST tool (CodeQL, Sonar, Semgrep)
reports `dist/orders.ts:742` in SARIF, and nothing turns that into
`src/orders.glyph:118`. The reviewer's question was which artifact is
authoritative for a security review, and the answer the docs should give is:
the `.glyph` source is what is reviewed and changed, the committed TypeScript is
what scanners read, and the map is the bridge. Two pieces: a `glyph`
subcommand that rewrites the locations in a SARIF file, or a single
`file:line`, onto Glyph source through the maps; and a guide,
`docs/guide/security-review.md`, that says so and states the edge, which is
that the maps are statement-level, so a finding lands on the statement rather
than the token. An answer page beside it, in the format `web/answers/` uses.

**The SBOM and the digest pin are already true and were nowhere written.**
Checked on a `glyph init` project against 0.1.123: the compiler is pinned
exactly as a devDependency, `package-lock.json` carries a sha512 integrity for
`@glyphlang/glyph` and for each platform binary, and `npm sbom --sbom-format
cyclonedx` lists the compiler and the installed platform package with hashes
and purls. The reviewer asked for digest pinning and an SBOM as if both were
open; they are in the lockfile npm already writes. The distribution guide now
says so. Still open: `glyph doctor` could print the installed binary's digest
so a review compares it to the release checksums without reading the lockfile.

**The study's report gains two columns the review asked for.** The 0.2.0 gate
above scores pass rate, missed sites, incorrect edits, repair iterations and
time to green. The review's own list adds two that cost nothing to record and
would be asked for afterwards: patch size against the reference patch per task,
which is the diff-stability pillar measured where it matters, and security
findings on both patches from the same scanner run on both outputs. Both go
into the instrument before the first run, not after; the rule in the gate is
that the columns are chosen before the numbers exist.

**The README and the home page do not lead with the compiler as something an
agent asks.** The review's model of Glyph is the table it drew: no `any`,
`Result`, exhaustive `match`, canonical formatting, fewer tokens. It never
mentions `glyph query`, the MCP server's queries, impact before an edit or the
repair protocol, and its closing concern, that better tooling on TypeScript
might win instead, is the comparison `web/answers/vs-tooled-ts/` and the 0.2.0
gate exist to run. A careful reader of the README reached the end without the
thing the study measures. The README's headings are the language, what the
compiler catches, why not TypeScript, the pillars, status; "ask the compiler"
is one sentence under status. Put it where the token chart is, and the chart
after it.

**Tests and coverage on a Glyph project are documented as `@example` and
nothing after.** The getting-started guide says tests live next to the code and
`@example` runs on every build; a reviewer listing test frameworks and coverage
tools as ecosystem it expects found no answer for a test that needs a fixture,
a mock or a coverage number. The emitted TypeScript runs under any Node test
runner and c8 reads the `.ts.map`, which may be the whole answer, and it has to
be tried on an app under `examples/apps/` and written down rather than assumed.

**The spec's D43 collision is resolved, in favour of the `mut`-across-`await`
rule.** D43 still resolves to that rule and its error code E0225, which is what
the shipped release notes cite it for; `never` as the bottom type is D47, and
its spec entry says where it moved from. `glyph llms --json` reports an empty
`decisions.duplicate_numbers` over 47 decisions.

**A stdlib export the checker holds no signature for still has no kind.** G231
gave every stdlib module key an export surface `glyph_symbol` and
`glyph_exports` answer from. The signature half closed in 0.1.123: the tables
came out of the walk as `stdlib_signature`, and `glyph_symbol` and
`glyph_exports` read them, so `std/array::filter` answers `kind: "function"`
with `fn(Array<T>, fn(T) -> bool) -> Array<T>`, its parameters and its return.
164 of the 327 exports are still in neither table, `std/result::all` and
`std/io::println` among them, and answer `kind: null` with the reason. What is
left is the record shapes: `stdlib_modeled_type` holds one for nine stdlib
types, and `fields` on those is still absent with a reason where it could be
answered.

**G242, G243 and the missing-field message closed in 0.1.125, and two
impact gaps they left.** A dotted JSX element is a component whatever the case
of its base, a component whose one parameter is not a record is E0229, and a
refused record or element names the fields it is missing. G244 came out of the
same work: once `<ui.Button>` was a component, the relations still did not see
it, and they record every element at its name now. G232 (an object literal
has a type) and G240 (a JSX attribute is checked) shipped in 0.1.124, and this
is the half of each they left. What the impact table still owes an element is
in G250 and G251, and both are scheduled in 0.1.126.

*Closed in 0.1.124. Kept as the record of the finding; the ledger entry is the live status.*
**G235. The editor's hover answers single-file where the tool answers from the
project.** `glyph_hover` answers at every name declared in another module since
0.1.122 (G227); `glyph lsp` still answers `null` at the same positions, because
its overlay database registers the buffers an editor opened rather than a
project. Two surfaces giving one question two answers is the G219 principle, and
here it is the editor that is behind. The fix is to layer the open buffers over
the project database `project_file_diagnostics` already builds rather than let
them stand in for it, which is a change to how the server holds state and not a
hover patch, so it is parked here until somebody scopes it.

*Closed in 0.1.122. Kept as the record of the finding; the ledger entry is the live status.*
**G236. `glyph fix` declines the E0200 repair over an imported union.** Fixed in
0.1.122 before the tag, after an adversarial review reproduced it on the release
branch. Both halves shipped: the self-check runs against the project rather than
against the candidate file alone, through `glyph_lsp::file_diagnostics_with_text`,
which puts the candidate into the project database for the length of the call and
never on disk; and the rule writes the variant imports the arms need, or the
namespace spelling (`orders.Paid({ transaction_id })`) when the module is
imported as a namespace. The decline reason names the diagnostic the candidate
drew or the variant it could not bring into scope. Kept here as the record of
where it was found, which was while writing the release notes, after the scope
was set.

*Closed; the ledger entry has the resolution. Kept as the record of the finding; the ledger entry is the live status.*
**G156. Named-record identity is inconsistent, and the fork has to be settled
before it is fixed.** `ship(d)` where `d: Draft` and `ship` takes a structurally
identical `Paid` is `E0211`, which is more than `tsc --strict` does. `let p:
Paid = d` then `ship(p)` is clean. So the compiler decides something here and
decides it in only some positions. Either a named record is nominal, and the
`let` and the union constructor should reject too, or it is structural and the
`E0211` is the anomaly. Both are defensible and the present state is not,
because it teaches a guarantee that a reader disproves in one line. It also
happens to be the encoding a state machine must use, since Glyph has no
transition construct and `typestate` is abandoned.

**E0211 between two structural records prints no shapes.** `takes_rec(narrow)`
where the parameter is `{ a: string, b: int }` and the argument is `{ a: string }`
reports `argument type mismatch: expected \`record\`, found \`record\``, which
names neither side. This is the argument 0.1.121 made for printing an
application's arguments ("expected `number`, found `Nullable`" names the
container and drops the thing the reader has to check), one arm over.
`ty_display` should print the field shapes. Present on 0.1.120 as well, so it is
not a regression.

*Closed in 0.1.97. Kept as the record of the finding; the ledger entry is the live status.*
**G157. `spec.md` D44 still calls G143 open.** It closed in 0.1.97. Cheap to
fix, and it sits in the document a sceptical reader opens right after a claim
about what the compiler catches.

**G150 is fixed and G151 is open, both found by the fuzz target within minutes
of it existing.** G150 grew a file by one copy of a comment on every `glyph fmt`
run, without bound: format-on-save grew it for as long as the editor stayed
open and `--check` could never pass. The cause was `raw_args` being verbatim
source, so an argument that does not close cleanly swallows the following
comment, the annotation emits it, and the comment machinery emits it again.

G151 is the same family and milder: the first pass and the second disagree, then
it is stable. It converges, so it is not a growing file, but `glyph fmt --check`
still fails on output `glyph fmt` just produced, which is the one thing `--check`
has to be trusted about.

**The nightly fuzz job will be red until G151 is fixed, and that is correct.** A
green fuzz job with a known open crash in it would be the useless kind of green.
Both are the G23 family: that one moved a comment out of the construct it
documented and its corruption was itself a fixed point, so nothing downstream
noticed. Diff stability is the pillar all three attack.

*Closed in 0.1.99. Kept as the record of the finding; the ledger entry is the live status.*
**G149. Glyph's own checker does not catch a `let` annotation mismatch, and the
editor is silent about it.** `let x: string = 42` produces no Glyph diagnostic;
`tsc` reports `TS2322` at `glyph build` and the help line sends the reader to the
generated `.ts`. Because `glyph lsp` runs the Glyph stages only, with no `tsc`
anywhere in it, the editor says nothing at all while you type: driving the server
against that line publishes one diagnostic, `E0107 unused variable`.

A binding annotated with a type it is not is close to the most common mistake
there is, and an editor catching it is close to the minimum anyone expects.
This is the concrete, cheap instance of Q46's second item, so it belongs here
rather than waiting for that whole track.

The former rolling-lane items (`--out` cleanup, store pattern, `@redact`,
`glyph regen`) are now scoped into 0.1.7 above. New small wins that surface later
land here until they're assigned a release.

- `alternatives` on a diagnostic carries variant names for a tagged union and literal contents for a string-literal union with nothing in the object saying which; `check --agent` reads the kind off `glyph_symbol`'s answer and drops its sentence for an inline union. The field wants a kind (`variants`, `literals`, `fields`, `exports`), which means `TypeError::accepted` carrying it from the construction sites
- On an unmarked tree with a nested module, `glyph check` keys the module from the nearest `src/` while the tools key it from the file's own directory (G180's fallback), so `src/sub/deep.glyph` is `sub/deep` on one surface and `deep` on the other; a `package.json` marker settles it and `glyph init` writes one. The fallback should agree before another surface publishes identities
- **`check_site.py` does not compile the code it publishes.** The site's answer
  pages carry Glyph programs, and nothing checks that they parse. A snippet on
  the verifiability page shipped for one review cycle missing the comma after a
  block-form match arm, on a page whose subject is what the compiler catches.
  The gate passes because it checks structure, links and version strings, never
  source. The awkward part is that many blocks are deliberate fragments (an arm
  or two, a signature with no body) and some are the *rejected* spelling shown
  next to its diagnostic, so a blanket "compile every `<pre>`" would fail on
  purpose-built failures. It needs a marker on the blocks that claim to be whole
  programs, and then the gate builds those. Small, and it closes a class where
  the site is wrong in exactly the way the site says the language is not.

- **Two version-carrying strings had no gate; one still cannot get one without
  changing the release ceremony.** `web/sitemap.xml` gives every page a
  `lastmod`, and twenty-six of the thirty-two entries still said July 2026.
  `/versions/` was one of them, on a page that has been committed to
  seventy-five times since that date. It had also never listed four pages: the
  answers index and the `binary`, `embedding` and `upgrades` answers. Both
  were fixed for 0.1.93, and `check_site.py` now fails when a served page has
  no sitemap entry or an entry points at no page, so the coverage half cannot
  drift again. The dates were the open half, and that is decided: the site
  generator writes `sitemap.xml` and dates each page from a hash of its own
  content, kept in the committed `site/lastmod.json`, so a page's `lastmod`
  moves only when what it says changes and the build needs no git history.
  The other string is the `placeholder` in
  `.github/ISSUE_TEMPLATE/bug_report.yml`, which read `0.1.9` from before
  0.1.10 existed and now reads `glyph 0.1.93`, the shape `glyph --version`
  prints. Pinning it in `check_versions.py` would stop it going stale and
  would add a seventeenth string to bump by hand every release.

- **The cross-module exhaustiveness check is a shallower copy of the local one
  (G143; G142's open half closed as G148).** Two holes with one address, one of
  them now closed. Exhaustiveness on a
  module-local union runs through `check_patterns_exhaustive`; an imported
  scrutinee is diverted in `check_match_exhaustiveness` to
  `check_imported_union_coverage`, which counts the outer union's variants and
  stops. It differs from the local path in two ways that are both silent
  miscompiles. First, the gate that reaches it used to be
  `matches!(scrutinee_ty, Ty::Unknown | Ty::Imported { .. })`, which an imported
  *generic* scrutinee (`Ty::App { base: Ty::Imported, .. }`) does not match, so a
  `match` on an imported `Tree<K>` omitting `Leaf` built clean and threw at run
  time where the non-generic spelling is `E0200` (G142's surviving half; the
  0.1.91 unwrap did not reach it, because an imported scrutinee never reaches
  `resolve_named_union`). **That half is closed as G148**: the gate reads
  `union_base(scrutinee_ty)` now, so the arity is invisible to it. Second, it has
  no equivalent of the payload recursion,
  so `B(X)` over an imported union whose payload is itself a union is never
  checked, and the arm that survives lowers `X` to a binding that shadows the
  variant, so `f(B(Y))` returns the `X` arm's answer (G143, and no type parameter
  is involved). `record_fields_of` already destructures the applied-imported
  shape, which is why the *nested pattern* half works across the import and only
  coverage is blind. What is left here is the depth, not the gate. The open
  question is unchanged and is the one G148 deliberately did not answer: whether
  to deepen `check_imported_union_coverage` in place, or to give an imported
  union a variant set the local path can consume so there is one check rather
  than two. The second removes the class, and is the larger change.
- **G20: a nested string literal inside `${...}` breaks the template parser.**
  The lexer has no template-literal mode, so it ends the outer string at the
  first inner quote, and `"${bytes.to_hex(bytes.from_text("x"))}"` does not
  parse. The diagnostic names the cause and the workaround (hoist it into a
  `let`), which is why the entry reads `[IMPROVED]` rather than open, but the
  limitation is untouched. Worth raising above "v1.1 deferral": it was hit twice
  in one session writing ordinary probe programs, so the frequency is higher
  than the marker suggests. The fix is a real lexer template mode.
- **Three copies of the same `Ty::App` unwrap in the emitter.**
  `union_variant_names`, `variant_payload_is_record`, and `payload_shape` in
  `glyph-emit/src/lib.rs` each open by unwrapping an application to its base,
  and `payload_shape` unwraps and then calls the other two, which unwrap again.
  G139 is what forgetting the third copy looked like: an imported generic union
  fell past every proof and the match was refused. The fourth caller will forget
  too. Two candidate fixes, and the choice is open: one `fn union_base(ty: &Ty)
  -> &Ty` that all three call, or normalise once where the checker records the
  pattern's type so the emitter never sees the application at all. The second
  removes the class rather than the duplication, and is the larger change.
- **`decl_annotations` is written twice.** `mcp.rs` reads a `Decl`'s annotation
  list with a six-arm match, and `glyph-resolver/src/lints.rs` has a private
  helper of the same name doing the same six arms. A seventh declaration form
  has to be added in two places or one surface goes quiet about it. The home is
  an accessor on `glyph_ast::Decl`, which both crates already depend on.
- **G19: no `T?` sugar over `Option<T>`.** A forward-compatible deferral, so
  adding it later changes no existing parse tree. The diagnostic already names
  the fix when someone writes `T?` in type position.
- **The deployment guide has no browser or worker example, and an outside author
  drew the wrong conclusion from that.** Their spec recorded, as settled before
  the build, that "the emitted code uses bare `std/*` specifiers that a build
  step must rewrite." Since 0.1.81 the premise is gone outright: emitted
  specifiers are relative, so any bundler resolves them with no tsconfig at
  all, and the guide's new hybrid-embedding section covers the Vite case. What
  is still owed is the plain-worker walkthrough: a module bundled with
  `esbuild dist/x.ts --bundle --format=esm` yields ESM with zero `node:`
  imports that runs in a bare realm, and no example in the guide shows that
  line. This matters more now that `std/bytes` and `std/url` are deliberately
  host-free.
- **G124: a library module with no `pub` builds green and exports nothing.**
  The Glyph checker sees only Glyph-side importers, so a module written for a
  host TypeScript app (the hybrid shape, D33-era or fresh) can have its whole
  export surface silently private, and the first tool that notices is the
  host's `tsc` with TS2459. The candidate fix is a build-time diagnostic on
  the library shape: no `main`, no `pub`, no Glyph-side importer means the
  module is useful to nobody, and the error can name `pub`. Parked here until
  the diagnostic is designed; the deployment guide's embedding section states
  the `pub` requirement in the meantime.
- **Nothing tells a user their pinned version is stale.** The glyph-kanban
  author evaluated 0.1.3 for a month while 62 releases shipped, and published a
  retrospective describing gaps that had been closed for weeks. An update
  notice from the CLI is a network call from a compiler, which is a policy
  question (opt-out, offline behaviour, CI noise), so this is parked as a
  decision to make deliberately, not a patch. The zero-policy half is doable
  now: the versions page and README could state plainly how fast the release
  cadence is, so an evaluator knows to check.
- **A server cannot be stopped once started** (`std/net` and `std/http`). Both
  `serve` functions resolve `Ok(void)` on close, and neither module exposes
  anything that closes one, so the `Ok` branch is unreachable unless the peer
  process does it and graceful shutdown is unwritable. Found when a probe of
  `std/net` hung with no way to end it. The design question is the shape of the
  handle: `serve`'s awaitable return is what makes a bind failure a value, so a
  second function returning a `Server` would fragment the lifetime across two
  calls. One option is for the connection handler to receive the server; another
  is a `stop` that takes the port. Neither is obviously right, which is why this
  is a note rather than a patch. Sharpens the parked "signals and graceful
  shutdown" item, which cannot be built before this is decided.
- **G128: `std/http` bounds nothing by default**, which is the same shape
  0.1.85 fixed in `std/tls` one file over. `http.get` and `http.post` take a URL
  and no deadline, `fetch_of` and `head` set `timeout_ms: 0`, and `request`
  reads 0 as permission to skip arming the timer. So the default path is an
  unbounded request in a module whose own comments argue that a request you
  cannot bound is not a request you can ship. Reproduced against 0.1.84 with a
  listener that accepts and then says nothing: `http.get` was still pending at
  45 seconds. Less severe than G127 was, and the difference matters: undici
  applies its own 300s header timeout underneath, so this is minutes rather than
  never, and `send` with a `Fetch` already bounds a request properly. The
  default is the bug. The fix is a decision rather than a patch, which is why it
  is parked here: either `get`/`post` grow a required deadline the way
  `tls.connect` did, breaking the two most-used functions in the stdlib, or
  `request` stops reading 0 as permission and `fetch_of` carries a real default.
- **Exhaustiveness over a product of record fields.** An object pattern's field
  takes a pattern as of 0.1.90 (G137), and a field that tests a value makes the
  arm refutable, so it no longer covers the variant it sits under. That is the
  safe reading and the one the checker can prove, but it asks for a catch-all
  where a reader can see there is nothing left: `Node({ color: Red, .. })` and
  `Node({ color: Black, .. })` between them cover `Node`, and E0200 still fires.
  Proving it needs usefulness over a product of fields (a decision tree, in the
  Maranget sense) rather than a set of tags. It accepts strictly more programs
  and rejects none that compile today, so it can land whenever the checker is
  being worked on rather than blocking anything.
- **`is_constructor_shaped` exists three times.** D9's capitalization rule is
  the hinge every pattern decision turns on, and it is copied into
  `glyph-parser/src/pat.rs`, `glyph-resolver/src/resolve.rs` and
  `glyph-typechecker/src/assign.rs`, each with a doc comment pointing at the
  others. The typechecker's copy calls itself "the single predicate shared by"
  the stages, which was already untrue at two copies. All three crates depend on
  `glyph-ast`, so the predicate belongs there, and as of the G137 work it does:
  `glyph_ast::is_variant_shaped` is public, because `Pattern::is_refutable` had
  to answer the same question. The parser's copy is gone with it; the resolver's
  and the typechecker's remain and should call it. Mechanical, and worth doing
  before a fourth stage needs it, since a rule that drifts between stages is how
  the typechecker and the emitter came to disagree in G130.
- **No TLS server** (`std/tls` is client-only). Deliberate, and recorded so it is
  not mistaken for an oversight: a server needs a certificate and a private key,
  which means a file format, a renewal story and a cipher policy, and shipping
  those half-done is worse than not shipping them. Terminate TLS in front until
  an application asks.
- **G105: a file can only be read whole, and there is no async iteration**, so a
  streaming pipeline is unwritable. Re-reproduced against 0.1.78: `fs.open` and
  `fs.read_line_at` are both `[E0105]`, and the runtime contains no
  `asyncIterator`, `AsyncIterable`, `createReadStream` or generator. One clause
  of the original premise has since closed, since `std/bytes` gave `readSync`'s
  buffer a name, but the shim still has no `openSync`/`closeSync`, `position`
  still needs a `null` Glyph cannot spell, and there is no iteration protocol at
  all. `std/stream` is already the property-testing sampler, so a design here
  has to pick another name first.
- **G106: `E0106` calls an import dead when only an `@example` uses it**, which
  contradicts the documented rule that an example must import what it compares
  against, and there is no warning-free spelling. Two independent rounds hit it,
  and an outside author wrote it into their own reference as a known cost of
  using the language. The fix is that the lint should count a reference from an
  `@example` as a use, since the compiler already runs those.
- **G107: in a multi-project build, a `tsc` error is reported against a
  same-named module in a different project.** The message and the line number
  are right and the file is wrong, so someone acting on it opens a file with
  nothing wrong in it and the quoted line looks plausible enough to try to fix.
  It only shows when two projects share a module name, which for `main.glyph` is
  every app in the tree. This is the class 0.1.60 closed for single-project
  builds; the multi-project path kept it.
- **Some declarations in the bundled node shim are still wider than node, all
  the same way.** G125 and G126 were both this bug. `net.Socket.setEncoding`,
  `http.IncomingMessage.setEncoding`, `new StringDecoder(...)`, `Buffer.from(s,
  enc)`, `Buffer.byteLength(s, enc)` and `buf.toString(enc)` take `string` where
  `@types/node` takes `BufferEncoding`, so passing a `string` variable compiles
  without the package and fails with it. The count is deliberately not stated:
  the previous entry said five, arrived at by reading the file, and missed
  `http.IncomingMessage`. A number counted by eye is the argument for the sweep,
  not a substitute for it. `check_runtime_against_types_node.py` now proves every
  exported *name* exists in the real package, which is a different question from
  whether its *shape* matches; the shape sweep is still owed.
  Not fixed in the same change because narrowing a declaration people already
  build on is a change to a guarantee, and it wants its own release with the
  runtime re-checked rather than a drive-by inside a feature.
- **Nothing compares the bundled node shim with `@types/node` declaration by
  declaration.** `check_runtime_against_types_node.py` builds the compiler's own
  runtime against the real package, so it catches a divergence the *runtime*
  depends on. A divergence only user code depends on is invisible to it, which is
  how the five above survived two releases spent on exactly this class of bug.
  The check is the larger piece: comparing two `.d.ts` surfaces means deciding
  what "narrower" means per position, and a parameter and a return want opposite
  answers. That is a design task, not a script.
- **`bytes.to_array` is 57 ms per megabyte**, with no `Buffer` equivalent to
  compare against, because it builds a JavaScript array of a million numbers.
  That is inherent to the target type rather than a defect, and it is the one
  `std/bytes` call with no fast path available, so a program converting large
  buffers to `Array<int>` should expect it. Recorded because it was measured and
  would otherwise be forgotten.
- **Inline the index bounds check before trying to prove it away.** After 0.1.76
  closed G117's allocation, the same benchmark is 33 ms for `for c in cells`,
  62 ms for `array.filter`, and 61 ms for an index loop. What is left of the gap
  is `__glyph_index`, and the interesting part is what that cost is made of.
  `glyph-emit/src/lib.rs` emits the helper for **every** `Expr::Index` with no
  specialization, and the helper takes `unknown`, so every array read in a
  program funnels through one generic keyed-load site that sees every array shape
  in the program. On top of the megamorphic access it pays `Array.isArray`,
  `typeof` and `Number.isInteger` per element, and the call itself stops V8 from
  treating the read as an element load at all.

  That splits into two changes, and they should not be attempted in one release.
  **Inlining the check at the site needs no analysis and is sound everywhere**,
  and it restores a monomorphic element access, which additionally lets V8 run
  its own bounds-check elimination over a counted loop, which it cannot do
  through an opaque call. **Eliding the check where the compiler can prove the
  index is in range** is the second change: strictly more work, strictly less
  general, and worth building only if inlining does not close the gap. Measure
  after the first before scoping the second, because if the first is enough then
  the second buys nothing a user can see.

  The proof obligation for the second, if it is ever needed, is that the bound is
  tied to that array's length, the binding is not rebound, and the array is not
  mutated in the body. Glyph is unusually well placed for the last one, since
  `mut xs.push(x)` is syntactically required and greppable; the real limit is
  aliasing through calls, so a first version has to be intraprocedural and bail
  when the array is passed anywhere. **Removing the check is not on the table**
  under either change: it is what makes `xs[i]` typed as `T` honest, and dropping
  it re-opens G30 rather than reverting it.

  One thing not to adopt along the way. `for i in 0..xs.length` is not Glyph and
  should not become it: G30 decided that `..` is language surface costing grammar
  and foreclosing later decisions, where a function costs nothing and reads
  beside `slice`. Taking the range syntax to make an optimizer's analysis easier
  would be paying for it in the wrong currency. And whatever lands, `for c in
  cells` stays the advice, because it is fastest, clearest, and needs no analysis
  to be either.
- **`std/bytes` codecs were 40x to 100x off `Buffer`, and are now 1x to 35x.**
  ✅ **mostly done.** Benchmarked after 0.1.78 shipped
  (`benchmarks/micro/bytes_vs_buffer.mjs`), the hand-written codecs were far
  slower than the platform on a megabyte: `to_base64` 135 ms against 1.2 ms,
  `to_hex` 160 ms against 4 ms, `to_base32` 170 ms with nothing to compare
  against. The cause was not the algorithm and not the validation. Every codec
  grew its output a few characters at a time with `+=`, and every decoder found
  a character's value with `alphabet.indexOf(ch)`, a scan of up to 64 characters
  per input character that needed a one-character string to scan for.

  The fix keeps one implementation and adds no host dependency. Each codec now
  builds its output as ASCII octets in a typed array and converts once with
  `TextDecoder`, which is part of the language rather than of node, and each
  decoder reads through a precomputed 128-entry reverse table indexed by
  character code. base32's case folding became a second table entry instead of a
  `toUpperCase()` per character, and `index_of` now finds candidate positions
  with `Uint8Array.prototype.indexOf` and only compares the rest by hand.
  `to_hex` went 160 ms to 5.3 ms, `to_base64` 135 ms to 9.7 ms, `to_base32`
  170 ms to 9.4 ms, `from_base64` 40 ms to 11 ms, `index_of` 2.6 ms to 1.1 ms.

  **A delegating fast path is no longer worth building.** It was the obvious
  answer and it was the wrong one: it would have made a guarantee depend on
  which host the code ran under, with two implementations of the same refusal
  rules chosen at runtime, and it would have needed a differential test between
  them forever to stay safe. Optimizing the single path recovered most of the
  gap without any of that. What remains is that `Buffer`'s codecs are native and
  these are not, which is the price of the bare realm and is now a small enough
  price to stop paying attention to.

  `scripts/check_bytes_codecs.mjs` runs in CI and is the thing that makes this
  safe to have done: 168k checks against `Buffer` over random data, asserting
  that encoders agree byte for byte at every length, that every encode
  round-trips, that anything a decoder accepts re-encodes to what it was given,
  and that anything `Buffer` produces is accepted. The published RFC 4648
  vectors only cover what someone thought to write down; this covers the rest,
  and it exists because these codecs were rewritten for speed once and could be
  again.

  Still open, and small: `equals` is 18x off `Buffer.equals` (1.4 ms per
  megabyte, which is 700 MB/s and hard to care about), and `from_hex` is 4.6x.
  Neither is worth a second code path.
- **`std/encoding`'s six functions are silent on malformed input.**
  `base64_decode("!!!")` is `""` and no error, because `Buffer.from` skips any
  character outside the alphabet, and a decode of bytes that are not valid UTF-8
  comes back with U+FFFD substituted. 0.1.78 put refusing codecs on `std/bytes`
  and left these alone: fixing them means changing six shipped signatures from
  `string -> string` to `string -> Result<string, _>`, which is a breaking change
  and wants its own release. Until then their header and the reference both point
  at `std/bytes` for anything that has to be right.
- **Integer codecs over `Bytes` (`u16_be`, `u32_be`, `u32_le`, and the writers).**
  Deliberately left out of 0.1.78 on the evidence that round 28 wrote big-endian
  decoding in ordinary Glyph and checked it against published vectors, so this is
  convenience rather than capability. It is still the shape every binary format
  reaches for first, and `(b[0] * 16777216) + (b[1] * 65536) + ...` is the kind of
  line that is wrong once and never noticed. Wants a real app asking for it.
- **Hex literals (`0xff` is still `[E0002]`).** A known deferral, designed to be
  forward-compatible, and unusually painful next to bytes: a 256-entry CRC32 table
  written in decimal is unreadable. Lexer-only, and the parse trees of existing
  files do not change.
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

### Release-pipeline gaps left open after the pipeline rebuild

The release workflow was audited and rebuilt after every GitHub Release tarball
since the release job was introduced turned out to ship `glyph` as mode 0644.
The npm copy of the same binary was correct, so the npx smoke test could not see
it. These are the findings from that audit that were deliberately not fixed in
the same change, recorded here so they are not rediscovered.

- **Publishing still runs on a token, and npm is retiring that path.** *Closed in 0.1.125: the six packages have trusted publishers and `release.yml` publishes through them with npm 11.21.0, with no token anywhere in the job.* The 0.1.122 publish failed twice before it succeeded: the `NPM_TOKEN` set on 2026-07-18 had stopped authenticating (401 at the dry-run gate, which held), and the replacement was refused at the first PUT because direct publishing needs a token with 2FA bypass, which npm now labels deprecated and removes in January 2027. The release job already holds `id-token: write` for provenance, so the move is to trusted publishing: a trusted publisher on each of the six packages (`chadetov/glyph`, `release.yml`), npm 11.5 or later on the runner, and the token gate, `npm whoami` and `NODE_AUTH_TOKEN` gone from the publish steps. Do it in the 0.1.123 ceremony, before the tag, and keep a local `npm whoami` against the stored credential in the pre-tag checklist until the token is gone
- **`darwin-x64` is executed nowhere in CI, and its only coverage is one
  machine.** The verify matrix runs each built binary on its own platform except
  this one: `macos-14` is arm64, and running an x64 binary there needs Rosetta,
  which is not guaranteed on the image. It is still covered today only because
  the maintainer's machine is an Intel Mac, so the manual npx smoke test runs
  the x64 binary. That is coverage by accident of hardware, and it inverts on
  the next laptop: `darwin-arm64` has CI coverage and `darwin-x64` would then
  have none. `macos-13` is the honest fix, and the reason it was avoided for
  building (the runner queues) weighs much less on a ten-second verify job.
- **No documented recovery for a half-failed publish.** npm versions are
  immutable, so a failure between the third and fourth `npm publish` leaves
  packages that can never be replaced. The rebuilt workflow makes this far less
  likely (an already-published probe and a dry-run of all six before the first
  real publish, plus a global concurrency group), but if it happens anyway there
  is no written procedure. It should say: burn the version, bump to the next
  patch, and never attempt to re-cut the same number.
- **musl and Alpine get a glibc binary.** The launcher maps on `platform-arch`
  only, so `linux-x64` resolves to the `x86_64-unknown-linux-gnu` build on musl,
  which cannot execute. The user sees a spawn failure rather than a sentence
  telling them what happened. A `linux-x64-musl` target is the real fix; a
  detection-and-diagnose path is the cheap one.
- **The npm packages ship no license text.** All six declare `"license": "MIT OR
  Apache-2.0"` and none contains a LICENSE file; `npm pack` on the launcher
  yields `README.md`, `bin/glyph.js`, `bin/resolve.js`, `package.json`. The repo
  is dual-licensed and the text exists at the root, so this is a copy step.
- **Yarn PnP keeps the platform packages zipped.** None sets
  `preferUnplugged: true`, so under Yarn Berry the resolved binary path points
  inside a zip that cannot be spawned.
- **Nothing gates the roadmap status flip.** The release notes are gated now:
  each release is `site/releases/0.1.N.html`, `scripts/check_releases.py` fails
  when a released version has no notes, and the site generator computes the
  `Latest` badge and fills the version pill from the Cargo version. The flip of
  this file's entry from *Landed on main* to *Shipped* is still unchecked.
- **The site deploys on merge, not on publish.** The Pages workflow fires on
  push to `main` under `web/**`, which the release PR merge matches, so the site
  can announce a version for the window between the merge and the tag.
- **`check_binary_fresh.py` runs nowhere mechanical.** It is first in the
  ceremony's gate list and is invoked by no workflow, so it holds only when
  someone remembers to type it. It checks a local build, so CI is not the right
  home; the release orchestration workflow is.

- **G135: the parser and the emitter disagree about positional variant
  patterns.** E0010 (new in this release) tells an author that a variant carries
  one payload and the tuple form does not exist. If they take the advice, write
  the record payload, and then write `Node(c, l, k, r)` in a match arm out of
  habit, E0300 tells them TS emission for a multi-argument pattern "is not
  implemented yet" — the opposite claim, one line later. A multi-argument
  pattern can never be valid under D8, so it should be refused on the rule by
  whatever owns the rule, not deferred by the emitter. The obstacle is that
  E0300 covers nested patterns in the same message, and a nested pattern *is* a
  real emitter deferral where the current wording is honest. Splitting the two
  is the work; which stage should own the D8 half is the decision.

### Website drift the 0.1.86 ceremony surfaced

The release ceremony audited the site and found four things nothing checks. Two
were fixed in that release (the hero pill, which had advertised v0.1.72 for
thirteen releases and is now gated by `check_versions.py`, and the newest release
entry's code sample, which did not compile and is now covered by
`check_docs_compile.py`). These are the rest.

- **The home page's answers grid omits three pages.** `embedding`, `upgrades`
  and `binary` exist under `web/answers/` and are missing from the grid at
  `web/index.html:1104-1124`, where the numbering also shifts. `check_site.py`
  verifies the sub-nav and that links resolve, so a page that is simply absent
  from the grid passes.
- **`web/sitemap.xml` lists 21 of the 24 answer pages**, missing the same three.
  Same cause: nothing compares the directory listing to either file.
- **A status-row claim on the long-running answer page is false.**
  `web/answers/long-running/index.html:226` says there is still no way to write
  down that a function does not return, which stopped being true. Prose claims
  on answer pages are checked by nobody.

The shape behind all four: `check_site.py` checks structure (links resolve, HTML
parses, sub-nav consistent) and nothing checks *claims*. The two fixed cases were
mechanical enough to gate. Whether a status-row sentence is still true is not,
and that is the honest limit.

## The loop this is all for

Everything in the three sections below serves one target, and it is worth
writing the target down concretely rather than as "better agent support",
because stated concretely it becomes testable.

A developer says "add OAuth login". The agent says it will change `auth.glyph`.
Before it edits anything, it asks the compiler six questions:

1. What is `AuthResult`?
2. Where is `Session` constructed?
3. What consumes a `Session`?
4. What resources must be closed?
5. Which errors are possible here?
6. Which `match` branches become non-exhaustive if I change this union?

It edits. The compiler answers with semantic errors, not TypeScript ones. The
agent fixes them, the tests run, and the change ships.

**The compiler already knows all six answers. It exposes one and a half.** That
is the whole gap, and it is smaller than it sounds:

| Question | Compiler knows it | Reachable today |
|---|---|---|
| What is `AuthResult`? | yes, `type_map` | partly: `glyph_hover`, but only at a cursor position, not by name |
| Where is `Session` constructed? | yes, resolution | no. `glyph_references` returns references, and a construction is not distinguished from a mention |
| What consumes a `Session`? | yes, every signature | no. Needs signature reachability: which functions take or return `T` |
| What must be closed? | yes, D25 `owned`/`resource`, 82 sites in the typechecker | no |
| Which errors are possible? | yes, the error arm of the `Result` | no |
| Which matches go non-exhaustive? | yes, computed in three functions in `assign.rs` and then discarded | no. This is R4 |

All five MCP tools today (`glyph_diagnostics`, `glyph_hover`,
`glyph_definition`, `glyph_references`, `glyph_symbols`) answer about a position
in a file. Every question above is about a *thing*: a type, a value, a
guarantee. That difference is the work, and it is why entity identity (R1) is
first: a query about a thing needs a name for the thing.

**Two consequences for the plan.**

*Question 6 is the one to build first,* and R4 already says so for a different
reason. It is the question with the most direct evidence behind it: five gaps in
one shape, and a bug class where the failing program compiles clean and passes
`tsc --strict`. It is also the only one of the six where the compiler is
currently throwing away an answer it has already computed.

*The loop does not close on reads.* The diagram has an edit step in it, and
every tool listed above is a read. An agent that has all six answers still
changes code by editing text. That half is not scheduled and should be, with the
sequencing stated: reads first, because a wrong read costs a turn and a wrong
write costs a file, and entity identity is a precondition for both. A rename
that cannot name what it is renaming is a search and replace with extra steps.

**What "semantic errors, not TypeScript ones" costs.** The diagram's fourth box
assumes the compiler answers in Glyph's own terms. 215 lines in the gap ledger
mention `tsc` or a `TS####` code, so today a meaningful share of what the agent
would get back is a TypeScript error about generated code it did not write. That
is Q46's second item, and this loop is the reason it is not cosmetic.

## Four questions about a payment API, and which of them we can answer

A useful way to pressure-test the semantic-graph plan is to take an application
nobody would write casually, a payment API with idempotency and audit logging,
and ask what an agent would actually want to know about it. Four questions came
out of an outside review. Checked against what the compiler can prove, one is
already answered more strongly than the question assumes, two are the scheduled
work, and one is a recorded non-goal that must not be promised.

**"Can this database connection leak?" Already answered, today, and not as a
query.** This is D25. A `resource` type held as `owned` must be consumed on
every path, and failing to is `E0206`, with `E0205` and `E0207` for the
neighbouring mistakes. It is a compile error rather than something an agent has
to think to ask, which is the stronger form: the agent cannot forget to ask, and
cannot ship the leak. The gap is that nothing advertises this. An agent has no
way to learn that the guarantee exists, which is the discovery problem again.

**"What endpoints are affected if I change `PaymentStatus`?" This is the
scheduled work, and it is the best argument for R4.** Impact follows from the
exhaustiveness relation plus `CALLS`: every match site over the type, which
variants each covers, and who reaches those functions. The compiler computes the
first half already and discards it.

**"What happens if Stripe returns this error?" Derivable and not exposed.** The
error arm of a `Result` is in the signature, and which branches handle it is the
same relation as above. This one needs no new analysis, only a query.

**"Can this payment ever be processed twice?" Not answerable soundly, and we
should say so rather than demo it.** As posed, this is a question about ordering
along a path: is the idempotency key persisted before the charge call. Answering
it in general needs interprocedural dataflow, which the requirements record as a
non-goal because it is unsound under first-class functions and would break the
invariant that every edge is exact or absent. A plausible answer here is worse
than no answer, because the whole value of a compiler-backed reply is that the
agent can stop looking.

What is soundly answerable is narrower and still useful: which functions take or
return the key, which paths reach the charge without one in scope by signature,
and whether the audit write is on every branch. That is signature reachability
and exhaustiveness, not dataflow. The honest framing is "here is the set of
places this value crosses, and here is what I could not check", which is Q46's
report of what was not verified arriving from the query side.

**The rule this establishes for any future demo.** Show the three we can prove
and state the fourth as the thing the compiler declines to claim. We have made
the other mistake once already, in reverse: a marketing draft showed a
diagnostic proving a branch unreachable from a balance guarantee, which
`where` refinements do not do, because D39 is boundary-validated rather than
statically tracked. A demo of a guarantee we do not have is the fastest way to
lose the credibility the guarantees we do have would earn.

## Semantic model and agent queries (Q45)

The compiler resolves every name, types every expression and finds every
reference, because compiling requires it. Almost none of that is reachable from
outside except through five MCP tools and the LSP, the MCP path does not use the
compiler's incremental engine at all, and nothing in a scaffolded project tells
an agent the interface exists.

The argument is closure rather than context. An agent with `grep` already has
context; what it cannot get is an answer complete by construction, which is the
only kind that lets it stop looking. The cost of not having one is in this repo
four times over: twenty-nine sites unwrap `Ty::App` to its base, and G139, G141,
G142 and G143 were each one of those sites not doing it. Four releases, one bug
shape, every fix correct and incomplete together.

The constraint that outranks any feature below: the semantic view is a projection
of the compiler's model and never a second parser.

**Two findings set the order.**

*MCP bypasses salsa.* `glyph-lsp/src/mcp.rs` contains no reference to the
database. It calls `analyze_full` on raw file text and walks `workspace_files`
per request, so `glyph_references` re-analyzes every file in the project on every
call: 174 full analyses against the examples tree to answer one question. Salsa
computes these facts once and incrementally, and MCP discards that. Every query
worth adding is multi-file, so each one added to the current path is built
expensive and has to be moved later.

*Nothing advertises the interface at the point of use.* `glyph init` scaffolds
`.gitignore`, `package.json`, `src/main.glyph` and `src/.types/README.md`, none
of which mention MCP. The root README does not mention it; the npm README
mentions it once. `web/llms.txt` documents it properly, in 1104 lines that name
all five tools and the position convention, but an agent has to already be
reading the website. An agent in a project sees a manifest and a source file and
reaches for `grep`. Closure is unreachable if the interface is never found.

**Scheduled.**

- **The projection constraint, written down.** One paragraph in the spec: the
  semantic view is derived from the compiler and may not re-derive meaning
  independently.
- **Advertise the interface where the work happens.** `glyph init` writes an
  `.mcp.json` pointing at `glyph mcp`, the config Claude Code, Cursor and others
  already auto-detect, plus a short `AGENTS.md` naming the five tools and when to
  prefer them over searching. A paragraph in the root README. Cheap, independent
  of everything else here, and it is the difference between a capability and a
  used capability.
- **Route MCP through salsa.** The gating item above. One semantic query boundary
  that LSP and MCP share, over the tracked queries that already exist (`resolve`,
  `module_symbols`, `type_map`, `decl_ty`, `module_exports` and the rest).
- **Identity for the entities that lack it.** `SymbolId` already exists in
  `glyph-resolver/src/symbol.rs`; `TypeId`, `ModuleId` and `ScopeId` do not. This
  earns its place on its own, because a diagnostic can then name a thing rather
  than a rendered string.

**Rolling, once the query boundary lands.**

- **`CALLS` as a first-class relationship**, distinct from `REFERENCES`, and the
  direction queries: callers, callees, dependents, dependencies. Resolution
  already computes the underlying facts.
- **Provenance on every answer, shipping with those queries.** Each fact carries
  its source and whether the compiler stands behind it: compiler-proven, read
  from source, asserted by an external `.d.ts` or npm package, or observed at
  runtime later. This is how the edge of the compiler's knowledge becomes a
  queryable fact instead of a silent omission, and it is the difference between
  an impact answer that is trustworthy and one that quietly stops at the
  TypeScript boundary.

**Later, and gated on measurement.**

- **Coverage and impact.** "Where is `T` matched, and which variants does each
  site handle" is the query that kills the bug class above; impact follows from
  it. Scope it to what the model derives. Reads, writes, parameters and type
  conflicts are derivable. A database mapping or a serialization path is not, and
  reporting one anyway manufactures the false confidence provenance exists to
  prevent.
- **`explain` with structured reasons.** `--explain E0300` is static prose per
  code today. Site-specific explanation means diagnostics carry structured facts
  rather than rendered strings, which is a real refactor and belongs last.

**How this gets judged.** Thor's agents are the target user, so the loop already
running is the experiment, and the discovery item is what makes it a fair one:
until a project advertises the interface, an agent not using it says nothing
about whether it helps. Baseline from the tick ledger: the fix lane is 59.6% of
agent-minutes, one gap took four review rounds and roughly six hours, another
seventy-five minutes. If these queries help, minutes per closed gap fall. If they
do not move, that is worth learning before the later items rather than after.

**Parked, and not as later phases of this.** A runtime overlay pairing static
structure with observed execution is a different product with its own collection
and privacy surface. A graph database, embeddings and retrieval are consumers of
this foundation and must not shape it. Provenance already reserves a slot for
runtime facts, which is the right amount of accommodation to make now.

Not a 1.0 blocker. The 1.0 gate is interop and this competes with it. The
scheduled items are cheap and architectural; skipping them is how a project
reaches 1.0 unable to add the rest.

## The diagnostic surface as the agent's interface (Q46)

A diagnostic's job for an agent is to leave it in a state where the next action
is determined. By that measure this compiler is already excellent in places and a
dead end in others, and both appear in the same build. A missing variant reports
`missing variants \`Blue\`` and tells you to add an arm. An unsupported construct
reports that emission "is not implemented yet" and tells you to see the spec,
from a single hardcoded string at `glyph-emit/src/lib.rs:194` shared by every
emitter refusal, while the compiler knows the working spelling it is not
offering.

The cost falls harder on an agent than on a person. A person reads the spec. An
agent abandons the approach or works around it, reaching for `extern_ts` or
reshaping the program until the error goes away, and what ships compiles while
meaning something else. Seven dogfooding apps blocked outright and `watchrun`
blocked three separate times, which is one wall rediscovered three times because
nothing told the first agent it was known.

**Scheduled.**

- **Measure the success rate.** The fraction of programs an agent writes that
  compile first time, and the cycles to green. Thor already produces the raw
  material on every fix attempt and every app build. Everything else in this
  section and in Q45 is argued rather than known until this exists, so it goes
  first despite being the smallest.
- **Every refusal names a rewrite, with a gate.** No `help()` may point at a
  document instead of naming a concrete alternative. E0200 is the standard,
  E0300 the counterexample. This is on the hot path of every failed attempt.
- **An unresolved import is a resolver diagnostic.** Today it falls through to
  `[TS2307] Cannot find module`, with a help line directing the agent to read the
  generated `.ts`. A hallucinated module name is among the most common things an
  agent does; it should name the modules that exist.

**Rolling.**

- **No raw TypeScript error reaches the agent.** Seventy of 145 gap entries
  mention a `tsc` error or a `TS####` code. Spans already map back to Glyph
  source, which is the hard half; the code and the advice still come from the
  back end. Classify each one as a known Glyph construct misuse, which becomes a
  Glyph diagnostic, or as genuinely about the boundary, which says so plainly and
  never tells the agent to go read emitted TypeScript.
- **Ship the known-limitations database with the compiler.** 144 documented gaps,
  each with a reproduction and a status. When a diagnostic matches one, say so,
  give the working spelling, and name the release it is scheduled for. No other
  language has this asset and it currently helps nobody outside the repo.

**Later.**

- **A report of what was not checked.** The worst outcome for an agent is a green
  build that is wrong, because it stops and reports success. Two of the three
  open friction points are exhaustiveness silently not firing, where a match
  missing a variant compiles clean, passes `tsc --strict`, and throws at run
  time. After a build, list where the compiler declined to verify something: a
  match it could not check, a value crossing `extern_ts`, a stdlib call whose
  return it does not model. This is Q45's fact provenance arriving from the other
  direction, and the two should share an implementation.

**Tested and dropped.** Ranking cascaded errors by root cause is not needed: a
bad field access consumed by three call sites produces exactly one diagnostic.
The compiler already suppresses cascades.

**Not the problem.** Latency. `glyph check` on a small module is 1.4 seconds,
fast enough to run per edit. The bottleneck is the fidelity of the answer, not
the speed of getting it.

## Semantic graph requirements (Q45, R1 through R8)

An outside requirements pass on the semantic graph, ordered by dependency rather
than by value, with the expensive-to-revise items first. It is compatible with
Q45 and sharper than Q45 on identity and serialization, which Q45 left implicit.
Recorded here with what was checked against the compiler, because three of its
premises had drifted.

**The invariant, which is the part worth adopting verbatim.** Every edge is exact
or absent, and absence of an edge means absence of a relation, never "analysis
did not reach here." Any requirement that cannot hold this is a non-goal. This is
the same rule as Q45's provenance and Q46's report of what was not checked,
arriving a third time from a different direction, which is a good sign it is
load-bearing.

**Prerequisites, rechecked.** The pass lists three. Two are already done rather
than pending: the incremental query architecture is salsa and has been since the
pipeline was built, and lexer spans plus AST trivia exist because the formatter
needs them. One declaration form per symbol holds by the greppability pillar.
So the graph starts from a database that already exists, which moves it earlier
than the pass assumed.

**R1. Entity identity.** Stable IDs that survive edits elsewhere in the file.
`module::kind::name` for declarations, parent plus field name for fields and
payloads, and match arms keyed by the variant they cover rather than by position.
Closures are the one unstable corner, keyed by parent plus ordinal. Positional
IDs would reintroduce diff churn at the tooling layer, which is the failure the
formatter exists to prevent. Decide before R4 through R7, because every later
requirement encodes these IDs. Acceptance: inserting a declaration at the top of
a file changes no other entity's ID.

**R2. One type identity, not three.** The graph's identity for a type is the same
identity as the runtime descriptor and the emitted brand. Note the drift: there
is no brand scheme today, so this constrains a design that has not been written
rather than correcting one that has. Keep it as a constraint on that future work.

**R3. Boundary node kinds.** Every node is `glyph`, `extern`, or `opaque-ts`. A
codebase that imports npm and is consumed from `.ts` has a frontier at many
points, and the frontier has to be a node kind or the invariant fails on the
first npm import. This gives `extern_ts` a second job beyond binding
implementations: marking where the guarantees stop. Acceptance: no reachable
identifier resolves to a node with no kind.

**R4. Retain the exhaustiveness relation.** Store arm-to-variant edges and a
per-site exhaustive or catch-all flag. The checker computes this and throws it
away. Retaining it turns "what breaks if I add a variant" from a search into a
lookup. Highest value per unit of cost in the list, and it is the direct
instrument for the bug shape this project has now hit five times: G139, G141,
G142, G143 and G148 were each one site not unwrapping `Ty::App` to its base.

**R5. Provenance edges for generated code.** Codegen emits `generated-from` edges
carrying source path and content hash. Codegen already holds both. Without the
edge a hand-edit to a generated wire type is silently discarded on the next run
and nothing tells the agent the repair belongs in the schema.

**R6. Bind `@example` blocks to their entity.** One edge over shipping `@example`
at all, and it turns "how is this called" into a compiler-verified answer.
`@example` is real and parsed today, so this one is cheap and available now.

**R7. Serialization.** Stable, versioned, line-oriented, sorted by entity ID,
byte-identical across runs on unchanged input. The graph will be diffed and
possibly checked in, and a format that reorders between runs recreates the churn
the formatter prevents.

**R8. Query surface, deferred.** Do not design before the workload is observed.
Note the drift: the pass proposes instrumenting "the hookrelay probe", and
hookrelay was a dogfood app from the 0.1.33 through 0.1.35 window, not a live
probe. The intent still holds and needs a real instrumentation point. The
honest one is Thor: its agents already issue every grep, glob and read against
this repo, and the tick ledger already records them.

**Three requested capabilities meet these non-goals, and the meeting is recorded
rather than resolved quietly.** Two are design conflicts; one is only scheduling.

*Tracing data flow through the program* (`Request` to validate to `User` to
authorize to `Response`) is a non-goal above, for a technical reason and not a
scheduling one. Under first-class functions the call target is not always known
statically, so interprocedural dataflow edges cannot be exact, and a graph that
answers "no relation" when it means "I could not follow it" has broken the one
promise everything else rests on. Three ways forward, cheapest first:
signature reachability alone, which the non-goal entry already says "answers most
of the real query exactly" and which stays sound but cannot follow a value
through a callback; dataflow with an explicit third state, exact where the target
is known and named as unknown where it is not, which keeps the invariant honest
at the cost of every consumer handling three cases; or full interprocedural
dataflow, which answers the question as asked and gives up exact-or-absent. The
last is not recommended, because the guarantee is the product.

*Verifying value-level semantic invariants* ("can this function ever receive a
non-Captured payment?") meets two non-goals: refinement facts beyond `where` are
not expressible, and D39 makes `where` boundary-validated rather than statically
tracked, deliberately. The type-level form is answerable with edges the graph
will already have, namely which call sites reach a function and what types they
pass. The value-level form needs the compiler to track that a particular payment
was captured along every path, which is refinement typing, and the entry above
calls invariant preservation "a different product." Answering it approximately
would advertise a guarantee the compiler does not make.

*Graph-driven refactoring* is different in kind: nothing technical blocks it, and
"rename and find-references stay at v1.1" above is a sequencing choice. It is
worth naming because every MCP tool shipped so far reads, so this would be the
first that writes, and a write surface is its own risk category rather than a
ride-along. Pulling it forward is the owner's call. Two constraints if it
happens: greppability is not relaxed to make writing easier, and a write tool
must never be the only path to a change.

Neither of the two design conflicts is scheduled. The sound subset of each is
cheap enough to be worth doing regardless of how the larger question lands.

**Non-goals it records, all of which stand.** Interprocedural dataflow edges are
unsound under first-class functions and would break the invariant; signature
reachability answers most of the real query exactly. Refinement facts beyond
`where` are not expressible. Invariant preservation is a different product.
Rename and find-references stay at v1.1. Greppability is not relaxed for the
graph: if the graph is ever cited to justify a second declaration form, the
pillar wins.

## Capability audit against an outside review

A twenty-item ranked list of "missing capabilities" arrived from a review written
without repository access. Checked item by item against the compiler and the
thirty-six stdlib modules, six are already shipped, seven are partial, and seven
are open. Two of its four critical items are wrong, which is worth recording so
the list is not re-adopted whole later.

**Already shipped, and the review did not know.**

- *A first-class `Result`/`Option` model*, ranked critical. `std/result` and
  `std/option` exist, D15 gives them named imports, and D18 makes postfix `?`
  bind tighter than `.`.
- *An async model*, ranked critical. `async` and `await` are lexer keywords,
  `std/task` exists, and `async fn f() -> Result<string, string>` compiles and
  passes `tsc --strict` today.
- *Serialization and schemas.* `std/json`, `std/schema`, D28 `infer_output`, and
  `glyph gen openapi|dts|zod`.
- *Time and duration.* `std/time`, `std/timers`, `std/intl`.
- *Generated API and client tooling.* This is `glyph gen`, shipped.
- *A testing framework.* `std/test` exports `property`, so property testing is
  there, alongside `@example`.

**Partial, and the remaining half is real.**

- *Stdlib maturity.* The breadth exists: thirty-six modules including `net`,
  `websocket`, `tls`, `dns`, `crypto`, `fs`, `encoding`, `collections`. What is
  missing is stability of those APIs, not their existence.
- *Ownership and mutation.* D5 `mut` and D25 `owned` cover the resource case.
  The manifesto rules out general linear types, so "a principled model" here
  means finishing the narrow one.
- *Databases.* `std/sqlite` and `std/store` exist. Postgres does not.
- *Observability.* `std/log` has structured logging with `with_fields`. There is
  no metrics or tracing module.
- *Configuration and secrets.* Reachable through `std/process`, not typed.
- *FFI.* `extern_ts` and D29 cover the TypeScript direction, which is the only
  direction that matters while TS is the runtime target.
- *Package and dependency story.* D41 settles the project root; npm interop is
  the 1.0 gate and is tracked above.

**Open, and correctly ranked.**

- *The semantic graph.* Q45, and the requirements section above.
- *Effect and side-effect boundaries.* Whether a function is pure, does I/O, or
  touches the network is not expressible. This is the strongest item on the list
  that we do not already have, and it is the one that most helps an agent reason.
- *Exhaustiveness completeness.* Five gaps in one shape, most recently G148.
  R4 above is the instrument for it.
- *Generics completeness.* Same family, same fix direction.
- *A CLI application framework.*
- *Memory and performance controls*, which are constrained by transpiling to TS.
- *Profiling.* Last, and the review agrees.

## Teaching models Glyph, and where that work lives

Glyph is new, so no model has seen it. Four levels of "a model knows Glyph" are
worth separating, because only the first two are ours to control.

**Discoverability now.** `web/llms.txt` is 1104 lines and `glyph llms` reprints
it offline, so the reference exists. What is missing is that nothing puts it in
front of an agent working in a project. That is Q45's discovery item and it is
scheduled there.

**A corpus.** The unusual asset here is that Glyph compiles to TypeScript, and
TypeScript has an enormous public corpus. A TypeScript-to-Glyph pipeline that
round-trips through the compiler and its tests generates verified pairs rather
than plausible ones. The compiler is the label. Start at roughly a thousand
examples that are checked rather than a million that are not, and include the
three kinds that documentation cannot express: wrong Glyph, the diagnostic it
produces, and the corrected Glyph. That last kind is the training analogue of
Q46, and both need the same thing from the compiler.

**A benchmark.** Because we own the compiler we own the grader, and it can score
past syntax: parse, typecheck, `tsc --strict`, run the tests. That is harder to
game than a generation benchmark, and it is a stronger public position than
another language announcement.

**Vendor training.** Not directly available, and not worth planning around. The
realistic path is being an excellent public target, which the three items above
are.

**Where it lives.** The compiler, runtime, stdlib, spec and examples stay here.
A corpus, a benchmark harness, evaluation runs and any fine-tuning belong in a
separate repository, because their lifecycle is different: dataset releases and
model evaluations move on a different clock from language releases, and a
half-gigabyte of data does not belong in the repository someone clones to build
the compiler. The split rule this project already uses applies cleanly, which is
to split when the lifecycle differs and keep it when a test enforces the
coupling. The exception is the agent-facing guide itself, which stays here so an
agent landing on the repository finds it without a second fetch.

The semantic graph is a different case and should not be split. It is a
projection of the compiler's own model, so a test enforces the coupling, and
moving it out would create exactly the second source of truth the invariant
forbids.

## Fuzz the parser

The parser is the one component where a crash is reachable from untrusted input
and where a corpus of interesting failures compounds. Scope:

- A recognized harness rather than a bespoke loop, so the corpus and the crash
  format are ones other tools understand.
- The parser first, then the lexer, since lexer bugs surface as parser crashes
  anyway.
- A scheduled run in Actions rather than per-PR, because a fuzzer that gates a
  PR either runs too briefly to find anything or blocks the PR.
- The corpus committed, minimized, and each entry named for what it broke. A
  crash without a committed reproduction is a crash that comes back.
- Findings go through the same gap ledger as everything else, so nothing is
  found and then lost.

## salsa 0.28 migration

Dependabot #47 is not a version bump. 0.28 moves the `Update` trait, so the
derive macro fails with `cannot find trait Update in crate salsa` at every call
site in the query layer. It is an API migration on the incremental engine, which
is the component every other feature reads through, so it wants its own change
and its own testing rather than riding a dependency sweep.

## The stub that made the project look immature, and the dead crate behind it

An outside review read this repository and concluded that Glyph's "compiler
runtime execution" is far less mature than its type system, resolver, emitter
and diagnostics. The facts it cited are true and the conclusion is wrong, and
the gap between those two things is entirely our own doing.

**What is true.** `crates/glyph-runtime` is 41 lines. Its whole content is a
`RuntimeError::NotImplemented` and a `run_example()` that returns it. It is a
workspace member and `glyph-cli` declares it as a dependency. Until this was
written, `glyph-cli/src/main.rs` opened with "Glyph CLI, stub for Phase 0" and
closed its header with "Phase 0 ships only the CLI structure; commands return
not yet implemented."

**What is false.** That execution is immature. `@example` and `@doc @run`
assertions run on every build and are the tree's main verification surface: 382
of them across twelve apps, 54 in `resilient`, 47 in `sheet`, 40 in `discord`,
39 in `depsolve`. `glyph build` on `resilient` reports "51 example(s) passed"
followed by "tsc --strict passed". None of it goes through `glyph-runtime`,
because the architecture went the other way: implementation decision I5 planned
a sandboxed tree-walking interpreter in Rust, and what shipped instead executes
the emitted TypeScript through node, in `glyph-cli/src/examples.rs`. The
interpreter was not deferred. It stopped being necessary, and nobody deleted it.

**So the finding is dead code plus two comments that lie, and the cost is
already paid.** A reviewer read them and misjudged the project. That is the
exact failure the "no obsolete documents" rule exists to prevent, applied to
code comments rather than to markdown, where nothing was checking. The headers
are corrected now and `glyph-runtime` says what it is.

Two things are scheduled rather than done, because both are decisions:

- **Remove `glyph-runtime`, or give it a job.** Deleting a workspace member is
  not a tidy-up. The alternative is real: if the semantic graph or a future
  `@example` sandbox wants an in-process evaluator, this is where it goes. What
  is not acceptable is the current state, where a crate nothing calls sits in
  the tree describing work that was abandoned.
- **A gate for stale code comments.** `check_doc_claims.py` reads markdown and
  would never have caught this, because the lie was in a `//!` header. The
  narrow, checkable version: no source file may describe the project as a stub,
  a phase, or "not yet implemented" while the thing it describes ships.

## The spec is ahead of the implementation, and that four-way distinction is an asset

The same review noted that the spec runs to D44 while naming the hand-written
Rust parser as normative, and that the repository consistently separates four
states: implemented, deliberately limited, known bug, and future work. That is
accurate and it is worth keeping deliberately rather than by habit, because it
is unusual and it is what makes an outside evaluation of Glyph possible at all.

The gap is that the distinction lives only in prose. A person reading
`docs/language/spec.md`, `docs/dogfooding-gaps.md` and this file can tell a
deliberate v1 limit from an open bug. Nothing the compiler emits can. An agent
that hits `E0300` gets "not implemented yet" and a pointer at the spec, and has
no way to learn whether it has found a decision, a scheduled fix, or a wall
nobody has recorded.

That is the same conclusion Q46 reaches from the diagnostic side, and it is the
same asset: 148 documented gaps, each with a reproduction and a status. Making
the four-way state machine-readable, and having a diagnostic name which state it
is in, is one piece of work serving both. It is scheduled under Q46's
known-limitations item rather than duplicated here.

## A nine-step outside roadmap, checked against the tree

An outside review proposed nine steps. Checked one at a time: one is wrong, four
are already scheduled or shipped, and two are real additions that were not on
this page. Recording the disposition so the list is not adopted whole later.

**1. "Finish the runtime." Rejected, and it is the review's own earlier mistake
repeated.** There is nothing to finish. `glyph-runtime` is dead code from an
abandoned design, execution ships and runs 382 `@example` assertions across the
apps tree, and the only evidence for the claim was two stale comments that are
now corrected. The section above has the full account. The action here is
deletion, not completion.

**2 and 3. "Make the semantic graph explicit" and "expose it through MCP."
Already scheduled, as 0.1.97.** The ordering the review implies is right and the
entry keeps it: route MCP through salsa first, because MCP today calls
`analyze_full` on raw text and re-analyzes the workspace per request, and every
query worth adding is multi-file.

**4. "Make agents able to navigate and modify Glyph." Half scheduled, and the
other half is a real gap.** Navigation is 0.1.97. Modification is not: every MCP
tool is a read, rename and find-references are deferred to v1.1, and an agent
that wants to change code has no compiler-backed way to do it. It edits text and
hopes. This is worth separating from the graph work because it has a different
risk profile: a wrong read wastes a turn, a wrong write corrupts a file. The
precondition is R1 entity identity, since a rename that cannot name what it is
renaming is a search and replace.

**5. "Strengthen conformance, property and fuzz testing." One third missing.**
Conformance snapshots exist (14 of them, in `glyph-emit/tests/snapshots`).
Property testing exists and ships: `std/test` exports `property`. Fuzzing does
not exist and is scheduled above.

**6. "Close the remaining cross-module semantic holes." This is 0.1.96, the
release carrying the Next marker.** G147, G146, G143, G138 and the namespace
half of G140, which share one address: the emitter cannot see an imported
union's variant list, so it falls back to the shape of a name.

**7. "Make the generated TypeScript boringly reliable." Real, and larger than it
sounds.** 215 lines in the gap ledger mention `tsc` or a `TS####` code. The goal
is not that the TypeScript is correct, which it largely is; it is that a
TypeScript error never reaches the person writing Glyph. Every one of those is a
place where the back end answered a question the front end should have. This is
Q46's second item and it stays there rather than becoming a separate track.

**8. "Dogfood increasingly large real applications." Ongoing, and the loop is the
main source of everything on this page.** 31 apps now, each in its own
directory. The constraint that makes it work is the one worth restating: an app
stops at the first thing Glyph cannot express and reports it, rather than
working around it. An app that ships having quietly absorbed three gaps has done
negative work.

**9. "Build the agent tooling around Glyph." Partly shipped this cycle.** `glyph
init` now writes `AGENTS.md` and `.mcp.json`, and `glyph agents` adds them to a
project that already exists, so an agent finds `glyph llms` and the analysis
server without being told they exist. The rest of this step is items 2, 3 and 4.

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

0.1.72 added a twelfth thing to that list the hard way. `cargo` writes each
workspace member's version into `Cargo.lock`, so bumping `[workspace.package]`
and stopping there leaves the lock a release behind. CI builds with `--locked`,
which refuses to update it, and the failure is a clippy step reporting "cannot
update the lock file ... because `--locked` was passed" with no mention of
versions at all. `check_versions.py` reads the lockfile now and names the fix.

*Branch protection, and what it deliberately does not include.* `main` blocks
force pushes and deletion, requires linear history, and applies all three to
admins, so no token can rewrite or remove the branch's history. Three checks are
required before a merge: version consistency, the examples build, and the site
links. Requiring them is what forced the move to pull requests, since GitHub
blocks a direct push to a branch whose checks cannot have run yet.

Required reviews are off, and that is not an oversight. A one-person repository
that requires a second approver either stops merging or grows a
self-approval habit, and the second is worse than not requiring one. The checks
are the reviewer here.

Requiring the checks paid for itself in the first week: it caught a workflow
whose `paths` filter meant a required check could never report on a
documentation-only change, which deadlocked its own merge.

*Supply-chain score, and what is left.* The first OpenSSF Scorecard run read
3.6. Pinning all eighteen action references by commit, dropping every workflow's
default token to read-only, and adding Dependabot took it to 5.4. The remaining
zeros, in rough order of value:

- **Signed-Releases (0).** The SLSA attestation is real but lives in GitHub's
  attestation store, so the check cannot see it. Attaching the provenance bundle
  to the release as an asset would fix it, and takes effect on the next release.
- **SAST.** CodeQL runs over Rust, TypeScript, and the workflows, on pull
  requests as well as `main`. Scanning only `main` is what the check reads as
  "0 commits out of 3 are checked.
- **Code-Review and CI-Tests.** Both read pull requests. The repo went to a
  trunk-based pull-request workflow with three required checks, so both now have
  something to measure.
- **Branch-Protection.** Enabled: no force pushes, no deletion, linear history,
  admins included.
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
