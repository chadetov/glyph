# Glyph v0.1 Release Notes

Glyph is a statically typed language that transpiles to TypeScript, designed so AI agents can read, write, and modify code safely. This is the first public release. It is early (v0.1), and these notes are deliberately honest about what works, what is a hypothesis, and what is not done yet.

## Highlights

- **A small, strict language that compiles to readable TypeScript.** Glyph runs anywhere TypeScript runs and can use any npm package. The compiler type-checks each program and verifies the emitted TypeScript with `tsc --strict`; the example and corpus programs pass it. (The v1 type checker is not yet complete; see Known limitations.)
- **A complete v0.1 toolchain.** A CLI (`glyph build`, `run`, `fmt`, `canonical`, `publish`, `lsp`, `--explain`), a language server, a VS Code extension, npm distribution (`npm install -g glyph` / `npx glyph`), and an in-browser WebAssembly playground with no backend.
- **Errors as values, exhaustive matching, and no `any`.** The language removes the most common ways a type checker can be silently lied to.
- **Designed around four pillars, in priority order.** Verifiability and greppability are the wedge: they target problems TypeScript developers feel today. Abstraction and diff stability are the polish.

A note on the central bet: the idea that *agents write correct code faster in Glyph* is a hypothesis. The structural metrics below are consistent with it, but it has **not** been validated with a real agent study. We are not claiming a speedup number.

## Language

The v0.1 feature set is locked:

- **Records** for product types and **tagged unions** for sum types, each with a single declaration form per symbol. One way to declare a thing means one thing to grep for.
- **Exhaustive `match`.** The compiler rejects a match that does not cover every variant of a union. If an agent adds a variant and forgets a case, the build fails.
- **Errors as values.** `Result` plus `match` plus the `?` operator. The `?` operator must sit in a `Result`-returning function, its operand must be a `Result`, and its error type must match the enclosing error type.
- **No `any`, and no cast expression.** There is no escape hatch to assert a value into a type. Unknown data must be validated (via an `is`-match, or by parsing against a schema that returns a `Result`) before it can be used as a typed value.
- **A narrow `owned` modifier for resource handles** (files, sockets, database connections). This is resource discipline for a small set of handle types, not a general affine or linear type system.

### What the verifiability pillar buys you, concretely

Two paired demos (in `benchmarks/verifiability/`) each show a bug that Glyph rejects at compile time and that `tsc --strict` accepts:

1. **Exhaustiveness.** An agent adds a `Triangle` variant to a `Shape` union and forgets to handle it. Glyph rejects the build with `E0200`: `non-exhaustive match on Shape: missing variants Triangle`. The equivalent TypeScript `switch` compiles clean under `tsc --strict` and silently returns `0` for the triangle case. TypeScript has no built-in exhaustiveness; the `assertNever` idiom is manual, and agents forget it.
2. **Unsafe cast.** Glyph has no cast expression, so `input as User` does not compile; you must validate `input` first. The equivalent TypeScript `input as User` compiles clean under `tsc --strict` and throws at runtime when `input` is not a `User`.

These are the demonstrated cases. Glyph does not catch every type error TypeScript misses; some checks are deferred (see Known limitations).

### Greppability and density

The same task was implemented in each of four languages and measured with an approximate, dependency-free token proxy (`benchmarks/measure.sh`). This proxy is **not** a tiktoken-exact count; a real tokenizer would change the absolute numbers but not the ranking. Only three functions have been measured so far (`parse_user`, `load_feed`, `slugify`).

| Function | Glyph | TypeScript | Python | Rust |
| --- | --- | --- | --- | --- |
| load_feed | 174 | 263 | 207 | 330 |
| parse_user | 144 | 181 | 141 | 176 |
| slugify | 50 | 56 | 60 | 143 |
| **Total** | **368** | **500** | **408** | **649** |

On these three functions, Glyph uses about 26% fewer tokens than equivalent TypeScript (368 vs 500), about 43% fewer than Rust, and about 10% fewer than Python, while remaining fully statically typed (Python is not). Non-blank, non-comment line counts tell the same story: Glyph 46, TypeScript 55, Python 57, Rust 67.

### Diff stability

A one-line Glyph edit produces a one-line TypeScript diff. This is demonstrated live in the playground: changing a per-seat price from `12` to `10` yields a minus-one / plus-one diff on both the Glyph and the emitted TypeScript sides. The structural guarantees behind it are one fixed formatting layout (`glyph fmt`), required trailing commas, one element per line past two elements, no line-length reflow, and no barrel files. A cross-language diff harness metric is future work, not a measured result.

## Tooling

Everything below works end to end in v0.1.

- **CLI.** `glyph build [--check] [--test]`, `glyph run`, `glyph fmt`, `glyph canonical`, `glyph publish`, `glyph lsp`, and `glyph --explain <code>` for long-form explanations of any diagnostic code. The compiler is a Rust, salsa-backed pipeline (lex, parse, resolve, typecheck, emit). 380+ workspace tests pass.
- **Language server (v1 complete).** Diagnostics with stable codes, hover types, go-to-definition (within-file and cross-module), completion, format-on-save, and document plus workspace symbols. It also exposes a canonical agent view (`glyph/canonicalView`) and a typecheck-gated structured-edit RPC (`glyph/applyEdit`) that applies an edit only if the result type-checks clean.
- **VS Code extension.** In `editors/vscode/`.
- **npm distribution.** `npm install -g glyph` or `npx glyph`, packaged esbuild-style: a launcher plus per-platform prebuilt binary packages.
- **In-browser playground.** A WebAssembly build with no backend: write Glyph and see the emitted TypeScript and diagnostics instantly.
- **Docs guide.** A five-minute tour, getting-started, a "Glyph for TypeScript developers" delta sheet, and a 30-minute todo-CLI tutorial. Every snippet in the guide compiles.

## Known limitations / not in v0.1

This is an early release. The following are deferred or unmeasured, and we would rather say so plainly.

- **The type checker is not complete.** A fuller unifier is planned for v1.1, and some checks are deferred. Glyph catches the demonstrated classes of bugs (exhaustiveness, unsafe casts); it does not yet catch every type error TypeScript misses. Do not read v0.1 as a superset of `tsc --strict` on every axis.
- **`glyph publish` does not yet bundle standalone libraries.** Library bundling for publish is not in this release.
- **No cross-language diff harness metric.** Diff stability is demonstrated live in the playground and backed by structural guarantees, but the cross-language diff-size measurement is future work.
- **The LSP lacks rename and find-references.** These are not implemented in v0.1.
- **No measured agent-productivity study.** The claim that agents write correct code faster in Glyph is a hypothesis supported by the structural metrics above, not a measured result. There is no benchmarked speedup.
- **Token metric is a proxy.** The density numbers come from an approximate, dependency-free counter, not from a real tokenizer.

## How to try it

- **Install:** `npm install -g glyph`, or run it once with `npx glyph`.
- **Build a program:** `glyph build path/to/file.glyph` (add `--check` to type-check the emitted TypeScript, `--test` to run tests).
- **Run a program:** `glyph run path/to/file.glyph`.
- **Explore in the browser:** open the WebAssembly playground and watch the emitted TypeScript and diagnostics update as you type.
- **In your editor:** install the VS Code extension from `editors/vscode/`.
- **Learn the language:** start with the five-minute tour and the "Glyph for TypeScript developers" delta sheet in the docs guide.

Source, issues, and the full documentation live at [github.com/chadetov/glyph](https://github.com/chadetov/glyph). Glyph is licensed under MIT OR Apache-2.0.
