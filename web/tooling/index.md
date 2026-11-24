<!-- Markdown copy of https://glyphlang.io/tooling/, generated from the same source as the page. Last changed 2026-11-24. -->

Setup · Editors, agents and the CLI

# Editors and integrations

Everything here runs off one binary. The `glyph` command from npm is the compiler, the language server (`glyph lsp`) and the MCP server (`glyph mcp`), so once it is installed, wiring up an editor or an agent is a line of config.

## VS Code

The extension is on the Visual Studio Marketplace as [Glyph (glyphlang.io)](https://marketplace.visualstudio.com/items?itemName=chadetov.glyph-vscode). Install it from the Extensions view, or paste this into the Quick Open box (Ctrl+P, or Cmd+P on a Mac):

quick open

```
ext install chadetov.glyph-vscode
```

The extension is a client for the language server, and the server lives in the `glyph` binary, so you need that too:

terminal

```
npm install -g @glyphlang/glyph
glyph lsp --help                # the server subcommand is there
```

Open a `.glyph` file and the server starts. If `glyph` is not on the `PATH` VS Code sees (a project-local install in `node_modules`, or a build from source), point the extension at it. This is the only setting it has.

settings.json

```
{
  "glyph.serverPath": "/absolute/path/to/glyph",
  "[glyph]": {
    "editor.defaultFormatter": "chadetov.glyph-vscode",
    "editor.formatOnSave": true
  }
}
```

The `[glyph]` block is optional and turns on format-on-save in the canonical `glyph fmt` layout.

What you get: highlighting from a grammar generated off the compiler's own keyword table, diagnostics with their `E0xxx` codes as you type, hover types, go-to-definition across modules, workspace-wide find-references and rename, completion, document and workspace symbols, inlay hints, code actions and formatting.

The server runs Glyph's own analysis and does not run `tsc`, so an error only TypeScript catches shows up at `glyph build` or `glyph check`, not while you type. Member completion after `.` is not implemented yet.

Cursor, VSCodium and other VS Code forks install extensions from Open VSX, and the extension is not published there yet. You can build the `.vsix` yourself from [`editors/vscode`](https://github.com/chadetov/glyph/tree/main/editors/vscode) and install it with the editor's "Install from VSIX" command, or skip the extension and use the generic LSP setup below.

terminal, in a clone of the repository

```
cd editors/vscode
npm ci
npx @vscode/vsce package --out glyph-vscode.vsix
```

## Any editor with an LSP client

Nothing in the server is specific to VS Code. Give your editor's LSP client three things: the command `glyph lsp`, the stdio transport, and the `glyph` language for `*.glyph` files. The server answers the same way in every editor. Outside VS Code it doesn't color anything, so highlighting depends on your editor having a grammar for the language, and none of the editors below ships one yet.

The snippets below follow each editor's documented configuration format. We have not run each one in its editor, so if one doesn't work for you, please [open an issue](https://github.com/chadetov/glyph/issues).

## Neovim

nvim-lspconfig has no entry for Glyph, so use the built-in client. This works on any Neovim with `vim.lsp.start` (0.8 and later):

init.lua

```
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

On Neovim 0.11 and later, `vim.lsp.config` says the same thing more briefly:

init.lua, Neovim 0.11+

```
vim.filetype.add({ extension = { glyph = "glyph" } })

vim.lsp.config("glyph", {
  cmd = { "glyph", "lsp" },
  filetypes = { "glyph" },
  root_markers = { "package.json", ".git" },
})
vim.lsp.enable("glyph")
```

## Helix

Helix doesn't know the language, so declare it along with its server in `languages.toml` (in your config directory, or `.helix/` in the project):

languages.toml

```
[language-server.glyph]
command = "glyph"
args = ["lsp"]

[[language]]
name = "glyph"
scope = "source.glyph"
file-types = ["glyph"]
roots = ["package.json"]
comment-token = "//"
language-servers = ["glyph"]
```

`hx --health glyph` tells you whether Helix found the server.

## Emacs

With eglot (built into Emacs 29), define a mode for `.glyph` files and register the server for it:

init.el

```
(define-derived-mode glyph-mode prog-mode "Glyph")
(add-to-list 'auto-mode-alist '("\\.glyph\\'" . glyph-mode))

(with-eval-after-load 'eglot
  (add-to-list 'eglot-server-programs '(glyph-mode "glyph" "lsp")))

(add-hook 'glyph-mode-hook #'eglot-ensure)
```

## Agents over MCP

`glyph mcp` runs a Model Context Protocol server over stdio. It gives an agent the compiler's answers about your project: what a symbol is, what a change would break, diagnostics, the type at a position, references. A project made by `glyph init` already has a `.mcp.json` registering it, and `glyph agents` writes the same file (and an `AGENTS.md`) into a project you already have:

.mcp.json

```
{
  "mcpServers": {
    "glyph": {
      "command": "glyph",
      "args": ["mcp"]
    }
  }
}
```

Claude Code picks up a project's `.mcp.json` and asks you to approve it. To add the server yourself instead:

terminal

```
claude mcp add glyph -- glyph mcp
```

Other MCP clients take the same command and arguments in their own config. `glyph mcp` queries the current directory unless you pass a project root as its argument. If your agent has no MCP client, `glyph query` asks the same questions from the command line and prints the same JSON. The [MCP page](https://glyphlang.io/mcp/) lists all eleven tools with examples.

## The npm package and the CLI

The compiler ships on npm as `@glyphlang/glyph`, a prebuilt binary for macOS, Linux and Windows that needs Node 18 or later. You can install it globally, or let each project pin its own copy: `glyph init` puts the compiler, `typescript` and `tsx` in the project's `devDependencies`, and inside the project `npx glyph ...` runs that pinned copy.

terminal

```
npx -y @glyphlang/glyph init hello   # scaffold a project, nothing global
cd hello
npm install
npx glyph run                        # type-check, compile and run
npx glyph check src                  # type-check only, writes nothing
```

An editor or MCP client that starts `glyph` needs to find it. If you only have the project's copy, give the client `./node_modules/.bin/glyph` or an absolute path to it. [Start here](https://glyphlang.io/start/) walks through a first program in about ten minutes.

## Not yet

There is no Vite plugin yet. One with React Fast Refresh is on the roadmap. Until then a Vite app imports what `glyph build --watch` emits, and a change to a Glyph file reloads the whole page rather than hot-swapping the component. [Using Glyph inside a TypeScript app](https://glyphlang.io/answers/embedding/) has the setup.

GitHub does not highlight `.glyph` files, because Glyph is not a GitHub Linguist language. Linguist already has an unrelated language named Glyph (Tcl-based, extension `.glf`), so this one cannot be submitted under that name. The grammar is mirrored at [github.com/chadetov/glyph-tmlanguage](https://github.com/chadetov/glyph-tmlanguage) for a future submission.

The VS Code extension is not on Open VSX, which is where Cursor and VSCodium look for extensions.

Zed only adds a language through an extension, and there is no Glyph extension for Zed yet, so Zed can't use `glyph lsp` today.

## From here

- [Next · ten minutes Your first program Scaffold a project, run it, break it on purpose. Start here](https://glyphlang.io/start/)
- [The MCP server All eleven tools and what each one answers.](https://glyphlang.io/mcp/)
- [The editor and error experience What the language server does, error codes, and debugging.](https://glyphlang.io/answers/tooling/)
