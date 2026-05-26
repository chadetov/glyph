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

Phase 1 week 1 complete. The Rust parser (lexer plus AST plus Pratt parser plus JSX sub-grammar plus template literals plus annotations) handles all 27 spec decisions and parses the four hard-case example programs end to end. 67 tests pass.

Phase 1 week 2 is next: name resolution, module graph, and a salsa-backed type representation.

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
