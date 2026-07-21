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
glyph run src/main.glyph
# hello from glyph
```

`glyph run` type-checked the program, compiled it to TypeScript, and executed
its `main(argv)`. `main` returns a `number` — the process exit code.

## 3. Add something real

Replace `src/main.glyph` with a tagged union and a `match` (Glyph's only
conditional):

```glyph
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

```sh
glyph run src/main.glyph
# finished
```

## 4. Break it, and read the error

Delete the `Done =>` arm — the mistake an agent makes when it adds a case and
forgets to handle it — and run again:

```
[E0200] non-exhaustive match on `Status`: missing variants `Done`
  Help: Add an arm for each missing variant, or an `else` arm to catch the rest.
```

It doesn't run. The equivalent TypeScript `switch` compiles clean and returns
`undefined`; here the missing case is a compile error with a stable code, the
exact location, and a fix. `glyph --explain E0200` prints the long form.

Put the arm back (or add an `else`) and it runs again. That's the loop: the
compiler tells you what's wrong and how, before the code ships.

## Next

- The five-minute [tour](tour.md) — the whole language, quickly.
- [For TypeScript developers](for-typescript-developers.md) — the deltas.
- The [answers](https://glyphlang.io/answers/) — React, typed APIs, why not tooled TS.
