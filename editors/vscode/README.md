# Glyph for VS Code

Syntax highlighting plus the Glyph language server: live diagnostics, hover
types, go-to-definition, completion, and format-on-save.

## What you get

- **Highlighting** — a TextMate grammar for `.glyph` files (keywords, types,
  strings with `${…}` interpolation, comments, annotations).
- **Language server** — the extension launches `glyph lsp` (the server built
  into the `glyph` binary) and wires it over stdio. It provides:
  - diagnostics (parse / resolve / typecheck errors, with their `E0xxx` codes),
  - hover types,
  - go-to-definition (within a file; cross-module lands with workspace support),
  - completion (keywords, in-scope declarations, prelude names),
  - document formatting (the canonical `glyph fmt` layout).

## Prerequisites

1. **Build the `glyph` binary** and put it on your `PATH` (or set
   `glyph.serverPath`):
   ```sh
   cd glyph-compiler && cargo build --release
   # then add target/release to PATH, or:
   #   "glyph.serverPath": "/absolute/path/to/glyph-compiler/target/release/glyph"
   ```
2. **`tsx` + `typescript`** on `PATH` for `glyph run` / `--check` (not required
   for the editor features above).

## Run it (no packaging needed)

```sh
cd editors/vscode
npm install            # fetches vscode-languageclient
code .                 # open this folder in VS Code
# press F5 — an Extension Development Host opens; open any .glyph file.
```

The server is plain CommonJS (`extension.js`), so there is no compile step.

## Settings

- `glyph.serverPath` (default `glyph`) — path to the `glyph` binary the
  extension runs as `glyph lsp`.

## Not yet

Rename and find-references (v1.1), member completion after `.`, and the
cross-module / workspace features (a single open file is analyzed today).
