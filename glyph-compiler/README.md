# glyph-compiler

The Rust workspace for the Glyph v1.0 compiler. See `../docs/implementation-plan.md` for the phase-by-phase scope; this README is a layout reference only.

## Crate layout

| Crate | Phase 0 status | Implements | Phase |
|---|---|---|---|
| `glyph-lexer` | stub | Hand-written lexer (D1, D12+D22, D13, D14, D17, D21, D22, D27) | Phase 1 week 1 |
| `glyph-ast` | stub | AST enums per node category, `Span` on every node | Phase 1 week 1 |
| `glyph-parser` | stub | Pratt parser; D7 context disambiguation; D6 JSX sub-grammar | Phase 1 week 1 |
| `glyph-resolver` | stub | Salsa-backed name resolution, module graph, D15 import rules | Phase 1 week 2 |
| `glyph-typechecker` | stub | Salsa-backed types + ADTs + Maranget exhaustiveness + D25 owned tracking | Phase 1 week 3 |
| `glyph-emit` | stub | Dumb AST→TS visitor (no IR); D6 JSX directive lowering | Phase 1 week 4 |
| `glyph-runtime` | stub | Sandboxed interpreter for D23 `@example` and D26 `@doc @run` | Phase 1 week 6 |
| `glyph-cli` | stub binary | `glyph build / run / fmt / regen / publish / --explain` | Phase 1 weeks 5–7 |

## Build (Phase 0 acceptance: workspace compiles empty)

```bash
cd glyph-compiler
cargo check --workspace
```

If `cargo check` passes with all stubs, Phase 0 P2 is complete.

## Library versions (P5)

Locked in `Cargo.toml` workspace dependencies. Pin rationale per `docs/implementation-plan.md §P5`:

- `salsa = "0.26"` — incremental query architecture (Q5 hybrid). The crate that was tracked as "salsa-2022" during the rewrite has reclaimed the canonical `salsa` crate name on crates.io. v0.26+ is the rewrite; v0.16 was the legacy generation.
- `ariadne = "0.4"` — Elm-quality diagnostic rendering (Q6)
- `insta = "1"` — golden snapshot tests from Phase 1 week 1
- `proptest = "1"` — property-based testing (Phase 1 week 8)
- `tower-lsp = "0.20"` — LSP framework (Phase 4)
- `tokio = "1"` — async runtime for LSP + subprocess
- `clap = "4"` — `glyph-cli` argument parsing
- `serde = "1"` / `serde_json = "1"` — `package.json` `"glyph"` key parsing (Q22)
- `thiserror = "1"` — internal error types

Update via the implementation-plan or a written justification; not ad-hoc.

## Phase 0 verification (2026-05-26)

Verified end-to-end on macOS with Rust 1.95.0 stable (rustup-managed; project pin in `rust-toolchain.toml`):

| Command | Result |
|---|---|
| `cargo check --workspace` | All 8 crates compile cleanly (52s cold, ~3s warm) |
| `cargo test --workspace` | All 7 stub tests pass |
| `cargo build --release --bin glyph` | Release binary builds in 27s |
| `./target/release/glyph --help` | Prints clap-generated help with build/run/fmt/regen/publish + `--explain` |
| `./target/release/glyph build src/ --out dist/` | Exits 1 with `phase 0 stub: \`glyph build\` not yet implemented` |
