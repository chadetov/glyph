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

## What it changed in Glyph

The most productive single-file app in the tree: two dedicated trips.

**G56 (0.1.44): `glyph run`'s build cache did not hash the shim directory.** You
edited your shim, ran `glyph run`, and the compiler printed a clean type-checked
build while executing the previous version of your TypeScript. Not a stale error,
a stale program, and nothing on screen distinguished it from a correct build. The
recursive walker also skipped symlinks outright, and this app shipped a symlinked
shim, so the same false green had a second door.

**0.1.46: `std/http` could only speak JSON.** `Response` was `{ status, body }`,
the content type was inferred from the body's shape, and the only constructors
were `json` and `text`, so a `location` header and an HTML page were both
unspellable. The app had declared its own `Response` and hand-written a server on
`node:http` behind a `.d.ts` the checker does not look inside, running as
unverified TypeScript wearing Glyph syntax. Adding headers, `html`, `redirect`
and `form` took the app from 615 lines to 494. It also found the availability
half of header sanitization: redirecting to a target with an emoji in it killed
the server, and the emoji came from a form field.

**G62 (0.1.52)**: the formatter collapsed interpolating multi-line strings, so all
five HTML builders had been written with escapes, reverted from the readable form
because `glyph fmt --check` failed on it.

## What it exercises

Multi-line interpolating string literals for the HTML, boundary validation of
the persisted snapshot, shared mutable state through `std/store`, and `async fn
main` with `await listen`. No shim and no Node import: `std/http`'s `html`,
`redirect` and `form` carry the whole wire.
