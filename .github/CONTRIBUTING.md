# Contributing to Glyph

Thanks for looking. Glyph is an early-preview language that transpiles to
TypeScript, built so AI agents can read, write, and modify code safely. It's
moving fast and pre-1.0, so this guide is short and honest about what's useful
right now.

## What helps most today

- **Bug reports.** A `.glyph` program that miscompiles, crashes, or produces
  TypeScript that `tsc --strict` rejects is gold. Use the bug template; a
  minimal reproducer plus the exact command and output is ideal.
- **Rough edges and confusing errors.** If a diagnostic didn't tell you how to
  fix it, or a feature surprised you, say so. Elm-quality error messages are a
  goal, not a given.
- **Docs and examples.** Fixes to the guide, the spec, `docs/reference/`, or a
  new example program that exercises a real pattern are very welcome.

## Please open an issue first for language changes

**The grammar is the spec.** Every syntax or semantics rule traces to a numbered
design decision, and the four pillars — verifiability, greppability, abstraction,
diff stability — decide every call. Glyph is *deliberately* stricter than
TypeScript on several axes (no `if`/`else`, `match` must be exhaustive, trailing
commas required, one declaration form per name). Those restrictions earn their
keep; the answer to an annoying one is usually documentation, not loosening the
rule.

So: before writing code that changes the language, open an issue to discuss the
design. A PR that relaxes syntax "to be helpful" will likely be declined — not
because the effort isn't appreciated, but because the constraint is the point.
Toolchain fixes, diagnostics, docs, and examples don't need this — just send them.

## Building and testing

The compiler is a Rust workspace under `glyph-compiler/`.

```sh
cd glyph-compiler
cargo test --workspace        # the full suite, ~1s warm
```

Requires Rust 1.95+ (pinned via `rust-toolchain.toml`). Some tests (`glyph run`,
`@example`/`@doc @run` execution, `--check`) shell out to `tsx`/`tsc`; install
them (`npm install -g tsx typescript`) or those tests skip.

After an intentional AST change, regenerate the parser snapshots:

```sh
INSTA_UPDATE=always cargo test -p glyph-parser --test snapshots
# or review interactively:
cargo insta review
```

New behavior should come with a test: a unit or integration test for the happy
path, and — for a new diagnostic — a case under `glyph-compiler/tests/negative/`
(a program that must fail with the named code). Every example must pass
`tsc --strict` (CI enforces this).

## Repository layout

```
glyph-compiler/   the Rust compiler (lexer -> parser -> resolver -> typechecker -> emit) + CLI
docs/             the guide, language spec, roadmap, and references
examples/         runnable example programs (all type-checked in CI)
web/              the glyphlang.io site (static, deployed via GitHub Pages)
npm/              the published npm launcher + per-platform binary packages
```

## Commits and PRs

- **Imperative, concise subject lines** ("Fix nested match lowering"), under ~70
  characters, no `feat:`/`fix:` prefixes.
- **Explain the why in the body** when it isn't obvious from the subject; note
  what the change deliberately does *not* do.
- Keep each commit a coherent unit; group by theme, not by file.
- Run `cargo test --workspace` before pushing.

## Stability and scope

Glyph is pre-1.0 and can change between 0.1.x releases; see
[docs/stability.md](../docs/stability.md). Self-hosting is a v1.0 non-goal — the
compiler stays Rust for now.

## Be kind

Assume good faith, keep discussion technical and respectful, and remember this
is early software built in the open. Questions are welcome in
[Discussions](https://github.com/chadetov/glyph/discussions).
