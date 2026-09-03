# jobq

## What it is

A durable job queue: an HTTP API, a SQLite store, and workers. Jobs are
submitted over HTTP, claimed under a lease, run, retried with backoff by
pushing `run_after_ms` forward rather than sleeping, and dead-lettered after a
cap. The point is the guarantee, not the feature list: a job that has been
accepted survives a restart, a worker that dies hands its job back, and no job
runs twice at once.

It was chosen because nothing in the repository had run `http.serve` before,
and because a queue is a state machine whose correctness is checkable rather
than a matter of taste.

## Running it

```sh
glyph run examples/apps/jobq/main.glyph serve --port 8080 --db /tmp/jobq.db
glyph run examples/apps/jobq/main.glyph work --db /tmp/jobq.db
glyph run examples/apps/jobq/main.glyph selftest
```

`selftest` runs the whole queue against an in-memory database with no network
and no clock dependence: three jobs submitted, a worker draining them on a
fixed clock, the failing job driven to its retry ceiling, and a deliberately
corrupt row confirmed to be reported rather than swallowed. It has also been
run the long way, as two separate processes: `serve` and `work` against the
same file, with the server killed mid-run and a fresh process reading the same
database back to the same state.

## What it exercises

A five-variant status union where `Dead` is deliberately kept separate from
`Failed`, because collapsing them into a `failed: bool` is how a queue ends up
retrying a poison message ten thousand times. A pure transition function that
takes `now_ms` as a parameter rather than reading a clock, so backoff and
lease expiry are checked without waiting for them. Thirty-two `@example` rows
across `job.glyph` and `api.glyph` cover the state machine and the router; the
persistence layer and the socket are the only impure parts, and they are
exercised by `selftest` instead.

## What it found, and what happened

**G65, fixed in 0.1.66.** `==` compiled to a deep comparison inside an
`@example` and to reference equality (`===`) everywhere else. This surfaced in
the first module written, inside ten minutes: `last_error(j) ==
Some("bad payload")` was false inside a function while the identical
expression as an `@example` passed. A test reporting success on code that does
not work is the worst thing the example gate can produce, and reaching it took
nothing more than writing the same comparison twice.

The fix is D42: `==` is value equality on every type. Records, tagged unions
and arrays now compare by structure; primitives were already correct, since a
comparison between two known primitives still lowers to `===`. The app never
needed a workaround for this (there was nowhere to route equality through
except the operator itself), so nothing here changed when the fix landed. The
`@example` rows and the plain `==` comparisons throughout `job.glyph` and
`api.glyph` are ordinary code today; before 0.1.66 they were the two halves of
the same bug, disagreeing silently.

**G39, half fixed.** A `sqlite.Row` is `Record<string, unknown>`, so every
column read is a member access against `unknown`. `row.naem` for `row.name`
compiles clean, passes `tsc --strict`, and evaluates to `undefined`, which then
renders as the text `"undefined"` and gets stored. There is no diagnostic at
any stage. A later pass (E0224) closed half of this: reading a key out of a
`Record<K, V>` that is a direct annotation or a module-local alias is now
rejected and points at `record.get`. That check does not reach a named type
that arrives from the stdlib, and `sqlite.Row` is exactly that: it is still
read unchecked, which is the shape that made this app the reproduction case in
the first place.

`store.glyph` carries the workaround, and it needs to stay because the gap is
still open. Every column name is written exactly once, as a `const`
(`C_ID`, `C_KIND`, `C_STATUS`, and so on), and every read goes through
`text_at` or `int_at`, which narrows the cell with a `match ... is string` /
`is number` before anything downstream sees it. A column that comes back as
the wrong shape or is missing produces `None`, which `decode` turns into a
`StoreError` naming the row rather than a stored `"undefined"`. This is
discipline an author has to remember, not a check the compiler makes; removing
it would silently reopen the exact defect the app was built to surface.

## What is deliberately still awkward

The column-name constants and the `text_at`/`int_at` narrowing helpers in
`store.glyph` look like boilerplate a language with a working type system
would not need. They exist because G39 is open for stdlib-sourced `Record`
types specifically. Do not fold them back into direct `row[column]` reads or
replace `text_at`/`int_at` with a cast: that would compile and pass `tsc
--strict` today, and it would also silently reintroduce the "misspelled column
reads as `undefined` and gets stored" failure this app exists to keep visible.
