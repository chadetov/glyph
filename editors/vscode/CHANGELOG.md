# Changelog

## 0.1.1

Fixed: the extension could not start the language server. The client was
configured with an explicit stdio transport, which makes `vscode-languageclient`
append a `--stdio` argument, and `glyph lsp` rejected unknown arguments and
exited 2. VS Code retried five times and gave up with "The Glyph Language Server
server crashed 5 times in the last 3 minutes."

Stdio is already the default for a `command` server, so the argument is simply
not sent any more and the extension works against the compiler you already have.
Compilers from 0.1.96 also accept and ignore `--stdio`, which is what other LSP
clients send.

## 0.1.0

First release.

- Syntax highlighting for `.glyph` files. The grammar is generated from the
  compiler's own keyword table, so a keyword the compiler knows is a keyword the
  editor colours. Covers declarations, control flow, the primitive types
  including `int` and `bigint`, operators, string interpolation, annotations,
  member access, and a distinction between a variant constructor and a type.
- The language server. The extension launches `glyph lsp` from the `glyph`
  binary and speaks LSP over stdio: diagnostics, hover types, go-to-definition
  across modules, find-references, rename, completion, document and workspace
  symbols, inlay hints, code actions, and formatting.

Known limitation: the server runs Glyph's own analysis stages and does not run
`tsc`, so the errors only TypeScript catches do not appear until `glyph build`.
`let x: string = 42` is the smallest example. Tracked as G149.
