<!-- Markdown copy of https://glyphlang.io/answers/debugging/, generated from the same source as the page. Last changed 2026-11-21. -->

28 · Operations

# When it breaks in production at 3am, what does on-call see?

Readable TypeScript you own, with a **source map back to your `.glyph`** beside every emitted file. A debugger, a bundler, or an error tracker traces the running code to your source, and even someone who's never seen Glyph can read the generated TS. Before any of that, `glyph build` and `glyph run` report the same diagnostics on the same tree, so the quick loop can't hide something the build would have caught. Two limitations, both on the `glyph run` path, are called out below.

## 1 · The generated TypeScript is yours to read

Glyph doesn't ship an opaque runtime. It emits ordinary, formatted TypeScript that you commit and deploy like any other TS.

So the worst case (an on-call engineer who doesn't know Glyph, at 3am) is still reading plain TypeScript, not a dialect. There's no black box between your source and production.

## 2 · Source maps back to `.glyph`

Every emitted `.ts` ships a standard v3 source map (`.ts.map`) next to it, with a `sourceMappingURL` comment and your Glyph source embedded (`sourcesContent`).

dist/main.ts (tail)

```
// ...emitted TypeScript...
//# sourceMappingURL=main.ts.map    // v3 map, .glyph embedded
```

Anything that consumes source maps (Chrome DevTools, a Node debugger, an error tracker like Sentry, your bundler) can chain through it and show you the `.glyph` line, not the generated one.

## 3 · Build and type errors already point at Glyph

You rarely get to production with a type error, because `glyph build` runs `tsc --strict` and maps every `tsc` error back onto your `.glyph` source (same caret, the TypeScript error code preserved).

And `glyph build --json` emits every diagnostic as structured JSON (code, severity, file, 1-based line/col, help) for your CI or tooling to act on.

**Details where the caret lands **

The caret goes on the smallest Glyph expression whose code `tsc` rejected: a wrong argument underlines that argument, a misspelt field inside a JSX event handler underlines that member access, not the whole `return` around it.

## 4 · `glyph run` tells you the same things

The command you type all day is `glyph run`, and until 0.1.40 it was quieter than the build. Now it reports the same set: warnings included, on the file you named and on its siblings, after the program's output, with a one-line count.

an unused import, added a step before the code that would have used it

```
$ printf 'look\nquit\n' | glyph run adventure.glyph
...the game plays and exits 0...

[E0106] Warning: lint: unused import `math`
    ╭─[adventure:21:1]
    │
 21 │ import std/math
    │         ╰─────── unused import `math`
    │ Help: Remove the import. Nothing in this module references it.

glyph run: 0 error(s), 1 warning(s) in the source tree
```

**Details the run cache **

The run cache carries them, so a second run of unchanged sources says what the first one said rather than falling silent, and a cache entry whose diagnostics are missing counts as a miss and rebuilds.

**History before 0.1.40 **

It compiled the whole directory, took the list of emitted files out of the report, and threw the diagnostics away. A tree that `glyph build` called out would run clean and exit 0.

## Two current limitations

- **`glyph run`'s own crash stack still shows `.ts` line numbers.** `glyph run` executes via `tsx`, and `tsx` doesn't chain the source map through its own `.ts`→`.js` transform, so an uncaught throw during `glyph run` reports the generated line. Remapping that stack is a follow-up.
- **An error in a sibling module doesn't change `glyph run`'s exit code.** A module that failed to compile is unavailable to import, and a program that never imported it runs anyway. You see the error now, but the process still exits with whatever `main` returned.

**Details production stacks, and why the exit code stays **

It matters for the local `run` loop; in a real deployment you `glyph build` to `.ts`, bundle it with your normal toolchain, and the `.ts.map` chains as usual, so the production stack you actually page on maps back to `.glyph`.

Making it exit non-zero would fail trees that run fine today, so it's an open decision rather than an oversight. If you want the tree's health to decide the exit code, that's `glyph build`.

## Where it stands

### Shipping today

- Readable committed TS.
- A v3 `.ts.map` per file.
- `tsc` errors remapped onto `.glyph`.
- `--json` diagnostics.
- `glyph run` reports what `glyph build` reports, warm cache included.
- Your deploy toolchain's source-map chain reaches `.glyph`.

### On the way

- Remapping the `glyph run` crash stack itself (so the local run loop shows `.glyph` frames too).
- A decision on whether a sibling module's error should make `glyph run` exit non-zero.
- Richer editor debugging via the LSP.

Code examples on this page are checked against Glyph 0.1.158 on every build. Last changed 21 Nov 2026.
