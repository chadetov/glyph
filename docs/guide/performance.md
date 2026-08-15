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
loop over octets written in TypeScript. The loops are why the module runs where
node's `Buffer` does not exist, such as a Web Worker.

One megabyte, per operation, warm (`benchmarks/micro/bytes_vs_buffer.mjs`).
These are the codecs as rewritten after 0.1.78; on 0.1.78 itself the four of
them are 5x to 30x slower than shown here:

| | `std/bytes` | node `Buffer` | |
|---|---|---|---|
| `to_hex` | 5.3 ms | 4.0 ms | 1.3x |
| `from_hex` | 9.7 ms | 2.1 ms | 4.6x |
| `to_base64` | 9.7 ms | 0.3 ms | 35x |
| `from_base64` | 11 ms | 0.6 ms | 19x |
| `to_base32` | 9.4 ms | no equivalent | |
| `from_text` / `to_text` | 0.6 ms | 0.7 ms | within noise |
| `equals` | 1.4 ms | 0.08 ms | 18x |
| `index_of` (full scan) | 1.1 ms | 0.14 ms | 8x |
| `slice`, `concat` | 0.5 / 1.0 ms | 0.5 / 0.9 ms | within noise |

At 32 bytes, the size of a key or a token, everything here is around a
microsecond. **Encoding a key, a hash, a nonce or a header costs nothing you can
measure.** The numbers above only matter for large payloads, and at roughly
100 MB/s for the codecs, "large" now means tens of megabytes rather than one.

**Read the absolute number, not the ratio.** `to_base64` shows 35x and `to_hex`
shows 1.3x, but they take the same 9.7 and 5.3 ms: the ratios differ because
`Buffer`'s base64 encoder is exceptionally fast and its hex encoder is not, so
that column describes `Buffer` more than it describes `std/bytes`. The same goes
for `equals` at 18x, which is 700 MB/s, fast enough that you would need to be
comparing megabytes in a loop to notice.

**`slice`, `concat`, `join` and the UTF-8 bridge are not worth avoiding.** They
are `.slice()`, `.set()`, `TextEncoder` and `TextDecoder` underneath.

Worth knowing what the remaining gap is not. It is not the validation: checking
a decode *after* a native one, by comparing the decoded length against what the
input claims, costs almost nothing, and a fully checked `Buffer` hex decode
measures 2.15 ms against the unchecked 2.12 ms. Refusing malformed input is
close to free. What is left is that `Buffer`'s codecs are native and these are
not, which is the price of running in a bare realm.

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
