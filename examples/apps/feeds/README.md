# feeds

## What it is

An RSS reader over a real npm package. It fetches a feed, parses it with
`fast-xml-parser`, stops the parser's `any` at a generated record descriptor,
resolves every item link against the feed URL, and prints title and href.

## Running it

```sh
glyph run examples/apps/feeds/main.glyph
glyph run examples/apps/feeds/main.glyph https://example.com/feed.xml
```

## What it changed in Glyph

Shipped **0.1.79**, and it is the app behind the 1.0 interop gate.

It reads an RSS feed with an ordinary typed npm dependency, imported by name,
constructed with `new` (D37), returning an `any` that `Document.parse` turns
into a checked value. No adapter, no hand-written `.d.ts`, no `extern_ts`. That
made it the first application in the tree to use a real npm package, so the 1.0
gate ("can a working engineer use their existing npm dependencies without a
hand-written adapter") has an app behind it rather than only a guide.

Two open findings it carries as source comments rather than working around:
**G118**, a client cannot say a response body is text, so `Response.body` is
`unknown` and `string.from` renders `[object Object]` on a JSON body; and
**G119**, `url.join`'s `Err` branch is nearly unreachable because against a valid
base anything that is not a URL is treated as a relative path.

The round also produced a process finding: the plan said this work could not be
committed at all, and three of its premises were already done. Five stale
premises across one plan, and the lesson is the one already written down: re-check
before implementing.

## What it exercises

An npm import by bare name with D37 `new`, generated record descriptors,
`async fn main` with `await http.get`, and five `@example` rows, three of them
pinning `url.join`'s actual behaviour rather than its documented one.
