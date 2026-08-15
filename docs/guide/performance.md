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

## How you iterate, in a hot loop

Three ways to walk a collection, and they are not equivalent. Counting a match
over an 81-element array, 200,000 rounds, warm:

| | |
|---|---|
| `for c in cells` | **33 ms** |
| `array.filter(cells, ...)` then `array.len` | 62 ms |
| `for i in array.range(array.len(cells))` then `cells[i]` | 61 ms |

**Iterate the collection directly when you do not need the index.** `for c in
cells` compiles to a `for...of` over the array you already have. Indexing costs
a bounds check per element (`cells[i]` is a checked read, which is what turns an
off-the-end index into an error instead of an `undefined` three frames later),
so taking the index when you do not use it is paying for nothing.

A closure is not the thing to avoid. `array.filter` with a closure is within a
few percent of the index loop; V8 inlines both the closure and the small runtime
helpers. Reaching for a manual loop to "avoid the closure" is not where the time
is.

`for i in array.range(n)` compiles to a counting `for` with no array behind it.
Before 0.1.76 it built the range as a real array first, which made the idiom that
reads like a counting loop the slowest of the three by a factor of nearly three;
if you are on an older release, that rewrite is worth doing by hand.

## Working with bytes

Some of `std/bytes` is a thin wrapper over a native call and some of it is a
loop over octets written in TypeScript. The split is what decides whether a
call is fast, so it is worth knowing which is which.

The loops are there for a reason: they are why the module runs somewhere
`Buffer` does not exist, such as a Web Worker. On small inputs they cost nothing
you can measure. On large ones they cost a great deal.

One megabyte, per operation, warm (`benchmarks/micro/bytes_vs_buffer.mjs`):

| | `std/bytes` | node `Buffer` | |
|---|---|---|---|
| `to_hex` | 160 ms | 4 ms | 40x |
| `from_hex` | 11 ms | 2 ms | 5x |
| `to_base64` | 135 ms | 1.2 ms | 100x |
| `from_base64` | 40 ms | 0.5 ms | 80x |
| `to_base32` | 170 ms | no equivalent | |
| `from_text` / `to_text` | 0.4 ms | 0.7 ms | within noise |
| `equals` | 1.4 ms | 0.075 ms | 18x |
| `index_of` (full scan) | 2.6 ms | 0.14 ms | 18x |
| `slice` | 0.48 ms | 0.48 ms | within noise |
| `concat` | 1.2 ms | 1.0 ms | within noise |

At 32 bytes, which is the size of a key or a token, every codec here is under
two microseconds and the ratios are between 1x and 7x. **Encoding a key, a
hash, a nonce or a header is free at any ratio.** The numbers above only matter
when the payload is large.

**`slice`, `concat`, `join` and the UTF-8 bridge are not affected.** They are
`.slice()`, `.set()`, `TextEncoder` and `TextDecoder` underneath, so they are
competitive with `Buffer` and there is nothing to avoid.

**Read the absolute number, not the ratio.** `to_hex` shows 40x and `to_base64`
shows 100x, but `to_hex` is the slower of the two: the ratios differ because
`Buffer`'s hex encoder is itself slower than its base64 encoder, so the ratio
column tells you about `Buffer` rather than about `std/bytes`. Scale matters the
same way for `equals`: 18x sounds alarming, but 1.4 ms per megabyte is 700 MB/s,
which you would have to be comparing megabytes in a loop to notice. The codecs
run at 6 to 25 MB/s, and that is the number worth remembering.

**If you are encoding megabytes on node, reach for `Buffer` through
`extern_ts` for now.** Base64-encoding a 1 MB attachment costs about a tenth of
a second through `std/bytes` and about a millisecond through `Buffer`. A
delegating fast path, native where `Buffer` exists and the current
implementation everywhere else, is on the roadmap.

Worth being precise about what the slowness buys, because it is not the
validation. Validating a decode *after* a native one, by checking that the
decoded length matches what the input claims, costs almost nothing: a fully
checked `Buffer` hex decode is 2.3 ms against the unchecked 2.1 ms, and a
checked base64 decode is within noise of the raw one. So refusing malformed
input is nearly free. What costs is the loop over 6-bit groups in JavaScript.
The reason to keep that loop is the bare realm, not the guarantee.

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

The fingerprint covers everything the build type-checks: every `.glyph` file,
every `.d.ts` under `<src>/.types/`, and every `.ts` or `.tsx` under
`<src>/extern/`. Paths count as well as contents, so renaming or deleting one of
those files rebuilds too. If a run ever looks like it is executing code you
already changed, that is a bug worth reporting, not something to work around by
clearing the cache.

## Measuring

Reason about cost the way you would for the emitted TypeScript, because that is
what runs. When in doubt, read the `.ts` in your build output: it is meant to be
legible, so a surprising cost is visible there. Profile with the JS tooling you
already use (`node --prof`, a flamegraph); the source maps tie frames back toward
your `.glyph`.

The honest summary: Glyph doesn't make your code faster or slower than the
equivalent TypeScript. It makes the equivalent TypeScript easier to keep correct,
and correct code is easier to make fast than fast code is to make correct.
