# glyph-compiler

The Rust workspace for the Glyph compiler. It is a complete transpile-to-TypeScript toolchain: `glyph build` runs the pipeline (lex → parse → resolve → typecheck → emit), writes TypeScript, and type-checks it with `tsc --strict`. This README is a layout reference; the curated design record is in `../docs/`.

## Crate layout

| Crate | Role |
|---|---|
| `glyph-lexer` | Hand-written lexer: keywords, significant newlines (D1), template literals (D22), triple-quoted strings, `//` comment recovery. |
| `glyph-ast` | AST node types with a `Span` on every node. |
| `glyph-parser` | Pratt parser for every D-decision: JSX sub-grammar (D6) incl. prop spread, annotations (D27), `extern_ts` escape hatch (D29), string-literal unions (D30), `typeof` type queries (D32). |
| `glyph-resolver` | Name resolution, module graph, cross-module import verification (D15), the prelude (incl. `int`, D31). |
| `glyph-db` | Salsa-backed incremental query pipeline (parse → collect → resolve → typecheck), per-decl input slicing, cross-file auto-invalidation. |
| `glyph-typechecker` | Match exhaustiveness (tagged unions, arrays, bool, number/string, and string-literal unions), the `?` operator rule, call/`await` synthesis, generic instantiation, `owned` single-consumption (D25), runtime descriptors, and the stdlib model: return types for the fixed-arity half of `std/string`/`std/array` plus the field and variant shapes of `fs.FsError`/`fs.FileInfo`/`fs.ErrorKind`. |
| `glyph-emit` | AST→TS visitor: emits every declaration/statement/expression, lowers `match`/`?`/JSX, and generates runtime descriptors (`is`/`parse`/`schema`) with deep, generic, membership, and integer validation. |
| `glyph-formatter` | Canonical reprinter behind `glyph fmt`: one layout, round-trips, idempotent, keeps every comment where it was written (including inside a record, literal, argument list, or match). A list is inline while it fits the 100-column print width from its starting column and one element per line otherwise, at every element count. |
| `glyph-lsp` | The language server and the MCP server, both over stdio, sharing one pure `analysis` layer (diagnostics, hover, definition, references, rename, symbols). |
| `glyph-cli` | The `glyph` binary: `build [--no-tsc] [--no-test] [--json]`, `check [--no-tsc] [--json]`, `run`, `fmt`, `regen`, `gen` (openapi/dts/zod), `init`, `publish`, `lsp`, `mcp`, `doctor`, `upgrade`, and `--explain`. |
| `glyph-wasm` | WebAssembly bindings: compile a Glyph source string to TypeScript + diagnostics in memory (powers the web playground). |
| `glyph-runtime` | A stub. Compile-time test execution (`@example` D23, `@doc @run` D26) reuses the emitter + `tsx` from `glyph-cli` rather than a separate interpreter, so this crate is currently unused. |

## Build + test

```bash
cd glyph-compiler
cargo test --workspace
```

1062 tests pass. `glyph build src/ --out dist/` walks a directory of `.glyph` files, emits TypeScript into `--out`, type-checks it with `tsc` (unless `--no-tsc`), and runs every `@example` / `@doc @run` test (unless `--no-test`). `glyph check [path]` is the read-only half: a `.glyph` file or a directory, the same pipeline into a temp dir it deletes on the way out, `tsc --strict` unless `--no-tsc`, and nothing written to your tree. It runs the same `@example` / `@doc @run` gate `glyph build` does, so it cannot report a clean tree that `build` would fail; `--no-test` skips it when you want an answer that executes nothing. The toolchain-dependent paths (`glyph run`, the `glyph build` example gate, `gen`) need `node`/`tsx`/`tsc` on `PATH`. A **spec conformance corpus** (`glyph-emit/tests/conformance/`, one program per language feature keyed to its D-decision) pins the exact emitted TypeScript as a committed snapshot, so any change to what a feature means fails the build until the diff is reviewed; regenerate with `INSTA_UPDATE=always cargo test -p glyph-emit --test conformance`.

## Library versions (P5)

Locked in `Cargo.toml` workspace dependencies. Pin rationale per `../docs/implementation-plan.md §P5`:

- `salsa = "0.26"` — incremental query architecture (Q5 hybrid). v0.26+ is the rewrite; v0.16 was the legacy generation.
- `ariadne = "0.4"` — Elm-quality diagnostic rendering (Q6)
- `insta = "1"` — golden snapshot tests
- `proptest = "1"` — property-based fuzzing of the parser/lexer and the exhaustiveness checker
- `tower-lsp = "0.20"` — LSP framework
- `tokio = "1"` — async runtime for the LSP + subprocesses
- `clap = "4"` — `glyph-cli` argument parsing
- `serde = "1"` / `serde_json = "1"` — `package.json` parsing and the `gen` helper JSON protocol
- `thiserror = "1"` — internal error types

Update via the implementation plan or a written justification, not ad-hoc.
