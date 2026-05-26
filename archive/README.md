# tree-sitter-glyph

Tree-sitter grammar for the Glyph language. The grammar file is the syntactic
spec for Glyph; `SPEC_DECISIONS.md` is the rationale for every choice the
example corpus left ambiguous.

## What's here

```
grammar.js                 Main grammar definition
src/scanner.c              External scanner (newlines, JSX text, string body)
SPEC_DECISIONS.md          Every contested decision with rationale
PRECEDENCE.md              Operator precedence (normative; locked in step 2)
MANIFESTO.md               Glyph's four pillars
examples/                  The four hard-case corpus files from step 2
queries/highlights.scm     Syntax highlighting queries for editor support
```

## Build and test

```bash
npm install
npx tree-sitter generate          # generates src/parser.c from grammar.js
npx tree-sitter parse examples/01_validator.glyph
npx tree-sitter parse examples/02_async_errors.glyph
npx tree-sitter parse examples/03_react_component.glyph
npx tree-sitter parse examples/04_cli_tool.glyph
```

If `tree-sitter generate` reports parse table conflicts, those are real
ambiguities in the spec — resolve them in `SPEC_DECISIONS.md` first, then
adjust the grammar.

## Status

This grammar was written against the four-file corpus locked in step 2 of the
Glyph plan. It has not yet been run through `tree-sitter generate` in this
environment (the CLI binary was unreachable). The next step is local
verification:

1. `npm install` and `tree-sitter generate`.
2. Parse all four example files. Fix any conflicts.
3. Once all four parse cleanly, the grammar becomes the spec for step 4
   (transpiler) and step 5 (typechecker).

## Conflicts you'll likely hit on first generate

Tree-sitter is LR(1) with GLR fallback. The grammar declares conflicts for the
known ambiguities (generic-call vs comparison, pattern-vs-expression in match
arms). Other conflicts that may surface on first generate:

- **Type expressions vs value expressions at `<`.** Resolved by context
  (`fn name<T>` always starts a generic parameter list). If the GLR engine
  can't pick, add the production pair to the `conflicts` array.
- **JSX `<` vs less-than `<`.** JSX is only legal in expression position, and
  the `<` must be followed by an identifier or constructor name (not a number
  or paren). GLR handles this fine in practice.
- **`else` keyword in match arms vs `<else>` JSX directive.** These never
  appear in the same position; the parser state distinguishes them.

## What the grammar is not

- Not the typechecker. `result?.field` is grammatically legal but the
  typechecker rejects it when `result` is `Result`-typed (PRECEDENCE.md).
- Not the formatter. The grammar accepts both single-line and multi-line
  tagged unions; the formatter chooses the canonical form.
- Not the linter. The grammar accepts `mut result.push(x)`; whether `push`
  is actually a mutating method is the typechecker's call.

The grammar is the syntactic spec, not the semantic one. That's intentional.
