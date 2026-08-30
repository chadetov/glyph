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
in the primitive.

**G37: a two-binding `for` over a call's result bound a string index**, so the
loop printed `01:` where it should print `1:`. Clean build, `tsc --strict`
passes, because `"0" + 1` is legal TypeScript.

**G34** cost a pillar directly: with no `array.fold`, every accumulation is a
`mut` in a loop, which dilutes `grep mut`. After the fix, `grep mut` over the
apps tree went from 192 sites to 161.

## What it exercises

`std/decimal` for exact money, a four-variant row-error union matched
exhaustively, `?` propagation, and `array.fold`. Nine `@example` rows.
