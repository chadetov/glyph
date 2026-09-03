# csvql

## What it is

A relational query engine over CSV files, in eleven modules. A JSON catalog
declares which files exist and each column's type; a SQL-ish query is scanned,
parsed, bound against the catalog, turned into a plan tree, and executed over
coerced rows. Nothing throws: sixteen failure modes are variants of one union,
rendered in one place.

## Running it

```sh
glyph run examples/apps/csvql/main.glyph
glyph run examples/apps/csvql/main.glyph -- --explain
glyph run examples/apps/csvql/main.glyph -- --query queries/join.sql
```

## What it exercises

Three string-literal unions read across a module boundary: `ColType` is
declared in `catalog.glyph` and matched on in `table.glyph`, `bind.glyph`, and
`render.glyph`; `CompareOp` and `Agg` are declared in `sql.glyph` and matched on
in `bind.glyph`. A recursive plan tree (`plan.glyph`'s `Plan` union holds
`Plan` in its own operator variants). A sixteen-variant failure union rendered
in one place. `std/store` holding the parser cursor across `sql.glyph`'s
recursive-descent parse, so the parse functions take no cursor argument.
Twenty-one `@example` rows, in `csv.glyph` and `table.glyph`.

## What it found, and what came of it

This is the first app where the interesting types are declared in one module
and consumed in another, and two of the compiler's cross-module guarantees
turned out not to reach across that split at all.

**G76, fixed in 0.1.57: an imported string-literal union lost D30's
exhaustiveness guarantee, and the compiler's help text told the author to
delete it.** `catalog.ColType` has four variants. A `match` in `catalog.glyph`
covering all four raised `E0218` anyway, with help text reading "Add an `else`
arm. A `number`/`string` match with only literal arms can never be exhaustive."
That help was false about the code in front of it, and following it turns a
compile-time check into a silent runtime fallthrough, which is what happened:
the match shipped with a dead `else`. The fix gave `DeclTyResolver` a query
that reads a sibling module's literal set, the same way the tagged-union
exhaustiveness check already did across an import; the exhaustiveness check
itself needed no change. `catalog.glyph`'s `is_numeric` is the result: a plain
four-arm match over `ColType`, no `else`, and dropping a variant is now
`E0200`, not `E0218`.

**G75, fixed in 0.1.58: imported record fields lowered to `Ty::Unknown`.** A
field read on a record type imported from a sibling module had no field set at
all, so a typo'd field drew no error, and `for i, x` over an imported record's
array field emitted `Object.entries(...)` and bound `i` to the string `"0"`
instead of the number `0`. `tsc` catches most of what a wrong number-vs-string
index breaks, but not string interpolation or a `record.get` key, so the app
carried three `let` hoists to route around it: copy the array into a
same-typed local first, since a local's field set resolves fine, then iterate
the local. Two were in `table.build`, one in `bind.fields_of`. The fix gave an
imported type an identity: `Ty` grew an `Imported { module, name }` variant
instead of degrading to `Unknown`, so field checking and array-ness both
resolve across the import without a local copy. The hoists are gone:
`table.glyph`'s `build` loops directly over `sheet.rows` and
`spec.columns`, and `bind.glyph`'s `fields_of` loops directly over
`spec.columns`.

**G78, half fixed: a multi-module app could not be built as part of an
enclosing tree.** `examples/apps/csvql/` always built fine on its own; building
it as part of the wider `examples/` tree failed, because a local import
resolves from the build root (D15) and the tree's root is not this app's root.
The failure pointed away from the cause, too: the imported type degraded and
the build reported `E0218` on a match that was actually exhaustive, or, with a
catch-all present, `TS2307: Cannot find module`. This turned CI red the moment
the first multi-module app in the repo landed and kept it red for three
releases.

D41 answers the compiler side: a `package.json` carrying a `"glyph"` key marks
its directory as its own module-resolution root, nearest marker wins.
`csvql/package.json`'s `"glyph": {}` is that marker, and it is why this app
resolves the same imports whether it is built on its own or as part of the
enclosing tree. What keeps the gap tracked as half fixed rather than closed is
that `glyph-cli` and `glyph-lsp` climb to a project root through two separate
implementations that have to be kept in agreement by hand rather than by
sharing code, and only the CLI's has been proven against a tree this size. An
editor open on a workspace that contains csvql alongside its sibling apps can
still resolve a different root than `glyph build`/`glyph check` do.

That residual is editor tooling, not the query engine, and it leaves nothing
in csvql's source to work around. There is no per-file marker or annotation
available to write against it, so the app's shape carries no trace of this
half. If you come back here later and go looking for a workaround to remove,
there isn't one: the fix that closed the half that touches this app already
landed, and the remaining half is the LSP catching up to the CLI, tracked
where the LSP work lives rather than here.
