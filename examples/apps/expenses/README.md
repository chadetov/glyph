# expenses

## What it is

An expense-report CLI over a CSV ledger. Every row is validated at the
boundary (four columns, an ISO-8601 date, a non-empty category, a parseable
amount), every bad row is collected with its source line number, and the report
gives per-category count, exact total, share, and a bar chart. Money is
`std/decimal`, so cents do not drift across a sum.

## Running it

```sh
glyph run examples/apps/expenses/main.glyph /tmp/ledger.csv --month 2026-01
glyph run examples/apps/expenses/main.glyph /tmp/ledger.csv --min -12.49
```

## What it changed in Glyph

Fifteen findings. Thirteen were "Glyph made me type more"; two were different
in kind.

**G31 (0.1.39): `time.parse_iso` accepted non-ISO text, read it in local time,
and rolled impossible dates over.** It was a bare `Date.parse`, so
`"January 5 2026"` parsed, `"2026-1-3"` parsed in the host's timezone, and
`"2026-02-31"` returned `Some` for March 3. The reference docs promised "None if
invalid". A boundary validator failing open while its docs promise it fails
closed is the verifiability pillar inverted. The rule that came out of it: when
an app writes a correctness guard around a stdlib primitive, the guard belongs
in the primitive. `parse_date` in `main.glyph` calls `time.parse_iso` with no
guard of its own; an impossible date or a non-ISO string comes back `BadDate`
with its line number rather than being silently accepted.

**G34 (0.1.47): `std/array` had no `fold`, so every accumulation was a `mut`
in a loop.** That dilutes `grep mut`, the search D5's restriction is supposed
to reward. `total_of` and `group_by_category` are both `array.fold` today, and
across `examples/apps/` the same release took `grep mut` from 192 sites to 161.

**G37: a two-binding `for` over a call's result bound a string index**, so
`for i, raw in array.slice(lines, 1)` printed `01:` where it should print `1:`.
Clean build, `tsc --strict` passes, because `"0" + 1` is legal TypeScript. This
app's own `for i, raw in lines` (`lines` being `array.slice(string.split(text,
"\n"), 1)`) used to need an explicit `Array<string>` annotation to bind a real
number, with a comment calling the annotation load-bearing. `array.slice`
getting a real stdlib signature in 0.1.71 closed the specific case this app
hit; the annotation and its comment are gone from `parse_ledger` now, and a
corrupted row reports its actual source line.

All three are closed, and nothing in this app is working around an open gap.

## What it exercises

`std/decimal` for exact money, `time.parse_iso` as a boundary validator, a
four-variant row-error union matched exhaustively, `?` propagation,
`array.fold` for both per-category and grand totals, and `std/record` for
grouping by category. Nine `@example` rows.
