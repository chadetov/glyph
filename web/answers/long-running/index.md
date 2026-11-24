<!-- Markdown copy of https://glyphlang.io/answers/long-running/, generated from the same source as the page. Last changed 2026-11-21. -->

19 · Servers & daemons

# Can I write a server, or does the process just exit?

You can write a server. `main` sets things up and returns, and the process **stays alive while sockets and timers are still pending**, the same rule Node uses. Until 0.1.63 it did not: the runner tore the process down the moment `main` returned, so a server bound its port and died in the same tick.

A server, start to finish

```
module main

import std/io
import std/process
import net { createServer }

fn main(argv: Array<string>) -> number {
  let server = createServer(fn(socket) {
    mut socket.write("hello\n")
    mut socket.end()
  })

  mut server.on("error", fn(err) {
    io.eprintln("cannot listen: ${err.message}")
    process.exit(1)
  })

  mut server.listen(4000, fn() {
    io.println("listening on 4000")
  })
  return 0
}
```

See it: a TCP server in full. `main` returns immediately; the callbacks run for as long as the process lives.

## Why it matters

This is the difference between a language that can run a script and one that can run a service. A server, a file watcher, a bot holding a gateway connection, a job runner: none of them finish inside `main`. They set up handlers and hand the process over to events.

**History how it failed before 0.1.63 **

The old behaviour failed in the worst possible way, which is quietly. `glyph run app.glyph --serve 4100` printed nothing, not even the line inside the `listen` callback, and exited 0. Nothing in the source looked wrong and it type-checked. An agent that hits that has no error to report and no reason to think anything failed.

## After `main` returns

Two things worth knowing. The `return 0` runs before the first client ever connects: it is the code the process will eventually leave with, not a statement about when it leaves.

And anything that fails *after* `main` has returned has to set its own exit code, because `main`'s return value is already spent. That is what the `error` handler is for. Without it, a server that cannot bind its port drains its event loop and exits 0, reporting success.

**Details types for the net import **

`net` is a Node builtin, so it is an ordinary import, but it is not one of the six the compiler ships ambient types for (`fs`, `http`, `path`, `os`, `crypto`, `url`). Either install `@types/node` and the build prefers it, or drop a declaration in `.types/net.d.ts` next to your source, which is what the chat app does so it needs no dependencies at all.

## A real one

`examples/apps/chat` is a chat server holding several TCP clients at once. The parts that do not touch the network stay separate and testable: line reassembly, routing, and the room engine are pure functions checked by `@example`, and one file talks to sockets.

Three clients connected at once

```
alice> /join #general
bob>   /join #general
carol> /join #random
alice> hello everyone

alice sees:  1  #general <alice> hello everyone
bob sees:    1  #general <alice> hello everyone
carol sees:  (nothing: she is in #random)
```

### Line reassembly

TCP does not deliver messages, it delivers bytes, so the tail of a chunk is usually half a line. That reassembly is a pure function with the awkward cases written down as examples rather than provoked over a socket:

examples/apps/chat/framing.glyph

```
@example feed("", "hel").lines == []
@example feed("hel", "lo\n").lines == ["hello"]
@example feed("", "a\nb\nc\n").lines == ["a", "b", "c"]
@example feed("", "a\nb").rest == "b"
@example feed("", "hi\r\n").lines == ["hi"]
pub fn feed(buffered: string, chunk: string) -> Framed {
```

## The other direction: a long-running client

A bot is the same shape pointed outward. `examples/apps/discord` is a Discord gateway client: it connects over a WebSocket, identifies, heartbeats on the interval the server dictates, tracks a sequence number so a dropped connection resumes rather than starting over, notices when the connection is open but dead, and backs off between reconnects.

The part worth copying is where the logic lives. Deciding what to do about a `RECONNECT`, a missed heartbeat, or a rejected token is a pure function from a session and one event to a new session and a list of actions. One module carries those actions out on a real socket.

That is why the reconnect logic, which is the hard part to get right, is checked by `@example` rather than by unplugging a network cable:

examples/apps/discord/session.glyph

```
@example a_close_while_a_reconnect_is_pending_is_ignored()
@example a_failed_attempt_schedules_another()
@example a_rejected_token_stops_rather_than_retrying()
pub fn disconnected(current: Session, code: int, reason: string) -> Step {
```

**History three bugs, two mocks **

Those three examples are each a bug that was in the code first. Writing a mock gateway and pointing the bot at it caught one of them. The other two were caught by a second mock written from Discord's documentation instead of from the client, which sends the things a cooperative mock never does: an unprompted heartbeat request, and a close code that means "your token is wrong, stop asking". A mock written by the author of the client only tests the parts of the protocol the author read.

## Where it stands

### Shipping today

- Returning from `main` sets the exit code without stopping the process, so servers, watchers and event loops run.
- A program that only computes still exits the moment `main` returns, with the same code, so nothing about short programs changed.
- Node builtins like `net` are ordinary imports, and a setup throw still terminates

  **Details**

  Node builtins like `net` are ordinary imports, and a program that throws during setup still terminates, now after its diagnostic has reached a piped stderr rather than possibly being truncated by the exit.

- `std/timers` schedules work (`after`, `every`, `cancel`, `unref`) and `std/websocket` opens a connection, so neither needs a hand-written declaration.

### Getting sharper

- A listener that fails to bind after `main` has returned exits 0 unless the program says otherwise

  **Details**

  A listener that fails to bind after `main` has returned exits 0 unless the program says otherwise: `process.set_exit_code(1)` records the failure without tearing down work still in flight, and `process.exit(1)` stops immediately.

- The compiler cannot yet warn that you forgot.
- And there is still no way to write down that a function does not return, so a server's `main` carries a `return 0` that is never reached.

Code examples on this page are checked against Glyph 0.1.176 on every build. Last changed 21 Nov 2026.
