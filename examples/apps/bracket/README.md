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

Shipped **0.1.43**, and the finding was that the build lied.

**G49: `@example` execution was opt-in behind `--test`, contradicting D23.** The
`--json` half was worse: the JSON emitter ran before the example block, so
`glyph build --test --json` printed `"ok": true, "tsc": "passed"` on a project
whose own `@example` asserted something false. Now a plain build runs them, and
flipping one assertion prints the failure and withholds the `tsc --strict
passed` line.

It also held the workaround for **G41** (a descriptor's `.parse` result was not
assignable to `Result`, so the app carried two identity re-wrap matches around
its parse calls) and drove **G30** (`array.range`), which deleted its
hand-rolled `upto` and `span` across 16 call sites with byte-identical output.

## What it exercises

A mutually recursive union and record, `Option` payloads, descriptor `parse`
with `.map_err`, and `std/random { seeded }` for a reproducible shuffle. Fifteen
`@example` rows.
