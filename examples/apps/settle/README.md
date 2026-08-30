# settle

## What it is

A group-expense splitter and debt simplifier. It splits each expense evenly,
by exact shares, or by weights, in whole cents, with a documented stable rule for
the leftover pennies, and asserts the parts sum to the whole before recording
anything. Then it computes the fewest transfers that square everybody up.

## Running it

```sh
glyph run examples/apps/settle/main.glyph -- --file $L add "dinner" \
  --payer alice --amount 84.30 --among alice,bob,carla
glyph run examples/apps/settle/main.glyph -- --file $L up
```

## What it changed in Glyph

Shipped **0.1.45**, and the finding explains an annotation the app was carrying
with a comment saying it could not be removed.

**G57: a `match` expression always typed as `Unknown`.** Glyph has no `if`, so
`match` is the branching construct, and the typechecker walked the arms and then
recorded `Unknown` for the whole expression, in every program. Anything taken out
of a branch was untyped from that point on. Two failures came from it: field
typos surfaced as TypeScript errors instead of Glyph ones, and a two-binding
`for` picked the wrong lowering, so the program printed `01:a` instead of `1:a`
with both `glyph build` and `tsc --strict` clean.

**G58: the `parse` on a type's runtime descriptor had no signature**, so the
boundary between untrusted input and typed data lost its type, which is the worst
place in a program to lose one because it undoes every inference downstream.
Fixing the arm join alone did not remove the app's annotation; this did.

## What it exercises

`type Cents = int` with decimals used only at the two human-facing boundaries
so nothing in between can drift, and a split union where each variant carries
exactly what that rule needs, so an exact split cannot be missing its amounts.
Twenty-five `@example` rows. It still carries a G24 workaround: `?` cannot appear
inside a match arm.
