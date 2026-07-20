# Shared state

Most programs need some state that more than one function reads and writes: a
request counter, an in-memory list of records, a cache. In Glyph that state has
one clean home — a **store**.

## The pattern

```glyph
import std/store { create }
import std/array

type Task = {
  id: number,
  title: string,
}

const tasks = create<Array<Task>>([])
const next_id = create(0)

fn add(title: string) -> Task {
  next_id.update(fn(n: number) -> number { return n + 1 })
  let task = { id: next_id.get(), title: title }
  tasks.update(fn(cur: Array<Task>) -> Array<Task> { return array.push(cur, task) })
  return task
}

fn all() -> Array<Task> {
  return tasks.get()
}
```

`create(initial)` returns a `Store<T>`. Read it with `get()`, replace it with
`set(next)`, or map it with `update(change)`. Declare the store as a module-level
`const` and every function in the module shares the same state — no `let`
threaded through `main`, no capturing closures.

An empty collection can't infer its element type, so seed it with an explicit
type argument: `create<Array<Task>>([])`. A store of a scalar (`create(0)`)
infers fine.

## Why a store, and not a mutable module variable

This is a deliberate design choice, not a missing feature. It lands on the four
pillars:

- **Abstraction.** A store is the single, named primitive for shared state. You
  reach for the same shape whether it holds a counter or a table, and a reader
  who sees `create(...)` at module scope knows exactly what they're looking at.
- **Greppability.** Every mutation is a literal `.set(` or `.update(` call.
  `grep 'tasks\.\(set\|update\)'` finds every place the task list changes — the
  whole write-surface of that state, in one search. A bare `mut`-able module
  variable would scatter writes across ordinary assignments that are far harder
  to enumerate.
- **Verifiability, without relaxing a rule.** Glyph keeps `mut` restricted to
  local assignments and method calls (D5), and module-level bindings are `const`
  (D20). A store needs neither loosened: the `const s` binding never changes —
  only the value *inside* the store does, through a method call. So shared state
  arrives with no new language surface, no linear types, and no weakening of the
  mutation rules. It is a library, not a syntax.

The store's internals are the one place a controlled `let` mutation lives, hidden
behind `get`/`set`/`update`. That keeps the mutation in exactly one audited spot
instead of spread across your program.

## Signatures

```
type Store<T>
create<T>(initial: T) -> Store<T>
store.get() -> T
store.set(next: T) -> void
store.update(change: fn(T) -> T) -> void
```
