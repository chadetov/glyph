# webhook_ingress

## What it is

An HTTP service that receives third-party webhooks and decides whether to keep
them. It looks up the named source's own secret rather than trying every secret
in turn, verifies an HMAC-SHA256 over the raw request body with a timing-safe
compare, requires a string event field, and appends accepted events to that
source's bounded ring buffer. An admin page renders recent events per source, and
every accept or reject is emitted as one structured log line before the response
is written.

## Running it

```sh
glyph run examples/apps/webhook_ingress/main.glyph --port 8080
```

## What it changed in Glyph

**No gap recorded, and the commit that added it says why that is the useful
result:** it was written to probe the language and found none.

It did surface something about the design rather than the implementation. Taint
is opt-in per call site rather than automatic dataflow, so the first draft
escaped the raw body and let the event type through unescaped, and a payload with
a script tag in that field rendered it live. The discipline holds where it is
applied and says nothing where it is not. The shipped renderer taints and
sanitizes both fields, with a comment explaining why the third does not need it.

It consumes features an earlier trip produced. Raw request body access is what
lets a signature-verifying server stay in Glyph at all, because HMAC must run
over the exact received bytes, which the server used to discard.

## What it exercises

Tagged unions used as decision values, so the response and the log cannot
drift apart. `std/taint`'s `Tainted`/`Trusted` with a single trusted sink, a
`Deque` as a ring buffer, timing-safe comparison over `std/bytes`, structured
logging, and a two-stage JSON boundary. Six `@example` rows.
