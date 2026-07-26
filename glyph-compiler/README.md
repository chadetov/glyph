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
| `glyph-typechecker` | Match exhaustiveness (tagged unions, arrays, bool, number/string, and string-literal unions), the `?` operator rule, call/`await` synthesis, generic instantiation, `owned` single-consumption (D25), runtime descriptors. |
| `glyph-emit` | AST→TS visitor: emits every declaration/statement/expression, lowers `match`/`?`/JSX, and generates runtime descriptors (`is`/`parse`/`schema`) with deep, generic, membership, and integer validation. |
| `glyph-formatter` | Canonical reprinter behind `glyph fmt`: one layout, round-trips, idempotent, preserves comments. |
| `glyph-lsp` | The language server and the MCP server, both over stdio, sharing one pure `analysis` layer (diagnostics, hover, definition, references, rename, symbols). |
| `glyph-cli` | The `glyph` binary: `build [--check] [--test] [--json]`, `run`, `fmt`, `regen`, `gen` (openapi/dts/zod), `init`, `publish`, `lsp`, `mcp`, and `--explain`. |
| `glyph-wasm` | WebAssembly bindings: compile a Glyph source string to TypeScript + diagnostics in memory (powers the web playground). |
| `glyph-runtime` | A stub. Compile-time test execution (`@example` D23, `@doc @run` D26) reuses the emitter + `tsx` from `glyph-cli` rather than a separate interpreter, so this crate is currently unused. |

## Build + test

```bash
cd glyph-compiler
cargo test --workspace
```

678 tests pass. `glyph build src/ --out dist/` walks a directory of `.glyph` files, emits TypeScript into `--out`, and (unless `--no-check`) type-checks it with `tsc`. The toolchain-dependent paths (`glyph run`, `glyph build --test`, `gen`) need `node`/`tsx`/`tsc` on `PATH`.

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
