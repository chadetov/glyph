# Editor setup

Glyph ships a language server built into the compiler: `glyph lsp`. It speaks
the Language Server Protocol over stdio, so any LSP-capable editor can use it.
VS Code has a ready-made extension in this repository; other editors point their
generic LSP client at the same `glyph lsp` command.

What the server provides:

- **Syntax highlighting** (VS Code, via the bundled TextMate grammar)
- **Diagnostics** — parse, resolve, and typecheck errors with their `E0xxx`
  codes (run `glyph --explain <code>` for the long-form fix)
- **Hover types**
- **Go-to-definition**
- **Completion** — keywords, in-scope declarations, prelude names
- **Format-on-save** — the one canonical `glyph fmt` layout

## Prerequisites

You need the `glyph` binary on your `PATH` (the extension runs it as
`glyph lsp`). Install it from npm:

```sh
npm install -g @glyphlang/glyph
# verify the server subcommand exists:
glyph lsp --help
```

Or build it from the repository:

```sh
cd glyph-compiler
cargo build --release
# put target/release on your PATH, or point glyph.serverPath at the binary
```

`tsx` and `typescript` are only needed for `glyph run` / `glyph build --check`,
not for the editor features above.

## VS Code

The extension is not on the Marketplace yet. Run it from source, or package a
`.vsix` and install that.

### Option A — run from source (fastest)

```sh
cd editors/vscode
npm install        # fetches vscode-languageclient
code .             # open this folder in VS Code
```

Press **F5**. VS Code opens an Extension Development Host window with the
extension loaded. Open any `.glyph` file there and the language server starts
automatically.

The extension is plain CommonJS (`extension.js`), so there is no compile step.

### Option B — install a packaged extension

To install it into your normal VS Code (not just the dev host), package a
`.vsix`:

```sh
cd editors/vscode
npm install
npx @vscode/vsce package          # produces glyph-vscode-0.1.0.vsix
code --install-extension glyph-vscode-0.1.0.vsix
```

Reload VS Code and open a `.glyph` file.

### Point the extension at your binary

If `glyph` is not on your `PATH`, set the path in VS Code settings
(`settings.json`):

```json
{
  "glyph.serverPath": "/absolute/path/to/glyph"
}
```

This is the only setting the extension contributes. The default is `glyph`
(resolved from `PATH`).

### Turn on format-on-save

The server is a document formatter, so VS Code can run `glyph fmt` on every
save. Add to `settings.json`:

```json
{
  "[glyph]": {
    "editor.defaultFormatter": "glyph.glyph-vscode",
    "editor.formatOnSave": true
  }
}
```

### Verify it works

Open a `.glyph` file and check:

1. Keywords, types, and strings are colored.
2. Introduce an error (e.g. a `match` missing a variant) — a red squiggle with
   an `E02xx` code appears.
3. Hover a value — its type shows.
4. Save a deliberately misformatted file — it snaps to the canonical layout.

If none of that happens, see Troubleshooting below.

## Other editors (any LSP client)

The server is editor-agnostic. Configure your editor's LSP client with:

- **Command:** `glyph lsp`
- **Transport:** stdio
- **Language / file type:** `glyph` for `*.glyph` files

Syntax highlighting outside VS Code is your editor's responsibility; the server
supplies diagnostics, hover, go-to-definition, completion, and formatting
regardless.

### Neovim (built-in LSP) example

```lua
vim.filetype.add({ extension = { glyph = "glyph" } })

vim.api.nvim_create_autocmd("FileType", {
  pattern = "glyph",
  callback = function(args)
    vim.lsp.start({
      name = "glyph",
      cmd = { "glyph", "lsp" },
      root_dir = vim.fs.dirname(vim.fs.find({ ".git" }, { upward = true })[1]),
    })
  end,
})
```

## Troubleshooting

- **"command 'glyph' not found" / server never starts.** The binary is not on
  `PATH`. Set `glyph.serverPath` (VS Code) or use an absolute `cmd` (other
  editors). Confirm with `glyph lsp --help` in the same shell the editor
  launched from.
- **No syntax highlighting in VS Code.** The file's language must be detected as
  Glyph. Check the language indicator in the status bar; `.glyph` should map to
  "Glyph" automatically. If you opened a buffer with another extension, switch
  it manually.
- **Diagnostics look stale.** The server analyzes the file you have open. Save
  the file or re-open it; restart the language server (`Developer: Restart
  Extension Host` in VS Code) if it appears stuck.
- **Format-on-save does nothing.** Make sure `editor.defaultFormatter` is set to
  the Glyph extension for the `[glyph]` language scope, as shown above.

## Not yet

Rename, find-references (both v1.1), and member completion after `.` are not
implemented. A Marketplace listing for the VS Code extension is planned but not
published; until then use Option A or B above.

## See also

- [`getting-started.md`](getting-started.md) — install Glyph and run your first
  program
- [`../error-codes.md`](../error-codes.md) — what a diagnostic code means and how
  to fix it
- [`editors/vscode/README.md`](https://github.com/chadetov/glyph/blob/main/editors/vscode/README.md)
  — the extension's own readme
