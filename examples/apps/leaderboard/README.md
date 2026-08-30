# leaderboard

## What it is

A speedrun leaderboard backed by a persistent order-statistics red-black tree
keyed by score. Submissions append to a log and nothing already written is
overwritten; each command re-reads the log, folds it into the tree, and answers
with an O(log n) walk instead of a re-sort. Every node carries its subtree size,
which is what makes rank and range logarithmic.

## Running it

```sh
glyph run examples/apps/leaderboard/main.glyph -- submit alice 120
glyph run examples/apps/leaderboard/main.glyph -- rank carol
glyph run examples/apps/leaderboard/main.glyph -- range 100 130
```

## What it changed in Glyph

**It drove no new gap, and that is the point of it.** This is the entire
content of **0.1.94**, a release with no compiler change, cut because the app
compiles.

`balance` is the part that could not be written before. Okasaki's four rotation
cases are four match arms, each nesting a constructor pattern inside another
constructor pattern's own field, two levels down, over a union whose `Node`
payload names `Tree<K, V>` again, generic over both parameters. Every piece of
that was a separate gap closed one release at a time: **G137** gave an object
pattern's field a pattern of its own and **G139** carried a nested arm across a
module boundary; **G141** and **G142** stopped a type parameter from making the
nested arm unmatchable; **G130** and **G145** stopped a variant name in payload
position from binding where it should test.

None of them was found by asking whether a red-black tree would compile. They
came out of apps that stopped, and out of reviewing the fixes for the apps that
stopped. This one did not stop.

What it deliberately does not prove: the union is declared in the module that
matches on it. Move it one import away and spell the import as a namespace and
the same arm is E0300 (G140).

## What it exercises

A self-referential generic union, two-level nested constructor patterns inside
object-pattern fields, eight generic functions with function-typed parameters,
and `where` refinements at the JSON boundary, where `-3` reports
`expected Score (int where value >= 0)` rather than clamping.
