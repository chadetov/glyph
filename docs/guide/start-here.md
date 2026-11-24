# Start here

Your first Glyph program in about ten minutes: install, scaffold, run, then
break it on purpose and read the compiler error. No prior Glyph knowledge
assumed. (The same walkthrough, nicely formatted, is at
[glyphlang.io/start](https://glyphlang.io/start/).)

## 1. Install

```sh
npm install -g @glyphlang/glyph
npm install -g tsx typescript   # the run + type-check toolchain
glyph doctor                    # confirm the toolchain is ready
```

## 2. Scaffold and run

```sh
glyph init hello
cd hello
glyph run
# hello from glyph
```

Seven files: `src/main.glyph`, a `src/.types/README.md` explaining that a typed
npm package needs nothing there and the directory is for one that ships no
types, a `package.json` pinning the compiler and the `typescript` and
`tsx` it runs through, a `.gitignore`, and a `.vscode/extensions.json` that
makes VS Code (and Cursor, Windsurf and VSCodium) offer the Glyph extension when
you open the folder.

The other two are for coding agents rather than for you. `AGENTS.md` says what
the project is and points at `glyph llms`, which prints the whole language
reference offline. `.mcp.json` registers `glyph mcp`, so an agent can ask the
compiler for diagnostics, types, definitions and references instead of guessing
from a search. Agents read both files without being told to. In a project you
already have, `glyph agents` writes them.

`glyph run` type-checked the program, compiled it to TypeScript, and executed
its `main(argv)`. `main` returns a `number`, the process exit code. Returning
sets the code; the process leaves when nothing is left to wait for, so a program
that started a server keeps serving.

## 3. Add something real

Replace `src/main.glyph` with a decision that matters, whether to ship an
order given where its payment stands. It uses a tagged union and a `match`
(Glyph's only conditional):

```glyph
module main

type Payment = Pending | Paid | Failed

fn ship(p: Payment) -> string {
  return match p {
    Pending => "hold: waiting for the money",
    Paid => "ship it",
    Failed => "hold: payment failed",
  }
}

fn main(argv: Array<string>) -> number {
  print(ship(Paid))
  return 0
}
```

```sh
glyph run
# ship it
```

## 4. Break it, and read the error

A ticket says "support refunds". Add a case to the type and leave `ship`
alone, which is what an agent does when it changes a type in one file and
never opens the code that decides on it:

```glyph
type Payment = Pending | Paid | Failed | Refunded
```

```
[E0200] non-exhaustive match on `Payment`: missing variants `Refunded`
  Help: Add an arm for each missing variant.
```

It doesn't run. A TypeScript `switch` that handles the two "hold" cases and
ends in `default: return "ship it"` passes `tsc --strict` after the new case
is added, and ships every refunded order. Here the missed case is a compile
error with a stable code, the exact location, and the name of the case.
`glyph --explain E0200` prints the long form.

Deciding what a refund means for shipping is your call, not the compiler's.
Add the arm and it runs again:

```glyph
    Refunded => "hold: refunded, nothing to ship",
```

That's the loop: the compiler tells you what's wrong and where, before the
code ships.

## Next

- The five-minute [tour](tour.md): the whole language, quickly.
- [For TypeScript developers](for-typescript-developers.md): the deltas.
- The [answers](https://glyphlang.io/answers/): React, typed APIs, why not tooled TS.
