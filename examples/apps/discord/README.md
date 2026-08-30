# discord

## What it is

A Discord gateway bot. It opens a WebSocket, sends IDENTIFY or RESUME,
heartbeats on the interval the server dictates, tracks the sequence number so a
dropped connection resumes rather than restarts, detects a socket that is open
but no longer acknowledging, backs off exponentially between reconnects, and
answers `!ping`, `!help`, `!echo`.

## Running it

```sh
glyph run examples/apps/discord/main.glyph --check
glyph run examples/apps/discord/main.glyph --url ws://127.0.0.1:4910 --token t
```

## What it changed in Glyph

It found the worst bug this project has recorded.

**G94 (0.1.64): a match arm whose last statement produced no value fell through
into the compiler's own non-exhaustive-match throw.** Code that compiles clean,
passes `tsc --strict`, and throws at run time on a match that is exhaustive. The
bot's socket callbacks are nothing but lambdas containing matches, which is why
this app hit it and others did not.

Running it against an adversarial mock found four protocol bugs that inspection
did not. One `RECONNECT` scheduled two reconnects, and after the fix latched, the
bot made exactly two attempts against a dead port and then went silent
permanently. Opcode 1 is a heartbeat request and was parsed as unknown and
ignored, which is how Discord decides a bot is unresponsive. Close codes were
discarded, so a rejected token retried forever. The lesson recorded: a mock
written by the author of the client only tests the parts of the protocol the
author read.

**G91**: Discord puts `"s": null` in every HELLO, so the natural spelling of a
gateway frame was unparseable at exactly the boundary validation is for. Half
closed in 0.1.68. **0.1.65** removed the app's hand-written `.d.ts` and six
escapes to raw TypeScript, which is what produced `std/timers` and
`std/websocket`.

## What it exercises

`resource type` with `owned` bindings and consuming parameters (D25),
`@redact` on the token field (D24), eleven `@open` wire records, and 40
`@example` rows. Zero `extern_ts`, zero TypeScript.
