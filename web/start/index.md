<!-- Markdown copy of https://glyphlang.io/start/, generated from the same source as the page. Last changed 2026-11-19. -->

Start here · ~10 minutes

# Your first Glyph program

Install it, scaffold a project, run it, then **break it on purpose** and read the error the compiler gives you. No prior Glyph knowledge assumed. If you've written a little TypeScript, you'll follow every line.

## 1 · Install

Glyph ships as a single prebuilt binary. You also need Node with `tsx` and `typescript`, which Glyph uses to run and type-check.

terminal

```
npm install -g @glyphlang/glyph
npm install -g tsx typescript   # run & type-check toolchain
glyph doctor                    # confirms your toolchain is ready
```

## 2 · Scaffold a project

`glyph init` creates a runnable starter: `src/main.glyph`, `src/.types/README.md` explaining that a typed npm package needs nothing there and the directory is for one that ships no types, a `package.json`, a `.gitignore`, and two files coding agents read on their own: `AGENTS.md`, which points at `glyph llms` for the full language reference, and `.mcp.json`, which registers `glyph mcp` so an agent can ask the compiler for types and references instead of searching for them. In a project you already have, `glyph agents` writes those two. The `package.json` pins the compiler, `typescript` and `tsx` in `devDependencies`, so anyone who clones the project can build it with `npm install` and no global install.

terminal

```
glyph init hello
cd hello
```

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
glyph run
hello from glyph
```

`glyph run` type-checked it, compiled it to TypeScript, and executed it. That's the whole loop.

## 4 · Add something real

Now replace the body with a tiny bit of logic: a task status and a function that labels it. This introduces the two things you'll use constantly: a *tagged union* (`type Status = ...`) and `match`, which is Glyph's only conditional.

src/main.glyph

```
module main

type Status = Todo | Doing | Done

fn label(s: Status) -> string {
  return match s {
    Todo => "not started",
    Doing => "in progress",
    Done => "finished",
  }
}

fn main(argv: Array<string>) -> number {
  print(label(Done))
  return 0
}
```

terminal

```
glyph run
finished
```

## 5 · Break it on purpose

Here's the part that shows you what Glyph is *for*. Delete the `Done` arm from the `match` (the kind of thing an agent does when it adds a case somewhere and forgets to handle it), and run again.

src/main.glyph (with the Done arm deleted)

```
fn label(s: Status) -> string {
  return match s {
    Todo => "not started",
    Doing => "in progress",
  }
}
```

$ glyph run

```
[E0200] Error: typecheck: non-exhaustive match on `Status`: missing variants `Done`
   ╭─[main:6:10]
   │
 6 │ ╭─▶   return match s {
   ┆ ┆
 9 │ ├─▶   }
   │ ╰───────── missing variants `Done`
   │
   │  Help: Add an arm for each missing variant.
───╯
```

It didn't run. In TypeScript this exact function, with its return type written out, is refused too (TS2366). A `switch` that only logs or updates state, or one with a `default:`, compiles clean and quietly skips the new case. Here every non-exhaustive match is a compile error with a stable code (`E0200`), the exact spot, and a one-line fix. Run `glyph --explain E0200` any time you want the long version.

## 6 · Fix it

Put the arm back (or add an `else`), and it runs again.

terminal

```
glyph run
finished
```

That's the core loop: write, run, and when something's wrong the compiler tells you exactly what and how, before your code ships, not after.

## From here

- [Next · no install The playground Try Glyph in your browser, no install. Open the playground](https://glyphlang.io/playground/)
- [The five-minute tour The whole language, quickly.](https://github.com/chadetov/glyph/blob/main/docs/guide/tour.md)
- [The answers Worked answers about React, typed APIs, npm and tooled TypeScript.](https://glyphlang.io/answers/)
