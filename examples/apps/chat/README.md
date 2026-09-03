# chat

## What it is

A multi-client TCP chat server over nine modules: nicks, joins, topics, room
posts, direct messages, scrollback. The engine is pure. `server.apply` folds one
command into a new state plus the events it caused, `audience.recipients`
decides who each event reaches, and `framing.feed` reassembles TCP chunks into
lines. One file, `daemon.glyph`, touches a socket; everything else is plain
functions over values, checked with `@example` instead of a running server. The
same `turn` function drives a live client, a recorded replay, and the real
server, so the transcript a replay prints is what a live client would have
seen.

## Running it

```sh
glyph run examples/apps/chat/main.glyph              # replays session.json
glyph run examples/apps/chat/main.glyph --serve 4100  # a real server
glyph run examples/apps/chat/main.glyph --stdin       # type into it yourself
```

## What it exercises

Tagged unions for events, errors and commands, `Record<string, T>` state
stores, a pure/impure split with 36 `@example` rows concentrated in the pure
modules, `async`/`await` over `net.listen`, and `std/net` for the socket layer
with no ambient declarations anywhere in the app.

## What it found, and what happened

This app has been through the loop three times and surfaced three compiler
defects, all now fixed. Each was reproduced against the exact program shape
below before it was fixed, and the current source has no trace of the
workaround left in it.

**G81 (closed in 0.1.62): `io.read_line` returned only at end of input.** The
first version of this app shipped as a session replayer over a recorded JSON
file instead of a live client, because typing into `read_line` produced nothing
until Ctrl-D, at which point every response printed at once. The stdlib had
never actually implemented a line reader; it slurped stdin once with
`readFileSync(0, "utf8")`, which only returns when the writer closes the
stream. Fixed by reading stdin incrementally instead, buffering partial lines
and returning as soon as one arrives. `main.glyph --stdin` now runs `converse`,
a real read/apply/print loop, and `client_line`/`converse` are the code that
depends on the fix.

**G84 (closed in 0.1.63): `glyph run` killed any program still doing something
when `main` returned.** The generated entrypoint called `process.exit(code)` as
soon as `main` came back, so a program that started a TCP server bound its port
and died in the same tick, silently: exit 0, no output. The one server path
already in the stdlib worked around this internally (`http.serve` returned a
promise that never resolved), which is why nobody had noticed the runner itself
was broken. Fixed by assigning `process.exitCode` instead of calling
`process.exit`, so Node's own rule holds: exit when nothing is left to wait for.
`daemon.serve` in this app is exactly the shape that used to fail; `main` calls
it and returns `0` immediately afterward, and the process now stays up because
the listener still holds the event loop open.

**G85 (closed in 0.1.63): a nested project's ambient declarations were dropped
in a tree build.** Since a `package.json` with a `"glyph"` key marks its own
resolution root, `glyph build examples/` builds this app as a nested project
inside a larger one. The outer `tsconfig.json` reached into this app's emitted
output without carrying the declarations this app depended on, so it built
clean standing alone and failed as part of the tree with `Cannot find name
'net'`. Fixed by having each project's config exclude the output directories of
projects nested inside it, so every project is checked exactly once, under its
own configuration. Confirmed today: `glyph check examples/apps/chat` and
`glyph check examples/apps/` both pass. Before the fix, the first of those
already passed on its own; only the tree build failed, which was the worst
shape the error could take, since the same code was rejected only under the
configuration CI actually uses.

Two follow-on releases changed what the socket layer looks like, though neither
closed a gap this app is credited with. 0.1.65 required every app under
`examples/apps/` to drop hand-written TypeScript entirely, which took this
app's `.types/net.d.ts` out; 0.1.79 shipped `std/net` and ported `daemon.glyph`
onto it, which is why the socket layer today calls `net.listen`, `net.on_text`
and `net.send` rather than a raw Node import. `daemon.glyph` is the only file
in the app that touches a socket, and it holds no ambient declaration and no
`extern_ts` escape.

## What is deliberately still awkward

Nothing in this app is currently standing in for an open gap. All three defects
it surfaced are fixed, and the code no longer contains the workarounds it once
needed: no `.types/net.d.ts`, no raw Node import, no session-replayer fallback
where a live client belongs. Where the code looks like it could be simpler,
such as `daemon.glyph` keeping its connection registry as a plain `Array<Conn>`
rather than a map, that is an ordinary design choice the file's own comment
explains (iterated far more often than it is looked up, and it stays small),
not a workaround for anything the compiler cannot do.
