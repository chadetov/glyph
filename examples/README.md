# Glyph examples

## Building the tree

The whole directory builds in one command:

```sh
glyph build examples --out /tmp/out
```

Each of the thirty-one directories under `apps/` is a program of its own whose
modules import each other by bare name, so each one carries a `package.json`
with a `"glyph"` key. That marker makes the directory its own module-resolution
root (D41), so `import catalog` inside `apps/csvql` finds
`apps/csvql/catalog.glyph` no matter which enclosing directory you point
`glyph build` at. Every app also carries a `README.md` saying what it is, how to
run it, and which compiler gaps it found. Their output lands under
`/tmp/out/apps/<name>/`.

The same app still builds standalone, and emits the same files:

```sh
glyph build examples/apps/csvql --out /tmp/out
```

A project's imports resolve within its own root only, in both directions:
`apps/csvql` cannot import a module of `examples/`, and `examples/` cannot
import one of `apps/csvql`.

Checking a single file compiles every `.glyph` in its project (G72), so
`glyph check examples/01_validator.glyph` reports its siblings' errors too.

The five hard-case programs at the top level are the seed corpus for the transpiler test suite; the originals are recorded in `archive/SESSION_1.md`. Everything under `apps/` came later, from building real programs and fixing whatever the language lacked.

| File | Stresses | Pillars |
|---|---|---|
| `01_validator.glyph` | Type system + runtime descriptors + auto-generated schemas | Verifiability |
| `02_async_errors.glyph` | `Result` types + `?` propagation + `par.all`/`par.all_ok` | Verifiability + abstraction |
| `03_react_component.glyph` | JSX sub-grammar + compiler-owned directives (`<if>`, `<for>`, `<match>`, `<case>`, `<else>`) + restricted JSX expressions | Abstraction + greppability |
| `04_cli_tool.glyph` | Program entry + exhaustive subcommand dispatch + file I/O + process exit codes + structured logging | Greppability + diff stability |
| `05_rest_api.glyph` | `std/http` server + typed request/response DTOs + descriptor-validated request bodies (`T.parse`) + auth check, all errors-as-values | Verifiability + greppability |

## V1.0 deviations from the original step-2 corpus

`01_validator.glyph` differs from the version inline in `archive/GLYPH.md §3.1` to reflect the **brainstorm Q1 resolution** (defer mapped types to v1.1). The original used `Schema<infer_output<Shape>>`; v1.0 uses an explicit `<Out>` type parameter supplied by the caller's type annotation. V1.1 will re-introduce `infer_output` so the caller's type and the shape's fields stay in sync automatically.

The other three files are faithful transfers, with template literals (D22) used in places where the original used `+` concatenation. The semantics are unchanged.

## `apps/` — end-to-end dogfood applications

Small but complete programs, each built by writing real Glyph against the
stdlib and fixing whatever the language lacked along the way. Each is a
directory carrying a `package.json` with a `"glyph"` key, so it is its own
module-resolution root (D41) and builds standalone or as part of the tree.
None contains a line of TypeScript: no `.d.ts`, no `extern_ts`, enforced by
`scripts/check_apps_are_glyph.py`.

Each app has a `README.md` with the full account. The third column is the
short version: what writing it changed in the compiler.

| App | What it is | What it found |
|---|---|---|
| [`adventure`](adventure/README.md) | Ten-room text adventure over a keyed world | `glyph run` threw its own diagnostics away (G38) |
| [`auth_api`](auth_api/README.md) | Signup/login HTTP API with sessions | A boundary said which field was wrong, never which rule (G79) |
| [`bracket`](bracket/README.md) | Single-elimination tournament as one recursive value | `@example` was opt-in, so a false assertion built green (G49) |
| [`chat`](chat/README.md) | TCP chat server holding several clients at once | `glyph run` killed any program still working when `main` returned (G84) |
| [`collections`](collections/README.md) | Generic `Heap`, `Cache`, `Trie`, and a fallible pipeline | A `for` over an unsettled type bound a string index (G109) |
| [`csvql`](csvql/README.md) | SQL-ish query engine over CSV files | An imported literal union lost exhaustiveness, and the help said to delete it (G76) |
| [`depsolve`](depsolve/README.md) | Dependency resolver with conflicts and cycles | `std/record` was not modeled in the typechecker at all (G71) |
| [`diff3`](diff3/README.md) | Three-way text merge, by line or by word | Nothing. A deliberate probe that found no gap |
| [`discord`](discord/README.md) | Discord gateway client: handshake, heartbeat, resume | An exhaustive `match` could throw at run time (G94) |
| [`expenses`](expenses/README.md) | Expense report over a CSV ledger, exact money | `time.parse_iso` failed open while its docs promised closed (G31) |
| [`feeds`](feeds/README.md) | RSS reader on the `fast-xml-parser` npm package | The app behind the 1.0 interop gate. Open: G118, G119 |
| [`fridge`](fridge/README.md) | Shopping-list CLI, JSON on disk | `json.parse<T>` was a cast, not a validating parse (G3) |
| [`i18n`](i18n/README.md) | Localized formatter, one catalogue per locale | `Intl` was unreachable, so CLDR plurals had no route (G113) |
| [`intake`](intake/README.md) | Batch validator merging every field's failure | Nothing recorded. Demonstrates `Validated` where `Result` short-circuits |
| [`jobq`](jobq/README.md) | Durable job queue: HTTP, SQLite, workers | `==` meant deep equality in a test and reference equality in the code (G65) |
| [`leaderboard`](leaderboard/README.md) | Order-statistics red-black tree over an append-only log | Nothing, by design. It is the proof four earlier releases landed |
| [`linkcheck`](linkcheck/README.md) | Markdown link checker, bounded concurrency | A value-position `match` picked its lowering on the wrong test (G43) |
| [`minilang`](minilang/README.md) | Interpreter for a small language, with a REPL | Nothing of its own. Shaped by the `read_line` defect (G81) |
| [`minesweeper`](minesweeper/README.md) | Terminal Minesweeper, seeded and replayable | `glyph fmt` relocated comments out of the construct they documented (G23) |
| [`pulse`](pulse/README.md) | Uptime monitor over DNS, TLS and a raw socket | A TLS dial that never settles, in a module promising values (G127) |
| [`resilient`](resilient/README.md) | Retry, backoff, circuit breaker, concurrency limiting | Its source disproved a premise the roadmap carried eight releases (G99) |
| [`schedule`](schedule/README.md) | Meeting-slot finder across calendars | A `where` refinement stopped working as a record field (G40) |
| [`settle`](settle/README.md) | Group expense splitter and debt simplifier | A `match` expression always typed as `Unknown` (G57) |
| [`sheet`](sheet/README.md) | Spreadsheet with formulas and dependency order | A declaration shadowed a global the emitted module needs (G63) |
| [`shortlink`](shortlink/README.md) | URL shortener you can point a browser at | The build cache served a stale program under a clean build (G56) |
| [`sitegen`](sitegen/README.md) | Static site generator on `marked` and `gray-matter` | No default import, so a callable npm package was unreachable (G112) |
| [`tasks`](tasks/README.md) | Persisted task API on SQLite | None. The demonstration half of 0.1.25's database work |
| [`watchrun`](watchrun/README.md) | Dev loop: watch, debounce, spawn, stream output | The bundled node shim was narrower than node, twice (G125, G126) |
| [`webhook_ingress`](webhook_ingress/README.md) | Verifies inbound webhooks, serves an admin page | None. Found that taint is opt-in, not dataflow |
| [`workflow`](workflow/README.md) | Statechart replay engine over a JSON definition | A namespaced `match` on an imported union was never checked (G73) |
| [`zipper`](zipper/README.md) | CLI shell over a virtual filesystem, Huet zipper | Provoked G139; its filed reproduction did not reproduce |

## `corpus/` — self-contained regression programs

`corpus/` holds small programs that depend on no stdlib or external modules (no `Result`/`Option` imports, no `react`, no `std/*`). Each exercises one emitter feature in isolation, and — because nothing is left untyped — its emitted TypeScript passes `tsc --strict --noEmit` end to end. The four hard-case examples above instead import external/stdlib modules, so their emitted code is type-correct only once those modules' types exist; the corpus is what proves the emitter itself produces fully `tsc`-clean output today.

| File | Stresses |
|---|---|
| `shapes.glyph` | Tagged union + exhaustive constructor-pattern match |
| `maybe.glyph` | Generic union + payload binding |
| `sum.glyph` | `for` loop accumulating into a `mut` binding |
| `list_ops.glyph` | Array-pattern match with a `...rest` binding |
| `classify.glyph` | Value (literal) match with an `else` catch-all |
| `higher_order.glyph` | Records + higher-order functions + lambdas |
| `generics.glyph` | Generic functions + explicit call-site type arguments |
| `tree.glyph` | Recursive tagged union + recursive function |
| `async_chain.glyph` | `async`/`await` functions awaited in sequence |

`repo_examples_emit_typescript_without_diagnostics` (in `glyph-cli`'s integration tests) builds the whole `examples/` tree — these plus the four hard-case files — and asserts every module emits with no diagnostics.
