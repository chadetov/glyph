# shortlink

## What it is

A URL shortener you can point a browser at. It serves an HTML form; a POST
validates the URL, mints a seven-character base62 code (or accepts a custom
alias, collision-checked), and answers post-redirect-get. A GET on a code issues
a 302 and increments a click counter, and appending `+` renders a stats page. The
link table persists to JSON and reloads at startup.

## Running it

```sh
glyph run examples/apps/shortlink/main.glyph
```

## What it exercises

Multi-line interpolating string literals for the HTML (`page`, `home_page`,
`stats_page`, `not_found_page`, and the row table all span several source lines
with `${...}` inside them), boundary validation of the persisted snapshot,
shared mutable state through `std/store`, and `async fn main` with `await
listen`. No shim and no Node import: `std/http`'s `html`, `redirect`, and `form`
carry the whole wire.

## What it found

The most productive single-file app in the tree: two dedicated trips, both now
closed.

**G56, fixed in 0.1.44: `glyph run`'s build cache did not hash a hand-written
shim.** An earlier version of this app carried a Node server behind an
`extern/*.ts` shim, and the shim was symlinked in. Editing the shim and running
`glyph run` printed a clean, type-checked build while the process kept executing
the *previous* copy of the TypeScript, because `source_fingerprint` hashed every
`.glyph` file and every `.types/**/*.d.ts` but not `extern/**/*.ts`, and the
recursive walker skipped symlinks outright, so the stale shim never even reached
the hasher. That is a stale program passed off as a correct build, and nothing
on screen told you otherwise. The fix hashes `extern/**/*.ts` and `.tsx` by
relative path and contents the same way `.types` is hashed, so a rename or a
deletion busts the cache too, and the walker now follows symlinks (reading the
target's contents while hashing the link's own path, with a canonical-path set
so a cycle terminates). The fix lives entirely in the compiler; there was no
corresponding line in this file to change. The app itself no longer has
anything for that bug to bite: 0.1.46 replaced the hand-written `node:http`
server and its shim with calls into `std/http` directly (see below), so
`examples/apps/shortlink` today has no `extern/` directory at all.

**0.1.46: `std/http` could only speak JSON.** Not a numbered gap (it is recorded
as stdlib shape in `docs/roadmap/releases.md`), but it is the reason the app
looks the way it does today. `Response` used to be `{ status, body }` with the
content type inferred from the body's shape, and the only constructors were
`json` and `text`, so a `Location` header and an HTML page were both
unspellable. The app had declared its own `Response` and hand-written a server
on `node:http` behind a `.d.ts` the checker does not look inside: unverified
TypeScript wearing Glyph syntax. Adding headers, `html`, `redirect`, and `form`
to `std/http` took the app from 615 lines to 494, deleted the shim, and closed
G56's other door along with it. The same round found the availability half of
header sanitization (redirecting to a target with an emoji in it killed the
server; that is G59, fixed the same cycle) but that finding belongs to a
different app's writeup.

**G62, fixed in 0.1.52: `glyph fmt` collapsed a multi-line string that
interpolates.** `glyph fmt` copies a plain string literal verbatim from source,
which is what lets a multi-line string stay multi-line. A literal with `${...}`
in it is a template, not a plain string, and the formatter rebuilt templates
through the same escaping path used for one-line strings, turning every raw
newline back into `\n`. All five HTML builders in this file (`page`,
`error_banner`, `row`, `table`, `home_page`, `stats_page`, `not_found_page`) had
been written as real multi-line strings, then rewritten with `\n` escapes,
because `glyph fmt --check` failed on the readable form and shipping a curated
example the formatter itself reformats is worse than the escapes. The fix gives
templates the same verbatim-by-span path plain strings already had, gated on
the source slice containing a raw newline, so a single-line template still gets
its `${...}` interior normalized. The app was rewritten back to real multi-line
strings once the fix landed, and that is what is in the file today: `page`,
`home_page`, `stats_page`, `not_found_page`, and the `table` row builder all
span several lines with a literal newline in the source, not an escape. `glyph
fmt --check main.glyph` reports the file already formatted.

## What's still awkward

Nothing here is standing in for an open compiler gap. `glyph check .` passes
clean and `glyph fmt --check main.glyph` reports the file already formatted, and
no gap this app surfaced is currently open.

The one thing worth flagging for a reader is the percent-encoding block at the
top of the file (`is_hex_pair` through `url_encode`, about eighty lines).
`std/http` has no query-string helpers, so the app hand-rolls both directions
by turning a string into hex and walking it two digits at a time, because
slicing UTF-16 units directly corrupts anything outside the BMP. That comment
in the file is still literally true today: `std/http` still doesn't do
percent-encoding for you. Nothing in this app's own history calls that a filed
gap, so it stays as written rather than being rewritten against a decision this
app was never asked to make.
