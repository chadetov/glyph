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

## What it changed in Glyph

Shipped **0.1.74**, and it found the quiet one.

**G109: a `for k, v` over an iterand whose type the checker had not settled
silently took the record protocol, so the index arrived as a string.** An
array's pairs are `it.entries()` and the index is a number; a record's are
`Object.entries(it)` and the key is a string. The emitter chose by static type
and, per its own comment, defaulted to a record when the type was unknown. A
loop over a parsed generic record printed `next=01` instead of `next=1`, from a
build reporting no diagnostics with `tsc --strict` passing. The round's own
verdict: this is the class the language exists to remove.

**G110**, the cause behind it: the `Ok` payload of a generic record's `parse` was
opaque to the checker, so a field typo on the parsed value produced no Glyph
diagnostic at all.

## What it exercises

The heaviest generics user in the tree: 27 generic functions, function-typed
record fields, closures crossing module boundaries, and a recursive generic
record. Twenty-three `@example` rows.
