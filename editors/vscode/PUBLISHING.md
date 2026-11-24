# Publishing this extension

The extension is published to the VS Code Marketplace as
`chadetov.glyph-vscode`, display name "Glyph (glyphlang.io)". Users install it
with:

```sh
code --install-extension chadetov.glyph-vscode
```

or `ext install chadetov.glyph-vscode` from the Quick Open box.

## A name collision to know about

Searching the Marketplace for "glyph language" returns
`GlyphLang.glyphlang`, "Glyph Language Support". That is a different project:
`github.com/GlyphLang/GlyphLang`, glyphlang.dev, described as a DSL for building
REST APIs with bytecode compilation and JIT optimization. It is unrelated to this
language and shares no code with it.

That is why the publisher id is `chadetov`, the GitHub owner of this
repository, rather than `glyphlang` (taken by that project) or `glyph` (too
generic to tell anyone which Glyph it is). The display name carries the domain
for the same reason: a search result has to say which Glyph it is without
opening it.

## Before every publish

1. Regenerate the grammar and confirm it is current:

   ```sh
   python3 scripts/gen_grammar.py --check
   ```

   The keyword rules come from `KEYWORDS` in
   `glyph-compiler/crates/glyph-lexer/src/token.rs`, the table the lexer uses,
   and the primitive types from `is_primitive_type_name` in
   `glyph-compiler/crates/glyph-resolver/src/reserved.rs`. The script fails on
   a keyword it has no scope for, so a new keyword cannot ship uncoloured. Never
   edit `syntaxes/glyph.tmLanguage.json` by hand; change the script and rerun
   it.

2. Tokenize real programs. Reading the JSON is not a check: eight keywords,
   `pub` among them, were once unscoped in a grammar that looked fine. Load the
   grammar with `vscode-textmate` and `vscode-oniguruma` in a scratch Node
   project, tokenize every `.glyph` file under `examples/apps/`, and confirm
   that every reserved word and primitive type outside strings and comments
   carries a scope other than `source.glyph`. Look specifically at `pub`,
   `mut`, `where`, `int`, `bigint` (no app uses it yet; the conformance test
   `glyph-compiler/crates/glyph-emit/tests/conformance/int_and_bigint.glyph`
   does), a variant constructor in a match arm such as `Some(x) ->`, and the
   `${` of a string interpolation.

3. If the grammar changed, see whether the copy GitHub's Linguist vendors has
   fallen behind:

   ```sh
   ./scripts/publish_grammar.sh --check
   ```

   Without `--check` the script pushes to `chadetov/glyph-tmlanguage`.

4. Bump `version` in `package.json` and `package-lock.json`, and add a
   `CHANGELOG.md` entry. The Marketplace refuses a version it already has.

## Package and publish

```sh
cd editors/vscode
npm ci
npx @vscode/vsce ls                                   # what will ship
npx @vscode/vsce package --out /tmp/glyph-vscode.vsix
code --install-extension /tmp/glyph-vscode.vsix       # try it before publishing
npx @vscode/vsce publish --packagePath /tmp/glyph-vscode.vsix
```

`vsce publish` needs an Azure DevOps personal access token with the Marketplace
"Manage" scope for the `chadetov` publisher, passed with `-p` or stored once by
`npx @vscode/vsce login chadetov`. Publishing with `--packagePath` sends the
file you tried rather than repackaging.

`.vscodeignore` keeps out this file, the lockfile, and the type declarations,
READMEs and changelogs of the dependencies; their licenses stay in. `vsce`
warns that the extension is not bundled (about 200 files, most of them
`vscode-languageclient`). The extension is plain CommonJS with no build step,
and bundling would add one.

## Open VSX (optional)

VSCodium and other editors that cannot use Microsoft's Marketplace install from
Open VSX. It is a separate account and token, under the same `chadetov`
namespace:

```sh
npx ovsx create-namespace chadetov -p <open-vsx-token>   # once
npx ovsx publish /tmp/glyph-vscode.vsix -p <open-vsx-token>
```
