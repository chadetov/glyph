# Glyph v1.0 — Implementation Plan

A concrete sequence of work from the current state (post-brainstorm, 2026-05-26) to v1.0 launch. **Tasks and deliverables, not goals.** Estimates assume one focused engineer; doubles if context-switching with other projects.

## What this is

- A weekly-cadence work plan, sequenced by dependency.
- Per-phase acceptance criteria (binary "done" tests).
- A mapping from each D-decision (D1–D27) to the phase that ships it.
- A list of implementation-time decisions deferred to coding sessions.

## What this is not

- The "why" — that lives in `docs/manifesto.md`, `docs/language/spec.md`, and `docs/open-questions.md` (Resolved sections). Read those first.
- A redo of the per-step roadmap docs (`docs/roadmap/`). Those have the per-step scope and constraints. This plan sequences them.
- A risk register or staffing plan. Solo project; risks are flagged inline where they appear.

## Timeline summary

| Phase | Weeks | What |
|---|---|---|
| 0 | week 0 | Prerequisites — validator rewrite, Rust workspace, scaffold |
| 1 | 1–8 | Transpiler + typechecker substep 5a + tests + emission |
| 2 | 9–11 | Narrowing & flow analysis (substep 5c) |
| 3 | 12–17 | Dogfooding (fridge shopping list) |
| 4 | 18–23 | Re-lock spec + LSP |
| 5 | 24–25 | Packaging, formatter polish, `glyph publish` |
| 6 | 26–27 | Installer (npm) + playground |
| 7 | 28–31 | Docs + book outline |
| 8 | 32–39 | Killer demo + benchmarks |
| 9 | 40+ | Launch + first 100 users |

**Total to launch: ~40 weeks of focused work (~10 months).** Calendar 15–24 months depending on context-switching. Matches the brainstorm's revised "9–13 months focused, 15–24 months calendar" estimate.

---

## Phase 0 — Prerequisites (week 0)

Five blocking items before week 1 can start.

### P1. Rewrite the validator example to remove `infer_shape<Shape>` (Q1)

The current validator example in `archive/GLYPH.md §3.1` uses `Schema<infer_shape<Shape>>` — mapped-type territory. **Q1 resolved → defer mapped types to v1.1.** Rewrite with an explicit output type parameter.

**Deliverable:** `examples/01_validator.glyph` at the repo root (NOT in archive/), with the validator using `fn object_schema<Out>(fields: ...) -> Schema<Out>` and an explicit `Out` from each call site.

**Risk:** the rewrite might force ergonomic compromises that surface other open questions. If the rewritten version feels significantly worse, that's data — escalate before continuing.

### P2. Bootstrap the Rust workspace

```
glyph-compiler/
├── Cargo.toml
├── crates/
│   ├── glyph-lexer/        # hand-written lexer
│   ├── glyph-parser/       # Pratt parser
│   ├── glyph-ast/          # AST enums + Span
│   ├── glyph-resolver/     # name resolution + module graph (salsa-backed)
│   ├── glyph-typechecker/  # types, exhaustiveness, owned tracking (salsa-backed)
│   ├── glyph-emit/         # AST-to-TS visitor (dumb, no IR)
│   ├── glyph-runtime/      # @example/@doc @run sandboxed interpreter
│   └── glyph-cli/          # `glyph build/run/fmt/regen/publish`
└── ...
```

**Dependencies (versions to lock):**
- `salsa-2022` for incremental queries (Q5 hybrid)
- `insta` for golden snapshot tests
- `ariadne` or `miette` for error rendering (Elm-quality bar, Q6)
- `proptest` for property-based testing (week 8)
- `tower-lsp` (phase 4)

### P3. Create `examples/` at the repo root

Four files transferred from `archive/GLYPH.md` to standalone `.glyph` files, with P1's rewrite applied to #1:

- `examples/01_validator.glyph`
- `examples/02_async_errors.glyph`
- `examples/03_react_component.glyph`
- `examples/04_cli_tool.glyph`

### P4. Create the `benchmarks/` scaffold (Q10)

```
benchmarks/
├── README.md           # what's measured, how to run
├── glyph/              # Glyph source
├── typescript/         # equivalent TS
├── python/             # equivalent Python
├── rust/               # equivalent Rust
├── measure.sh          # token count + line count + diff size per fn
└── results/            # checked-in measurements per commit
```

Start with 3 functions: a `parseUser` (validator-like), a `loadFeed` (async with Result), and a small list rendering (JSX directive). Grow to 5–10 by end of week 8.

### P5. Pick libraries

| Concern | Choice | Why |
|---|---|---|
| Parser strategy | Hand-written Pratt | `archive/glyph-transpiler-plan.md` decision; best error recovery |
| Salsa version | `salsa = "0.26"` | The "salsa 2022" rewrite reclaimed the canonical `salsa` crate name on crates.io. v0.26+ is the rewrite; v0.16 was the legacy generation. |
| Error rendering | `ariadne = "0.4"` | Cleaner spans than `miette`; closer to Elm aesthetic |
| Property testing | `proptest = "1"` | Industry default for Rust |
| LSP framework | `tower-lsp = "0.20"` | Endorsed in `archive/glyph-lsp-discussion.md` |
| Code generation | None (dumb visitor) | Q5 hybrid: visitor for emit, salsa for typecheck |
| Rust toolchain | `1.95` stable | Pinned in `glyph-compiler/rust-toolchain.toml`. Bumped from initial 1.75 to match salsa 0.26's MSRV and the installed toolchain. |

### Phase 0 acceptance

- [ ] `examples/01_validator.glyph` rewritten without `infer_shape`
- [ ] `glyph-compiler/` Rust workspace builds (empty crates compile)
- [ ] `benchmarks/` scaffold exists with 3 functions in 4 languages
- [ ] All library versions pinned in `Cargo.toml`

---

## Phase 1 — Transpiler + typechecker core (weeks 1–8)

The heart of v1.0. Step 4 + substep 5a + the compile-time-execution machinery for D23/D26.

### Week 1: Lexer, Pratt parser, AST, golden tests

**Spec decisions implemented this week:**
- D1 (significant newlines outside brackets) — external scanner-equivalent in the lexer
- D12 + D22 (one string syntax with template literal interpolation)
- D13 (numeric literals with underscore separators)
- D14 (`//` comments only)
- D17 (trailing commas everywhere)
- D7 (types vs values context-disambiguated at `<`)
- D11 (spread in arrays and objects)
- D18 (precedence table from `archive/GLYPH.md §2`)
- D21 (`for x in iter` and `loop { }`)
- D27 (`@<name> <args>` annotation form)

**Tasks:**
1. Lexer (~300 LoC). Tokens for keywords, operators, identifiers, numeric literals, string literals (including `${...}` interpolation per D22), comments, newlines, brackets.
2. Pratt parser. Operator table derived directly from `archive/GLYPH.md §2`. Error recovery via "skip to next statement boundary."
3. AST enums: `Expr`, `Stmt`, `TypeExpr`, `Pattern`, `Decl`, `Annotation`. Every node carries a `Span`. Use `Arc<str>` for identifiers (no interning for v0).
4. `insta` snapshot tests for all 4 example files plus 5–10 micro-cases (template literal, owned binding, annotation block, loop forms).

**Acceptance:** All 4 example files parse to AST with snapshots checked into git. Snapshot diff on parser change is exact and minimal.

### Week 2: Name resolution, module graph, basic types (salsa-backed)

**Spec decisions implemented this week:**
- D15 (three import forms; no barrel files, no re-exports, no relative imports)
- D19 (`component` is a top-level form)
- D20 (`const` module-level, `let` function-level)
- D4 (one `fn` form, name optional)

**Tasks:**
1. **Salsa-2022 setup** (Q5 hybrid). Define the query database: source file → tokens → AST → resolved module → typed module. Inputs are tracked at file granularity; intermediate queries are memoized.
2. Module graph builder. Walk import statements; reject barrel files (a module that only re-exports), reject relative imports.
3. Type representation enum: `Primitive`, `Record`, `Array`, `Function`, `SumType`, `GenericParam`, `Unknown`. **No mapped types** (Q1 deferred).
4. Name resolution: every identifier resolves to a definition site or fails. Cross-module resolution via the module graph.
5. **No inference yet.** Function signatures and `let` bindings at function boundaries require annotations. Local `let` inference inside a function body via tiny unification.
6. Prelude module: `Result<T, E>`, `Option<T>`, `Ok`, `Err`, `Some`, `None`, `par` helpers. Hand-written, lives in a `glyph-prelude` crate.

**Acceptance:** Every example file resolves all names; every expression node has a type (some `Unknown` is fine).

### Week 3: ADTs, match exhaustiveness, `?` propagation, owned tracking

**Spec decisions implemented this week:**
- D2 (match arm commas)
- D3 (match is the only conditional)
- D8 (tagged union punctuation)
- D9 (`else` arm vs `_` position wildcard)
- D5 (`mut` syntactic only — grammar restriction enforced)
- D16 (`void` type and value)
- D25 (narrow `owned` modifier for resource handles)
- D8 partial (runtime descriptors for ADTs)

**Tasks:**
1. **Maranget exhaustiveness checker** (~400 LoC, from Maranget 2007). On every `match` expression, verify all variants and patterns are covered. Tagged unions are implicitly sealed (no `_` catch-all reduces accidental fall-through).
2. **`?` operator typing rule.** `expr?` requires `expr: Result<T, E>` and the enclosing function returns `Result<_, E>` with `E` matching exactly. No `From` conversion in v1 (the brainstorm's Q5 plan).
3. **Runtime descriptors (Q8 resolved).** Every `type` and `record` declaration emits a runtime descriptor: field names, field types, parse function. The descriptor is what makes `is TypeName` checks work at runtime.
4. **D25 owned tracking.** Single-consumption analysis across paths. A new `OwnedAnalysis` salsa query: for each `let owned` binding, compute the set of paths through the function and verify each path consumes the binding exactly once. Forgetting, double-consuming, or returning without consuming = compile error with span pointing to the relevant line.
5. **`resource` marker** (TBD between keyword and `@resource` annotation; pick during this week). Types declared `resource type X { ... }` can have `owned` bindings. Stdlib's `FileHandle`, `S3Upload`, `DbConnection`, `Mutex` are all `resource`.

**Acceptance:** `examples/01_validator.glyph` and `examples/02_async_errors.glyph` typecheck end-to-end. Adding a variant to a sum type produces a compile error at every match site that doesn't update.

### Week 4: TS emission, JSX directive lowering, async

**Spec decisions implemented this week:**
- D6 (JSX as sub-grammar; directives are regular elements with semantic-pass handling)

**Tasks:**
1. **AST-to-TS visitor** (Q5 hybrid: this is the dumb-visitor part, no IR). Mapping table per `archive/glyph-transpiler-plan.md §4`:
   - `fn name(x: T) -> U` → `function name(x: T): U`
   - `record User { ... }` → `interface User { ... }` + `const User = { parse(input: unknown) { ... } }`
   - ADTs → discriminated unions with `tag` field
   - `match` → `switch` on tag for tagged, `if`-chain for value matches
   - `result?` → inlined unwrapping pattern (ugly emitted TS is fine; humans read Glyph)
   - Template literals (D22) → TS template literals directly (`` `hello ${name}` ``)
   - Loop (D21) → TS `for` and `while(true)` respectively
2. **JSX directive lowering** as AST rewrite *before* emission. Each lowering pass produces a new AST that emission then visits as ordinary code:
   - `<if cond={x}>A</if><else>B</else>` → ternary
   - `<for x in={xs}>...</for>` → `xs.map(x => ...)`
   - `<match value={v}>...</match>` → switch-returning IIFE
   - `<case Variant({ field })>` → pattern binding via destructure in the case scope
3. **Async** → `async`/`await` directly. `par.all` and `par.all_ok` are stdlib functions (Q18 resolved).
4. **Owned consumption** at emission: `handle.close()` (a consume) emits as-is; the typechecker has already verified the consume happened. No runtime owned-tracking is needed because the analysis is purely static.

**Acceptance:** All 4 examples emit TS. `tsc --strict --noEmit` passes on the output.

### Week 5: Formatter, CLI, runtime prelude

**Spec decisions implemented this week:**
- D10 (no object literal shorthand)

**Tasks:**
1. **Formatter** (`glyph fmt`). Recursive AST walk printing to a string. Rule: one element per line if more than two elements in a literal/list/argument. No line-length-based reflow. ~600 LoC.
2. **CLI** (`glyph` binary):
   - `glyph build src/ --out dist/` — walk module graph, typecheck whole program, emit TS files into `dist/`, shell out to `tsc` with a generated `tsconfig.json`. **Subprocess `tsc`, not embedded.**
   - `glyph run examples/01_validator.glyph` — build then run via node.
   - `glyph fmt` — invoked manually and by LSP format-on-save (phase 4).
   - `glyph regen <fn>` (Q40 resolved) — reads the `@generate by:` annotation on a function, invokes the configured generator with `@example`/`@property`/`@budget` context, replaces the body, runs the `@example` tests, commits or rolls back.
   - `glyph --explain E0042` — opens long-form error documentation. Top 20 errors get a paragraph + code-fix example (week 7).
3. **Runtime prelude** (`glyph-prelude` crate). Hand-written `.ts` shipped with the compiler. `Result`, `Option`, `Ok`, `Err`, `Some`, `None`, `par.all`, `par.all_ok`, the issue type for record parsers. **Under 200 lines.** Every emitted file imports from it via a generated top-of-file import. Resist inlining per-file — one module, full-path import.
4. **Stdlib bootstrap (Q3):** ship `result`, `option`, `array`, `string`, `io`, `json`, `fs`, `time` as Glyph source files compiled at install time. Everything else (`http`, `process`, `crypto`, React bindings) is v1.1.

**Acceptance:** `glyph run examples/04_cli_tool.glyph add "buy milk"` works end-to-end through tsc → node.

### Week 6: Compile-time test execution (D23 @example + D26 @doc @run)

**Spec decisions implemented this week:**
- D23 (`@example expr == expr` inline tests)
- D26 (`@doc """ ... @run ... """` executable documentation)

**Tasks:**
1. **Sandboxed interpreter** for the Glyph subset allowed inside `@example` and `@doc @run` blocks. No filesystem access, no network, no clock — unless the test capability is granted explicitly (deferred to v1.1 capabilities, Q17). Budget-bounded per assertion (timeout, memory).
2. **`@example expr == expr`** parsing and execution. Multiple `@example` lines per function are allowed. Each `@example` runs as part of `glyph build`. A failed equality fails the build.
3. **`@doc """ ... ```glyph @run ... ``` """`** parsing. Markdown body with fenced `glyph @run` blocks. The compiler extracts each `@run` block, compiles, executes in the sandbox. Failed `assert` inside fails the build.
4. **Property tests as stdlib** (Q11 resolved → Option A). `test.property(predicate, generator)` is a stdlib function. Lives in `stdlib/test/property.glyph`. Uses Glyph's own `Stream<T>` for generators.

**Acceptance:** Every `@example` in the examples passes on `glyph build`. `@doc @run` blocks in stdlib functions (start with 2–3) run on every build.

### Week 7: Error messages (Elm-quality bar, Q6)

**Concrete bar:** when the compiler rejects code, the message must tell the agent (or human) exactly what to change. Format:

```
error[E0042]: type mismatch in function argument
   ├─ examples/01_validator.glyph:42:15
   │
42 │   fetch_user(user_id_string)
   │              ^^^^^^^^^^^^^^^^ expected UserId, got String
   │
help: wrap with the UserId constructor:
   │   fetch_user(UserId(user_id_string))
   │
   = note: UserId is a nominal newtype over String; see docs/language/spec.md D7
```

**Tasks:**
1. **Audit every error path** in the compiler. Every rejection produces a structured `Diagnostic { code, span, primary_label, secondary_labels, help, note }`.
2. **Source rendering via `ariadne`.** Multi-line, syntax-highlighted, with span underlines.
3. **`--explain E0042`** documentation for the top 20 error codes. Each gets one paragraph + a code-fix example. Authored this week; ~12 hours.
4. **Error code allocation strategy.** `E0001-E0099` reserved for parser, `E0100-E0199` for resolver, `E0200-E0299` for typechecker, etc. Document in `docs/error-codes.md` (to be created).

**Acceptance:** A team review of 20 random error messages confirms they meet the Elm-quality bar. Subjective; this is the gate.

### Week 8: Hardening, property tests, CI, benchmarks track

**Tasks:**
1. **The 4 examples + 15 negative examples as the CI suite.** Negative examples test "this code must fail with this specific error code." Add as `tests/negative/*.glyph` with sibling `*.expected_error` files.
2. **`proptest` on parser + exhaustiveness checker** (~4 days budgeted in `archive/glyph-transpiler-plan.md`). Property-based fuzzing on the parser (it accepts only valid syntax, rejects invalid with proper diagnostics) and on the exhaustiveness checker (it accepts only exhaustive matches, rejects non-exhaustive with the missing-variant span).
3. **Benchmarks populated** (Q10). 5–10 functions measured: `parseUser`, `loadFeed`, list rendering, `slugify`, `groupBy`. Token count, line count, diff size per function checked into `benchmarks/results/`.
4. **CI configuration.** GitHub Actions workflow that runs: `cargo test`, `glyph build examples/`, the negative-test suite, the `@example` and `@doc @run` execution pass, the `proptest` suite, and updates the benchmark results.

**Phase 1 acceptance:**
- [ ] All 4 examples + the corpus grown during P0 parse, typecheck, emit TS, and run via `glyph run`.
- [ ] `tsc --strict --noEmit` passes on all emitted TS.
- [ ] 15 negative examples each fail with the expected error code.
- [ ] `@example` tests run on every `glyph build`; failure fails the build.
- [ ] `@doc @run` blocks in stdlib run on every build.
- [ ] Benchmark results checked in.
- [ ] CI is green.

---

## Phase 2 — Narrowing & flow analysis (weeks 9–11)

Substep 5c from `docs/roadmap/05-typechecker.md`. The piece that makes `match` and tagged-union dispatch feel native.

**Tasks:**
1. **Flow-sensitive type narrowing.** Inside `match` arms, the matched value's type is refined to the arm's pattern. Inside `if expr is TypeName { ... }` blocks (cf. Q8 runtime descriptors), the binding is refined to `TypeName`.
2. **`Result<T, E>` narrowing.** After `result?`, the binding's type is refined from `Result<T, E>` to `T`. Same for `Option<T>` if Q15 nominal newtype answer extends to `Some`/`None` narrowing.
3. **Pattern-binding scope.** `<case Variant({ field })>` inside JSX binds `field` in the case body's scope (D6 + the AST rewrite from phase 1 week 4 must thread the binding through name resolution).

**Phase 2 acceptance:**
- [ ] A 30-line function using nested `match` on tagged unions typechecks correctly without redundant `as` or unsafe coercions.
- [ ] JSX `<case Variant({ field })>` bindings are usable in the case body.
- [ ] Narrowing is exposed as a salsa query, so the LSP (phase 4) reuses it for hover types.

---

## Phase 3 — Dogfooding (weeks 12–17)

Step 6 per `docs/roadmap/06-dogfooding.md`. The fridge shopping list app. **6 weeks because dogfooding finds compiler/stdlib bugs and fixing them is part of the work.**

### Week 12–13: Build the shopping list app

Glyph source. CLI or simple web UI. Storage as JSON on disk. Domain model: `ShoppingItem`, `ShoppingList`, `Quantity`, `Category`, `Unit`.

**Tasks:**
1. Build the app end-to-end in Glyph. Add, remove, check off, reorder items.
2. Save and load to `~/.shopping-list.json`. **First stress test of Q8 runtime descriptors + Q21 stdlib migration pattern.**
3. Write `@example` tests for every public function. **First large body of D23-annotated Glyph code.**
4. The app must produce a real shopping list the user can take to the store.

### Week 14–17: Use the app + harvest gaps

Use the app for two weeks (weeks 14–15) before starting #2 — but the next 4 weeks are the compiler-fixing-and-stdlib-extension period. As gaps surface, fix them in the compiler/stdlib, not in the app.

**What to harvest:**
1. **Stdlib gaps.** Likely candidates: date utilities, currency/quantity formatting, fuzzy string matching for "did I mean cilantro vs coriander."
2. **Ergonomics failures.** Patterns tolerable at 200 lines but intolerable at 2000.
3. **Q15 nominal newtypes test.** `type Sku = String`, `type Quantity = Float` — does the newtype boilerplate hurt?
4. **D25 `owned` stress test.** Saving `shopping-list.json` opens a file handle; the `owned` discipline says it must be consumed before the function returns. If this feels gratuitous on a 10-line save function, escalate.
5. **Q33 `Tainted<T>` stress test** if the app gains a search box. User input → query → file read.
6. **Q34 `withBudget` stress test** if the app calls an LLM ("summarize my weekly meals").
7. **Auto-generated `T.schema` compile-time cost.** Track build time as the codebase grows past 5000 lines.

**Phase 3 acceptance:**
- [ ] Shopping list app shipped to personal use for two weeks minimum.
- [ ] Written gap list (concrete issues, not vibes) with 10–25 specific compiler/stdlib gaps prioritized as critical / nice-to-have / v1.1+.
- [ ] Stdlib extended where dogfooding demanded it (within reason — escalate before adding speculative APIs).
- [ ] Step 4's example corpus has grown to 30–50+ Glyph programs (Q2 resolved).

---

## Phase 4 — Re-lock + LSP (weeks 18–23)

### Week 18: Re-lock the syntax corpus

If phase 3 produced any breaking spec changes (a new D-decision, an overruled old one), re-run the syntax-lock review against the new corpus. The LSP about to ship bakes in syntactic assumptions; they should be final.

**Outputs:** an updated `docs/language/spec.md` if needed; a note in `docs/open-questions.md` documenting what changed and why.

### Weeks 19–23: LSP (5 weeks)

Per `docs/roadmap/07-lsp.md` updated through sessions 1–3.

**Deliverables:**
1. Diagnostics (Elm-quality bar from phase 1 week 7)
2. Hover types (reuses phase 2 narrowing query)
3. Go-to-definition (cross-module via D15)
4. Completion
5. Format-on-save (calls `glyph fmt`)
6. **Virtual document `agent://file.glyph.canonical`** (Q32). Stable line numbers `L001`, `L002`; SSA-like value names `$0`, `$1`. The LSP serves this on demand for any open Glyph file.
7. **`applyEdit` RPC** (Q29 resolved → Option B). Agents send `edit { ... } @verify { ... }` blocks; the LSP applies atomically or returns structured rejection `{ ok: false, failed: "all_tests_pass", counterexamples: [...] }`.
8. **Workspace symbol index** for discoverability (Q12).

**Latency gates (from Q6 + `archive/glyph-lsp-discussion.md`):**
- p95 < 100ms diagnostics on a warm file under 1000 lines
- p95 < 30ms completion

**Deferred to v1.1:** rename, find-references. Called out explicitly in the launch communication.

**Phase 4 acceptance:**
- [ ] LSP serves diagnostics + hover + go-to-def + completion + format-on-save with latency gates met.
- [ ] Virtual document `agent://file.glyph.canonical` is queryable for every open Glyph file.
- [ ] `applyEdit` RPC accepts edits and applies-or-rejects atomically with structured feedback.
- [ ] Workspace symbol index answers "what's importable from where."

---

## Phase 5 — Packaging, formatter polish, `glyph publish` (weeks 24–25)

Step 8 per `docs/roadmap/08-09-packaging.md`.

**Tasks:**
1. **`"glyph"` key in `package.json` schema.** Document the schema (audit metadata, import declarations). No separate `glyph.json`.
2. **`glyph publish`.** Builds, runs all tests, computes AST diff vs registry (deferred for v1.0 — registry is npm; AST-diff stub for v1.1).
3. **Audit metadata in `"glyph"` key** (Q22). `imports.<path>.audit` = `stdlib | internal | third-party`; `last_reviewed: DATE`. `glyph publish` warns/fails on stale third-party reviews.
4. **`glyph regen` polish.** First-pass shipped in phase 1 week 5; polish based on phase 3 dogfooding feedback. Hookable generator interface (OpenAI/Anthropic/local model adapters).

**Phase 5 acceptance:**
- [ ] `glyph publish` runs the local test suite and emits a publishable npm package.
- [ ] Audit metadata in `package.json` is read and enforced.
- [ ] `glyph regen` works against a sample function with `@generate by: claude`.

---

## Phase 6 — Installer + playground (weeks 26–27)

Step 9 per `docs/roadmap/08-09-packaging.md`.

**Tasks:**
1. **`npm install -g glyph`** as the canonical install. The package bundles the Rust binary per-platform (prebuilds via cargo-dist or a similar tool). No curl-pipe-bash.
2. **Playground.** Three panes:
   - Left: Glyph source (editable)
   - Center: Emitted TypeScript
   - Right: **Agent-edit preview** showing the same code with a one-line semantic change producing a one-line diff. The third pane is the demo that makes diff stability legible.
3. Default example: `loadFeed` from `examples/02_async_errors.glyph` (Result types + `?` propagation + async).
4. Hosted at a domain to be picked. (Deferral: domain choice happens at week 27.)

**Phase 6 acceptance:**
- [ ] `npm install -g glyph` installs on macOS, Linux, Windows.
- [ ] Playground compiles `loadFeed` to TS in < 1 second, side-by-side.
- [ ] Third pane demonstrates an agent edit and resulting one-line diff.

---

## Phase 7 — Docs + book outline (weeks 28–31)

Step 10. **Four weeks of concentrated authoring**, but docs were maintained continuously since phase 1.

**Deliverables:**
1. **5-minute tour** — `docs/tour.md`. Hello-world to async-with-Result.
2. **30-minute tutorial** — `docs/tutorial.md`. Build a tiny CLI (subset of the shopping list).
3. **Complete language reference** — `docs/reference/*.md`. Spec (`spec.md`) + grammar + precedence + stdlib API.
4. **Book outline** — `docs/book-outline.md`. Even if the book ships in two years, the outline forces gaps to be confronted.
5. **`--explain` content** for all 50+ error codes (the top 20 were drafted in phase 1 week 7).

**Phase 7 acceptance:**
- [ ] Tour, tutorial, and reference all complete.
- [ ] An external reviewer (engineer who's never seen Glyph) can write a working `hello, world` in 10 minutes following the tour.

---

## Phase 8 — Killer demo + benchmarks (weeks 32–39)

Step 11. **The empirical claim Glyph is making.** Without this, "designed for AI agents" is just a claim.

**Tasks:**
1. **Comprehensive benchmark.** 20+ functions across 4 languages (Glyph, TS, Python, Rust). Token count, line count, diff size for a controlled edit, agent task-success-rate when given the same task in each language.
2. **Agentic coding demonstration.** Side-by-side: same Claude (or Claude Code) instance given the same task in Glyph vs TS. Measure correctness, time-to-completion, diff size, follow-up question count.
3. **Video.** 5-minute screen recording showing the demonstration.
4. **Blog post.** Numbers + the demo video + the manifesto's bet. Title TBD; aim for one canonical "show HN" post.

**Phase 8 acceptance:**
- [ ] Benchmark results checked in for 20+ functions across 4 languages.
- [ ] Demo shows a >1.5x speedup or correctness improvement in Glyph vs TS for at least 5 representative tasks.
- [ ] Blog post and video published.

---

## Phase 9 — Launch (week 40+)

Step 12. Ongoing.

**Tasks:**
1. **Show HN** with the blog post and playground link.
2. **Conference CFPs:** Strange Loop, JSConf, AI Engineer Summit. One submission per quarter.
3. **First 100 users.** Personally onboard. They define the language's character; treat them as co-designers, not customers.

**Phase 9 acceptance:** Glyph v1.0 has 100+ developers building real things in it. Concrete: 100 GitHub usernames who've checked in at least one Glyph file in the year after launch.

---

## D-decision to phase mapping

| D | What | Implemented in |
|---|---|---|
| D1 | Significant newlines | Phase 1 week 1 (lexer) |
| D2 | Match arm commas | Phase 1 week 3 |
| D3 | `match` only | Phase 1 week 3 |
| D4 | One `fn` form | Phase 1 week 2 |
| D5 | `mut` syntactic | Phase 1 week 1 (grammar) + week 3 (no typechecker enforcement) |
| D6 | JSX sub-grammar + directives | Phase 1 week 1 (parse) + week 4 (lower) |
| D7 | Types vs values | Phase 1 week 1 (context disambiguation in parser) |
| D8 | Tagged union punctuation | Phase 1 week 3 |
| D9 | `else` vs `_` wildcards | Phase 1 week 3 |
| D10 | No object literal shorthand | Phase 1 week 5 (formatter check) |
| D11 | Spread in arrays/objects | Phase 1 week 1 |
| D12 | One string syntax | Phase 1 week 1 |
| D13 | Numeric literals | Phase 1 week 1 |
| D14 | `//` comments only | Phase 1 week 1 |
| D15 | Three import forms | Phase 1 week 2 |
| D16 | `void` keyword | Phase 1 week 3 |
| D17 | Trailing commas | Phase 1 week 1 (lexer/parser) + week 5 (formatter) |
| D18 | Precedence | Phase 1 week 1 |
| D19 | `component` keyword | Phase 1 week 2 |
| D20 | `const` module, `let` local | Phase 1 week 2 |
| D21 | `for` + `loop` | Phase 1 week 1 |
| D22 | Template literals | Phase 1 week 1 (lexer with `${` recognition) |
| D23 | `@example` inline tests | Phase 1 week 6 |
| D24 | `@redact` PII enforcement | Phase 1 week 3 (typechecker carries metadata) + Phase 1 week 4 (emit redaction at log boundaries) |
| D25 | `owned` modifier | Phase 1 week 3 |
| D26 | `@doc @run` exec docs | Phase 1 week 6 |
| D27 | Annotation meta-rule | Phase 1 week 1 (grammar) + week 6 (handler dispatch) |

---

## Cross-cutting concerns

### Benchmarks (continuous from phase 1 week 8)

Every commit on `main` re-runs the benchmark suite and updates `benchmarks/results/<commit-sha>.json`. Regressions over 10% on any metric require explanation in the commit message.

### Error message audit (continuous from phase 1 week 5)

Every new error path added to the compiler must include a structured `Diagnostic`, a span, and at least a placeholder `help` string. `--explain` content can be deferred but the error code is allocated when the path is added.

### Stdlib API stability (locked end of phase 1)

The 8 v1 stdlib modules (`result`, `option`, `array`, `string`, `io`, `json`, `fs`, `time`) get their APIs locked at the end of phase 1. Changes after that require a written justification and a one-paragraph migration note in `docs/stdlib-changes.md`.

### Documentation (continuous; concentrated in phase 7)

Every new spec decision (D28+ if any land) requires an update to `docs/language/spec.md` in the same commit. Every new stdlib function requires a `@doc """ ... """` block in the same commit (D26 makes the doc executable, so the test is whether `glyph build` passes).

---

## v1.0 acceptance criteria (the gate before launch)

Hard requirements; v1.0 doesn't ship until all are checked:

- [ ] All 4 step-2 examples + the ~50-program corpus from step 6 parse, typecheck, emit TS, and run.
- [ ] `tsc --strict --noEmit` passes on all emitted TS.
- [ ] 30+ negative examples each fail with the expected error code.
- [ ] LSP latency gates met (p95 < 100ms diagnostics, p95 < 30ms completion).
- [ ] Shopping list app shipped, in personal use for 30+ days.
- [ ] Benchmark suite shows Glyph favorably or neutrally on token count, line count, diff size across 20+ functions.
- [ ] At least one third-party engineer has successfully built and run a Glyph program from `npm install` + tour alone, with no help.
- [ ] `--explain` content for the top 50 error codes.

---

## Implementation-time open questions

Decisions deliberately deferred to the coding session that hits them. Each has a recommended default; the implementation engineer chooses at the time and updates this section.

| # | Decision | Default | Triggered in |
|---|---|---|---|
| I1 | `resource` keyword vs `@resource` annotation for D25 marker | `resource` keyword (consistent with `record`, `component`) | Phase 1 week 3 |
| I2 | `@example` with multiple expressions vs single `==` per line | Single `==` per line (D23 as written) | Phase 1 week 6 |
| I3 | `glyph regen` generator adapter interface | Synchronous trait with three methods: `name()`, `generate(spec) -> Result<String, Err>`, `cost_estimate(spec)` | Phase 1 week 5 |
| I4 | Salsa query granularity (per-file vs per-declaration) | Per-file inputs, per-declaration intermediates | Phase 1 week 2 |
| I5 | Sandboxed interpreter implementation (tree-walking vs bytecode) | Tree-walking AST interpreter (~1000 LoC; bytecode is v2) | Phase 1 week 6 |
| I6 | LSP virtual-document update strategy (push vs poll) | Push on file save; poll on explicit RPC | Phase 4 week 19 |
| I7 | Glyph package format (npm tarball vs custom) | npm tarball (Q22 resolved → ride npm) | Phase 5 week 24 |

---

## Status checklist

**Phase 0 complete (2026-05-26).** Phase 1 week 1 is the next concrete action.

- [x] P1: Validator example rewritten (Q1) — `examples/01_validator.glyph` uses explicit `<Out>` type parameter
- [x] P2: Rust workspace bootstrapped — `glyph-compiler/` with 8 crates, all `cargo check` targets
- [x] P3: `examples/` at repo root with 4 files
- [x] P4: `benchmarks/` scaffold with 3 functions in 4 languages — first smoke-test run already gave honest early signal (Glyph wins on `load_feed`, loses on tiny `slugify`)
- [x] P5: Library versions locked in `Cargo.toml` workspace

### What Phase 0 produced

- `examples/01_validator.glyph` — Q1 rewrite. Caller declares output type explicitly; mapped types deferred to v1.1.
- `examples/02_async_errors.glyph`, `03_react_component.glyph`, `04_cli_tool.glyph` — faithful transfers with D22 template literals in places where original used `+`.
- `examples/README.md` — index documenting the four examples and the Q1 deviation.
- `glyph-compiler/Cargo.toml` — workspace with 8 crate members, `[workspace.dependencies]` pinning `salsa = "0.26"`, `ariadne = "0.4"`, `insta = "1"`, `proptest = "1"`, `tower-lsp = "0.20"`, `tokio = "1"`, `clap = "4"`, `serde = "1"`, `thiserror = "1"`.
- `glyph-compiler/rust-toolchain.toml` — pinned to Rust 1.95 with rustfmt + clippy.
- `glyph-compiler/README.md` — crate-by-crate layout reference.
- 8 crate stubs (`glyph-lexer`, `glyph-ast`, `glyph-parser`, `glyph-resolver`, `glyph-typechecker`, `glyph-emit`, `glyph-runtime`, `glyph-cli`). Each documents which D-decisions and which Phase 1 week it implements.
- `benchmarks/` with `parse_user` / `load_feed` / `slugify` across Glyph, TypeScript, Python, Rust. `measure.sh` produces `results/<timestamp>.json` with line counts (token counts and diff-size wire up Phase 1 week 8).

### Phase 0 verification (2026-05-26)

Rust 1.95.0 stable installed via rustup. Workspace verified end-to-end:

```
cargo check --workspace    →  All 8 crates compile cleanly (52s, cold)
cargo test --workspace     →  All 7 stub tests pass
cargo build --release      →  glyph binary builds (27s)
./target/release/glyph --help  →  prints clap-generated help with build/run/fmt/regen/publish
./target/release/glyph build src/ --out dist/  →  exits 1 with "phase 0 stub: `glyph build` not yet implemented"
```

Phase 0 acceptance is hard-passed, not just file-correct.

### Next action

Phase 1 week 1: lexer + Pratt parser + AST + golden tests. See the week-1 task list above.

---

## Phase 1 week 1 status (day 1–2 slice shipped, 2026-05-26)

**Real code merged**, not stubs. 27 tests pass across the workspace.

### Implemented this slice (lexer + AST + parser day 1–2)

**glyph-lexer** (~330 LoC of real lexer code; 9 tests):
- D1 significant newlines outside brackets via `bracket_depth` tracking
- D12 string literals with escape sequences
- D13 numeric literals with `_` separators, decimals, exponents
- D14 `//` line comments (block comments rejected with explicit error)
- D17 trailing commas (passed through; parser enforces)
- D21 `for`/`loop`/`break`/`continue` keywords lexed
- D22 strings tokenize but `${...}` interpolation parsing is deferred (lexed opaquely)
- D27 `@<name>` annotation prefix lexed (`At` token + identifier)
- Multi-char punctuation: `->`, `=>`, `==`, `!=`, `<=`, `>=`, `&&`, `||`, `??`, `?.`, `..`, `...`

**glyph-ast** (~230 LoC of enum definitions):
- `Module`, `Decl::{Import, Fn, Type, Const}`, `Stmt::{Let, Return, Expr}`, `Expr::{Number, String, Bool, Void, Ident, Binary, Unary, Postfix, Call, Member, Index, Await, Array, MatchPlaceholder}`, `TypeExpr::{Path, Generic, FnPlaceholder, UnionPlaceholder}`, `Pattern::{Wildcard, Ident, ConstructorPlaceholder}`, `Annotation`
- Every node carries a `Span`. Identifiers are `Arc<str>`.

**glyph-parser** (~520 LoC including the Pratt expression parser; 14 tests, including 1 insta snapshot):
- `module path/name` declaration (D15)
- All three import forms (`namespace`, `{ Named }`, `as alias`) (D15)
- `fn` declarations with parameters, return type, async modifier, and function body
- Pratt expression parser at levels 4–11 (D18): arithmetic, comparison, logical, nullish-coalesce, prefix `!`/`-`, prefix `await`, postfix `?`, member access `.`/`?.`, index `[]`, call `()`
- Array literal with spread (D11)
- `let` statement with optional `owned` modifier (D25 syntactic-only for now), optional type annotation, expression
- `return` statement
- Type expression: dotted path + generic args
- **Snapshot test infrastructure via `insta`** — `tests/snapshots.rs` + `tests/fixtures/hello.glyph` + checked-in `.snap` file

### Deferred to week 1 day 3+

- **D22** template literal `${...}` interpolation (currently lexed opaquely as a single string)
- **D6** JSX sub-grammar
- **D3** `match` expression (parser yields `MatchPlaceholder` and crudely skips the body so fixtures parse)
- **D2/D9** pattern matching beyond `Pattern::ConstructorPlaceholder`
- **D8** tagged unions in type expressions
- **D5** `mut` statement
- **D25** owned-modifier consumption analysis (parser accepts the keyword, no analysis)
- **D21** `for x in iter { }` and `loop { }` statement parsing (keywords lex)
- **D27** annotations on declarations (lexer emits `@` + ident; parser doesn't attach to decls yet)
- Generics on `fn` declarations (only generic args at type positions)
- Error recovery via skip-to-next-statement-boundary

### Acceptance status

The week-1 acceptance criterion is "all 4 example files parse to AST with snapshots checked into git." Currently:
- `tests/fixtures/hello.glyph` (small representative fixture) **parses** ✓ — snapshot checked in.
- `examples/01_validator.glyph` through `04_cli_tool.glyph` **do not yet parse** — they use match expressions, JSX, tagged unions, `mut`, and `for` loops that the day-3+ work brings online.

### Test summary

| Crate | Tests | Status |
|---|---|---|
| glyph-lexer | 9 | All pass |
| glyph-ast | 1 | All pass |
| glyph-parser (lib) | 13 | All pass |
| glyph-parser (snapshot) | 1 | All pass — insta snapshot checked in |
| glyph-resolver, glyph-typechecker, glyph-emit, glyph-runtime, glyph-cli | 1 each (stubs) | All pass |
| **Total** | **27** | **All pass** |

## Phase 1 week 1 day 3 status (shipped 2026-05-26)

**Match expressions + tagged unions + record types + type/const/generic declarations landed. `02_async_errors.glyph` now parses end-to-end.**

### Implemented this slice (parser day 3 + AST expansion)

**glyph-ast** additions:
- `Expr::Match { scrutinee, arms }`, `Expr::Object { fields }`, `Expr::Lambda { params, return_ty, body }` — replacing day-2 placeholders.
- `MatchArm { pattern, body }` with `MatchArmBody::{Expr, Block}`.
- `ObjectField::{KeyValue, Spread}` (D11 inside object literals).
- `Pattern::{Else, Literal, Constructor (with arg patterns), Object, IsType}` — replacing day-2 placeholder. `Pattern::ArrayPlaceholder` remains for day 4.
- `TypeExpr::{Fn, Record, Union}` — replacing the day-2 placeholders. `RecordTypeField` carries optional flag. `UnionVariant` carries optional payload.
- `FnDecl.generics` and `TypeDecl.generics` (D7 generic parameters on declarations).
- `GenericParam { name, bounds, span }` — bounds always empty in v1 (substep 5a will populate).

**glyph-parser** additions:
- Real `match` expression parser (D2/D3/D9). Comma between arms is required per D2.
- Pattern parser (`pat.rs`) covering literal, identifier binding, wildcard `_`, `else` catch-all, constructor with nested args, object pattern with `{ key }` or `{ key: alias }`, `is TypeName` guard.
- Object literal parser with D10 shorthand-forbidden enforcement (parser produces "expected `:` after field name (D10: no shorthand)") and D11 spread.
- Tagged union parser for both single-line (`A | B | C`) and multi-line (`| A\n  | B`) forms per D8.
- Inline record type parser `{ field: Type, optional?: Type }`.
- `fn(args) -> T` function type expressions.
- `type X<T> = ...` and `const X = ...` top-level declarations.
- Generic parameters on `fn` and `type` declarations.
- Lambda expressions: `fn(args) { body }` and `fn(args: T) -> U { body }` (D4 anonymous form).
- **Keyword-as-field-name** support: `Token::as_field_name()` lets keywords act as identifiers in object keys, record field names, named-import items, and object-pattern fields.
- **Soft-keyword-as-identifier** support: modifier keywords (`owned`, `resource`, `mut`, `as`, `type`, etc.) are demoted to identifier expressions in expression position.

### Acceptance gates this slice

- `examples/02_async_errors.glyph` parses to AST end-to-end. **Snapshot checked in** (`tests/snapshots/snapshots__example_02_async_errors_parses.snap`, 2,623 lines).
- Three example snapshot tests added with `#[ignore]` plus an always-passing diagnostic test (`day3_progress_report`) that reports byte-offset of the first parse error per file. Use it to track day-4 progress.

### Remaining example-parse blockers

| File | Blocker | Earliest week-1 day |
|---|---|---|
| `01_validator.glyph` | `for key, sub_schema in shape` (D21 `for` with destructuring) | Day 4 |
| `02_async_errors.glyph` | ✅ **PARSES** | — |
| `03_react_component.glyph` | `component Foo(props) -> Component { ... }` (D19) + D6 JSX | Day 5–6 |
| `04_cli_tool.glyph` | `["help", ..._]` array patterns + `mut x[k] = v` mut statement | Day 4 |

### Updated test summary

| Crate | Tests | Notes |
|---|---|---|
| glyph-lexer | 9 | unchanged |
| glyph-ast | 1 | unchanged (struct construction smoke test) |
| glyph-parser (lib) | 24 | +11 from day 3: type decls (4), match expressions (4), object literal (2), keyword-as-field (1) |
| glyph-parser (snapshots) | 3 (1 active, 3 ignored, 1 diagnostic) | `hello.glyph` + `02_async_errors.glyph` snapshots checked in |
| Stubs (resolver / typechecker / emit / runtime / cli) | 1 each | unchanged |
| **Total** | **42** | All pass (3 `#[ignore]` example snapshots tracked separately) |

## Phase 1 week 1 day 4 status (shipped 2026-05-26)

**Three of four example files parse end-to-end.** Only `03_react_component.glyph` remains, gated on component declarations and JSX (day 5+).

### Implemented this slice

**AST additions:**
- `Stmt::{Mut, For, Loop, Break, Continue}` with `MutKind::{Assign, AssignIndex, AssignField, MethodCall}` (D5 + D21)
- `Pattern::Array { elements, rest }` replacing the placeholder (D9 + D11 spread)
- `Pattern::Constructor` extended to `path: Vec<Ident>` for dotted-path variants like `fs.ErrorKind.NotFound`

**Parser additions:**
- `mut` statement parser enforcing D5's grammar restriction (only assignment, indexed assignment, field assignment, or method call — anything else is a syntax error citing D5)
- `for X in iter`, `for K, V in iter`, `loop { }`, `break`, `continue` (D21)
- Array pattern parser with rest element (D9)
- Dotted-path variant patterns (extends `Pattern::Constructor`)
- Match arm body extended to accept single-statement bodies (`Ok(_) => return 0`, `Ok(v) => mut x = v`); previously only expressions and blocks
- `looks_like_object_literal` extended to recognize soft-keyword keys and `...` spread

### Spec deviation found and resolved

`examples/01_validator.glyph` had a **D20 violation** — `let` and `match` at top-level in its "example usage" section. Per D20 (`const` module-level, `let` function-level), these are syntactically illegal. The example was updated to:
- Keep `type User = { ... }` at module level
- Promote `let user_schema:` to `const user_schema:`
- Wrap `let input` and the `match` in a `fn demo() { ... }`

The example is now D20-compliant and more representative of how real Glyph code structures schema usage.

### Example parse status

| File | Status | Snapshot lines |
|---|---|---|
| `01_validator.glyph` | ✅ PARSES | 2,931 |
| `02_async_errors.glyph` | ✅ PARSES (since day 3) | 2,641 |
| `03_react_component.glyph` | ❌ component + JSX (day 5+) | — |
| `04_cli_tool.glyph` | ✅ PARSES | 6,498 |

Total checked-in snapshot: 12,520 lines across the 3 examples plus hello-world.

### Test summary after day 4

| Crate | Tests | Notes |
|---|---|---|
| glyph-lexer | 9 | unchanged |
| glyph-ast | 1 | unchanged |
| glyph-parser (lib) | 32 | +8 from day 4: for (2), loop with break/continue (1), mut variants (3), array patterns (1), dotted variant patterns (1) |
| glyph-parser (snapshots) | 5 (4 active, 1 ignored, 1 diagnostic) | 01, 02, 04 plus hello-world all snapshot in |
| Stubs (resolver / typechecker / emit / runtime / cli) | 1 each | unchanged |
| **Total** | **51** | All pass (1 `#[ignore]` for example 03 — pending day 5+) |

### Still deferred

- D6 JSX sub-grammar (day 5–6)
- D19 `component` declaration (day 5)
- D22 template literal `${}` interpolation parsing (still tokenized opaquely)
- D27 annotations on declarations (lexer emits `@example`; parser doesn't attach to `fn`/`type` decls yet)
- Error recovery via skip-to-next-statement-boundary
- The proper contextual-keyword refactor (currently using the day-3 soft-keyword fallback in `expr.rs`)

## Phase 1 week 1 day 5 status (shipped 2026-05-26)

**All 4 example files now parse end-to-end.** Week-1 acceptance criterion ("all 4 example files parse to AST with snapshots checked into git") is met.

### Implemented this slice

**AST additions:**
- `Decl::Component` with `ComponentDecl` (name, annotations, generics, params, return_ty, body)
- `Expr::Jsx(JsxElement)`
- `JsxElement { name, attrs, children, self_closing, span }`
- `JsxAttr::{String, Expr, Positional}` — positional supports `<case Loaded>`-style attrs
- `JsxChild::{Element, Expr, Text}`

**Parser additions:**
- `component` top-level declaration (D19), parallel to `fn` with optional `-> Component` return type
- `jsx.rs` sub-module implementing the JSX parser
- `Cursor` now holds `&'a str source` for text-run reconstruction
- JSX disambiguation in `parse_primary`: `<` followed by identifier-like token → JSX, otherwise error
- Text run reconstruction by slicing source between the closing `>` of an opening tag and the next `<` or `{`
- Directive elements (`<if>`, `<else>`, `<for>`, `<match>`, `<case>`) parse as ordinary JSX elements per D6 — keywords are accepted as JSX names via `Token::as_field_name()`
- Self-closing tags (`<Foo />`), explicit close tags (`</name>`) with name-match validation

### Example parse status — week 1 acceptance met

| File | Status | Snapshot lines |
|---|---|---|
| `01_validator.glyph` | ✅ PARSES | 2,931 |
| `02_async_errors.glyph` | ✅ PARSES | 2,641 |
| `03_react_component.glyph` | ✅ PARSES (21 Jsx nodes in the snapshot) | 2,190 |
| `04_cli_tool.glyph` | ✅ PARSES | 6,498 |

Total checked-in AST snapshots: 14,710 lines.

### Test summary after day 5

| Crate | Tests | Notes |
|---|---|---|
| glyph-lexer | 9 | unchanged |
| glyph-ast | 1 | unchanged |
| glyph-parser (lib) | 37 | +5 from day 5: component decl (1), JSX self-closing with attrs (1), JSX with children + text (1), JSX directive `<case>` with positional attr (1), JSX expression child (1) |
| glyph-parser (snapshots) | 6 (all active) | All 4 example files + hello-world + diagnostic |
| Stubs (resolver / typechecker / emit / runtime / cli) | 1 each | unchanged |
| **Total** | **58** | All pass — no `#[ignore]`'d tests remaining |

### Still deferred (day 6+)

- D22 template literal `${expr}` interpolation parsing (currently tokenized opaquely; strings parse but contain the raw `${...}` text)
- D27 annotations on declarations (lexer emits `@example`; parser doesn't attach to `fn`/`type` decls)
- Error recovery via skip-to-next-statement-boundary
- Contextual-keyword refactor (currently using the day-3 soft-keyword fallback)
- Generic *call sites* — `json.parse<TodoFile>(text)` currently parses as a chained comparison `((json.parse < TodoFile) > text)`. The AST is technically valid but semantically wrong; the typechecker will reject it. Fix is a lookahead/backtrack in `parse_postfix` when seeing `<` after a member expression — day 6 cleanup.

## Phase 1 week 1 day 6 — WEEK 1 COMPLETE (shipped 2026-05-26)

**Three day-6 cleanups landed; week 1 acceptance is fully met with semantically-correct ASTs.**

### Implemented this slice

**D22 — template literal interpolation:**
- `Expr::TemplateString { parts, span }` with `TemplatePart::{Text, Expr}` alternation
- In `parse_primary`, when the lexed string contains `${`, post-process by walking the de-escaped content, finding balanced `${...}` regions (tracking brace nesting and string literals inside), and recursively re-parsing each interpolation via a synthetic `module __template fn __f() { return EXPR }` wrapper
- **V1 limitation**: literal `${` is indistinguishable from interpolation because `\$` de-escapes to `$` at the lexer level. The lexer needs a proper template-literal mode to fix this — deferred to v1.1. Workaround in v1 is string concatenation.

**Generic call sites — lookahead heuristic:**
- `Expr::Call` now carries `type_args: Vec<TypeExpr>`
- `parse_postfix` checks `<` via `looks_like_generic_call` before falling through to `parse_cmp`. The lookahead scans for balanced `<...>` followed by `(`, aborting on any token that can't appear in a type expression (binary operators, statement keywords, etc.)
- Fixes the 04 snapshot's previously-incorrect `((json.parse < TodoFile) > text)` shape
- Same heuristic-with-pessimistic-abort approach TypeScript uses; accepts the rare false positive `a < b > (c)` case

**D27 — annotations on declarations:**
- `parse_top_level` collects leading `@<name> <args>` lines via `parse_annotations` before dispatching to a declaration parser
- Annotations attach to `Fn`, `Type`, `Const`, `Component` decls (rejected on `Import`)
- Raw args captured as source slice (per `Annotation.raw_args`); the typechecker parses them later

### Test summary after day 6 — **WEEK 1 COMPLETE**

| Crate | Tests | Notes |
|---|---|---|
| glyph-lexer | 9 | unchanged |
| glyph-ast | 1 | unchanged |
| glyph-parser (lib) | 46 | +9 from day 6: D22 (4), generic call sites (2), D27 annotations (3) |
| glyph-parser (snapshots) | 6 | all 4 examples + hello-world + diagnostic, all active |
| Stubs (resolver / typechecker / emit / runtime / cli) | 1 each | unchanged |
| **Total** | **67** | **All pass — no `#[ignore]`'d tests** |

### Snapshot updates from day 6 changes

| File | Day 5 | Day 6 | Δ |
|---|---|---|---|
| `01_validator.glyph` | 2,931 | 3,063 | +132 (D22 TemplateString nodes in the demo printf) |
| `02_async_errors.glyph` | 2,641 | 2,743 | +102 (D22 in `/api/users/${id}` URL strings) |
| `03_react_component.glyph` | 2,190 | 2,201 | +11 (call sites gained `type_args: []`) |
| `04_cli_tool.glyph` | 6,498 | 7,204 | +706 (D22 in `${number.to_string(...)}` printfs + `json.parse<TodoFile>(text)` is now a Call with type_args instead of mis-shapen comparison) |
| **Total** | 14,710 | **15,664** | +954 lines of AST detail |

### Week 1 acceptance — FULLY MET

- [x] Lexer covering D1, D12, D13, D14, D17, D21, D22 (with v1 interpolation limitation), D27
- [x] AST with full enum architecture; Span on every node; `Arc<str>` idents
- [x] Pratt parser with D18 precedence
- [x] All 4 example files parse to AST end-to-end with snapshots checked into git
- [x] `insta` snapshot infrastructure with auto-update workflow
- [x] All 27 D-decisions parseable to some degree (some semantics deferred to typechecker)

### Still deferred (week 2+)

- Lexer-level template-literal mode (D22) to distinguish `\${` from `${` — v1.1 cleanup
- Error recovery via skip-to-next-statement-boundary — Phase 1 week 7 error-message audit
- Contextual-keyword refactor (currently using the day-3 `is_soft_keyword_in_expr_position` fallback) — week 7 cleanup if it shows up in dogfooding

## Phase 1 week 1 day 7 — /simplify cleanup pass (2026-05-26)

Two `/simplify` passes ran: the first reviewed day-6 changes only; the second covered days 1–5 (which had been written without review). Both used the three-agent parallel pattern (reuse / quality / efficiency).

### Day-6 review fixes applied

1. **Eliminated the `extract_template_expression` synthetic-module wrapper.** D22's `split_template_parts` now calls `crate::parse_expression(source)` directly instead of wrapping the interpolation in `module __template fn __f() { return EXPR }` and parsing the whole thing. Inner expression spans are now correctly relative to the interpolation source; `extract_template_expression` was deleted. (Snapshot regen confirmed the span correction.)
2. **`split_template_parts` returns `Option<Vec<TemplatePart>>`** — the upfront `value.contains("${")` guard was dropped (one walk instead of two).
3. **Added `Cursor::parse_comma_separated<T>(terminator, skip_newlines, item_fn)`** and refactored ~9 hand-rolled comma-loop sites: call args (`parse_postfix`), generic call type args, fn params (`parse_fn`), component params, generic params (`parse_generic_params`), lambda params, record-type fields, fn-type params, constructor-pattern args, object-pattern fields.
4. **Deleted `_expr_use_marker`** dead code in `decl.rs`.
5. **Cheap pre-checks added to `looks_like_generic_call`**: O(1) `is_callable_receiver` and first-token-type filter; the 200-token scan now only runs when the receiver is a callable shape and the token after `<` is type-shaped.
6. Minor comment cleanup; deleted unused `Cursor::peek_skipping_newlines_is`.

### Days-1-5 review fixes applied

1. **`parse_postfix` no longer clones `expr` 6× per chain step.** Pattern: extract `let start = expr.span().start;` first, then `Box::new(expr)`. Real perf win on long member/call/index/postfix-? chains.
2. **`Token::keyword()` and `Token::as_field_name()` now both walk a single `KEYWORDS` static.** Drift between the two ~28-arm tables is structurally impossible.
3. **Deleted redundant `Lexer.bytes` field** — it was just `source.as_bytes()` cached. Inline `self.source.as_bytes()` at the 3 use sites.
4. **Added `Span::join(self, end: Span) -> Span` helper.** Available for incremental adoption; 23-site refactor of `Span::new(a.start, b.end)` deferred.
5. **Extracted `parse_callable_signature(p) -> CallableSignature`** shared by `parse_fn` and `parse_component`. Removed ~25 lines of D4+D19 parallel code.
6. **Split `parse_pattern(p, allow_else: bool)` into `parse_pattern(p)` and `parse_arm_pattern(p)`.** No more boolean flag through every recursive call.
7. **`jsx_name` now delegates to `expect_field_name`.** ~20 lines deleted.
8. **Refactored 4 remaining manual comma loops** (named imports, mut-method-call args, array literal with spread, object literal with spread) to use `parse_comma_separated`.
9. **Cleaned up `type_to_variant`** — removed `let _ = args.drain(..); let _ = base;` dead-code drainage by using struct pattern with `..`.
10. **Fixed stale doc comment** in `parser/src/lib.rs` that referenced deleted `Pattern::ConstructorPlaceholder`.

### Skipped (intentional)

- **`Span::join` mass refactor** (23 sites) — helper in place; future passes can adopt incrementally without bug-risk per site.
- **`Token::Display` impl + `ExpectedKind` enum** — Phase 1 week 7 Elm-quality error-message audit territory.
- **`AST::span()` boilerplate / `Node<T>` wrapper / `#[derive(Spanned)]` proc macro** — too invasive for /simplify.
- **Identifier interning** — implementation plan I4 explicitly defers (`Arc<str>` is fine for v0).
- **JSX text → `Arc<str>`** — on reconsideration, the AST has a consistent convention: `Arc<str>` for names, `String` for text content. JSX text is content.

### Net result

| Metric | Before week-1 day 7 | After |
|---|---|---|
| Workspace tests | 67 | 67 |
| Snapshot lines | 15,664 | 15,664 (spans corrected, count nominally same) |
| Comma-loop hand-rolls | ~13 | 0 (all use `parse_comma_separated`) |
| Dead-code items | 4 (`_expr_use_marker`, `Lexer.bytes`, `extract_template_expression`, `type_to_variant` drains) | 0 |
| Keyword-table drift risk | 2 separate tables | 1 source of truth |
| `parse_postfix` clones per chain step | 1 per node | 0 |

## Phase 1 week 2 day 1 status (shipped 2026-05-26)

**Name resolution against the four example files is in.** Every identifier in
every example file resolves to a local binding, a module-level symbol, an
imported-name wrapper, or a prelude built-in.

### Implemented this slice

**glyph-typechecker** (new):
- `Ty` enum (`ty.rs`, ~150 LoC): `Unknown` (compiler placeholder), `Prim`
  (string/number/bool/void), `UnknownTop` (user-facing `unknown`), `Named`,
  `Param`, `App`, `Record`, `Fn`, `Union`, `Tuple`, `Var`. No mapped types
  (Q1 → v1.1), no refinement types (Q15).
- `TypeMap` (`type_map.rs`, ~50 LoC): span-keyed map for "every Expr gets a Ty"
  bookkeeping. The week-3 typechecker fills this.

**glyph-resolver** (real implementation, was stub):
- `Symbol` + `SymbolKind` (`symbol.rs`, ~180 LoC). `SymbolKind` covers
  Function/Type/Const/Component/Variant (the new one, for tagged-union
  variants hoisted to module scope), ImportNamespace/ImportAlias/ImportNamed,
  and Prelude.
- `PreludeKind` enum: closed list of built-in primitives, generics, and
  values. Decouples the typechecker boundary from string matching.
- `Prelude` (`prelude.rs`, ~80 LoC): primitives (string/number/bool/void/unknown),
  generic containers (Result/Option/Array/Record/Schema/Component), value
  constructors (Ok/Err/Some/None), `par` namespace, `print` built-in.
- `ModuleSymbols` + `collect_module_symbols` (`collect.rs`, ~200 LoC). Walks
  top-level decls; introduces variant names alongside their type decl;
  enforces no-duplicate-top-level and no-relative-imports (D15).
- `ResolvedModule` + `ResolutionMap` + `resolve_module` (`resolve.rs`, ~380
  LoC). Pure-function walker over the AST. Three-tier name lookup
  (local → module → prelude). Generic type parameters bind into the
  declaration's scope so `T` and `Out` resolve inside fn bodies. JSX
  directive bindings handled: `<for X in={iter}>` introduces `X` as a child
  binding; `<case Variant bind={X}>` introduces `X` as a binding visible to
  children.

**glyph-resolver/tests/examples.rs** — week-2 acceptance integration tests
against the four example files.

### Acceptance

| File | Total errors | Unresolved names |
|---|---|---|
| `01_validator.glyph` | 0 | — |
| `02_async_errors.glyph` | 0 | — |
| `03_react_component.glyph` | 0 | — |
| `04_cli_tool.glyph` | 0 | — |

**The first half of week-2 acceptance is met:** every example file resolves all
names. The second half ("every expression node has a type") lands in the
day-2+ slice when `TypeMap` is populated.

### Deferred to week 2 day 2+

- **Cross-module verification**. `import std/result { Ok, Err }` is accepted in
  the importing module, but the resolver does not yet load the target module
  and check that `Ok`/`Err` actually exist there. This is the "module graph"
  half of week 2; needs a stdlib-module synthesis layer or stubs.
- **Expression type assignment**. `TypeMap` exists; nothing populates it yet.
  Day 2's job: walk every expression and write at least `Ty::Unknown` for
  every node, with concrete types pulled from declared function signatures
  and `const` annotations.
- **Salsa wiring (I4)**. The pipeline is pure-function pipe-by-hand right
  now. Wrap `parse → collect → resolve → typemap` as salsa-tracked queries
  with per-file inputs and per-declaration intermediates.
- **D15 barrel-file detection**. Needs the module graph to spot
  "this module only re-exports."

### Test summary after week 2 day 1

| Crate | Tests | Notes |
|---|---|---|
| glyph-lexer | 9 | unchanged |
| glyph-ast | 1 | unchanged |
| glyph-parser (lib) | 46 | unchanged |
| glyph-parser (snapshots) | 6 | unchanged |
| **glyph-resolver (lib)** | **21** | **+20**: 5 collect, 4 prelude, 8 resolve, 2 symbol-table, 2 smoke |
| **glyph-resolver (examples)** | **3** | **+3**: progress_report, example_02, duplicate_detection |
| **glyph-typechecker** | **6** | **+5**: 3 ty, 2 type_map |
| glyph-emit, glyph-runtime, glyph-cli | 1 each | unchanged |
| **Total** | **94** | **All pass** (up from 67) |

## Phase 1 week 2 day 2 status (shipped 2026-05-26)

**Second half of week-2 acceptance is met:** every expression node in every
example file has a `Ty` entry in the `TypeMap`. Most entries are
`Ty::Unknown` and will be refined by the week-3 bidirectional checker; this
slice ships the side table and the static lowering that produces concrete
types for the parts day-2 can compute directly.

### Implemented this slice

**glyph-typechecker** additions:
- `lower.rs` (~140 LoC, 6 tests): `lower_type_expr(te, resolved, prelude) -> Ty`
  turns a `glyph_ast::TypeExpr` into a `Ty` using the resolver's resolution
  map. Handles `Path` against prelude primitives + generic containers + user
  type decls, `Generic` lowering to `Ty::App`, function types, record types,
  unions. Generic parameter references lower to `Ty::Param`. Multi-segment
  paths (e.g. `http.Response`) lower to `Ty::Unknown` until cross-module pass
  lands.
- `assign.rs` (~270 LoC, 5 tests): `assign_types(module, resolved, prelude)
  -> TypeMap` walks every expression and records a `Ty` for each. Concrete
  types are emitted for the cases we can determine statically without
  inference: number/string/template-string/bool/void literals, function
  references (lower the signature), component references, lambda
  expressions. Operator results, calls, member access, indexing, await, and
  match are intentionally `Ty::Unknown` — propagating those is the bidirectional
  checker's job in week 3.
- `From<glyph_resolver::SymbolId> for SymbolRef` conversion at the
  resolver↔typechecker boundary.

**TypeMap and ResolutionMap keying fix:** both side tables previously keyed
by `span.start` alone, which collides for nested chains like `foo.bar.baz`
(three Member expressions all starting at byte 0). Fixed to key by the full
`(start, end)` pair. A regression test in `type_map.rs` covers the
foo.bar.baz case. Concrete type-entry counts on the examples roughly
doubled after the fix — the prior keying was silently overwriting outer
Member types with inner Ident types.

### Acceptance

| File | Expression spans | With Ty entry | Concrete (non-Unknown) |
|---|---|---|---|
| `01_validator.glyph` | 153 | 153 | 20 (12 string, 1 number, 7 fn) |
| `02_async_errors.glyph` | 135 | 135 | 21 (6 string, 5 number, 10 fn) |
| `03_react_component.glyph` | 101 | 101 | 10 (2 string, 2 number, 6 fn) |
| `04_cli_tool.glyph` | 440 | 440 | 67 (25 string, 8 number, 4 bool, 24 fn) |

**Week-2 acceptance — fully met:**
- [x] Every example file resolves all names (day 1)
- [x] Every expression node has a `Ty` entry (day 2)

### Deferred to week 2 day 3+

- **Cross-module verification** (`import std/result { Ok }` must check that
  `Ok` exists in `std/result`). Needs synthetic stdlib module stubs or a
  real module graph spanning files.
- **Salsa wiring (I4)**. Pipeline is still pure-function pipe-by-hand;
  wrap `parse → collect → resolve → typemap` as salsa-tracked queries with
  per-file inputs and per-declaration intermediates.
- **Local-binding type propagation**. Right now an `Ident` resolving to a
  `Local` is `Ty::Unknown` even when the binding is a typed parameter. A
  tiny scope→type-map side table during the assign walk would lift the
  concrete-count substantially without crossing into week-3 inference
  territory.

### Test summary after week 2 day 2

| Crate | Tests | Notes |
|---|---|---|
| glyph-lexer | 9 | unchanged |
| glyph-ast | 1 | unchanged |
| glyph-parser (lib) | 46 | unchanged |
| glyph-parser (snapshots) | 6 | unchanged |
| glyph-resolver (lib) | 21 | unchanged |
| glyph-resolver (examples) | 3 | unchanged |
| **glyph-typechecker (lib)** | **20** | **+14**: 6 lower, 5 assign, +1 type_map regression test, +2 ty (already there) |
| **glyph-typechecker (examples)** | **2** | **+2**: every-expr-has-a-type, typed-count diagnostic |
| glyph-emit, glyph-runtime, glyph-cli | 1 each | unchanged |
| **Total** | **110** | **All pass** (up from 94) |

## Phase 1 week 2 day 3 status (shipped 2026-05-26)

**Local-binding type propagation lands.** Identifier references to typed
function/component/lambda parameters and to typed `let` bindings now resolve
to their declared type instead of `Ty::Unknown`. The concrete-entry count
roughly doubles on every example.

### Implemented this slice

`Assigner` gained a `local_tys: HashMap<u32, Ty>` keyed by the resolver's
def-site span start (the same key `ResolvedRef::Local` carries). The map is
populated from three sources:

- Function and component parameters — via a new `bind_param_tys` helper
  called once per declaration before walking its body.
- Lambda parameters — same helper at the `Expr::Lambda` arm of `walk_expr`.
- Typed `let` bindings — the `Stmt::Let` arm lowers `l.ty` if present and
  records the result under `l.span.start`.

`type_of_ident_ref` consults the map for `ResolvedRef::Local(def_start)`
before falling through to `Ty::Unknown`. Untyped `let` bindings, for-loop
bindings (which share a def-site span across K/V), and match-arm payload
bindings remain `Unknown` — the bidirectional checker handles those in
week 3.

### Acceptance

| File | Concrete entries — before day 3 | After day 3 |
|---|---|---|
| `01_validator.glyph` | 20 | 37 |
| `02_async_errors.glyph` | 21 | 34 |
| `03_react_component.glyph` | 10 | 19 |
| `04_cli_tool.glyph` | 67 | 80 |

The lift is largest where the example has many typed parameters that get
read multiple times in the body.

### Test summary after week 2 day 3

| Crate | Tests | Notes |
|---|---|---|
| glyph-lexer | 9 | unchanged |
| glyph-ast | 1 | unchanged |
| glyph-parser (lib) | 46 | unchanged |
| glyph-parser (snapshots) | 6 | unchanged |
| glyph-resolver (lib) | 21 | unchanged |
| glyph-resolver (examples) | 3 | unchanged |
| **glyph-typechecker (lib)** | **23** | **+3**: typed-param propagates, typed-let propagates, untyped-let stays unknown, lambda-param propagates (replaced one obsolete negative test) |
| glyph-typechecker (examples) | 2 | unchanged |
| glyph-emit, glyph-runtime, glyph-cli | 1 each | unchanged |
| **Total** | **113** | **All pass** (up from 110) |

### Still deferred to week 2 day 4+

- **Cross-module verification**. Stdlib stubs and a module graph.
- **Salsa wiring (I4)**.
- **For-loop binding spans**. Currently two bindings on `for k, v in iter`
  share a def-site span; either give bindings per-binding spans (AST
  change) or accept that K and V are not differentiable in `local_tys`.
- **Match-arm payload typing**. Needs scrutinee→pattern type flow, which is
  bidirectional-checker territory.

## Phase 1 week 2 day 4 status (shipped 2026-05-29)

**Cross-module verification lands.** `import std/result { Ok }` now checks
that `Ok` is an export of `std/result`. The day-4 slice covers the import
side of the module graph; full module-graph traversal (parsing other Glyph
files in a project and using their actual exports) waits for the salsa
wiring (day 5+).

### Implemented this slice

**glyph-resolver** additions (`module_graph.rs`, ~270 LoC, 8 lib tests + 2
integration tests):

- `ModuleGraph` trait with `exports_of(path) -> Option<&ModuleExports>`.
  Permissive default: `None` means "unknown module, skip verification" so
  third-party packages (`react`) and project-local modules (`api/users`)
  don't error until the Phase 5 package manifest lands.
- `ModuleExports { names: BTreeSet<Ident> }` carries the export surface.
- `StdlibStubs` hard-codes the export surface of the Q3 stdlib bootstrap
  modules (`std/result`, `std/option`, `std/array`, `std/string`, `std/io`,
  `std/json`, `std/fs`, `std/time`) plus `std/http` and `std/process`
  (Q3 calls them v1.1 but the examples reference them; stubbing avoids a
  day-4 special case). Names listed are the actual exports the examples
  consume, not a speculative surface.
- `CompositeGraph` composes two graphs with first-then-second fallthrough.
  The example tests use this to combine stdlib stubs with a tiny
  project-local graph for `react` and `api/users`.
- `verify_imports(module, &dyn ModuleGraph) -> Vec<ResolveError>` walks
  every `ImportDecl::Named` and emits `ResolveError::UnknownExportedName`
  for any name the target module doesn't export. `Namespace` and `Aliased`
  imports skip name checks — member resolution remains typechecker
  territory.
- `ResolveError::UnknownExportedName { name, module, span }` — new variant.

### Acceptance

| File | Named imports verified | Errors |
|---|---|---|
| `01_validator.glyph` | `std/result { Result, Ok, Err }` | 0 |
| `02_async_errors.glyph` | `std/result { Result, Ok, Err }` | 0 |
| `03_react_component.glyph` | `std/result { Result, Ok, Err }`, `react { use_state, use_effect, use_memo, Component }`, `std/time { debounce, Duration }`, `api/users { search_users, SearchError }` | 0 |
| `04_cli_tool.glyph` | `std/result { Result, Ok, Err }` | 0 |

Negative path covered: `cross_module_unknown_export_is_flagged` patches
`02_async_errors.glyph` to import `Boom` from `std/result` and asserts the
verifier flags it.

### Deferred to week 2 day 5+

- **Salsa wiring (I4)**. Per-file inputs, per-declaration intermediates.
  Pipeline is still pure-function pipe-by-hand; wrap parse → collect →
  verify → resolve → typemap as salsa-tracked queries.
- **Filesystem-backed module graph**. The day-4 graph is in-memory stubs.
  Once `glyph build` walks a directory, the graph builder parses each `.glyph`
  file, collects its top-level symbols, and surfaces the export set
  automatically. This also unlocks D15 barrel-file detection.
- **Aliased named imports** (`import std/result { Ok as O }`). D15 doesn't
  reserve syntax for it yet; revisit if dogfooding produces a strong case.

### Test summary after week 2 day 4

| Crate | Tests | Notes |
|---|---|---|
| glyph-lexer | 9 | unchanged |
| glyph-ast | 1 | unchanged |
| glyph-parser (lib) | 46 | unchanged |
| glyph-parser (snapshots) | 6 | unchanged |
| **glyph-resolver (lib)** | **29** | **+8**: module_graph (8 — known/unknown name in known module, namespace/aliased skip, unknown module silently passes, composite graph fall-through and surfacing, Q3 module seed) |
| **glyph-resolver (examples)** | **5** | **+2**: examples_pass_cross_module_verification, cross_module_unknown_export_is_flagged |
| glyph-typechecker (lib) | 23 | unchanged |
| glyph-typechecker (examples) | 2 | unchanged |
| glyph-emit, glyph-runtime, glyph-cli | 1 each | unchanged |
| **Total** | **123** | **All pass** (up from 113) |

## Phase 1 week 2 day 5 status (shipped 2026-05-29)

**Salsa wiring (I4) lands — per-file half.** A new `glyph-db` crate wraps
the per-file pipeline as `#[salsa::tracked]` queries. Re-running a query
with unchanged input reuses the cached `Arc` (verified by a
`Arc::ptr_eq` assertion); mutating the input via `file.set_text(...)`
invalidates downstream queries automatically. Per-declaration tracked
intermediates land in day 6+.

### Implemented this slice

**glyph-db** (new crate, ~470 LoC, 9 tests):

- `Db` trait extending `salsa::Database` with `prelude()` and
  `module_graph()` accessors.
- `CompilerDb` concrete struct holding `salsa::Storage<Self>`, an
  `Arc<Prelude>`, and an `Arc<dyn ModuleGraph + Send + Sync>`. Neither
  the prelude nor the graph is salsa-tracked — both are immutable for
  the lifetime of a db. `CompilerDb::with_default_stdlib()` is the test
  factory.
- `SourceFile` as a `#[salsa::input]` carrying `virtual_path` and `text`.
  Tests construct files via `SourceFile::new(&db, path, text)`; mutate
  via `file.set_text(&mut db).to(new_text)` (requires `use salsa::Setter`).
- Five tracked queries wired in a pipeline:
  - `parse_module(db, file) -> ParsedModule`
  - `module_symbols(db, file) -> Symbols`
  - `import_diagnostics(db, file) -> Diagnostics`
  - `resolve(db, file) -> Resolved`
  - `type_map(db, file) -> Types`
- Five wrapper newtypes (`ParsedModule`, `Symbols`, `Diagnostics`,
  `Resolved`, `Types`), each `Arc`-shared internally and implementing
  `salsa::Update` by hand via `PartialEq` on the inner. The unsafe
  Update impl is justified by an inline `// SAFETY:` comment: the Eq
  invariant the trait doc requires holds because every payload type
  derives `Eq`.

**Upstream `PartialEq`/`Eq` derives** so salsa's change detection works:

- `glyph-resolver`: `ModuleSymbols`, `SymbolTable`, `ResolvedModule`,
  `ResolutionMap` all gain `PartialEq, Eq`.
- `glyph-typechecker`: `TypeMap` gains `PartialEq, Eq`. Also gains
  `len()` and `is_empty()` accessors (the previous-day API didn't
  expose count, useful for the day-5 tests).

### Why manual `Update` impls instead of `#[derive(salsa::Update)]`

The salsa-2022 derive macro requires the *transitive* type closure of a
struct to implement `Update`. For our AST (`Module → Decl → … → Expr`)
that's ~25 types across `glyph-ast`, `glyph-resolver`, and
`glyph-typechecker`. Adding `salsa` as a dep to those crates would
leak an incremental-compilation concern down into the AST layer, which
should stay agnostic. The wrapper-with-manual-`Update` pattern keeps
`salsa` confined to `glyph-db`. Trade-off: each new pipeline stage
needs its own wrapper, but the pattern is mechanical.

### Acceptance

The `unchanged_text_returns_same_result` test verifies salsa memoizes
correctly: a second call to `parse_module(&db, file)` returns a
`ParsedModule` whose inner `Arc` is pointer-equal to the first call's
inner — i.e. salsa skipped the recomputation entirely. The
`changing_text_invalidates_downstream` test confirms a `set_text(...)`
call triggers re-parsing.

### Deferred to week 2 day 6+

- **Per-declaration tracked intermediates** (I4 second half). The
  assigner's `decl_ty_cache` is currently a per-`Assigner` HashMap; a
  `#[salsa::tracked] fn decl_ty(db, file, decl_idx) -> Ty` query would
  let the LSP / future re-typecheck reuse per-decl results across edits.
  Needs `Ty: salsa::Update` (or a wrapper).
- **Filesystem-backed module graph**. Day-5 stops at in-memory
  `StdlibStubs`. Once a project walker exists, `module_graph()` becomes
  a salsa query reading from `SourceFile` inputs across the project,
  unlocking D15 barrel-file detection on real files.
- **Cross-module reads through `import_diagnostics`**. The query reads
  `db.module_graph()` (untracked); a future graph backed by other
  `SourceFile` queries would let salsa invalidate downstream files when
  an upstream module's exports change.
- **Accumulator-based diagnostics**. salsa has `#[salsa::accumulator]`
  for cross-query diagnostic bundling. Day-5 returns errors in-band
  inside each wrapper; a future `Diagnostic` accumulator would let
  `glyph build` ask "all errors across this database" without
  re-iterating files.

### Test summary after week 2 day 5

| Crate | Tests | Notes |
|---|---|---|
| glyph-lexer | 9 | unchanged |
| glyph-ast | 1 | unchanged |
| glyph-parser (lib) | 46 | unchanged |
| glyph-parser (snapshots) | 6 | unchanged |
| glyph-resolver (lib) | 29 | unchanged |
| glyph-resolver (examples) | 5 | unchanged |
| glyph-typechecker (lib) | 23 | unchanged |
| glyph-typechecker (examples) | 2 | unchanged |
| **glyph-db (lib)** | **11** | **new**: parse/symbols/imports/resolve/type_map happy paths, downstream short-circuit on parse error, memoization (Arc::ptr_eq), invalidation observed by every downstream stage (parse + collect + type_map), cross-file isolation (touching file A doesn't invalidate file B), import-diagnostics empty-success path, concrete-Ty assertion for the `42` literal span |
| glyph-emit, glyph-runtime, glyph-cli | 1 each | unchanged |
| **Total** | **134** | **All pass** (up from 123) |

## Phase 1 week 2 day 6 status (shipped 2026-05-29)

**Per-declaration tracked intermediates land (I4 second half).** A new
`#[salsa::tracked] fn decl_ty(db, file, decl_idx) -> DeclTy` query lowers
the signature of one top-level declaration. The shared lowering moved
from `Assigner` (a method) to `Lowerer` (free method) so the salsa
query and `assign_types` use the same implementation. 140 workspace
tests pass (up from 134).

### Implemented this slice

**glyph-typechecker** — `Lowerer` gained two new methods (`lower.rs`):

- `lower_callable_signature(params, return_ty, is_async) -> Ty` —
  builds a `Ty::Fn` from raw parts.
- `lower_decl_signature(&Decl) -> Ty` — dispatches on
  `Decl::{Fn, Component}` returning the signature `Ty`; other decls
  return `Ty::Unknown`.

`Assigner::decl_ty_for` (`assign.rs`) and `Expr::Lambda` lowering both
call the new `Lowerer` methods. The inline `fn_decl_ty` / `fn_ty`
helpers in `Assigner` were deleted.

**glyph-db** additions:

- `DeclTy { inner: Arc<DeclTyInner> }` wrapper newtype, same
  `impl_wrapper_update!` pattern as the other five wrappers.
- `decl_ty(db, file, decl_idx) -> DeclTy` salsa-tracked query.
- Test-only `EventLog` helper threaded through `CompilerDb::new`. The
  db now installs a salsa event callback when built with `#[cfg(test)]`;
  tests call `db.drain_events()` to read recorded `EventKind`s.

### Acceptance

| Behavior | Test | Mechanism |
|---|---|---|
| `decl_ty` returns the expected `Ty::Fn` for a callable decl | `decl_ty_returns_lowered_fn_signature` | direct assertion |
| Non-callable decls return `Ty::Unknown` | `decl_ty_unknown_for_non_callable_decl` | direct assertion |
| Out-of-range `decl_idx` returns `Ty::Unknown` | `decl_ty_unknown_for_out_of_range_idx` | direct assertion |
| Per-decl memoization within a revision | `decl_ty_memoizes_per_decl_index_within_a_revision` | salsa event-log: phase-2 repeat calls produce zero `WillExecute` events |
| Editing one fn's body produces content-equal `DeclTy` for other fns | `editing_one_fn_body_keeps_other_fn_decl_ty_content_equal` | `assert_eq!(ty_before, ty_after)` (salsa backdates the revision, downstream consumers will see "no change") |
| Editing one fn's signature DOES change its `DeclTy` | `changing_fn_signature_changes_its_decl_ty_content` | `assert_ne!` |

### What the backdating gives us (and what it doesn't)

`salsa::function::backdate::backdate_if_appropriate` works at the
`changed_at` revision level — when a re-executed query produces a
content-equal value, salsa "backdates" the revision counter so
downstream queries see "input unchanged" and can be skipped. It does
NOT preserve the memo's `Arc` identity (the new value replaces the
old in storage, just with a backdated revision). So testing the
backdating effect via `Arc::ptr_eq` doesn't work; testing via content
equality + downstream-skipping does.

Day-6 doesn't ship a salsa-tracked downstream consumer of `decl_ty`,
so the skip-downstream benefit is theoretical until week 3's
bidirectional checker (per-decl `typecheck_decl(file, decl_idx)`)
lands. The infrastructure is in place; the win compounds once
consumers exist.

### Deferred to week 2 day 7+

- **Per-decl `resolved_decl(file, decl_idx) -> Resolved`** — slicing
  the resolution map by decl. Enables truly per-decl invalidation
  without the salsa-internal "re-execute and backdate" round trip.
- **Filesystem-backed module graph** (still deferred from day 5).
- **Threading `Db` through `Assigner`** so `assign_types` calls the
  cached `decl_ty(file, k)` instead of re-lowering inside its own
  `decl_ty_cache`. The two paths produce identical results today; the
  refactor only matters once the next slice of per-decl tracked queries
  reads from the cache.

### Test summary after week 2 day 6

| Crate | Tests | Notes |
|---|---|---|
| glyph-lexer | 9 | unchanged |
| glyph-ast | 1 | unchanged |
| glyph-parser (lib) | 46 | unchanged |
| glyph-parser (snapshots) | 6 | unchanged |
| glyph-resolver (lib) | 29 | unchanged |
| glyph-resolver (examples) | 5 | unchanged |
| **glyph-typechecker (lib)** | **25** | **+2** (post-review): lower_decl_signature_for_fn, lower_decl_signature_for_type_is_unknown |
| glyph-typechecker (examples) | 2 | unchanged |
| **glyph-db (lib)** | **19** | **+8**: decl_ty happy path (1), decl_ty Unknown for type (1), Unknown for const (1, post-review), out-of-range idx (1), per-decl memoization via event log (1), body-edit preserves other fns' content (1), signature edit changes content (1), Component returns Fn shape (1, post-review) |
| glyph-emit, glyph-runtime, glyph-cli | 1 each | unchanged |
| **Total** | **144** | **All pass** (up from 134) |

## Phase 1 week 2 day 7 status (shipped 2026-05-29)

**`type_map` now consumes the salsa-tracked `decl_ty` query.** A
`DeclTyResolver` trait sits at the `glyph-typechecker` ↔ `glyph-db`
boundary; the default `LocalDeclTy` impl preserves the pre-day-7
behavior for db-less callers, and the new `SalsaDeclTy` in `glyph-db`
routes per-decl lookups through `decl_ty(db, file, idx)`. The
Assigner's old `decl_ty_cache: HashMap<u32, Ty>` is gone — caching is
now exclusively the resolver's responsibility (per-call for `LocalDeclTy`,
cross-revision via salsa for `SalsaDeclTy`). 146 workspace tests pass
(up from 144).

### Implemented this slice

- **`glyph-typechecker`**: new `DeclTyResolver` trait, `LocalDeclTy`
  default impl (RefCell-backed local HashMap), and
  `assign_types_with_resolver(module, resolved, prelude, &dyn DeclTyResolver)`
  entry point. `assign_types(...)` keeps its existing signature and
  internally constructs a `LocalDeclTy`. The `Assigner` struct no
  longer holds a `module` field or a `decl_ty_cache`; both shifted
  into `LocalDeclTy`.
- **`glyph-db`**: `SalsaDeclTy { db, file }` impl of `DeclTyResolver`.
  The `type_map` query constructs one and passes it to
  `assign_types_with_resolver`.

### Acceptance

| Behavior | Test |
|---|---|
| `type_map` warms `decl_ty`'s salsa memo for referenced decls | `type_map_warms_salsa_decl_ty_for_referenced_decls` (after type_map runs, calling decl_ty directly fires zero WillExecute) |
| `type_map` produces the same content across body-only edits | `type_map_consumes_decl_ty_so_body_edit_does_not_relower_other_fns` (entry-count parity + zero WillExecute on no-op repeat) |

### Deferred to week 2 day 8+

- **Per-decl resolved-ref slicing** (`resolved_decl(file, decl_idx)`)
  — true per-decl input granularity so editing fn 5's body doesn't
  re-execute `decl_ty(file, k)` for the other k's. Today's win is
  output-level backdating.
- **Filesystem-backed module graph** (still deferred from day 5).

### Test summary after week 2 day 7

| Crate | Tests | Notes |
|---|---|---|
| glyph-lexer | 9 | unchanged |
| glyph-ast | 1 | unchanged |
| glyph-parser (lib) | 46 | unchanged |
| glyph-parser (snapshots) | 6 | unchanged |
| glyph-resolver (lib) | 29 | unchanged |
| glyph-resolver (examples) | 5 | unchanged |
| glyph-typechecker (lib) | 25 | unchanged |
| glyph-typechecker (examples) | 2 | unchanged |
| **glyph-db (lib)** | **21** | **+2**: type_map_warms_salsa_decl_ty_for_referenced_decls, type_map_consumes_decl_ty_so_body_edit_does_not_relower_other_fns |
| glyph-emit, glyph-runtime, glyph-cli | 1 each | unchanged |
| **Total** | **146** | **All pass** (up from 144) |

## Phase 1 week 2 day 8 status (shipped 2026-05-29)

**Per-decl input granularity lands.** Two new salsa queries —
`decl_ast(file, k)` and `resolved_decl(file, k)` — slice the per-file
AST and resolution map down to one declaration's signature. `decl_ty`
depends on those slices instead of `parse_module` / `resolve`
directly. Editing one fn's body no longer causes `decl_ty(file, k≠edited)`
to re-execute — its body is served from the memo as a
`DidValidateMemoizedValue` event. 147 workspace tests pass (up from 146).

### Implemented this slice

- **`glyph-resolver`**: re-exports `ResolutionMap`; new
  `ResolvedModule::sliced(|span| keep) -> ResolvedModule` helper
  produces a per-decl filtered resolution map.
- **`glyph-db`**: new `DeclAst` and `ResolvedDecl` wrapper newtypes
  with the standard `impl_wrapper_update!`. New `decl_ast(file, k)`
  and `resolved_decl(file, k)` tracked queries; `decl_ty(file, k)`
  rewired to depend on them instead of the whole-file queries. New
  `collect_signature_spans` helper walks a `Decl`'s param/return
  `TypeExpr`s and collects every span the `Lowerer` will query —
  drives the slicing inside `resolved_decl`.

### What changes about invalidation

Day 7: `decl_ty(file, k)` depended on `parse_module(file)` +
`resolve(file)`. Any file edit re-executed `decl_ty` for every `k`;
backdating at the output level let downstream consumers skip.

Day 8: `decl_ty(file, k)` depends on `decl_ast(file, k)` +
`resolved_decl(file, k)`. A body-only edit to fn 5:
- `decl_ast(file, 5)` content changes → not backdated.
- `decl_ast(file, k≠5)` content equal → backdated.
- `resolved_decl(file, 5)` slice changes if fn 5's signature spans
  ended up shifted; otherwise content equal.
- `resolved_decl(file, k≠5)` slice content equal → backdated.
- `decl_ty(file, 5)` re-executes.
- `decl_ty(file, k≠5)` served as a memo hit
  (`DidValidateMemoizedValue`); only its cheap slicing dependencies
  re-validate.

### Acceptance

| Behavior | Test |
|---|---|
| `decl_ty(file, k≠edited)` is a memo hit after a body-only edit | `editing_one_fn_body_skips_decl_ty_for_other_fns` — asserts ≤2 `WillExecute` (decl_ast + resolved_decl re-validation) and ≥1 `DidValidateMemoizedValue` (decl_ty served from cache) |
| Existing per-file queries unchanged | 21 prior db tests still pass |

### Caveat: span-shifting

`Decl: Eq` is structural and includes `Span` values. If a file edit
shifts the byte positions of later decls (e.g. inserting a line
before them), `decl_ast(file, k>edited)`'s content compares
unequal even when the decl text is the same — so backdating doesn't
fire. The day-8 acceptance test uses an equal-length body swap
(`a + 1` → `1 + a`) to exercise the win deterministically. A future
span-insensitive equality (or per-decl normalized spans) would
generalize the result; that's deferred.

### Deferred to week 2 day 9+

- **Span-insensitive equality on `DeclAst`** so byte shifts in
  earlier decls don't invalidate later decls' slices.
- **Filesystem-backed module graph** (still deferred from days 5–7).

### Test summary after week 2 day 8

| Crate | Tests | Notes |
|---|---|---|
| glyph-lexer | 9 | unchanged |
| glyph-ast | 1 | unchanged |
| glyph-parser (lib) | 46 | unchanged |
| glyph-parser (snapshots) | 6 | unchanged |
| glyph-resolver (lib) | 29 | unchanged |
| glyph-resolver (examples) | 5 | unchanged |
| glyph-typechecker (lib) | 25 | unchanged |
| glyph-typechecker (examples) | 2 | unchanged |
| **glyph-db (lib)** | **22** | **+1**: editing_one_fn_body_skips_decl_ty_for_other_fns |
| glyph-emit, glyph-runtime, glyph-cli | 1 each | unchanged |
| **Total** | **147** | **All pass** (up from 146) |

## Phase 1 week 2 day 9 status (shipped 2026-05-29)

**Filesystem-backed module graph lands.** Cross-module imports between
project-local `.glyph` files now resolve against parsed exports
instead of in-memory `StdlibStubs`. A new salsa-tracked
`module_exports(file)` query collects per-file exports (top-level
decls + tagged-union variants, excluding imports per D15); a
non-tracked `ProjectGraph` aggregator implements `ModuleGraph` over
many files' export sets. Composes with `StdlibStubs` via the existing
`CompositeGraph`. 152 workspace tests pass (up from 147).

### Implemented this slice

- **`glyph-resolver`**: `ModuleExports` gained `PartialEq, Eq` derives
  so it can cross salsa boundaries via the wrapper.
- **`glyph-db`**:
  - `Exports` wrapper newtype (`impl_wrapper_update!` pattern) holding
    a per-file `ModuleExports`.
  - `module_exports(db, file)` tracked query — reads from
    `module_symbols(db, file)` and filters out `Import*` symbol kinds.
    Editing a fn body doesn't change the export set, so the salsa memo
    backdates and downstream consumers see "no change."
  - `ProjectGraph::build(db, [(path, file), ...])` — eager aggregation
    that fetches each file's exports via salsa. The aggregator itself
    is in-memory (not salsa-tracked); callers rebuild it on demand. The
    per-file fetches are cached, so rebuilding after editing one file
    is O(N) HashMap inserts + 1 cache miss.

### Acceptance

| Behavior | Test |
|---|---|
| Top-level decls export; imports do not | `module_exports_lists_top_level_decls_only` |
| Tagged-union variants are exports | `module_exports_includes_union_variants` |
| Cross-module `import lib { helper }` resolves via the project graph | `project_graph_serves_cross_module_named_imports` |
| Bogus cross-module import is flagged | `project_graph_flags_unknown_export` |
| `module_exports` memo survives body-only edits | `module_exports_memoizes_across_body_edits_that_dont_change_decl_names` |

### Limitation: graph-level invalidation

The `ProjectGraph` aggregator is NOT salsa-tracked — when a file's
exports change, `import_diagnostics` for *importing* files won't
auto-invalidate unless the caller rebuilds the graph and re-runs the
query. A future salsa-tracked `ProjectExports` value (with
`impl ModuleGraph`) would close that gap. The per-file `module_exports`
salsa cache means rebuilding after one file edit is cheap; the cost
is the explicit rebuild call, not redundant work.

### Deferred to day 10+

- **Salsa-tracked `ProjectExports`** so changing a project file's
  exports automatically invalidates importing files' diagnostics.
- **Span-insensitive `DeclAst` / `ResolvedDecl`** so non-equal-length
  edits also benefit from day-8's win.
- D15 **barrel-file detection** at the export level: a file with
  only imports (no fn/type/const/component) has empty exports — the
  current `module_exports` returns empty `BTreeSet` for those, but
  there's no explicit diagnostic yet.

### Test summary after week 2 day 9

| Crate | Tests | Notes |
|---|---|---|
| glyph-lexer | 9 | unchanged |
| glyph-ast | 1 | unchanged |
| glyph-parser (lib) | 46 | unchanged |
| glyph-parser (snapshots) | 6 | unchanged |
| glyph-resolver (lib) | 29 | unchanged |
| glyph-resolver (examples) | 5 | unchanged |
| glyph-typechecker (lib) | 25 | unchanged |
| glyph-typechecker (examples) | 2 | unchanged |
| **glyph-db (lib)** | **27** | **+5**: module_exports lists top-level decls only, module_exports includes union variants, ProjectGraph serves cross-module named imports, ProjectGraph flags unknown export, module_exports memoizes across body edits |
| glyph-emit, glyph-runtime, glyph-cli | 1 each | unchanged |
| **Total** | **152** | **All pass** (up from 147) |

## Phase 1 week 2 day 10 status (shipped 2026-05-29)

**Cross-file auto-invalidation lands.** A new `salsa::input ProjectFiles`
lives on `CompilerDb` (lazy-init via `OnceLock`); the new salsa-tracked
`project_exports(db, project)` query aggregates every project file's
`module_exports` into a single `ProjectExports` value that impls
`ModuleGraph`. `import_diagnostics` now composes the stdlib graph with
`project_exports` — so editing `lib.glyph` to remove an export
auto-invalidates `app.glyph`'s diagnostics without any explicit graph
rebuild. 156 workspace tests pass (up from 152).

### Implemented this slice

- **`#[salsa::input] ProjectFiles { entries: Vec<(String, SourceFile)> }`**:
  the project's file list as a tracked input. Mutated via
  `CompilerDb::set_project(entries)`.
- **`OnceLock<ProjectFiles>` field on `CompilerDb`**: lazy-creates the
  input the first time `project_files_input()` is called. Sidesteps the
  chicken-and-egg of "salsa-input creation requires the db to exist."
- **`Db` trait gained `fn project_files_input(&self) -> ProjectFiles`**:
  tracked queries fetch the input through the trait, so salsa records
  the dependency.
- **`ProjectExports` wrapper** (same `impl_wrapper_update!` pattern):
  holds an `Arc<BTreeMap<String, ModuleExports>>` and impls
  `ModuleGraph::exports_of` via the existing `path_key`. BTreeMap
  rather than HashMap so the aggregate is order-independent and the
  wrapper's `Eq` is content-only.
- **`project_exports(db, project) salsa::tracked`**: iterates
  `project.entries(db)`, fetches each file's `module_exports`, builds
  the BTreeMap. Salsa tracks the per-file dependencies; when one file's
  exports change, the aggregate re-runs.
- **`import_diagnostics` rewired**: composes the static stdlib graph
  (`db.module_graph()`) with the salsa-tracked `project_exports` via
  `glyph_resolver::CompositeGraph`. Cross-file dependency is now
  part of salsa's graph.

### Acceptance

| Behavior | Test |
|---|---|
| Project-registered file's exports resolve cross-module | `import_diagnostics_resolves_against_project_via_db_input` |
| Bogus cross-module import auto-flagged via db input | `import_diagnostics_flags_unknown_project_export_via_db_input` |
| Removing lib's export auto-invalidates app's diagnostics | `removing_a_lib_export_auto_invalidates_dependent_app_diagnostics` |
| Body-only edit to lib does NOT invalidate app's diagnostics | `body_only_edit_to_lib_does_not_invalidate_app_diagnostics` (asserts ≥1 `DidValidateMemoizedValue`) |

### Deferred to day 11+

- **Span-insensitive `DeclAst`/`ResolvedDecl`** so non-equal-length
  edits also benefit from day-8's win.
- **D15 barrel-file detection** — diagnostic for files with zero
  top-level decls (only imports).
- **`CompilerDb::clone` semantics around `OnceLock`** — when cloning
  a db whose project_files was set, the clone's `OnceLock` either
  inherits the ProjectFiles ID via the `Clone` impl on OnceLock (when
  initialized) or stays empty. Tests use a single db throughout; the
  clone path isn't exercised here.

### Test summary after week 2 day 10

| Crate | Tests | Notes |
|---|---|---|
| glyph-lexer | 9 | unchanged |
| glyph-ast | 1 | unchanged |
| glyph-parser (lib) | 46 | unchanged |
| glyph-parser (snapshots) | 6 | unchanged |
| glyph-resolver (lib) | 29 | unchanged |
| glyph-resolver (examples) | 5 | unchanged |
| glyph-typechecker (lib) | 25 | unchanged |
| glyph-typechecker (examples) | 2 | unchanged |
| **glyph-db (lib)** | **31** | **+4**: import_diagnostics_resolves_against_project_via_db_input, import_diagnostics_flags_unknown_project_export_via_db_input, removing_a_lib_export_auto_invalidates_dependent_app_diagnostics, body_only_edit_to_lib_does_not_invalidate_app_diagnostics |
| glyph-emit, glyph-runtime, glyph-cli | 1 each | unchanged |
| **Total** | **156** | **All pass** (up from 152) |

## Phase 1 week 2 day 11 status (shipped 2026-05-29)

**CLI wiring lands.** `glyph build src/ --out dist/` now walks a source
directory, registers every `.glyph` file on a salsa-backed `CompilerDb`,
runs the analysis pipeline (parse → collect → verify-imports → resolve →
type_map), and reports diagnostics. Exit code 0 on clean projects, 1 on
diagnostics, 2 on I/O / invalid-input errors. TS emission is still week-4
work, so `--out` is accepted (and the directory is created) but no files
are written yet. 164 workspace tests pass (up from 156).

### Implemented this slice

- **`glyph-cli` got a library**: `src/lib.rs` exposes `build_project`,
  `BuildReport`, `BuildError`. Tests link the library directly (no
  subprocess) — fast, deterministic.
- **`build_project(src, out)`** walks the source tree (skipping hidden
  dirs and `target/`), reads each `.glyph` file, registers it on a
  fresh `CompilerDb` via `set_project`, runs the salsa pipeline for
  every file, and collects pre-rendered diagnostic strings.
- **Module path derivation**: `src/foo/bar.glyph` → `foo/bar`. The
  native path separator is normalized to `/` so the result matches
  what `import foo/bar` produces.
- **Binary** (`main.rs`): the `Build` clap arm now dispatches to
  `build_project` and translates the report into stderr output + an
  exit code.

### Acceptance

| Behavior | Test |
|---|---|
| Clean two-file project with cross-module import reports no diagnostics | `build_reports_no_diagnostics_on_clean_project` |
| Bogus cross-module import is flagged | `build_flags_unknown_cross_module_export` |
| Subdirectories are walked | `build_recurses_into_subdirectories` |
| Missing src dir → `SrcMissing` error | `build_fails_for_missing_src_directory` |
| Empty src dir → `NoSources` error | `build_fails_for_empty_directory` |
| `.git/`, `target/` are skipped | `build_skips_hidden_and_target_directories` |
| `derive_module_path` handles top-level files | unit test |
| `derive_module_path` drops `.glyph` and normalizes separators | unit test |

Plus a real-binary smoke test (run manually in the day-11 session)
confirming exit codes and stderr output match.

### Deferred to day 12+

- **TS emission** (phase 1 week 4 in the implementation plan). The
  `--out` directory is created but unused.
- **Span-insensitive `DeclAst`/`ResolvedDecl`** so non-equal-length
  edits also benefit from day-8's win.
- **Pretty error rendering via ariadne** (phase 1 week 7 "Elm quality"
  bar). Today's diagnostics are one-line strings.
- **`glyph run` / `fmt` / `regen` / `publish`** subcommands still
  return "phase 0 stub."

### Test summary after week 2 day 11

| Crate | Tests | Notes |
|---|---|---|
| glyph-lexer | 9 | unchanged |
| glyph-ast | 1 | unchanged |
| glyph-parser (lib) | 46 | unchanged |
| glyph-parser (snapshots) | 6 | unchanged |
| glyph-resolver (lib) | 29 | unchanged |
| glyph-resolver (examples) | 5 | unchanged |
| glyph-typechecker (lib) | 25 | unchanged |
| glyph-typechecker (examples) | 2 | unchanged |
| glyph-db (lib) | 31 | unchanged |
| **glyph-cli (lib)** | **2** | **+2** (new): derive_module_path unit tests |
| **glyph-cli (integration)** | **6** | **+6** (new): clean project, bogus import, subdirs, missing src, empty src, hidden+target skipped |
| glyph-emit, glyph-runtime, glyph-cli (bin) | 1 each | unchanged (bin has 0 tests but counts the original stub bin test elsewhere) |
| **Total** | **164** | **All pass** (up from 156) |

## Phase 1 week 2 day 12 status (shipped 2026-05-29)

**Span-insensitive caching lands for DeclAst and ResolvedDecl.** Both
wrappers now use the **source bytes** covered by the decl's outer
span as their canonical fingerprint. PartialEq compares only those
bytes, so absolute-span shifts caused by length-changing edits to
*other* decls no longer invalidate this decl's wrapper. The day-8
"editing one fn body skips decl_ty for other fns" win now extends to
non-equal-length edits. 165 workspace tests pass (up from 164).

### Implemented this slice

- `DeclAstInner` and `ResolvedDeclInner` each grew a
  `canonical: Arc<str>` field carrying the source bytes covered by
  the decl's outer span. The inner wrappers' `PartialEq` compares
  this canonical only — not the carried `Decl` / `ResolvedModule`.
- `DeclAst::new(decl, source, span)` and `ResolvedDecl::new(resolved,
  source, span)` extract the canonical bytes at construction time.
- `decl_ast` and `resolved_decl` salsa queries read `file.text(db)`
  and pass it through `canonical_bytes(source, span)`. A defensive
  helper checks `is_char_boundary` and bounds before slicing.
- `decl_outer_span(d)` is a small helper that returns the outermost
  span for any `Decl` variant.
- Outer `DeclAst` / `ResolvedDecl` got hand-written `PartialEq` that
  fast-paths on `Arc::ptr_eq` then falls through to the inner's
  source-byte compare.

### Acceptance

`length_changing_body_edit_skips_decl_ty_for_other_fns` exercises
the case day-8 explicitly couldn't: `return a` → `return a + 1 + 2
+ 3` (length-changing) on fn 0's body. After the edit, calling
`decl_ty(&db, file, 1)` for the untouched `other` fn:
- Fires 5 `WillExecute` events (parse_module, module_symbols,
  resolve, decl_ast, resolved_decl all re-validate)
- Fires 1 `DidValidateMemoizedValue` for `decl_ty` itself (it sees
  its deps' Updates returned false and serves the cached Ty without
  re-executing)

The two assertions are jointly load-bearing — `we <= 5` excludes
decl_ty from the re-executed set (a regression where decl_ty re-runs
pushes the count to 6); `valid >= 1` confirms a memo hit.

### Trade-off

Source-byte canonical is broader than strictly necessary:
- A comment-only edit within this decl changes the source bytes →
  the wrapper invalidates and `decl_ty` re-executes (even though the
  AST is semantically identical).
- A whitespace-only edit within this decl: same.

Both are practical non-issues (comments/whitespace rarely change
without surrounding code changing too) and the trade-off is
explicitly documented on the wrappers. A future structural span-strip
implementation could tighten this — that's day-13+ work if anyone
asks.

### Test summary after week 2 day 12

| Crate | Tests | Notes |
|---|---|---|
| glyph-lexer | 9 | unchanged |
| glyph-ast | 1 | unchanged |
| glyph-parser (lib) | 46 | unchanged |
| glyph-parser (snapshots) | 6 | unchanged |
| glyph-resolver (lib) | 29 | unchanged |
| glyph-resolver (examples) | 5 | unchanged |
| glyph-typechecker (lib) | 25 | unchanged |
| glyph-typechecker (examples) | 2 | unchanged |
| **glyph-db (lib)** | **32** | **+1**: length_changing_body_edit_skips_decl_ty_for_other_fns |
| glyph-cli (lib) | 2 | unchanged |
| glyph-cli (integration) | 6 | unchanged |
| glyph-emit, glyph-runtime | 1 each | unchanged |
| **Total** | **165** | **All pass** (up from 164) |
