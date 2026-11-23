<!-- Markdown copy of https://glyphlang.io/answers/interactive-cli/, generated from the same source as the page. Last changed 2026-11-21. -->

18 · Programs you type into

# Can I write a program someone types into?

Yes. `io.read_line` returns **as soon as a full line arrives** and does not wait for stdin to close, so a REPL, a game loop, or a chat client answers each line before it reads the next one. Until 0.1.62 it did wait, and that is worth saying out loud, because it made every interactive program impossible.

The whole live-session loop of the chat app. Read, apply, print, repeat:

examples/apps/chat/main.glyph

```
fn converse(limit: Option<int>) -> Server {
  let state = server.empty()
  io.println(LIVE_BANNER)
  loop {
    let text = match io.read_line() {
      Some(l) => l,
      None => { break },
    }
    match client_line(text) {
      None => { io.println(LIVE_HINT) },
      Some(sent) => {
        let done = turn(state, sent.from, sent.line)
        report(done.lines)
        mut state = done.state
      },
    }
  }
  return state
}
```

Driven at a real terminal, with each line typed about 0.6 seconds apart and stdin left open. The left column is milliseconds since the process started:

glyph run examples/apps/chat/main.glyph --stdin

```
1825ms | bob: hey, is the build green?
1826ms |    1  #general <bob> hey, is the build green?
2434ms | alice: green since 9am
2435ms |    2  #general <alice> green since 9am
3042ms | bob: /who
3044ms |       * #general: alice, bob (2)
3648ms | carol: hello?
3649ms |       * ! carol is not in any room, so there is nowhere to send that
```

One to two milliseconds from typed line to answer.

**Details before the fix, and where the first 1.6 seconds go **

Before the fix, nothing printed until 6460ms, which is when stdin closed. The first response in a full run lands around 1.6 seconds in, and that is `tsx` starting up, not read latency.

## Why it matters

A line reader that only returns at end of input is a file reader wearing a line reader's name.

Piping a file through it works perfectly, which is why nothing caught it: every app in the repository that read stdin read a pipe. Then someone wrote a chat client, and it sat silent while they typed and printed every answer at once after Ctrl-D.

**History the apps shaped around it **

That app shipped as a session replayer over a recorded JSON file, with a comment in its source saying why. Three other apps had been shaped around the same thing without anyone naming it: minesweeper could not redraw between moves, the text adventure could not answer `look`, and a REPL evaluated nothing until you hung up.

## The parts that bite in practice

- **CRLF** A trailing `\r` is stripped, so a CRLF file and an LF file give you the same lines and you are not debugging an invisible character on Windows input.
- **No final newline** Input that ends without a final newline still hands back that last line once before `None`, so the last row of a file with no trailing newline is not silently dropped.
- **Split UTF-8** Multi-byte UTF-8 split across a read boundary is held and completed rather than turning into replacement characters.

### `read_line`, then `read_to_string`

`read_to_string` drains the same buffer, so the two compose: `read_line` for a header and then `read_to_string` for the body gives you the rest, not a second copy and not an empty string. Called first, it still returns all of stdin.

A header line, then the body

```
let header = io.read_line()
let body = io.read_to_string()
```

200,000 piped lines are counted in about 20ms, so reading a line at a time is not a throughput tradeoff.

## Where it stands

### Shipping today

- `io.read_line` returns when a line arrives, not when the stream closes, so prompt/read/respond loops work at a terminal and through a pipe alike.
- CRLF is normalized, a final unterminated line is delivered once, multi-byte characters survive chunk boundaries, and `read_to_string` takes the rest of the same buffer.
- A regression test holds stdin open and requires the echo back

  **Details**

  A regression test holds stdin open and requires the echo back, because a test that pipes a file passes against the broken implementation too.

- `io.print` and `io.eprint` write without a newline
- `io.is_terminal` / `io.stdin_is_terminal` tell a program whether it is talking to a person or a pipe

  **Details**

  `io.print` and `io.eprint` write without a newline, so a REPL's `> ` prompt sits on the line the answer is typed on, and `io.is_terminal` / `io.stdin_is_terminal` tell a program whether it is talking to a person or a pipe, so a flag like the chat app's `--stdin` is no longer the only way to ask.

### Still rough

- A program that wants to redraw a screen has no cursor control

  **Details**

  A program that wants to redraw a screen has no cursor control: `std/io` has no cursor or screen functions and no other stdlib module adds them, so a full-screen terminal UI still means writing the escape codes yourself.

Code examples on this page are checked against Glyph 0.1.173 on every build. Last changed 21 Nov 2026.
