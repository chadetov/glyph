# settle

## What it is

A group-expense splitter and debt simplifier. It splits each expense evenly,
by exact shares, or by weights, in whole cents, with a documented stable rule
for the leftover pennies, and asserts the parts sum to the whole before
recording anything. Then it computes the fewest transfers that square
everybody up. Expenses are appended to a JSON ledger file on disk, so `add`
and `up` are separate runs of the same program rather than one interactive
session.

## Running it

```sh
glyph run examples/apps/settle/main.glyph -- --file $L add "dinner" \
  --payer alice --amount 84.30 --among alice,bob,carla
glyph run examples/apps/settle/main.glyph -- --file $L up
```

## What it exercises

`type Cents = int`, with decimals accepted and printed only at the two
human-facing boundaries (parsing a `--amount` flag, formatting a total) so
nothing in between can drift by a fraction of a cent. A split union
(`Even`, `Exact`, `Weighted`) where each variant carries exactly the fields
that split's rule needs, so an exact split cannot be missing its amounts.
A typed wire record (`WireLedger`) read back through that type's own `parse`
at the file-load boundary. Twenty-five `@example` rows drive `glyph check`.

## What it found, and what happened

**G57: a `match` expression always typed as `Unknown`.** Glyph has no `if`,
so `match` is the branching construct, and the typechecker walked the arms
and then recorded `Unknown` for the whole expression, in every program.
Anything taken out of a branch was untyped from that point on. Two failures
came from it in this app: a field typo on a match-bound value surfaced as a
TypeScript error instead of a Glyph one, and a two-binding `for` over a
matched value picked the wrong lowering, so the program printed `01:a`
instead of `1:a` with both `glyph build` and `tsc --strict` clean. Closed in
0.1.45: a `match` now types through an equality join across its arms, so a
value taken out of a branch keeps its type.

**G58: the `parse` on a type's runtime descriptor had no signature.** This
app reads its ledger through `WireLedger.parse(decoded)`, and the checker
knew nothing about the `parse` a `type` declaration's descriptor emits, so
the call typed `Unknown`, and everything downstream of it did too. That is
the boundary between untrusted file input and typed data, the worst place in
a program to lose a type. Fixing G57 alone did not remove this; the app was
still carrying a type annotation, with a comment explaining it could not be
dropped, to work around it. Also closed in 0.1.45: `T.parse` now types as
`Result<T, Array<Issue>>` for any type that gets a descriptor, and the
annotation and its comment came out of the app in the same change.

**G24: `?` was rejected inside a `match` arm.** `Some(raw) => parse_day(raw,
label)?` failed to compile, so this app's `--date` and `--since` flag parsing
had to go through two small wrapper functions (`day_flag`,
`optional_day_flag`) whose only job was to move the `?` outside the `match`,
to a second call at the use site. Closed: the arm case builds clean now, and
the wrapper functions have been inlined into the two call sites, for example:

```glyph
let date = match flag(f, "--date") {
  None => time.now(),
  Some(raw) => parse_day(raw, "--date")?,
}
```

This was re-verified directly against the built compiler (0.1.105) while
writing this file, not just read off the ledger: `?` inside a `match` arm
that is the whole value of a `let` builds and passes `tsc --strict`, both for
a plain arm value and for one wrapped in `Some(...)`.

## What is deliberately still awkward

Nothing, for the three gaps this app is tracked against. All three are
closed and the workarounds they forced are gone from the source. The app has
no open-gap workaround left to point at; if a future pass finds the source
carrying an annotation or a helper function with no other justification,
that is a regression worth reporting, not a pattern to copy.
