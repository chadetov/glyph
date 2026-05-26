# Glyph — Session 1: Syntax Lock via Examples

**Goal:** Step 2 of the Glyph roadmap — "lock the syntax with examples, not a grammar." Write small Glyph programs by hand; let real code force every syntactic decision.

**Status at end of session:** Design phase of step 2 complete. Volume phase (26-46 remaining programs) not started. Four hard cases written, ~49 syntactic decisions locked, one precedence spec produced.

---

## How we worked

Hard cases first, easy cases later. The four programs were chosen to stress-test the parts of the language most likely to fight back:

1. **Validator** — type system + verifiability pillar
2. **Async error flow** — Result types + control flow without exceptions
3. **React component** — JSX, reactive state, declarative UI
4. **CLI tool** — program structure, exhaustive dispatch, I/O

Each file produced syntactic decisions. After every file, decisions were flagged, alternatives noted, and locked or deferred by explicit user choice.

---

## The four files

Located in `/home/claude/glyph-examples/`, also exported as `.txt`:

- `01_validator.glyph` — Zod-style validator with runtime descriptors
- `02_async_errors.glyph` — async pipeline returning `Result<Feed, FeedError>`
- `03_react_component.glyph` — search-as-you-type with restricted JSX
- `04_cli_tool.glyph` — `todo` CLI with subcommands and typed errors
- `PRECEDENCE.md` — operator precedence spec note

---

## Locked decisions

### Module & imports
- `module path/name` at top of every file
- `import path/name { Named, Items }` only
- No default imports, no `import *`, no barrel files
- Path-based imports use `/`, not `.`

### Declarations
- `type` for all shapes (no `interface`)
- `fn` for all functions (both named and anonymous)
- `component` keyword for React components (distinct from `fn`)
- `const` for compile-time constants
- `let` for runtime bindings
- `mut` at call site for in-place mutation only (not for setter calls)

### Types
- Tagged unions: `type E = | Variant({ field: T }) | Other`
- `Result<T, E>` with `Ok`/`Err`
- `Option<T>` with `Some`/`None` (TS-style nullable `T?` deferred as future sugar)
- `Record<string, T>` distinct from named records
- `void` is both type and value
- Auto-generated `T.schema` for every type declaration (verifiability pillar)
- Generic syntax: `Schema<T>`, `fn array_schema<T>(...)`

### Expressions
- `match` is an expression
- `match` handles both patterns and `is TypeName` guards (unified, still provisional)
- Postfix `?` propagates `Result` only (not `Option`, not optional chaining)
- `&&` `||` `!` for booleans (symbols, not words)
- `??` for nullish coalescing
- No `as T` cast — forced through `match` or schema parse
- Array patterns: `[]`, `[head, ...rest]`, `[_, ...]`
- Record destructuring with shorthand: `{ users }` binds local `users`
- Spread in updates: `{ ...t, done: true }`, `[...items, new_item]`

### Methods & functions
- Real methods on core types (Result, Option, etc.)
- UFCS: `r.foo(x)` desugars to `foo(r, x)` for free functions

### JSX (the biggest design surface)
- Tag syntax kept: `<Foo prop={...}>...</Foo>`
- `class=` not `className=`
- `on_input`, `on_click` — snake_case events
- **Restricted expression model**: typechecker rule that `{...}` only allows literals, identifier reads, member access, and pure calls
- **Compiler-owned directives**: `<if>`, `<else>`, `<for user in={...}>`, `<match value={...}>`, `<case Variant({ fields })>`
- **The architectural rule:** value expressions allowed; structural rendering requires structural syntax. `class={active ? "a" : "b"}` is fine. `{loading ? <Spinner/> : <Dashboard/>}` is not — use `<if>/<else>`.
- Directive tags lowercase; components PascalCase. Greppable.

### Reactive state
- `use_state` returns `{ value, set }` object, not a tuple
- Optional destructuring sugar available
- Setter calls are calls, not mutations — no `mut` prefix
- Component-level hooks: `use_state`, `use_effect`, `use_memo` (snake_case)

### Concurrency & errors
- `async fn`, `await` (same as TS)
- `par.all`, `par.all_ok` from stdlib for parallelism
- Recoverable errors → `Result<T, E>`
- Unrecoverable → `panic` (not yet shown in corpus)
- `?` propagates recoverable errors only
- String errors banned in library code; allowed only at I/O boundary for stderr output
- Stdlib errors are tagged unions with a `kind` field (e.g. `fs.ErrorKind.NotFound`)

### Program structure
- `async fn main(argv: Array<string>) -> number` is the entry point
- Exit code from `main` return value
- `io.println` / `io.eprintln` from stdlib
- Primitives have stdlib modules: `number.parse`, `string.join`, etc.

---

## Operator precedence (separate spec)

See `PRECEDENCE.md`. The critical rules:

- `await E?` parses as `(await E)?` — await binds looser than `?`
- `E?.field` is illegal — postfix `?` is for Result only; optional chaining is `?.` (one token)
- Method chains bind left
- `await` is a prefix operator, not a statement keyword
- Assignment is statement-level only; no `if (x = foo())` foot-guns

---

## Deferred decisions (revisit later)

| Item | Why deferred | When to revisit |
|------|--------------|-----------------|
| `T?` sugar over `Option<T>` | Pure ergonomics, zero capability cost to defer | Anytime; parser-level desugar |
| Unified vs split `match` (patterns + type guards) | Needed more examples to decide | After batch 2 (10 medium programs) |
| Tuple destructuring for non-reactive returns (`let a, b = ...`) | Never came up in 4 examples | When an example actually needs it |
| `panic` syntax for unrecoverable errors | Not yet shown in corpus | First batch-2 program that needs it |
| Schemas-as-immutable spec language | Already true in practice; just needs writing down | When formal spec is drafted |
| Async resource primitives | Should be stdlib library, not language feature | After 6 months of dogfooding the pattern |
| Semantic event payloads (vs raw DOM events) | Library-first via stdlib helpers like `on_text_input` | Same — library first, language never if avoidable |

---

## Rejected suggestions (with rationale)

These were proposed mid-session and explicitly rejected:

- **`<if cond>` bare-expression directive tags** — breaks JSX attribute uniformity for four characters of savings
- **`bind={x}` directive for variant destructuring** — initially used, dropped in favor of `<case Variant({ fields })>` matching `match` arm syntax
- **`mut set_state(...)` for setter calls** — overloads `mut`; setters are calls, not mutations
- **Tuple-destructuring `use_state`** — `{ value, set }` object is unambiguous; tuples obscure the type
- **`as T` cast** — banned in favor of schema parsing or `match`
- **Word operators `and`/`or`/`not`** — symbols (`&&`/`||`/`!`) chosen for consistency with `==`, `+`
- **"Normalize all UI events into semantic payloads"** — year of design work; defer to stdlib helpers
- **"Separate direct mutation from reactive scheduling"** — already separated; reject until proposal is concrete

---

## What survived from external review (ChatGPT critique)

Three items applied this session:

1. **Schema output type inference** — `object_schema({...})` now returns `Schema<{...}>` with inferred shape, no `as T` artifact
2. **Typed `ParseError` in CLI** — replaced `Result<Command, string>` with a proper algebraic union
3. **Precedence rules written down** — `PRECEDENCE.md`, prevents drift before tree-sitter grammar

Six items deferred or rejected because they would either complicate the language without immediate gain, or are already implicit in the design.

---

## Open questions to sit with

Worth a fresh-eyes second pass before writing batch 2:

1. **Unified `match`** for patterns and `is TypeName` guards — coexisting cleanly in files 01-04, but no definitive verdict
2. **`mut` on method calls** like `mut issues.push(...)` — conceptually awkward (the method does the mutation, not the caller). Works, but might feel off at scale
3. **`void` as both type and value** — reads fine in isolation; might feel strange at 200+ occurrences

---

## What's next

### Immediate
- Sit on the corpus for a day or two before writing more. Re-read cold.
- Don't rewrite anything until volume work surfaces a real problem.

### Batch 2 — 10 medium programs
Aim for breadth, not depth. Surface decisions the four hard cases didn't need:

- HTTP server with routing
- JSON pipeline / stream processor
- Small game (turn-based, e.g. tic-tac-toe with AI)
- Recursive descent parser
- LRU cache
- Pub/sub event bus
- State machine (e.g. traffic light or auth flow)
- Worker pool with backpressure
- Config loader (env + file + defaults)
- Date/time utility module

### Then
- Re-read the MANIFESTO against the actual corpus. Do the four pillars hold up under real code, or did we convince ourselves they did?
- Scope the transpiler honestly before committing to step 3. Three implementation commitments hidden in the locked syntax:
  - Auto-generated `T.schema` requires a schema-emission compiler pass
  - Restricted JSX expressions require a "pure expression" classifier in the typechecker
  - `is TypeName` runtime checks require every type to have a runtime descriptor available at every callsite — the single biggest implementation cost in the language

---

## Honest assessment

The corpus is small but real. The spec is sharper than most languages have at month two. The four pillars from the manifesto (abstraction, verifiability, diff stability, greppability) are visibly present in every file, not just claimed.

The biggest risk now is over-confidence. Four files is enough to lock decisions; it is not enough to know whether those decisions are right. Volume work is the test.
