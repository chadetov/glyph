# Glyph Transpiler: 4–6 Week Build Plan

## Context

Build a transpiler that takes Glyph source → TypeScript output, then let `tsc` handle JS emission (inheriting its target/module handling for free). Goal: all 50 example programs compile and run within 4–6 weeks.

## Up-front reality check

The window is aggressive but doable *only* with ruthless scoping of what the typechecker actually verifies in v0. The existing examples already exercise ADTs with payloads, exhaustive `match`, `Result<T, E>` with `?` propagation, async, generics, JSX-with-directives, and a custom formatter. That's a real language. Defer or cheat where called out below.

---

## Language choice: Rust

Pick **Rust** over Go. Two reasons that matter for this specific project:

1. **Diagnostics ecosystem.** `chumsky`/`winnow` for parsing plus `ariadne`/`miette` for error rendering. Glyph's whole value prop is "agents read errors and fix code" — error quality is a product feature, not polish. Rust's diagnostic tooling is meaningfully ahead of Go's.
2. **ADT-heavy compiler code.** AST, type IR, exhaustiveness checker — all painful in Go (sealed-interface boilerplate, type switches). Glyph itself uses sum types because they're the right tool for this shape of problem; write the compiler in a language that agrees.

**Counter-case for Go:** faster iteration loop, simpler concurrency for parallel file compilation, easier onboarding. If the team is Go-native and Rust would be a 2-week tax, take Go. Otherwise Rust.

---

## Week-by-week plan

### Week 1 — Lexer, parser, AST, golden tests

- **Lexer:** hand-written (~300 lines). Full control matters for the JSX-mode transitions in week 4.
- **Parser:** hand-written Pratt parser. `PRECEDENCE.md` is already a spec for it; the operator table is small. Pratt also gives the best error recovery on half-edited files, which is the real workload.
- **AST:** one big enum per node category (`Expr`, `Stmt`, `TypeExpr`, `Pattern`). Every node carries a `Span`. Skip string interning for v0; `Arc<str>` is fine.
- **Golden tests from day one:** every example program parses to a snapshot AST, checked in. Use `insta`. When the parser changes in week 3, the diff tells you exactly what moved. Highest-leverage discipline in a compiler under time pressure.

**End-of-week target:** all 50 example files parse to AST with snapshots. No semantic analysis yet.

### Week 2 — Name resolution, module graph, basic types

- Resolve every identifier to a definition. Build the module graph from `import` statements.
- **Enforce the manifesto's import rules in the resolver:** full-path imports, no barrel files. Reject re-exports. The rule is a feature, not just documentation.
- **Type representation:** `Type` enum covering primitives, records, arrays, functions, sum types, generic params, `unknown`.
- **No inference yet.** Require annotations on all function signatures and `let` bindings crossing function boundaries. Local `let` inference inside a function body via a tiny unification pass is fine.
- `Result<T, E>` and `Option<T>` are built-in (not library) — load-bearing for `?` propagation. Define them in a prelude module the compiler ships.

**End-of-week target:** every example resolves all names and has a type on every expression node, even if some are `unknown` or stubbed.

### Week 3 — The pillars: ADTs, match exhaustiveness, `?` propagation

This is the week Glyph becomes Glyph rather than "TypeScript with different syntax." Three things must land:

1. **ADT exhaustiveness checking.** Implement properly via *Warnings for Pattern Matching* (Maranget, 2007). Don't hand-roll a heuristic — agents will write match expressions on five-variant ADTs with nested record patterns, and a heuristic will either reject valid code or accept invalid code. Either is worse than TS. Maranget is ~400 lines of Rust and it's the difference between credibility and toy.
2. **`?` operator.** Type-level rule: `expr?` requires `expr: Result<T, E>` and the enclosing function returns `Result<_, E'>` with `E` convertible to `E'`. For v0, require exact match — no `From`-trait equivalent.
3. **Runtime descriptors for `record` and ADT types.** The manifesto promises `User.parse` is generated. Generate it. For v0, emit `parse` as a static method doing shallow validation (right keys, right primitive types). Defer deep validation, custom refinements, full Zod-equivalent surface — those are v1.

**End-of-week target:** `01_validator.txt` and `02_async_errors.txt` typecheck end-to-end, including exhaustiveness on `FeedError` matches.

### Week 4 — Emission to TypeScript, JSX directives, async

TypeScript emission is the easy half if you keep it dumb. One AST-node-to-TS-string visitor, no IR in between. The mapping is almost 1:1:

| Glyph | TypeScript |
|---|---|
| `fn foo(x: T) -> U` | `function foo(x: T): U` |
| `record User { ... }` | `interface User { ... }` + `const User = { parse(input: unknown): Result<User, Issue[]> { ... } }` |
| ADTs | discriminated unions with `tag` field + frozen constants object for nullary variants |
| `match` | `switch` on tag for tagged ADTs, `if` chain for value matches. No cleverness; `tsc` optimizes what it can |
| `result?` | `const __r = expr; if (__r.tag === "Err") return __r; const value = __r.value;` inlined at use site. Ugly emitted code is fine — humans read Glyph, not emitted TS |
| `await expr?` | `(await expr)` then apply `?` lowering (per `PRECEDENCE.md`) |

**JSX directives** (`<if>`, `<for>`, `<match>`, `<case>`) are compile-time macros, not runtime components. Desugar in the AST *before* emission:

- `<if cond={x}>A</if><else>B</else>` → ternary
- `<for x in={xs}>...</for>` → `xs.map(x => ...)`
- `<match>` → switch-returning IIFE

The trap: `<case Loaded bind={users}>` is a pattern binding. Thread the bound name into the case body's scope. Do this as an AST rewrite during lowering, not during emission, so the typechecker sees the bound variable.

**Async** is free — `async fn` → `async function`, `await` → `await`. Only wrinkle: `par.all` and `par.all_ok` from the async example are prelude functions, not language features.

**End-of-week target:** all 50 examples emit TypeScript and `tsc --strict --noEmit` passes on the output.

### Week 5 — Formatter, CLI, runtime prelude

- **Formatter:** non-negotiable per the diff-stability pillar, and easier than expected because Glyph has *no* line-length-based reflow. Rule: "one element per line if more than two elements." Recursive AST walk printing to a string. ~600 lines. No Prettier-style document model needed.
- **CLI:** `glyph build src/ --out dist/` walks module graph, typechecks the whole program, emits TS files into `dist/`, then shells out to `tsc` with a generated `tsconfig.json`. **Don't embed `tsc` or call it programmatically.** Subprocess is fine; you get every `tsc` upgrade for free, which is the point of not emitting JS directly.
- **Runtime prelude:** hand-written `.ts` shipped with the compiler. `Result`, `Option`, `Ok`, `Err`, `Some`, `None`, `par` helpers, issue type for record parsers. Under 200 lines. Every emitted file imports from it via a generated top-of-file import. **Resist inlining the prelude per-file** — one shared module, full-path import, exactly like the manifesto demands of user code.

**End-of-week target:** `glyph run examples/todo.glyph add "buy milk"` works end-to-end through tsc → node.

### Week 6 — Hardening, the 50 examples, error messages

Resist the urge to add features. Spend the week on:

- **The 50 examples as an end-to-end test suite, in CI.** Each has expected stdout (or expected typecheck error for negative examples). You should have ~15 negative examples that test "this code must fail with this specific error." Any regression = red build.
- **Error messages.** Go through every error path and ask: would an agent, reading only this error, know what to change? "expected `Result<User, FeedError>`, got `Result<User, string>`" is fine. "type mismatch" is not. Use `ariadne`/`miette` for source spans. The manifesto's argument is that agents fix what the compiler tells them to fix; unclear error = product bug.
- **`--explain E0042` style command** for the top 20 errors, each with a paragraph plus a code-fix example. ~12 hours of writing, pays back in agent task-success-rate forever.

---

## What to cut from v0 to make the deadline

Three things will eat time and aren't load-bearing for "does it compile and run":

1. **Full type inference.** Require annotations on function signatures and any `let` whose type isn't obvious from the RHS literal. TS people will grumble; ship anyway. Add in v1.
2. **Generics beyond the simplest case.** `fn array_schema<T>(element: Schema<T>) -> Schema<Array<T>>` should work. Higher-kinded stuff, generic constraints, conditional types — defer. Rewrite examples that need them.
3. **The `infer_shape<Shape>` mapped-type magic in `01_validator.txt`.** That's TypeScript-grade type-level computation, a multi-week project on its own. For v0, require explicit output type or use `unknown` and emit a TS cast. Document as a known limitation; the example still compiles and runs, which is the goal.

---

## The one pushback on the brief

"Every one of your 50 example programs compiles and runs" is the right target *for the demo*, but the wrong target for the *compiler*. A compiler that passes 50 hand-picked examples and breaks on the 51st is worse than one that passes 40 plus handles fuzzer-grade malformed input gracefully.

**Budget two of the six weeks' worth of slack (~4 days) for property-based testing** with `proptest` on the parser and exhaustiveness checker. Agents produce weird inputs; the compiler that survives that is the one that ships.

---

## Open follow-ups

Possible deeper dives:
- Pratt parser table derived from `PRECEDENCE.md`
- Maranget exhaustiveness implementation, concretely
- JSX directive desugaring rules end-to-end
- Emitted-TS shape for `?` and `match`, fully worked out
