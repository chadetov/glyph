# Glyph

A statically typed, transpile-to-TypeScript language designed so AI agents can read, write, and modify code safely.

Glyph looks almost like TypeScript. A TS developer reads a Glyph file on day one without a tutorial. The differences are deliberate and small in number, and every one of them exists to make code an agent can reason about correctly, edit without breakage, and explain back to a human without lying.

## The four pillars

Every design decision is tested against these. If a feature improves one without harming the others, it ships. If not, it doesn't.

1. **Abstraction** — express intent at the level the writer is thinking. Pattern matching over switch ladders, `Result` over thrown exceptions, named records over positional tuples.
2. **Verifiability** — anything the type system claims must be true at runtime. No `any`. No structural-typing surprises. No type erasure.
3. **Diff stability** — a one-line change produces a one-line diff. Fixed-width, single-element-per-line formatting. No barrel files. Trailing commas everywhere.
4. **Greppability** — every symbol has exactly one syntactic form at its declaration site. `grep -n "fn parseUser"` finds the definition. Always.

Verifiability and greppability are the wedge. Abstraction and diff stability are the polish.

## Status

Phase 1 weeks 1 and 2 complete; week 3 (typechecker) underway. 14 days of work shipped:

- Hand-written Rust lexer, Pratt parser, and AST handle all 27 spec decisions. All four hard-case example programs parse end to end.
- Name resolution, module graph, and cross-module verification (`import M { N }`).
- Full salsa-tracked incremental query pipeline (parse → collect → resolve → per-declaration type → project exports → import diagnostics). Per-decl input slicing, source-byte canonical fingerprints, automatic cross-file invalidation when a project file's exports change.
- `glyph build src/ --out dist/` walks a source tree, runs the pipeline, and reports ariadne-rendered diagnostics with source-context lines and caret pointers.
- First real type-system check: Maranget-style variant-set exhaustiveness for `match` over user-defined tagged unions.

179 workspace tests pass. Next: bidirectional checker, `?` propagation typing, `owned` single-consumption analysis, runtime descriptors. Then TS emission (Phase 1 week 4). Live record: [`docs/implementation-plan.md`](docs/implementation-plan.md).

## Where to start

| If you want to | Read |
|---|---|
| Understand the project's thesis | [`docs/manifesto.md`](docs/manifesto.md) |
| See concrete Glyph programs | [`examples/`](examples/) |
| Read the language specification | [`docs/language/spec.md`](docs/language/spec.md) |
| See the roadmap | [`docs/roadmap/overview.md`](docs/roadmap/overview.md) |
| See the full implementation plan | [`docs/implementation-plan.md`](docs/implementation-plan.md) |
| Compare Glyph against TS, Python, Rust | [`benchmarks/`](benchmarks/) |

## Building

```sh
cd glyph-compiler
cargo test --workspace
```

Requires Rust 1.95 or later (pinned via `rust-toolchain.toml`).

## License

Dual-licensed under either of

* [Apache License, Version 2.0](LICENSE-APACHE)
* [MIT License](LICENSE-MIT)

at your option.
