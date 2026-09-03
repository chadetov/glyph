# collections

## What it is

A generic collections library plus a fallible pipeline: a binary `Heap<T>`
ordered by a caller-supplied comparison, an LRU `Cache<K, V>` whose keys compare
by value so a record key works as well as a string, a `Trie<V>` recursive
through its own field, and `try_map`/`partition` that collect every failure
instead of short-circuiting the way `?` does.

## Running it

```sh
glyph run examples/apps/collections
```

## What it exercises

The heaviest generics user in the tree: 27 generic functions, function-typed
record fields, closures crossing module boundaries, and a recursive generic
record. Twenty-three `@example` rows.

## What it found, and what happened

This app is what surfaced G109 and G110, both fixed in 0.1.74.

**G109: a `for k, v` over an iterand whose type the checker had not settled
silently took the record protocol, so the index arrived as a string.** An
array's pairs are `it.entries()` and the index is a number; a record's are
`Object.entries(it)` and the key is a string. The emitter chose by static type
and, per its own comment, defaulted to a record when the type was unknown. A
loop over a parsed generic record printed `next=01` instead of `next=1`, from a
build reporting no diagnostics with `tsc --strict` passing. Fixed in 0.1.74:
`iter_shape` now answers Array, Record, or Unknown as three distinct cases, and
Unknown emits a runtime helper that checks `Array.isArray` instead of guessing.
A settled type still emits the direct form, so this only ever affects the case
that could not be typed.

**G110: the `Ok` payload of a generic record's `parse` was opaque to the
checker**, which is what made G109 possible: with the parsed value's shape
unknown, the loop above it had nothing to go on. It also meant a field typo on
that value produced no Glyph diagnostic, only a `tsc` error mapped to the whole
enclosing function. Fixed in 0.1.74 alongside G109, by giving a generic
record's descriptor the same per-type-parameter checker arity the emitter
already wrote.

Neither loop in this app (`find` in `cache.glyph`, `try_map_located` in
`pipeline.glyph`) ever hit the unknown-shape path itself: both iterate a
concretely typed `Array<T>`, which the checker could already settle before
0.1.74. The bug showed up on a generic record's `parse` result, a shape this
app doesn't produce. Nothing here is a workaround for either gap; the app's
loops are written the plain way because the plain way has been correct since
0.1.74.
