<!-- Markdown copy of https://glyphlang.io/answers/tooling/, generated from the same source as the page. Last changed 2026-11-21. -->

25 · Day-to-day

# What's the editor, debugging, and error experience?

`glyph lsp` speaks the Language Server Protocol over stdio, so any editor with an LSP client gets diagnostics, hover types, go-to-definition, completion and format-on-save. A VS Code extension is in the repository; it is not on the Marketplace yet, so you build it from source. Every error carries a stable code, the fix, and the reason.

glyph build

```
[E0203] the `?` operator propagates error type `HttpError`,
        but this function returns `Result<_, string>`
   ╰─ Help: map the error first, e.g. `.map_err(...)`,
      so its `E` matches the function's.
```

See it: every diagnostic carries a stable code, a one-line fix, and often the design reason. `glyph --explain` prints the long form for any of them.

## Why it matters

Editing a language your editor doesn't understand feels like a step backward, and early on that was a fair complaint. It isn't anymore: your editor gives you the modern niceties, and the errors are written to teach, not just to reject.

## Is it a real LSP, or just a VS Code plugin?

A real one. The compiler ships a language server you launch as `glyph lsp`, which speaks the Language Server Protocol over stdio (the standard JSON-RPC transport every LSP client understands).

The bundled VS Code extension is a thin client around it, but nothing is VS-Code-specific: point **any** LSP-capable editor, Neovim, Helix, Zed, Emacs (eglot), at `glyph lsp` and it behaves the same.

It's the same binary you already installed (`npx @glyphlang/glyph lsp`), so there's no separate server to fetch.

### What the server implements

The server implements what an editor needs: live diagnostics with the stable codes below, hover types, go-to-definition (across modules through a named import; through a namespace, `ui.Button` still answers nothing), completion, document and workspace symbols for outline and symbol search, workspace-wide find-references, rename, and formatting in the canonical `glyph fmt` layout for format-on-save.

Rename is complete: renaming a module-level declaration edits its definition, every reference, and each importing module's `import` binding across the workspace, and it checks the new name (a real identifier, not a keyword) before touching anything.

Two limits worth knowing: the cross-file index is rebuilt on each request rather than cached (fine for a normal project, worth speeding up later), and a file that doesn't currently parse is skipped, so a rename won't reach references inside a file that's itself broken.

## Error messages

We tightened the messages exactly where agents and newcomers stumble most: writing `if` now points you at `match` instead of a generic “unexpected token,” and a wrong argument count, a non-exhaustive `match`, or a mistyped variant each get their own coded, fixable error.

Type errors that only the TypeScript back-end can catch used to point at the generated `.ts`. Now they're mapped back onto your `.glyph` (same ariadne caret, same stable code), so a `tsc` error reads like a native Glyph one.

## Debugging

You debug the emitted TypeScript with the tools you already use, like Node's inspector, browser devtools, or your IDE's JS/TS debugger.

Every build ships a standard v3 source map beside each `.ts` (with the Glyph source embedded), so a debugger or a bundler that chains maps steps back into your `.glyph` line numbers.

One thing it doesn't cover yet: `glyph run`'s own crash stack still prints `.ts` lines, because `tsx` doesn't chain the map through its transform. Remapping that stack is next.

## Asking “does this compile?” without running it

`glyph check path.glyph` type-checks one file, or a whole directory, and exits. It writes nothing into your tree, which `glyph run` could not promise and `glyph build` would not do at all (it refused anything that wasn't a directory).

--no-tsc--json--no-test

**Details what it runs, and the flags **

It runs the same pipeline a build runs, including `tsc --strict` over the emitted TypeScript, into a temp directory it deletes on the way out. `--no-tsc` stops after the Glyph stages when you want the fast answer; `--json` gives an agent the same diagnostic shape `glyph build --json` does. A file is checked in the context of its own directory, so a broken sibling still fails the check. Your `@example` and `@doc @run` tests run here too, the same gate `glyph build` uses, so a check cannot report a clean tree that a build would fail; `--no-test` skips them when you want an answer that executes nothing.

## The transcript doesn't lie to you either

The summary now prints after every stage that can turn the build red, so a red build never shows a green line.

**History the summary that came too early **

A failing build used to open with “no diagnostics” and then print `tsc` errors underneath it, because the Glyph-stage summary was written before the TypeScript stage ran.

In the same pass, `glyph run app.glyph --min -12.50` started working: a leading hyphen used to make clap reject the argument before your `main` ever saw it.

Flags glyph owns still bind to glyph, so put `--` in front of a program flag that collides with one of them.

## And for the agent in the loop

`glyph build --json` emits every diagnostic as structured JSON: a stable code, severity, message, file, and 1-based line/column range, plus the help and note, including `tsc` errors mapped onto your Glyph source. An agent reads and acts on them directly instead of scraping terminal text.

### `glyph mcp` for interactive agents

For interactive agents there's more than batch diagnostics: [`glyph mcp`](https://glyphlang.io/mcp/) runs a Model Context Protocol server over stdio (the same binary, the same analysis the editor uses), exposing the language server's queries as MCP tools. An agent can ask for a file's diagnostics, the type at a position, where a name is defined (following imports), every reference to a symbol across the whole project, or a symbol search, without running a full build or reading every file.

It's a thin adapter over the same pure analysis layer, not a second implementation, so it can't drift from what the compiler and editor see.

## Not everything is a hard error

Glyph has a warning tier that flags dead weight without failing the build: an *unused import* (`E0106`), an *unused* `let` (`E0107`, exempting names led by `_`), and *unreachable code* after a `return`/`break`/`continue` (`E0108`).

E0106 E0107 E0108

These keep a module honest: a dead import is exactly the kind of greppability noise that makes a reader hunt for a dependency the code doesn't really have. Warnings surface in the same rendered and `--json` output, marked by severity, and still emit TypeScript.

## Where it stands

### Shipping today

- A language server for any editor, answering across a module boundary since 0.1.124

  **Details**

  A language server (`glyph lsp` over stdio, any editor; VS Code extension bundled) with highlighting, diagnostics, hover, go-to-def, completion, symbols, workspace-wide find-references and rename, and format-on-save, answering across a module boundary since 0.1.124: the server holds one database per project with your open buffers layered over the files on disk, so hovering an imported function or a field of an imported record gives the answer `glyph query hover` gives, an unsaved edit in one file is seen from another, and a keystroke costs one to nine percent more than it did single-file;

- A `glyph mcp` Model Context Protocol server with eleven tools

  **Details**

  a `glyph mcp` Model Context Protocol server with eleven tools, the editor's queries plus `glyph_symbol`, `glyph_impact`, `glyph_assignable`, `glyph_dependencies` and `glyph_exports`, each with a `glyph query` verb printing the same JSON for an agent with no MCP client;

- Stable error codes, `glyph --explain`, `tsc` back-end errors mapped onto `.glyph` source, a warning tier (unused import/binding, unreachable code), and `--json` structured diagnostics.
- `glyph check [path]` type-checks a single file or a tree without running it or writing output.
- `glyph fmt --check` gates formatting in CI: it writes nothing and exits non-zero when a file is not already in the one canonical layout.

### On the way

- Caching the cross-file index instead of rebuilding it per request (a speed win on large trees).
- Remapping `glyph run`'s own crash stack, since `tsx` does not chain the emitted map through its transform, so that stack still names the emitted `.ts` rather than your `.glyph`.

Code examples on this page are checked against Glyph 0.1.163 on every build. Last changed 21 Nov 2026.
