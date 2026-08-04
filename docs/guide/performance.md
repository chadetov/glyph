# Performance

Glyph compiles to readable TypeScript and runs on a JavaScript engine (Node, or
whatever runs your output). Its performance *is* JavaScript's performance:
there is no interpreter of Glyph's own, no hidden runtime, and the emitted code
is close to what you would write by hand. That has practical consequences worth
knowing.

## What is cheap and what is not

- **Function calls, records, arrays, closures** are ordinary JS values and
  operations. A Glyph record is a plain object; a tagged union variant is an
  object with a `tag` field. No boxing beyond what V8 already does.
- **`match`** lowers to a `switch` on the `tag` (for unions) or the value, not a
  chain of type checks. It is as fast as a hand-written switch.
- **`?` and `Result`** are plain objects (`{ tag, value }`), not exceptions.
  Propagating an error is a branch and a return, not a stack unwind.
- **Runtime validators** (`T.parse`, descriptors) are real work: they walk the
  value checking every field, recursively. That is the cost of validating
  untrusted input, and you only pay it where you call `.parse`. Validate at the
  boundary, then pass the typed value around freely.

## Where the cost actually is

The descriptor a `type` generates is the one place Glyph adds runtime work you
didn't write. For a hot path that re-validates the same trusted data in a loop,
validate once at the edge and keep the typed value; don't call `.parse` inside
the loop. For data you produced yourself and never left the process, you don't
need `.parse` at all, the type already holds.

## Concurrency

There is one execution thread (the JS event loop). `async`/`await` and
`std/task` overlap I/O-bound work (network, disk), which is where most server
latency lives; they do not parallelize CPU-bound work across cores. For genuine
CPU parallelism you would reach for worker threads through an `extern_ts` or a
JS library, the same as in TypeScript.

## Startup

A Glyph program starts as fast as the Node process that runs it. `glyph run`
caches the build and the type-check by a fingerprint of the sources, so a
repeated run of an unchanged program skips both and just executes. The
diagnostics are cached alongside the output, so a cached run still reports the
warnings the first one reported rather than going quiet.

## Measuring

Reason about cost the way you would for the emitted TypeScript, because that is
what runs. When in doubt, read the `.ts` in your build output: it is meant to be
legible, so a surprising cost is visible there. Profile with the JS tooling you
already use (`node --prof`, a flamegraph); the source maps tie frames back toward
your `.glyph`.

The honest summary: Glyph doesn't make your code faster or slower than the
equivalent TypeScript. It makes the equivalent TypeScript easier to keep correct,
and correct code is easier to make fast than fast code is to make correct.
