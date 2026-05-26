# Step 4 — Transpiler

Status: **planned, not started.** Full week-by-week plan in `archive/glyph-transpiler-plan.md`.

## Updates from brainstorm session 1 (2026-05-26)

- **Q1 → v2 (defer mapped types).** `infer_shape<Shape>` is out of v1. The `01_validator.glyph` example must be rewritten to use an explicit type parameter *before* step 4 begins. No emission strategy for type-level computation needed in v1.
- **Q2 → fold corpus into step 6.** The "write 26–46 more examples" phase is dropped. Step 4's test corpus is the 4 hard cases plus whatever step 6 dogfooding produces. CI gates on those.
- **Q5 → hybrid compiler architecture.** **Typechecker and name resolver built around salsa-style queries from day one**; AST→TS emission stays a dumb visitor. This adds ~1–2 weeks to the original step-4 plan but unblocks step 7 at its original 4-week budget.

**Revised time estimate: 6–8 weeks** (was 4–6).

The week-by-week plan in `archive/glyph-transpiler-plan.md` is otherwise still correct. Week 2 (name resolution, basic types) and Week 3 (ADTs, exhaustiveness, `?` propagation) now use salsa queries as their substrate. The Q20 loop construct (`for` + `loop` per D21) is added to Week 1's parser scope.

## Updates from brainstorm session 3 (2026-05-26)

- **Q3 → stdlib bootstrap set for v1.** Step 4 ships with: `result`, `option`, `array`, `string`, `io`, `json`, `fs`, `time` + prelude (`Result`, `Option`, `Ok`, `Err`, `Some`, `None`, `par`). Everything else (`http`, `process`, `crypto`, React bindings) is v1.1.
- **Q10 → benchmarks track starts here.** Add `benchmarks/` directory in step 4. 5–10 functions (e.g., the example corpus subset) measured in Glyph vs TS vs Python vs Rust. Track token count, line count, diff size over time. Strategic infrastructure for the manifesto's empirical claim — instrument from first transpiler output, not at step 11.
- **New D-decisions for step 4 grammar/parser/lexer scope:**
  - D21 — `for x in iter { ... }` and `loop { ... break ... continue }` (session 1).
  - D22 — template literals: `"${expr}"` (session 3 D12 re-litigation).
  - D27 — `@<name> <args>` annotation form above declarations (umbrella rule).
- **New D-decisions for step 4 transpiler compile-time execution:**
  - D23 — `@example expr == expr` runs on every `glyph build`; failure fails the build.
  - D26 — `@doc """ ... ```glyph @run ... ``` """` runs on every build; failed assertions fail the build.

These two compile-time-execution constructs (D23, D26) share machinery: a sandboxed interpreter for the subset of Glyph allowed inside them (no IO unless the test capability is granted; budget-bounded execution per assertion).

## Target

Glyph source → TypeScript output. `tsc` handles JS emission (we inherit its target/module handling for free). Original goal: all 50 example programs compile and run within 4–6 weeks. Note: only 4 examples exist today (see `open-questions.md`, blocker #1).

## Decisions already made

- **Rust over Go.** Reasons: (a) diagnostics ecosystem (`chumsky`/`winnow` for parsing + `ariadne`/`miette` for error rendering — Glyph's whole value prop is "agents read errors and fix code"); (b) ADT-heavy compiler code (sum types are painful in Go). **Pivot to Go only if** the team is Go-native and Rust would be a 2-week tax.
- **Hand-written Pratt parser.** Operator table is small; `PRECEDENCE.md` (inline in `archive/grammar.js` and `archive/GLYPH.md §2`) is already a spec for it. Pratt also gives the best error recovery on half-edited files — which is the real workload.
- **Hand-written lexer (~300 lines).** Full control matters for JSX-mode transitions in week 4.
- **AST: one big enum per node category.** `Expr`, `Stmt`, `TypeExpr`, `Pattern`. Every node carries a `Span`. Skip string interning for v0; `Arc<str>` is fine.
- **Golden tests from day one with `insta`.** Every example program parses to a snapshot AST, checked in. When the parser changes in week 3, the diff tells you exactly what moved.
- **Maranget exhaustiveness checking** (Maranget 2007, ~400 lines of Rust). Don't hand-roll a heuristic — agents will write match expressions on five-variant ADTs with nested record patterns, and a heuristic will either reject valid code or accept invalid code.
- **`?` operator with exact-type-match.** No `From`-trait equivalent in v0. `expr?` requires `expr: Result<T, E>` and the enclosing function returns `Result<_, E>` with `E` matching exactly.
- **Runtime descriptors for `record` and ADT types.** Generate `User.parse` as a static method doing shallow validation (right keys, right primitive types). Deep validation, custom refinements, and full Zod-equivalent surface are v1.
- **Dumb AST-to-TS visitor — no IR.** One AST-node-to-TS-string visitor. The mapping is almost 1:1. Ugly emitted code is fine — humans read Glyph, not emitted TS.
- **JSX directives as compile-time AST rewrites, not runtime components.** `<if>` → ternary, `<for>` → `.map()`, `<match>` → switch-returning IIFE. Lower in the AST *before* emission so the typechecker sees bound variables from `<case Loaded bind={users}>`.
- **Subprocess `tsc`, not embedded.** `glyph build` shells out to `tsc` with a generated `tsconfig.json`. Get every `tsc` upgrade for free.
- **Runtime prelude as a shared `.ts` module.** Hand-written, <200 lines: `Result`, `Option`, `Ok`, `Err`, `Some`, `None`, `par` helpers, issue type for record parsers. Every emitted file imports from it via a generated top-of-file import. Resist inlining per-file — one module, full-path import, exactly like the manifesto demands of user code.

## What's cut from v0

1. **Full type inference.** Require annotations on function signatures and any `let` whose type isn't obvious from the RHS literal. TS people will grumble; ship anyway.
2. **Generics beyond the simplest case.** `fn array_schema<T>(element: Schema<T>) -> Schema<Array<T>>` works. Higher-kinded stuff, generic constraints, conditional types — defer.
3. **`infer_shape<Shape>` mapped-type magic in `01_validator.glyph`.** TS-grade type-level computation, multi-week project on its own. For v0, require explicit output type or use `unknown` and emit a TS cast.

See `open-questions.md` for the unresolved decision about whether `infer_shape` is v1 or v2.

## Week-by-week (summary)

| Week | Goal |
|------|------|
| 1 | Lexer, Pratt parser, AST, golden tests. All 50 example files parse to AST with snapshots. |
| 2 | Name resolution, module graph, basic types. Every example has a type on every expression node (some stubbed). |
| 3 | ADTs, match exhaustiveness, `?` propagation. `01_validator.glyph` and `02_async_errors.glyph` typecheck end-to-end. |
| 4 | TS emission, JSX directive lowering, async. All 50 examples emit TS and `tsc --strict --noEmit` passes. |
| 5 | Formatter, CLI (`glyph build`), runtime prelude. `glyph run examples/todo.glyph add "buy milk"` works end-to-end through tsc → node. |
| 6 | Hardening: 50 examples as CI suite (~15 negative examples), error-message audit, `--explain E0042` for top 20 errors. **Budget ~4 days for property-based testing** on parser + exhaustiveness checker with `proptest`. |

Full per-week detail in `archive/glyph-transpiler-plan.md`.

## Tension with step 7 (LSP)

The "dumb visitor, no IR" decision here conflicts with the salsa-style incremental query architecture the LSP needs. One of the two has to give. Tracked in `open-questions.md`.
