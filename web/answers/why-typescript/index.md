<!-- Markdown copy of https://glyphlang.io/answers/why-typescript/, generated from the same source as the page. Last changed 2026-11-21. -->

04 · Positioning

# Why TypeScript, and not Go or a new language?

Because TypeScript is where agents already write the most code and where your ecosystem (npm, React, Node) lives. Glyph is adoptable one file at a time **only because its target is TS**.

the same project: .glyph and .ts, side by side

```
// cart.glyph: new, written in Glyph
import { formatPrice } from "./format.ts"

// checkout.ts: existing, untouched
import { total } from "./cart.glyph"
```

Glyph slots in beside the `.ts` files you already have, and both directions import cleanly.

## Why it matters

A new language stands or falls on how you get from zero to shipping. Compiling to TypeScript means you never leave your codebase, your bundler, your test runner, or your deploy.

You cannot drop Go into a TypeScript codebase file by file: it's a different runtime, a different module system, a different ecosystem. That single fact is why the target is TS.

## The fair point about Go

Its small, stable keyword surface gives an agent fewer ways to go wrong. We agree: it's exactly why Glyph keeps a deliberately small core with one way to do each thing.

Go is included in our density [benchmark](https://github.com/chadetov/glyph/blob/main/benchmarks/FINDINGS.md) for a straight comparison.

## Where it stands

### Shipping today 3

- Clean TypeScript output.
- Bidirectional imports with your existing `.ts`.
- A small core that runs anywhere Node or a browser bundle does.

### Being measured 1

- The cross-language benchmark (Glyph, TS, Go, Python, Rust) grows as the corpus does, so the “small surface” claim stays a number, not an assertion.

Code examples on this page are checked against Glyph 0.1.188 on every build. Last changed 21 Nov 2026.
