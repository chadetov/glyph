# depsolve

## What it is

A dependency resolver over a small package registry. It parses version and
constraint strings into typed values (prerelease identifiers rank correctly, so
`2.0.0-rc.1` sorts below `2.0.0`), then does what a package manager does: expand
each requirement, pick the highest version satisfying every constraint gathered
so far, and backtrack when a later requirement invalidates an earlier choice.
Unknown package, unsatisfiable constraints, and dependency cycles all carry the
requirement path that reached them.

## Running it

```sh
glyph run examples/apps/depsolve/main.glyph
glyph run examples/apps/depsolve/main.glyph why http
glyph run examples/apps/depsolve/main.glyph --manifest conflict.json
```

## What it changed in Glyph

Shipped **0.1.55**. It is the app that made the tree multi-module, and the one
that leaned on `std/record` hard enough to find that it was not modeled at all.

**G71: `record.get` into a `match` into a two-binding `for` bound the index as a
string.** The program printed `01:x` where it should print `1:x`. Two
independent causes, each reproducing alone: `std/record` was not modeled anywhere
in the typechecker (the string `"std/record"` did not appear in the typechecker
source), and the arm-join compared arm types by equality, so an empty array
literal read as disagreeing with an `Array<string>` arm and sank the whole match.

The emitted TypeScript is well typed either way, which is what makes the D21
lowering choice load-bearing semantics with no `tsc` backstop behind it. The
workaround it removed was an annotation with a three-line comment saying the
annotation was load-bearing, which is the shape of a workaround for a defect
nobody had filed.

## What it exercises

A wire and domain split in its own module, type aliases over `Record`,
backtracking search carried by value so a failed branch is discarded by
returning from it, and the highest `@example` count in the tree at 39.
