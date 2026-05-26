# Grammar status

The step-3 deliverable is a tree-sitter grammar that encodes the Glyph syntactic spec.

## What exists in archive/

- **`archive/grammar.js`** — the tree-sitter grammar (~84 rules). Encodes the precedence table from `archive/GLYPH.md §2` exactly. Each rule references its D-decision by number in a comment.
- **`archive/scanner.c`** — external scanner for three context-sensitive tokens the regex lexer can't produce:
  - `NEWLINE` — significant only at bracket depth zero (D1).
  - `JSX_TEXT` — text run inside JSX children, terminated by `<` or `{` (D6).
  - `STRING_CONTENT` — body of a `"..."` literal supporting escapes and embedded newlines (D12).
  - State carried across calls: a 4-byte `bracket_depth` counter.

## What does not exist on disk

The README inside the archive (`archive/README.md`) and the consolidated step-3 deliverable (`archive/GLYPH_STEP3.md`) describe a layout that was never fully materialized:

- No `src/` directory — the scanner lives at the archive root, not at `src/scanner.c`.
- No `examples/` directory — the four hard-case `.glyph` files referenced everywhere live only inline in `archive/GLYPH.md` and `archive/SESSION_1.md`.
- No `queries/highlights.scm` — referenced for editor support, never written.
- No `PRECEDENCE.md` — referenced as normative throughout. The precedence levels live inline at the top of `archive/grammar.js` (the `PREC` constant) and as `archive/GLYPH.md §2`.
- No `package.json` or `binding.gyp` — would be needed to actually run `tree-sitter generate`. Canonical contents for both are spelled out in `archive/GLYPH_STEP3.md §7`.

## What this means for the next step

The grammar has been *written* but not *verified*. `tree-sitter generate` has never been run in this environment. If the project commits to using the tree-sitter grammar as part of the toolchain, the scaffolding above must be created first and the four example files must parse cleanly. Any parse-table conflicts that surface are real ambiguities in the spec — resolve them in `language/spec.md` before patching `grammar.js`.

**However:** the step-4 plan (`roadmap/04-transpiler.md`) explicitly does *not* consume the tree-sitter grammar at runtime. It specifies a hand-written Pratt parser in Rust. The tree-sitter grammar's role going forward is as the syntactic *spec* the Rust parser must match — a reference artifact and an editor-tooling source, not a build dependency.

That leaves an open question worth surfacing in the brainstorm: is it worth finishing the tree-sitter scaffolding now (for syntax-highlighting in editors during dogfooding), or defer until v1.1 once the Rust transpiler is real? Tracked in `open-questions.md`.
