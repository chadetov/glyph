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

## Why Glyph

1. **Built for AI agents.** They can read, write, and change code safely.
2. **Looks like TypeScript.** You can read it on day one, no tutorial.
3. **Compiles to TypeScript.** It runs anywhere TS runs and uses any npm package.
4. **No `any`.** What the types say is true when the code runs.
5. **One name, one form.** `grep` always finds where something is defined.
6. **Errors are values, not exceptions.** You handle them with `match`.
7. **`match` must cover every case.** The compiler tells you what you missed.
8. **A one-line change makes a one-line diff.** Reviews stay small.
9. **Tests live next to the code.** They run on every build.
10. **Clear error messages.** Each one tells you how to fix the problem.

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
