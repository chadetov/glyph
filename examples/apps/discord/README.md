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

**G94, fixed in 0.1.64: a match arm whose last statement produced no value fell
through into the compiler's own non-exhaustive-match throw.** Code that
compiled clean, passed `tsc --strict`, and threw at run time on a `match` that
was exhaustive. A lambda body is a value block in return position, so an arm
ending in a `mut` (or a `let`, `for`, `loop`, none of which produce a value)
emitted no `return` and no `break` either, and the generated `switch` ran
straight into its own `default: throw new Error("non-exhaustive match")`. The
bot's socket callbacks are nothing but lambdas containing matches, which is why
this app hit it and others did not. The fix made the emitted `break` depend
only on being inside a `switch` case, not on the arm's position, and
`gateway.glyph`'s `on_open`, `on_message`, `on_close` and `on_error` handlers
are exactly that shape today: a `match` inside a lambda with an arm ending in
`mut`, compiling and running with no throw.

Running it against an adversarial mock found four protocol bugs that inspection
did not. One `RECONNECT` scheduled two reconnects, and after the fix latched, the
bot made exactly two attempts against a dead port and then went silent
permanently. Opcode 1 is a heartbeat request and was parsed as unknown and
ignored, which is how Discord decides a bot is unresponsive. Close codes were
discarded, so a rejected token retried forever. The lesson recorded: a mock
written by the author of the client only tests the parts of the protocol the
author read.

**G91 is half fixed.** Discord puts `"s": null` in every HELLO. A single
`Frame` record with `s: Option<int>` cannot read that: an `Option` field only
ever accepted Glyph's own `{"tag":"Some","value":1}` encoding, so a bare
`null` and a bare `1` were both rejected. 0.1.68 closed half of it: an optional
field (`field?: T`) now treats a JSON `null` as the key being absent, which is
the mapping `glyph gen openapi` already documented and the runtime had not
implemented. That does not reach `Option<T>` itself, which is what this
protocol would actually need, since `s` is present and numeric on a DISPATCH
frame and absent on a HELLO. Reassessed in the cycle that shipped 0.1.100, the
choice was reported as a fork rather than picked: loosen `Option<T>.parse` to
accept a bare `null` or a bare value, or add a distinct boundary type that
decodes into `Option<T>` and leave `Option` itself strict. Neither has shipped.

The app works around the open half the way the entry recommends: a per-opcode
`@open` record that declares only the fields that opcode carries, so a field
nobody declares is a field nobody has to decode. `protocol.glyph`'s
`DispatchFrame`, `HelloFrame` and the rest exist for exactly this reason. This
is deliberate, not dated: collapsing them into one `Frame` type with
`s: Option<int>` would put the parse back where G91 found it, so the
per-opcode records should stay until the fork above is decided.

Separately, **0.1.65** removed the app's own hand-written `.d.ts` and six
escapes to raw TypeScript for the socket and the timers (G90, not G91), which
is what introduced `std/timers` and `std/websocket`; `gateway.glyph`'s socket
layer is ordinary Glyph as a result.

## What it exercises

`resource type` with `owned` bindings and consuming parameters (D25),
`@redact` on the token field (D24), eleven `@open` wire records, and 37
`@example` rows. Zero `extern_ts`, zero TypeScript.
