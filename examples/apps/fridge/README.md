# fridge

## What it is

A shopping-list CLI for the fridge: `add`, `check`, `uncheck`, `remove`,
`clear`, `list`, persisted as JSON. Adding an item already on the list updates
its quantity and keeps its checked state instead of duplicating. An unparseable
quantity is recorded as "no quantity" rather than an error.

## Running it

```sh
glyph run examples/apps/fridge/main.glyph add milk 2
glyph run examples/apps/fridge/main.glyph list
```

## What it changed in Glyph

The origin app. The gap ledger opens with it, and four of its findings are
about the same thing: validation that was not validating.

**G3: `json.parse<T>` was a cast, not a validating parse.** The runtime was
`Ok(JSON.parse(text) as T)` with no shape check, so the persistence boundary
trusted on-disk data blindly. That is the failure the manifesto's first example
says Glyph exists to prevent.

**G4: the validating descriptor only checked one level.** It tested `typeof` for
primitive fields and bare `"field" in value` presence for everything else, never
recursing, so even the validating path did not validate this app's shape.

**G6: the typechecker did not check field existence or argument types.** `u.naem`
built with zero Glyph diagnostics.

Two weeks of real use produced a second round: `glyph run` latency at two
seconds per invocation, no `array.any` or `array.sort`, and the finding that
`mut` stayed unused because it was never reachable for "update field F of item
N".

## What it exercises

Array patterns with literal heads and rest, four tagged unions matched
exhaustively, `Option` record fields, `fs.ErrorKind` matched on `e.kind`, and a
validating `json.parse<Fridge>` boundary. Ten `@example` rows.
