# Getting started

## Install

Glyph is distributed on npm the way esbuild and swc are — a small launcher plus
a prebuilt binary for your platform:

```sh
npm install -g glyph
# or run without installing:
npx glyph --help
```

You also need Node with `tsx` and `typescript` available, which Glyph uses to
type-check and run the TypeScript it emits:

```sh
npm install -g tsx typescript
```

### Building from source

If you are working from the repository (or your platform has no prebuilt
binary):

```sh
cd glyph-compiler
cargo build --release
# the binary is target/release/glyph
```

Put `target/release` on your `PATH`, or invoke it directly.

## Your first program

Create `hello.glyph`:

```glyph
module hello

import std/io

fn main(argv: Array<string>) -> number {
  io.println("hello from glyph")
  return 0
}
```

Run it:

```sh
glyph run hello.glyph
```

`glyph run` type-checks the program, compiles it to TypeScript, and runs
`main(argv)` via `tsx`. Arguments after the file are passed through as `argv`:

```sh
glyph run hello.glyph one two three
```

`main` returns a `number` — the process exit code.

## The commands

| Command | What it does |
|---|---|
| `glyph run <file> [args]` | Type-check, compile, and run a program |
| `glyph build <src> --out <dir>` | Compile a source tree to TypeScript, type-checked with `tsc --strict` |
| `glyph build <src> --out <dir> --test` | Also run every `@example` and `@doc @run` test |
| `glyph fmt [path]` | Format files in place (the one canonical layout) |
| `glyph canonical <file>` | Print the agent canonical view (stable line numbers + per-declaration fingerprints) |
| `glyph publish [dir]` | Audit-gate, build, and type-check a package for `npm publish` |
| `glyph lsp` | Run the language server (an editor extension spawns this) |
| `glyph --explain <code>` | Long-form explanation and fix for an error code |

`glyph build` type-checks by default; pass `--no-check` to skip the `tsc` pass.

## Editor support

The repository ships a VS Code extension (`editors/vscode/`) that launches
`glyph lsp` and gives you live diagnostics, hover types, go-to-definition,
completion, and format-on-save:

```sh
cd editors/vscode
npm install
code .          # then press F5 to open an Extension Development Host
```

Point `glyph.serverPath` at your `glyph` binary if it is not on `PATH`.

## Tests live next to the code

Glyph runs example tests on build. Add an `@example` above a function and it is
checked every time you run `glyph build --test`:

```glyph
@example double(21) == 42
fn double(n: number) -> number {
  return n * 2
}
```

## Next

- A guided build of something real: [`tutorial.md`](tutorial.md).
- Coming from TypeScript: [`for-typescript-developers.md`](for-typescript-developers.md).
- The language reference: [`../language/spec.md`](../language/spec.md).
- Error codes and fixes: [`../error-codes.md`](../error-codes.md).
