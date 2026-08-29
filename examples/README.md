# Glyph examples

## Building the tree

The whole directory builds in one command:

```sh
glyph build examples --out /tmp/out
```

Each of the twenty directories under `apps/` (`auth_api`, `chat`,
`collections`, `csvql`, `depsolve`, `diff3`, `discord`, `feeds`, `i18n`,
`intake`, `jobq`, `minilang`, `pulse`, `resilient`, `sheet`, `sitegen`,
`watchrun`, `webhook_ingress`, `workflow`, `zipper`) is a program of its own whose
modules import each other by bare name, so each one carries a `package.json`
with a `"glyph"` key. That marker makes the directory its own module-resolution root (D41), so
`import catalog` inside `apps/csvql` finds `apps/csvql/catalog.glyph` no matter
which enclosing directory you point `glyph build` at. Their output lands under
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
stdlib and fixing whatever the language lacked along the way.

| File | Stresses | Pillars |
|---|---|---|
| `fridge.glyph` | Shopping-list CLI: JSON-on-disk persistence, optional fields, list mutations, and a persistence boundary that must `parse` back into a real `Fridge` | Verifiability + greppability |
| `tasks.glyph` | Persisted task API: `std/sqlite` (Node's built-in SQLite) for durable storage, a storage/domain type split at the DB boundary (SQLite has no `bool`), `std/http` routes, wire-body validation, all errors-as-values. Data survives restarts | Verifiability + greppability |
| `minesweeper.glyph` | Terminal Minesweeper: a 9x9 grid, lazy first-click mine placement, flood-fill reveal, a flag/unflag stdin command loop, and a seeded RNG so a piped transcript replays byte for byte | Verifiability + diff stability |
| `expenses.glyph` | Expense-report CLI over a CSV ledger: every row validated at the boundary with its source line number, exact money via `std/decimal`, per-category totals and shares, and a nonzero exit that lists every bad row at once | Verifiability + greppability |
| `adventure.glyph` | Ten-room text adventure: a keyed world where every exit names a room id, a command parser over free-form stdin, conditional world rules (the cellar is dark until the lantern is lit), and a save file that must validate back into a `World` | Verifiability + greppability |
| `schedule.glyph` | Meeting scheduler over a JSON calendar: validate participants and busy blocks at the boundary, merge overlapping blocks, subtract the union from a working window per day, and print the slots long enough to hold the meeting | Verifiability + abstraction |
| `linkcheck.glyph` | Markdown link checker: inline links, reference definitions, autolinks, and image sources across a file or a directory, with links inside code fences and code spans deliberately excluded, and bounded concurrency on the network checks | Verifiability + greppability |
| `bracket.glyph` | Single-elimination tournament bracket: the whole tournament is one recursive value (a `Match` of two `Slot`s, each an entrant, a bye, or another `Match`), so advancing a winner is a read, not a write into a parallel table | Abstraction + verifiability |
| `shortlink.glyph` | URL shortener you can point a browser at: an HTML form, base62 codes, a 302 redirect with click counting, a stats page, and escaped server-rendered output. No `extern/` shim and no Node import: `std/http`'s `html`, `redirect`, and `form` carry the whole wire | Verifiability + greppability |
| `settle.glyph` | Group expense splitter: split evenly, by exact shares, or by weights, in whole cents with a documented rule for the leftover, then compute the fewest payments that settle everyone up. Its ledger round-trips through `WireLedger.parse` at the boundary | Verifiability + abstraction |
| `leaderboard.glyph` | Speedrun leaderboard over an append-only JSON log: every submission is folded into a persistent order-statistics red-black tree keyed by score, so a rank, a top-N and a range count are O(log n) walks instead of a re-sort. The four Okasaki rotation cases are four match arms nesting a constructor pattern inside another one's field, over a union that names itself with both its type parameters | Abstraction + verifiability |

### Multi-module apps

Each is a directory carrying a `package.json` with a `"glyph"` key, so it is its
own module-resolution root (D41) and builds standalone or as part of the tree.
None of them contains a line of TypeScript: no `.d.ts`, no `extern_ts`, enforced
by `scripts/check_apps_are_glyph.py`.

| Directory | What it is | Stresses |
|---|---|---|
| `auth_api` | Signup/login HTTP API with sessions | Boundary validation, hashing, errors-as-values |
| `chat` | Chat **server** holding several TCP clients at once | Long-lived sockets, line framing over TCP, per-client routing |
| `collections` | Generic `Heap`, `Cache`, and `Trie`, plus a fallible pipeline | Generics, `Option` returns, recursive structure |
| `csvql` | SQL-ish query engine over CSV files | Lexing, planning, joins, aggregate rollups |
| `depsolve` | Dependency resolver with conflict and cycle reporting | Recursive types, backtracking, diagnostics |
| `diff3` | Three-way text merge, by line or by whitespace-separated word | Generic LCS diff, edit-script merging, conflict reporting |
| `discord` | Discord gateway client: handshake, heartbeat, resume | WebSocket, timers, reconnect backoff, `owned` sockets (D25) |
| `feeds` | RSS reader over the `fast-xml-parser` npm package | An npm dependency imported by name with no adapter, its `any` stopped at `Document.parse` |
| `i18n` | Localized message formatter with one catalogue per locale | CLDR plural selection, fallback chains, locale-aware numbers and currency |
| `intake` | Batch validator for JSON applicant records | Per-field checks merged into one report instead of stopping at the first bad record |
| `jobq` | Durable job queue: HTTP API, SQLite store, workers | Persistence, retry with backoff, dead-lettering, state transitions |
| `minilang` | Interpreter for a small language, with a REPL | Parsing, evaluation, interactive stdin |
| `pulse` | Uptime monitor for HTTPS endpoints | DNS, a certificate-verified TLS handshake, hand-written HTTP/1.1, an append-only history file |
| `resilient` | Retry, backoff, circuit breaker, and concurrency limiting against a server that misbehaves on purpose | Policy composition, timers, failure classification |
| `sheet` | Spreadsheet with formulas and dependency order | Topological evaluation, cycle detection |
| `sitegen` | Static site generator over `content/*.md` | The `marked` and `gray-matter` npm packages, front-matter validated at the boundary |
| `watchrun` | Dev-loop tool: watch a directory, debounce a burst, run a command | The `minimatch` npm package, child-process spawn, streamed stdout |
| `webhook_ingress` | HTTP service that verifies inbound webhooks and serves an admin page | HMAC-SHA256 signatures, a bounded per-source history |
| `workflow` | State-machine runner over a JSON definition | Wire/domain type split, optional fields, exhaustive transitions |
| `zipper` | CLI shell over a virtual filesystem, navigated by a Huet zipper | A generic rose tree, a zipper that rebuilds the path on every move, illegal moves as values |

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
