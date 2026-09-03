# bracket

## What it is

A single-elimination tournament bracket: seeding by standard order, byes when
the entrant count is not a power of two, results, standings, and an ASCII
bracket that re-renders after every match. The design point is recursion.
`Slot = Seeded | Bye | Feeder({ source: Match })`, so advancing a winner is
reading the child match's outcome from the parent's slot, not a write into a
parallel table.

## Running it

```sh
glyph run examples/apps/bracket/main.glyph -- new entrants.txt --title T
glyph run examples/apps/bracket/main.glyph -- report alice --score 3-1
glyph run examples/apps/bracket/main.glyph -- standings --champion
```

## What it changed in Glyph

This is the app that produced the **0.1.43** trip, and the finding was that
the build lied.

**G49 (0.1.43): `@example` execution was opt-in behind `--test`, contradicting
D23.** The `--json` half was worse: the JSON emitter ran before the example
block, so `glyph build --test --json` printed `"ok": true, "tsc": "passed"` on
a project whose own `@example` asserted something false. A plain `glyph
build` now runs them, and flipping one assertion prints the failure and
withholds the `tsc --strict passed` line. All fifteen of this app's own
`@example` rows are the ones a plain build used to skip.

**G41 (0.1.52): a descriptor's `.parse` result was not assignable to
`Result`.** `Bracket.parse` and `SeedFile.parse` used to return a bare `{
tag, value }` object, so `load` and `read_seed_file` each wrapped the call in
an identity re-wrap `match`: an `Ok(b) => Ok(b)` arm next to an `Err` arm that
only reworded the message, just to get something the rest of the function
could treat as a real `Result`. A descriptor's `.parse` now returns the actual
`Result`, built with the prelude constructors, so both functions call
`.map_err(...)` directly on the parse result. The two re-wrap `match` blocks
are gone; see `load` and `read_seed_file` in `main.glyph`.

**G30, the range half (0.1.52): `for` had no counted range.** This app had
hand-rolled the counted loop as `upto` and `span`, used across 16 call sites
in the bracket layout and rendering code. Both helpers are deleted; every one
of those call sites now reads `array.range(n)` or `array.range_from(lo, hi)`,
and the emitted TypeScript is byte-identical to what the hand-rolled helpers
produced. G30's other half, `xs[i]` typing as `Unknown` with no bounds check,
closed separately in 0.1.70 with a runtime bounds check on the emitted read
rather than a code shape change in any app; this app's own indexing
(`widths[r]`, `rest[i]`, `argv[0]`, and so on) was already in range by
construction, so nothing here needed to change for it.

None of the three gaps above leaves a workaround behind. There is nothing
currently open that this app is carrying around.

## What it exercises

A mutually recursive union and record, `Option` payloads, descriptor `parse`
with `.map_err`, and `std/random { seeded }` for a reproducible shuffle. Fifteen
`@example` rows.
