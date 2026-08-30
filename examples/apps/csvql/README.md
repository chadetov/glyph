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

## What it changed in Glyph

Two releases and one language decision. This is the first app where the
interesting types are declared in one file and consumed in another, and three
guarantees turned off the moment it split.

**G76 (0.1.57): an imported string-literal union lost D30's exhaustiveness
guarantee, and the compiler's help text told the author to delete it.** A match
covering all four variants got E0218 with "Add an `else` arm. A `number`/`string`
match with only literal arms can never be exhaustive." That help is false about
the code in front of it, and following it converts a compile error into a runtime
fallthrough. The author followed it, and the dead `else` shipped.

**G75 (0.1.58): imported record fields lowered to `Ty::Unknown`**, so a typo'd
field on an imported record drew no error and `for i, x` over one bound a string
index. The app carried three `let` hoists to work around it.

**G78**: a multi-module app could not be built as part of an enclosing tree. This
turned CI red the moment the first multi-module app landed and kept it red for
three releases. Resolved as **D41**: a `package.json` with a `"glyph"` key marks
a module-resolution root, nearest wins, which is why every app here carries one.

## What it exercises

Two string-literal unions used across modules, a recursive plan tree, a
sixteen-variant failure union, and `std/store` holding the parser cursor.
Twenty-one `@example` rows.
