# fridge

## What it is

A shopping-list CLI for the fridge: `add`, `check`, `uncheck`, `remove`,
`clear`, `list`, persisted as JSON in `.fridge.json` next to wherever it runs.
Adding an item already on the list updates its quantity and keeps its checked
state instead of duplicating. An unparseable quantity is recorded as "no
quantity" rather than an error. This is the origin app: the first thing built
in Glyph, and the gap ledger (`docs/dogfooding-gaps.md`) opens with what it
found.

## Running it

```sh
glyph run examples/apps/fridge/main.glyph add milk 2
glyph run examples/apps/fridge/main.glyph list
```

## What it exercises

Array patterns with literal heads and rest, four tagged unions (`Command`,
`ParseError`, `LoadError`, `SaveError`) matched exhaustively, `Option` record
fields, `fs.ErrorKind` matched on `e.kind`, and a validating `json.parse<Fridge>`
boundary at load time. Ten `@example` rows run on every build.

## What it found, and what happened

Three of the first findings recorded against Glyph were about the same thing:
a validating path that did not validate.

**G3, fixed.** `json.parse<T>` was a plain cast, `Ok(JSON.parse(text) as T)`
with no shape check, so this app's own persistence boundary trusted whatever
was on disk. That is the exact failure the manifesto's first example names.
The fix (`Route typed json.parse through the type's descriptor`, landed before
the first published release) rewrites `json.parse<T>(text)` to
`json.parse_with(text, T.schema)` wherever `T` has a descriptor, so the decoded
value goes through the type's own validator. Building this app today emits
exactly that: `main.ts` contains `json.parse_with(text, Fridge.schema)` at the
`load` call site, and a `.fridge.json` with a string where `quantity` should be
a number now fails with `corrupt fridge file ./.fridge.json: field quantity
must be Option<number>` instead of loading or crashing.

**G4, fixed.** The descriptor's `is`/`parse` guard checked `typeof` on
primitive fields and bare `"field" in value` presence on everything else, one
level deep, so even the "validating" path never actually validated a nested
shape like this app's `Fridge { items: Array<Item> }`. The fix (`Recurse the
record descriptor into nested fields`) makes the guard recurse: an
`Array<Item>` is checked with `Array.isArray` plus `Item.is` on every element,
and an `Option<number>` field is checked by its tag and, when the tag is
`Some`, the payload type too. The emitted descriptor for `Item` in this app
checks `quantity` and `category` down to the `Some` payload's type, and
`Fridge.is` calls `Item.is` on every array element rather than stopping at
"is this an array."

**G6, fixed.** The typechecker did not check field existence or call-argument
types at all; a typo like `u.naem` or a wrong-typed argument built clean and
only `tsc` caught it, in emitted-TypeScript terms. The fix (`Check field
access and call arguments in the typechecker`) added E0210 (unknown field) and
E0211 (argument type mismatch) as real Glyph diagnostics with carets into the
`.glyph` source. Reproducing the original symptom against the current compiler
now reports it at the Glyph level before `tsc` ever runs:

```
[E0210] Error: typecheck: type `U` has no field `naem`
[E0211] Error: typecheck: argument type mismatch: expected `number`, found `string`
```

All three were fixed before the first published release; the compiler that
shipped as 0.1.0 already carried the validating parse, the recursive
descriptor, and the field/argument checks. `glyph check .` against this app
passes clean today: 10 examples, one module, no diagnostics, `tsc --strict`
passed.

A later round of real use with this same app (extending it with merge-on-add
and the `summary` footer) surfaced a further set of findings: `glyph run`
latency, no `array.any`/`array.sort`, and `mut` never being reachable for
"update field F of item N." Those are recorded in the ledger as R1 through R5
and G12/G13. They are not part of this pass; their current status lives in
`docs/dogfooding-gaps.md`, not here.

## What's deliberately still awkward

Nothing in this app is standing in for a gap that is still open. `add_item`,
`set_checked`, and `clear_checked` all rebuild the list with `array.map`/
`array.filter` and object spread rather than mutating in place; that is the
idiom the app was written in, not a workaround, and nothing here should be
"cleaned up" into a `mut`-based loop on the strength of this README.
