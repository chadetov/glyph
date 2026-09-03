# linkcheck

## What it is

A Markdown link checker. It takes one or more files or a directory, pulls every
link out of the prose (inline links, reference definitions, bare autolinks, and
image sources), and reports the ones that are broken. Links inside a fenced code
block or an inline code span do not count, because a URL in a snippet is
documentation, not a claim.

Every link carries the file and line it came from. A relative link resolves
against the directory of the file that wrote it and is checked on disk; a
`file.md#anchor` link is resolved by reading the target and matching the anchor
against its headings, slugified the way GitHub does; an external URL is fetched
once per unique address through a bounded worker pool, and the one answer is
fanned back out to every place that URL appears. `--offline` skips the network
half so a run is deterministic; that is what CI wants and what the example below
uses.

The shape is a CLI that reads files, does async network I/O with a concurrency
cap, and reports through a handful of tagged unions rather than strings. Those
three things together are what made it useful for finding gaps: it is a small
program, but it touches the parts of the language a real tool touches.

## Running it

```sh
glyph run examples/apps/linkcheck/main.glyph /tmp/docs --offline
glyph run examples/apps/linkcheck/main.glyph /tmp/docs --max-concurrency 4
```

## What it exercises

Three tagged unions with record payloads (`LinkKind`, `Outcome`, `ArgError`) so a
new link shape or a new failure mode cannot be silently ignored; exhaustive
`match` over `fs.ErrorKind`, where deleting an arm is E0200 rather than a
run-time surprise; the D40 async function type (`fn task_for(url: string) ->
async fn() -> Fetched`) to hand a thunk into a worker pool; and bounded
concurrency and partial failure via `task.pool_settled`. Twenty `@example` rows,
all pure functions (slugging, classification, argument parsing), checked by
`glyph check`.

## What it found, and what happened

This is the highest-yield app in the tree: thirteen findings in its dogfooding
round, G43 through G55. Five of them are recorded against this app
specifically, and all five are closed. Nothing in the current source works
around any of them. One of the five, G52, was revisited a second time: its
observe half closed at 0.1.46 and its bound half, which is what the app had
been working around, closed later at 0.1.69.

**G43, fixed in 0.1.42. A value-position `match` picked its lowering on the
wrong condition.** The emitter decided between a flat `switch` and a value IIFE
by asking "is any arm a block?" instead of "is this match the whole
initializer?", and that one wrong guard produced three symptoms: an `await`
inside an arm landed in a synchronous arrow and `tsc` rejected it, a `mut`
reassignment built from a `match` tripped circular type inference, and `mut` had
no `match` path at all. All three built clean under `glyph build` and only
failed at `tsc`. The fix deletes the special case: a `match` that is the whole
value of a `let` or a `mut` assignment always lowers to the flat `switch`. The
app leans on exactly this path throughout, for example the fence-tracking loop
in `prose_lines` (`mut in_fence = match fence { ... }`) and the accumulator in
`heading_slugs` (`mut slugs = match in_fence || fence { ... }`), both of which
build and type-check as ordinary assignments today.

**G51, fixed in 0.1.48. `regex` could not iterate captures.** `regex.find_all`
mapped each match to the whole matched text and dropped the groups, so an
extractor that needed the capture text had no choice but to hand-roll a
character-by-character scanner: at the time this was filed, that scanner was
180 lines. The fix added `regex.captures_all(pattern, text) ->
Array<Array<string>>`, one inner array of groups per match. The app was
rewritten around it: `scan_inline` in `main.glyph` is now two regex constants
(`INLINE_LINK`, `REF_DEF`) and a short dispatch function (`link_of`) that reads
which group is non-empty to tell an inline link from an autolink from a code
span. The 180-line scanner, and the types and helper functions it needed, are
gone from the source.

**G52, fixed in two halves, 0.1.46 and 0.1.69. `std/http` could not bound or
observe a request.** `Response` had no headers and no final URL, so a redirect
was invisible, and `RequestInit` had no timeout, so the only way to bound a slow
request was to race it against a timer with `task.race`, which left the loser
in flight: the exact thing the task module's own documentation says a bounded
call exists to prevent. 0.1.46 added `Response.headers`. 0.1.69 added
`http.send(f: Fetch)`, where `Fetch` carries `timeout_ms` and a `redirect`
policy, and `Response.url` reports where a followed redirect landed. The app's
`check_external` now builds one `http.Fetch` record with `timeout_ms: 8000` and
`redirect: "follow"` and calls `http.send`, and `from_response` compares
`response.url` against the URL it asked for to report a real redirect location
when the request follows one. `from_status` still falls back to the literal
string `"?"` for a location, but only on the path reached when a 3xx code
surfaces directly instead of through a followed redirect, which a comment above
it marks as normally unreachable now that every request sets `redirect:
"follow"`. A later stdlib addition, `HttpError.kind`, also shows up in
`check_external`'s `match e.kind`: that field did not exist when G52 was filed,
but the app was updated to use it once it shipped, so a timeout and a DNS
failure are told apart by a typed field instead of parsing an error string.

**G53, fixed in 0.1.48. `task.pool` was fail-fast with no settled variant.**
`task.pool` is built on `Promise.all`, so one worker's rejection discards every
other worker's result, even the ones that had already succeeded. This was
measured on two copies of the app differing only in that one call: with a throw
injected into one of three fetches, the `pool_settled` copy printed all three
rows and named the failing URL, and the `pool` copy printed nothing and died on
an unhandled rejection, losing both surviving results. The fix added
`task.pool_settled(limit, tasks)`, returning one `Settled<T>` per task in order
and never rejecting. `fetch_unique` in the app calls it directly, and
`settled_outcome` turns a failed slot into a `NetworkFail` outcome like any
other, so one bad URL costs one row in the report instead of the whole run.

**G55, fixed in 0.1.51. Three things that were not gaps at all.** Multi-line
strings, `math.max`, and the two-import rule for `std/time` all already worked;
an earlier version of this app had reimplemented `max_of` by hand because
`math.max` was reachable only through a slash-grouped line on the stdlib
reference page, and grepping for it found nothing. This was a discoverability
problem, not a language gap, and it closed as a documentation round: the
reference page now lists one call per line. The app's own `widest` function
calls `math.max` directly (`math.max(w, string.len(v))`), and there is no
hand-rolled equivalent left anywhere in the source.

## Current state

No workaround for an open gap is carried in this app. Every gap this app
surfaced is closed, and the source reflects the fixed language and stdlib
throughout rather than routing around any of them. `glyph check .` passes with
20 examples and a clean `tsc --strict`.
