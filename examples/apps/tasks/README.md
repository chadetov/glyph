# tasks

## What it is

A persisted task API over a real SQLite database. `std/sqlite` backs a table;
the HTTP surface lists, creates and toggles. The point of the app is the two-type
split at the storage boundary: SQLite has no boolean, so rows validate into a
`TaskRow` with an int and map to a domain `Task` with a bool in one visible line.
Data survives a restart.

## Running it

```sh
glyph run examples/apps/tasks/main.glyph
```

## What it changed in Glyph

**It carries no gap number, and it is the demonstration half of a release
rather than a probe.** Shipped as part of **0.1.25**, whose point was evidence
for the 1.0 gate: build a real, persisted, validated HTTP service and see what
the language does and does not do well. It went green with the boundary catching
a genuine bug on the first try.

What it demonstrates is the storage and domain split a database boundary forces.
A `done` column comes back as an integer, and a silent `row as Task` cast would
leave `task.done === true` never true. The two types and the one mapping line are
the alternative.

It was later used as the difficulty baseline for a harder trip: raw-body HMAC
verification, a recursive rule engine, and bounded-concurrency dispatch were
scoped as "like tasks, but harder".

## What it exercises

`std/sqlite` with rows as `Record<string, unknown>` validated by a descriptor
rather than cast, nested `match` routing, and a handler closure passed to
`listen`. No unions, no generics: it is deliberately the plain case.
