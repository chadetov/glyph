# pulse

## What it is

A CLI uptime monitor for HTTPS endpoints. Per target it does DNS resolution, a
certificate-verified TLS handshake, then one hand-written HTTP/1.1 request
directly over the socket, deliberately not through `std/http`, because the point
is reading a response off a socket the way it actually arrives: as an event, not
a return value. It appends one JSON line per check so reliability can be watched
across runs.

## Running it

```sh
glyph run examples/apps/pulse . -- examples/apps/pulse/config.example.json /tmp/history.jsonl
```

## What it changed in Glyph

**G127 (0.1.85): a `tls.connect` to a peer that never answers never settled,
and kept the process alive forever.** So there was a third outcome besides `Ok`
and `Err`, and it was *never*, in the module whose stated promise is that a
failed connection is a value. The obvious workaround does not work either:
racing a sleep against it gives you the winner and leaves the loser running,
which is fine when the loser holds a socket you can close, and here the loser
never produced one.

`connect` now takes a required timeout. Both ends of the range are refused,
including the maximum, because node clamps a longer delay to one millisecond
rather than rejecting it, so an unchecked dial asked to wait 35 days fails after
a millisecond and reports it as "no TLS handshake within 3000000000ms". That is a
confident wrong answer where the original bug was at least a visible hang. The
deadline is a scalar rather than an options record, because a field can be
defaulted by whatever builds the record and mandatory-ness would weaken.

**G128 stays open**: `std/http` bounds nothing by default, the same shape. This
app walked past it by hand-rolling HTTP, but it would hit an uptime monitor
squarely.

**G136 (0.1.89): a `bool` binding could not be matched**, because TypeScript had
already narrowed it to `false`. `glyph build` reported no diagnostics of its own,
so the whole failure was a TypeScript error naming a type the author never
wrote.

## What it exercises

Five modules, a four-variant outcome union matched exhaustively in three
places, and a callback-to-value bridge that exists because Glyph has no `Promise`
a program can construct by hand and `std/task` has no callback-to-promise
bridge. The app still carries that shape, and its header says why.
