# jobq

## What it is

A durable job queue: an HTTP API, a SQLite store, and workers. Jobs are
submitted over HTTP, claimed under a lease, run, retried with backoff by pushing
`run_after_ms` forward rather than sleeping, and dead-lettered after a cap. The
point is the guarantee, not the feature list: a job that has been accepted
survives a restart, a worker that dies hands its job back, and no job runs twice
at once.

## Running it

```sh
glyph run examples/apps/jobq/main.glyph serve --port 8080 --db /tmp/jobq.db
glyph run examples/apps/jobq/main.glyph work --db /tmp/jobq.db
glyph run examples/apps/jobq/main.glyph selftest
```

## What it changed in Glyph

Shipped **0.1.66**. Chosen because nothing in the repository had ever run
`http.serve`, and because a queue is a state machine whose correctness is
checkable rather than a matter of taste.

**G65: `==` meant a deep comparison in an `@example` and reference equality in
the program.** Found in the first module written, inside ten minutes: an
assertion passed while the identical expression inside a function was false. A
test reporting success on code that does not work is the worst artifact the
example gate can produce. Fixed as **D42**, value equality on every type. All 959
existing tests passed unchanged and no snapshot moved, which says no test in the
repository had ever compared two aggregates with `==`.

**G39 made concrete**: a `sqlite.Row` is `Record<string, unknown>`, so `row.naem`
compiles clean, passes `tsc --strict`, and evaluates to `undefined`, which then
renders as the text "undefined" and gets stored. Every database read in every
application is that surface.

It also narrowed a new rule rather than only finding bugs: D43's first draft
flagged ordinary local accumulators in this app's async functions, so the rule
now requires the write to go through a parameter and touch a field.

## What it exercises

A five-variant status union where `Dead` is deliberately separate from
`Failed`, because collapsing them into a `failed: bool` is how a queue retries a
poison message ten thousand times. A pure transition function taking `now_ms` as
a parameter rather than reading a clock. Thirty-four `@example` rows.
