# Getting started

## Install

Glyph is distributed on npm the way esbuild and swc are — a small launcher plus
a prebuilt binary for your platform:

```sh
npm install -g @glyphlang/glyph
# or run without installing:
npx @glyphlang/glyph --help
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

`main` returns a `number`, the process exit code.

Returning from `main` sets that code but does not force the process to stop. It
stops when there is nothing left to wait for, which is Node's own rule. A
program that only computes and prints exits the moment `main` returns; a program
that started a server or scheduled a timer keeps running until that work is
done. See [long-running programs](#long-running-programs) below.

`glyph run` reports every diagnostic `glyph build` would report on the same
directory, warnings included, and prints them after the program's output with a
one-line count. A sibling module that failed to compile does not stop the run
(it is simply not importable), but its errors are still printed, so `run` and
`build` never disagree about what is wrong with your tree.

## The commands

| Command | What it does |
|---|---|
| `glyph init [dir]` | Scaffold a runnable starter project (`src/main.glyph`, `.types/`, `package.json`, `.gitignore`) |
| `glyph check [path]` | Type-check a file or a tree without running it or writing output |
| `glyph run <path> [args]` | Type-check, compile, and run a program. `<path>` is a `.glyph` file, or a directory whose `main.glyph` is the program, the same spelling `glyph build` takes |
| `glyph build <src> --out <dir>` | Compile a source tree to TypeScript, type-checked with `tsc --strict`, running every `@example` and `@doc @run` test |
| `glyph build <src> --out <dir> --no-test` | Skip the `@example` and `@doc @run` tests |
| `glyph fmt [path]` | Format files in place (the one canonical layout) |
| `glyph fmt --check [path]` | Check formatting for CI: writes nothing, exits non-zero if any file is unformatted |
| `glyph canonical <file>` | Print the agent canonical view (stable line numbers + per-declaration fingerprints) |
| `glyph publish [dir]` | Audit-gate, build, and type-check a package for `npm publish` |
| `glyph doctor` | Check the JavaScript toolchain (node/tsx/tsc present + new enough), and report this compiler against the latest published release. `--offline` skips the registry lookup |
| `glyph upgrade [dir]` | Move the project's pinned Glyph version to the latest release and `npm install` it. `--to <version>` for a specific one, `--dry-run` to preview |
| `glyph lsp` | Run the language server (an editor extension spawns this) |
| `glyph llms` | Print the agent bootstrap (the `AGENTS.md` reference) offline; alias `glyph docs` |
| `glyph --explain <code>` | Long-form explanation and fix for an error code |

The scaffolded `package.json` pins `typescript` and `tsx` in `devDependencies`,
so after `glyph init` you can run `npm install` in the project to get a
consistent toolchain locally (instead of, or in addition to, the global install
above) — everyone on the team then builds against the same TypeScript.

It pins Glyph itself the same way, and **exactly**, with no `^`. Before 1.0 a
0.1.x release may add a diagnostic that rejects code which compiled yesterday,
which is usually the point of the release; a floating range would let that reach
you through an `npm install` you ran for some unrelated reason. Commit the
`package-lock.json` alongside it and a clone builds with the toolchain you
tested. When you want a newer compiler, ask for one:

```sh
glyph doctor            # is there a newer release?
glyph upgrade --dry-run # what would change
glyph upgrade           # move the pin and install it
glyph build src --out dist
```

`glyph upgrade` rewrites one line of `package.json` and runs `npm install`. It
prints the release-notes link, and building after it is on you: a new release is
allowed to report diagnostics the project did not have before.

`glyph build` type-checks and runs your `@example`s by default; `--no-tsc`
skips the `tsc` pass and `--no-test` skips the examples. `--no-tsc` is the same
flag on all three of `build`, `check`, and `run` (`--no-check` is its old
spelling on `build` and `run`, still accepted). The example runner needs `tsx`
on `PATH`; if it is missing on a project that has examples, the build fails
rather than reporting a pass it never checked.

## Editor support

The repository ships a VS Code extension (`editors/vscode/`) that launches
`glyph lsp` and gives you live diagnostics, hover types, go-to-definition,
completion, and format-on-save:

```sh
cd editors/vscode
npm install
code .          # then press F5 to open an Extension Development Host
```

Point `glyph.serverPath` at your `glyph` binary if it is not on `PATH`. For
packaging a `.vsix`, format-on-save, other editors, and troubleshooting, see the
[editor setup guide](editor-setup.md).

## Tests live next to the code

Glyph runs example tests on build. Add an `@example` above a function and it is
checked every time you run `glyph build`:

```glyph
@example double(21) == 42
fn double(n: number) -> number {
  return n * 2
}
```

## Long-running programs

A server, a watcher, a bot: anything driven by events rather than by one pass
through `main`. These work the way they do in Node. `main` sets things up and
returns, and the process stays alive as long as something is holding it open.

```glyph
module main

import std/io
import net { createServer }

fn main(argv: Array<string>) -> number {
  let server = createServer(fn(socket) {
    mut socket.write("hello\n")
    mut socket.end()
  })
  mut server.listen(4000, fn() {
    io.println("listening on 4000")
  })
  return 0
}
```

`net` is a Node builtin and imports like any other. The compiler ships ambient
types for it, so nothing is installed and no declaration file is written. See
[`external-imports.md`](external-imports.md) for the full list and for what to
do about a package that ships no types.

Two things to know about the shape.

The `return 0` runs immediately, long before the first client connects. It is
the exit code the process will eventually leave with, not a statement about when
it leaves.

Anything that fails after `main` has returned has to set its own exit code,
because `main`'s return value is already spent. A listener that cannot bind its
port is the common case, and without this the process drains its event loop and
exits 0, reporting success for a server that never started:

```glyph
mut server.on("error", fn(err) {
  io.eprintln("cannot listen on 4000: ${err.message}")
  process.exit(1)
})
```

A program that schedules rather than listens uses `std/timers`
(`timers.every`, `timers.after`, `timers.cancel`), and one that talks to a
WebSocket uses `std/websocket`. Both keep the process alive the same way a
listening socket does.

Two worked examples: `examples/apps/chat` is a chat server holding several TCP
clients at once, and `examples/apps/discord` is a gateway client with a
heartbeat, resume, and reconnect backoff. Both keep the parts that do not touch
the network separate and tested with `@example`, and neither contains a line of
TypeScript.

## Next

- A guided build of something real: [`tutorial.md`](tutorial.md).
- Coming from TypeScript: [`for-typescript-developers.md`](for-typescript-developers.md).
- The language reference: [`../language/spec.md`](../language/spec.md).
- Error codes and fixes: [`../error-codes.md`](../error-codes.md).
