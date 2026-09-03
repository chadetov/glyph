# resilient

## What it is

A resilience policy library for HTTP calls (retry with exponential backoff and
full jitter, a circuit breaker, failure classification, per-attempt and
whole-call deadlines, concurrency limiting) plus a flaky server it starts in the
same process to demonstrate against. Five scenarios: a call that fails twice and
recovers, a 401 that must not be retried, an upstream that stays down until the
breaker gives up, a twelve-request fan-out held to four in flight, and a deadline
firing mid-request.

## Running it

```sh
glyph run examples/apps/resilient
```

## What it changed in Glyph

Its own round produced no gap attributed to it by name. What it did instead is
become evidence in three later releases, which is a different kind of
contribution and worth recording as such.

**0.1.80**: `http.serve` was deleted rather than kept, partly because this app
was already working around the missing bind signal by ignoring the return value
and sleeping 150ms. It now awaits `listen`, which resolves when the port is
actually bound.

**0.1.81 / G99**: this app's source disproved a premise the roadmap had carried
for eight releases. The claim was that the type of an async thunk could not be
named; D40 had already named it, and `main.glyph:43` had been spelling it for
releases. The mechanism by which that survived is worth more than the gap: the
typechecker carried twenty lines of doc comment describing three functions as
modeled and describing the bug in the past tense, and none of the three was
implemented. Anyone who checked read the comment and stopped.

**0.1.85 / G127**: its `with_deadline` is cited as the idiom that cannot bound a
TLS dial, and the app's own comment says the same thing about itself. It exists
to show the difference between aborting a request and abandoning one.

## What it exercises

The heaviest `@example` user in the tree at 51, covering an entire state
machine with no network and no waiting. Tagged unions modelling state and
decisions, so fields that only make sense in one state cannot be read in the
others, and a gate type that hands back the decision and the state change
together so a caller cannot take one without the other.
