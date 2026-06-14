# Glyph: a language designed for agents to read, write, and change safely

AI agents are writing a growing share of the code that ships. They are good at it, but they fail in predictable ways. An agent adds a new case to a union type and forgets one of the places that switches on it. An agent reaches for `as` to make a type error go away, and the cast quietly defers the problem to runtime. An agent makes a one-line change and the formatter reflows fifty lines around it, burying the real edit in noise. None of these are exotic. They are the everyday failure modes of writing code in a language that was designed for humans with IDEs, not for agents reasoning over text.

Glyph is a statically typed language that transpiles to readable TypeScript, designed so those failure modes are caught by the compiler or prevented by construction. It is early (v0.1), and we want to be precise about what it does and does not yet prove.

## Four pillars

Glyph is held to four properties, in priority order. **Verifiability** and **greppability** are the wedge: they fix problems TypeScript developers already feel. **Abstraction** and **diff stability** are the polish. When the properties conflict, the wedge wins.

Verifiability means the compiler rejects whole categories of mistakes instead of trusting the author. Greppability means there is one declaration form per symbol, so finding every definition is a literal text search, not a guess. Abstraction means the language is expressive enough to be worth writing. Diff stability means a small change produces a small diff.

## Before and after

Here is the verifiability pillar made concrete. Suppose an agent adds a `Triangle` variant to a shape union and updates the area function but misses one case. In Glyph, the compiler stops it:

```
error[E0200]: non-exhaustive match on Shape: missing variants Triangle
```

The same program written as a TypeScript `switch` compiles clean under `tsc --strict` and silently returns `0` for the triangle. TypeScript has no built-in exhaustiveness check; the `assertNever` idiom that approximates one is manual, and agents forget it. In Glyph, `match` must be exhaustive, so a missing variant fails the build rather than slipping through.

A second example: Glyph has no cast expression. Writing `input as User` does not compile, because there is no escape hatch. To turn an `unknown` into a `User` you validate it (an `is`-match, or parsing against a schema, which returns a `Result`). The equivalent TypeScript `input as User` compiles clean and throws at runtime if the input is not actually a `User`.

These two pairs live in `benchmarks/verifiability/` and are asserted by a check script, so the claim is reproducible. We are careful here: a fuller type unifier is planned for v1.1, so Glyph does not yet catch every type error TypeScript misses. It catches the demonstrated ones.

## The evidence, honestly

The deeper bet, that agents write correct code faster in Glyph, is a hypothesis. We have not run an agent study, and we are not going to quote a speedup number we did not measure. What we can show is the structural evidence that bet rests on.

**Density.** We implemented the same three functions (`parse_user`, `load_feed`, `slugify`) in Glyph, TypeScript, Python, and Rust and counted tokens. Totals: Glyph 368, TypeScript 500, Python 408, Rust 649. That is roughly 26% fewer tokens than equivalent TypeScript, about 43% fewer than Rust, and about 10% fewer than Python, while Glyph stays fully statically typed and Python does not. Counting non-blank, non-comment lines tells the same story: 46 for Glyph against 55, 57, and 67. The caveat matters: this is an approximate, dependency-free token proxy from `benchmarks/measure.sh`, not a real tokenizer like tiktoken. A real tokenizer would move the absolute numbers, not the ranking, and only three functions are measured so far.

**Diff stability.** In the in-browser playground you can change a per-seat price from `12` to `10` and watch the emitted TypeScript change by exactly one line on each side: minus one, plus one. That is not luck. It falls out of one fixed formatting layout (`glyph fmt`), required trailing commas, one element per line past two elements, no line-length reflow, and no barrel files. A cross-language diff harness that measures this at scale is still future work.

## What you can use today

Glyph is small but the toolchain is real and works end to end:

- A CLI: `glyph build` (with `--check` to verify the emitted TypeScript under `tsc --strict`, and `--test`), `glyph run`, `glyph fmt`, `glyph canonical`, `glyph publish`, `glyph lsp`, and `glyph --explain <code>` for any diagnostic.
- A compiler written in Rust (a salsa-backed pipeline: lex, parse, resolve, typecheck, emit) with 380+ passing workspace tests.
- Output that is readable TypeScript, runs anywhere TypeScript runs, and can use any npm package.
- A language server: diagnostics with stable codes, hover types, go-to-definition within and across modules, completion, format-on-save, document and workspace symbols, a canonical agent view, and a structured-edit RPC that applies an edit only if the result type-checks clean.
- A VS Code extension, npm distribution (`npm install -g glyph` or `npx glyph`, with per-platform prebuilt binaries), and an in-browser WebAssembly playground with no backend.
- A docs guide: a five-minute tour, getting started, a "Glyph for TypeScript developers" delta sheet, and a 30-minute todo-CLI tutorial whose snippets all compile.

The language itself keeps the parts that make code legible to an agent: errors as values (`Result`, `match`, and `?`), exhaustive `match`, no `any`, and one declaration form per symbol so search is exact.

## Try it, and tell us where it breaks

Glyph is v0.1. The structural metrics are encouraging and the verifiability demos are reproducible, but the central productivity claim is still a hypothesis waiting for a real agent study, and parts of the typechecker are deferred to v1.1. We would rather you find the rough edges than hear us oversell the smooth ones.

The fastest way in is the playground: write a little Glyph, see the TypeScript it emits and the diagnostics it raises, and make a one-line edit to watch the diff stay small. The source is at github.com/chadetov/glyph, licensed MIT OR Apache-2.0. If you try it, tell us what felt wrong. That feedback is what v0.2 is for.
