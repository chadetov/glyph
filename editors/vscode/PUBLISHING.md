# Publishing this extension

**It is not published.** There is no Glyph extension on the VS Code Marketplace,
which is why nobody gets highlighting without building this from source.

## A name collision to know about

Searching the Marketplace for "glyph language" returns
`GlyphLang.glyphlang`, "Glyph Language Support". **That is a different project**:
`github.com/GlyphLang/GlyphLang`, glyphlang.dev, described as a DSL for building
REST APIs with bytecode compilation and JIT optimization. It is unrelated to this
language and shares no code with it.

It matters for two reasons. Anyone searching the Marketplace for Glyph finds
that one first and will assume it is ours. And when this extension is published,
the publisher id and display name have to be distinct enough that the two are
not confused. `glyph.glyph-vscode` is what this manifest currently claims;
confirm the publisher id is available and unambiguous before the first publish.

## Publishing, when that decision is made

```sh
cd editors/vscode
npx @vscode/vsce package          # produces glyph-vscode-<version>.vsix
npx @vscode/vsce publish          # needs a PAT for the publisher account
```

## Before publishing

The grammar is generated from `glyph-compiler/crates/glyph-lexer/src/token.rs`,
which is the single source of truth for keywords. Regenerate rather than editing
the JSON by hand.

Check the result against real programs rather than by reading it. Tokenize a
file from `examples/apps/` with `vscode-textmate` and confirm that `pub`, the
primitive types including `int` and `bigint`, `where`, and a constructor in a
match arm all carry a scope. All four of those were unscoped until the grammar
was regenerated against the keyword table, and reading the JSON did not reveal
it; tokenizing did.
