# Glyph for VS Code

Syntax highlighting and the Glyph language server for `.glyph` files.

[Glyph](https://glyphlang.io) is a statically typed language that compiles to
TypeScript, designed so AI agents can read, write, and modify code safely.

## Setup

The extension needs the `glyph` binary, which carries the language server:

```sh
npm install -g @glyphlang/glyph
```

Open any `.glyph` file and it starts. If the binary is not on your `PATH`, point
the extension at it:

```json
{ "glyph.serverPath": "/absolute/path/to/glyph" }
```

## What you get

**Highlighting.** The TextMate grammar is generated from the compiler's own
keyword table rather than maintained by hand, so a keyword the compiler knows is
a keyword the editor colours. It covers declarations, control flow, the
primitive types including `int` and `bigint`, operators, `${...}` interpolation,
annotations, member access, and it scopes a variant constructor differently from
a type, so `Some(x)` in a match arm does not read as an annotation.

**Language server.** The extension runs `glyph lsp` and gives you diagnostics
with their `E0xxx` codes, hover types, go-to-definition following imports across
modules, workspace-wide find-references and rename, completion, document and
workspace symbols, inlay hints, code actions, and formatting in the canonical
`glyph fmt` layout.

## What it does not do yet

The server runs Glyph's own analysis and does not run `tsc`. Errors that only
TypeScript catches therefore do not appear while you type; they arrive at
`glyph build`. The smallest example is `let x: string = 42`, which the editor
reports only as an unused variable. This is tracked as G149 and is scheduled.

Member completion after `.` is not implemented.

## Settings

| Setting | Default | What it does |
|---|---|---|
| `glyph.serverPath` | `glyph` | Path to the `glyph` binary the extension runs as `glyph lsp`. |

## Links

[Website](https://glyphlang.io) ·
[Playground](https://glyphlang.io/playground/) ·
[Source](https://github.com/chadetov/glyph) ·
[Issues](https://github.com/chadetov/glyph/issues)

## License

MIT or Apache-2.0, at your option.
