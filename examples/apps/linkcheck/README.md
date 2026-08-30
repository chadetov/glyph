# linkcheck

## What it is

A Markdown link checker. It walks files or a directory, extracts inline links,
reference definitions, autolinks and image sources, and reports the broken ones,
deliberately excluding links inside code fences and code spans because a URL in
a snippet is documentation rather than a claim. Relative links resolve on disk;
anchors resolve by reading the target and slugifying its headings; external URLs
are fetched once per unique URL through a bounded pool.

## Running it

```sh
glyph run examples/apps/linkcheck/main.glyph /tmp/docs --offline
glyph run examples/apps/linkcheck/main.glyph /tmp/docs --max-concurrency 4
```

## What it changed in Glyph

The highest-yield app in the tree: thirteen findings, G43 through G55.

**G43 (0.1.42): a value-position `match` picked its lowering on the wrong
condition.** It asked "is any arm a block?" instead of "is this match the whole
initializer?", and three symptoms came out of that one guard: an `await` in an
arm landed in a synchronous arrow, a `mut` reassignment from a match tripped
circular inference, and `mut` had no match path at all. All three built clean.

**G51: `regex` could not iterate captures**, which turned a fifteen-line link
extractor into a 180-line character scanner. It sat half-fixed for one release on
the belief the alternation could not be discriminated, and closed only when
rewriting the app proved it could.

**G52: `std/http` could not bound or observe a request.** The app's timeout
workaround left the loser in flight, which is the exact thing the task module's
own comment says its scope exists to prevent. A network client that cannot bound
a request is not shippable.

**G53: `task.pool` was fail-fast with no settled variant**, measured on two
copies of the app differing only in that call: one printed all three rows and
named the failing URL, the other printed nothing and died on an unhandled
rejection, losing both surviving results.

**G55** is three findings that were not gaps at all: multi-line strings,
`math.max` and the two-import rule already existed. A shipped feature nobody can
find is not a feature, so it closed as a docs round.

## What it exercises

Three tagged unions with record payloads so a new link shape cannot be
silently ignored, exhaustive `match` over `fs.ErrorKind` where deleting an arm is
now E0200, the D40 async thunk type, and bounded concurrency via
`task.pool_settled`. Twenty-one `@example` rows.
