<!-- Markdown copy of https://glyphlang.io/start/, generated from the same source as the page. Last changed 2026-11-20. -->

Start here · ~10 minutes

# Your first Glyph program

Scaffold a project, run it, then **break it on purpose** and read the error the compiler gives you. No prior Glyph knowledge assumed. If you've written a little TypeScript, you'll follow every line.

## 1 · Create a project

Glyph ships on npm as a prebuilt binary for macOS and Linux (x64 and ARM) and Windows (x64), and needs Node 18 or later. This path installs nothing globally: `npx` fetches the compiler to scaffold the project, and the project pins its own copy along with `tsx` and `typescript`, which Glyph uses to run and type-check.

terminal

```
npx -y @glyphlang/glyph init hello
cd hello
npm install
```

If you'd rather have `glyph` on your PATH, install it globally with the two tools it runs through, and drop the `npx` from the commands on this page:

terminal, the global alternative

```
npm install -g @glyphlang/glyph tsx typescript
glyph doctor                    # confirms your toolchain is ready
glyph init hello
cd hello
```

## 2 · Look at what it made

`glyph init` creates a runnable starter: `src/main.glyph`, `src/.types/README.md` explaining that a typed npm package needs nothing there and the directory is for one that ships no types, a `package.json`, a `.gitignore`, and two files coding agents read on their own: `AGENTS.md`, which points at `glyph llms` for the full language reference, and `.mcp.json`, which registers `glyph mcp` so an agent can ask the compiler for types and references instead of searching for them. In a project you already have, `glyph agents` writes those two. The `package.json` pins the compiler, `typescript` and `tsx` in `devDependencies`, so anyone who clones the project can build it with `npm install` and no global install.

Open `src/main.glyph`. It's a hello-world:

src/main.glyph

```
module main

import std/io

fn main(argv: Array<string>) -> number {
  io.println("hello from glyph")
  return 0
}
```

Two things to notice: a program runs its `main(argv)`, and `main` returns a `number`, the process exit code. Returning sets that code without forcing the process to stop, so a program that starts a server keeps serving.

## 3 · Run it

terminal

```
npx glyph run
hello from glyph
```

`glyph run` type-checked it, compiled it to TypeScript, and executed it. That is the loop you repeat for the rest of this page.

## 4 · Add something real

Now replace the body with a decision that matters: whether to ship an order, given where its payment stands. This introduces the two things you'll use constantly: a *tagged union* (`type Payment = ...`) and `match`, which is Glyph's only conditional.

src/main.glyph

```
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

terminal

```
npx glyph run
ship it
```

## 5 · Break it on purpose

Here's the part that shows you what Glyph is *for*. A ticket says "support refunds", so add a `Refunded` case to `Payment` and leave `ship` alone. That is what an agent does when it changes a type in one file and never opens the code that decides on it. Run again.

src/main.glyph (one word added)

```
type Payment = Pending | Paid | Failed | Refunded
```

$ npx glyph run

```
[E0200] Error: typecheck: non-exhaustive match on `Payment`: missing variants `Refunded`
    ╭─[ main:6:10 ]
    │
  6 │ ╭─▶   return match p {
    ┆ ┆
 10 │ ├─▶   }
    │ ╰───────── non-exhaustive match on `Payment`: missing variants `Refunded`
    │
    │     Help: Add an arm for each missing variant.
────╯
```

It didn't run. The TypeScript most code is written with goes the other way: a `switch` that handles the two "hold" cases and ends in `default: return "ship it"` passes `tsc --strict` after the new case is added, and ships every refunded order. Here every match that misses a case is a compile error with a stable code (`E0200`), the exact spot, and the name of the case. Run `glyph --explain E0200` any time you want the long version.

## 6 · Fix it

The compiler can't decide what a refund means for shipping; that is your call. Add the arm, and it runs again.

src/main.glyph

```
fn ship(p: Payment) -> string {
  return match p {
    Pending => "hold: waiting for the money",
    Paid => "ship it",
    Failed => "hold: payment failed",
    Refunded => "hold: refunded, nothing to ship",
  }
}
```

terminal (with `print(ship(Refunded))`)

```
npx glyph run
hold: refunded, nothing to ship
```

That's the core loop: write, run, and when something's wrong the compiler tells you exactly what and how, before your code ships, not after.

## From here

- [Next · no install The playground Try Glyph in your browser, no install. Open the playground](https://glyphlang.io/playground/)
- [The five-minute tour The whole language, quickly.](https://github.com/chadetov/glyph/blob/main/docs/guide/tour.md)
- [The answers Worked answers about React, typed APIs, npm and tooled TypeScript.](https://glyphlang.io/answers/)
