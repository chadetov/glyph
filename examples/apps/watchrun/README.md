# watchrun

## What it is

A dev-loop tool. It polls a directory's file mtimes, filters candidates through
include and ignore globs evaluated by `minimatch`, diffs successive snapshots,
debounces a burst into one run, then spawns the configured command as a real
child process. Child output streams into a log as chunks arrive, a watchdog
SIGKILLs an overrun, and each run prints one PASS, FAIL or TIMEOUT line with exit
code and duration.

## Running it

```sh
glyph run examples/apps/watchrun/main.glyph -- <dir> --cmd "<command>" \
  [--include "glob,glob"] [--debounce ms] [--timeout ms] [--log path]
```

## What it changed in Glyph

**This app blocked on the same wall more than once, and that repetition is its
most useful finding.** Two gaps are named, and both are the same bug: the bundled
Node shim being narrower than Node itself.

**G125 (0.1.83): installing `@types/node` broke every build, on the compiler's
own runtime.** A file containing only `pub fn main() -> number { return 0 }`
produced four errors, all in files the user did not write.

**G126 (0.1.84): the bundled shim had only the blocking half of
`child_process`.** A dev-loop tool runs a long-running command and reports output
while it runs, which is `spawn`; the two the shim declared both block until the
child is done and hand back everything at once, so neither can do it. The failure
named the wrong thing twice over.

It finally built with no workaround in **0.1.88**.

One honest discrepancy in our own records: the release notes say this app blocked
**twice** and name G125 and G126, while the Q46 write-up says **three separate
times**. There is no third gap ID anywhere in the repo. Treat it as two named
gaps and a third rediscovery recorded only as a count. The reason it matters is
the point Q46 makes: nothing told the second agent the wall was already known.

## What it exercises

A Node builtin imported directly with no adapter, an npm package whose own
types are checked at every call site, `std/timers` driving both the poll loop and
the debounce, nested closures reassigning outer `mut` state, and `is`-narrowing
on untyped stream data.
