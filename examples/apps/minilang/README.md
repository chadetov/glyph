# minilang

## What it is

A tiny scripting language implemented end to end: lexer, Pratt parser,
tree-walking evaluator, and a REPL. It has numbers, strings, booleans, nil,
arrays, records and first-class closures, plus seven builtins. Its rule is that
nothing throws: a lexer, parser or runtime failure is a value carrying the span
that caused it, and all three render the same way, with the offending line quoted
and a caret under the column. Interpreter limits come from a config file through
a record descriptor, so runaway recursion is a diagnostic rather than a blown
host stack.

## Running it

```sh
glyph run examples/apps/minilang/main.glyph
glyph run examples/apps/minilang/main.glyph -- --tokens
glyph run examples/apps/minilang/main.glyph -- --repl
```

## What it exercises

Recursive tagged unions for the AST and for tokens, four separate error unions
each carrying spans, `Record<string, Value>` environments with an `Option<Env>`
parent chain, and record-descriptor validation of the config. No `@example`
rows.

Its other value is size: at 2,205 lines this is the largest single Glyph file
in the tree, and that it was written at all is a finding in its own right.

## What it found, and what happened

This app did not open either gap below; it was a bystander that got fixed
along with the rest of `std/io`. Both are closed now, and no line here changed
because of it.

- **G81, closed in 0.1.62.** `io.read_line` called `readFileSync(0, "utf8")`,
  which returns only when stdin closes, so `run_repl`'s
  `loop { match io.read_line() { ... } }` compiled and ran but never answered a
  line until the person typing pressed Ctrl-D, at which point every response
  printed at once. The fix rewrote `read_line` in `runtime/std/io.ts` to read
  stdin incrementally and return as soon as a line arrives. `run_repl`'s code
  did not change; the same loop now evaluates each line as it comes in.
- **G82, closed in 0.1.66.** `std/io`'s only writers were `println` and
  `eprintln`, both of which append a newline, so there was no way to print a
  prompt and have the answer land on the same line. `run_repl` worked around
  this by not printing a prompt at all: it reads a line, then `run_repl_line`
  prints `>>> ${text}` back afterward, so the transcript looks prompted only in
  hindsight. `io.print` and `io.eprint` (writers that do not append a newline)
  closed the gap, but the workaround in `run_repl` is still there today: it
  still does not call `io.print` before reading.

## What is deliberately still awkward

`run_repl` still does not print a live prompt before `io.read_line()`, even
though `io.print` has made that possible since 0.1.66. This was tried while
writing this note and reverted, because it does not simplify anything: adding
`io.print("> ")` before the read makes a real terminal show `> ` followed by
whatever the person types, but `run_repl_line` still prints `>>> ${text}`
after the line comes back, so every line of a live session is shown twice
(`> 1 + 1` from the live prompt and the person's own typing, then `>>> 1 + 1`
again from the echo). Verified against a real pty, not just a piped file.

Removing the double print requires `run_repl_line` to stop echoing the input
line when it is driven live, while still echoing it when it replays a
`session.txt` through `run_session`, since a replayed line was never typed at
a terminal and has to be printed to appear at all. That is a change to a
function both call paths share, not a one-line fix, so it was left alone
rather than decided here.
