# schedule

## What it is

A meeting-slot finder across several calendars. It validates a JSON calendar at
the boundary, merges each participant's overlapping blocks (reporting
double-bookings), unions busy time across everybody, subtracts that from a
working window on each day, and prints the slots long enough to hold the
meeting. Everything is UTC.

## Running it

```sh
glyph run examples/apps/schedule/main.glyph /tmp/calendar.json \
  --from 2026-01-05 --days 5 --duration 60 --hours 09:00-17:00
```

## What it changed in Glyph

Shipped **0.1.41**. The headline was that a `where` refinement stopped working
the moment the type was used as a field; probing one step further found the same
hole on a second axis, and the two were one defect.

**G40: descriptor resolution scanned only the emitting module and knew nothing
about refinements.** So a refined alias in field position dropped its predicate
(`Instant.parse("no")` was an error but `Block.parse({ start: "no" })` was fine),
and a field typed by a record imported from another module was checked by
`!== undefined`, which covers every non-generic cross-module composition in every
multi-file Glyph program. Both built clean and passed `tsc --strict`. The sharp
part: the cross-module machinery already existed for imported *generic*
descriptors, so the hard version was built and the easy one was not. One change
closed seven downstream call sites.

**G42: `glyph build` printed "no diagnostics" above its own `tsc` errors**,
because the Glyph-stage summary was printed before the TypeScript stage ran. A
red build introduced by a green line.

## What it exercises

The refinement `type Instant = string where is_instant(value)`, which is what
let the app delete a hand-rolled validation pass that existed only because the
refinement stopped at the field. A wire and domain split, and a half-open
interval convention documented in the type. Twenty-seven `@example` rows.
