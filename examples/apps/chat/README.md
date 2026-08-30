# chat

## What it is

A multi-client TCP chat server over nine modules: nicks, joins, topics, room
posts, direct messages, scrollback. The engine is pure. `server.apply` folds one
command into a new state plus the events it caused, `audience.recipients`
decides who each event reaches, and `framing.feed` reassembles TCP chunks into
lines. One file touches a socket. The same turn function drives both the live
server and a recorded replay.

## Running it

```sh
glyph run examples/apps/chat/main.glyph              # replays session.json
glyph run examples/apps/chat/main.glyph --serve 4100 # a real server
glyph run examples/apps/chat/main.glyph --stdin
```

## What it changed in Glyph

The most productive app in the tree: four releases came out of it.

**G84 (0.1.63): `glyph run` killed any program still doing something when `main`
returned.** The entrypoint called `process.exit(code)` as soon as `main` came
back, so a program that created a TCP server bound its port and died in the same
tick. Every long-lived program was affected. It survived because the one server
path in the stdlib had a private workaround built into it: `http.serve` returned
a promise that never resolved, so HTTP serving worked and nothing else did.

**G81 (0.1.62): `io.read_line` returned only at end of input**, so no
interactive program could be written, and three apps had been quietly shaped
around it. Its regression test is a timing test: write one line with stdin held
open and require the echo back within 20 seconds.

**G85**: a nested project's ambient declarations were dropped in a tree build, so
the app built alone and failed as part of `examples/`, which is the worst shape a
build error can have. **G87**: `owned` was unusable for sockets, the case it was
specced for, because a socket arrived from an ambient `.d.ts` as an opaque type
that cannot be declared `resource`. **0.1.65** removed the app's `.d.ts`
entirely, and **0.1.79** shipped `std/net` because this app held the last raw
host call in the examples tree.

## What it exercises

Tagged unions for events, errors and commands, `Record<string, T>` state
stores, a pure/impure split with 36 `@example` rows concentrated in the pure
modules, and `async`/`await` over `net.listen`.
