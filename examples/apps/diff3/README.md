# diff3

## What it is

A three-way text merge. A generic LCS differ fills a dynamic-programming grid
and walks it, emitting `Edit<T> = Same | Added | Removed` with a caller-supplied
equality in place of `==`, so one table-builder works for any element type. The
merge walks two edit scripts together, pairing each position: a match inside a
match, so all nine variant combinations are explicit arms. The CLI instantiates
that core twice, once over lines and once over words.

## Running it

```sh
glyph run examples/apps/diff3/main.glyph -- base.txt ours.txt theirs.txt
glyph run examples/apps/diff3/main.glyph -- --words base.txt ours.txt theirs.txt
```

## What it changed in Glyph

**Nothing, and that is the result it was written for.** This is a probe: a
deliberately hard shape chosen to see whether the compiler would break on it. It
did not. The commit that added it says so plainly: the generic union, the
closure-parameterized generic function, the `where`-refined `LineNo` used as a
plain record field, and two instantiations of one core from a single CLI all
worked first try.

Negative results are worth recording. An app that finds nothing tells you the
class it covers is solid, which is only useful if you say which class that
was.

## What it exercises

Generic unions with type-parameterized payloads, a closure-parameterized
generic function, a `where` refinement used as a plain record field, nested
exhaustive matching over nine variant pairs, and `loop` plus `match`/`break` as
the iteration form. No `@example` rows: it is verified by fixtures and exit
codes.
