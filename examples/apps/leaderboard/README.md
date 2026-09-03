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
that was a separate gap, closed one release at a time: **G137** gave an object
pattern's field a pattern of its own and **G139** carried a nested arm across a
module boundary (0.1.90); **G141** and **G142** stopped a type parameter from
making the nested arm unmatchable and the match unchecked (0.1.91); **G130**
and **G145** stopped a variant name in payload position from binding where it
should test (0.1.93). `balance` above is what the fixed shape looks like: four
arms, each testing two levels into an object pattern, over a generic
self-referential union.

None of them was found by asking whether a red-black tree would compile. They
came out of apps that stopped, and out of reviewing the fixes for the apps that
stopped. This one did not stop.

At the time this app shipped, one thing it deliberately did not prove was still
true: the union here is declared in the module that matches on it, and moving
it one import away and spelling the import as a namespace made the same nested
arm E0300 (**G140**). That closed in 0.1.97: `union_variant_payload` gained a
`Ty::Imported` branch that resolves through the same generic substitution the
local-declaration path already used, so a namespace-qualified `tree.Node({
left: tree.Node({ .. }) })` compiles the same way a locally declared one does.
Checked directly against the current compiler: a two-module version of this
app's nested pattern, matched as `tree.Node(...)` through a namespace import,
type-checks clean. Keeping this app in one module was never a workaround for
that gap; it is just a small CLI that never needed a second module, and there
is nothing about its shape left standing in for an open limitation.

## What it exercises

A self-referential generic union, two-level nested constructor patterns inside
object-pattern fields, eight generic functions with function-typed parameters,
and `where` refinements at the JSON boundary, where `-3` reports
`expected Score (int where value >= 0)` rather than clamping.
