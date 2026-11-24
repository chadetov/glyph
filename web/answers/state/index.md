<!-- Markdown copy of https://glyphlang.io/answers/state/, generated from the same source as the page. Last changed 2026-11-21. -->

14 · Shared state

# How do I share mutable state between functions?

With a **store**. Glyph has no mutable global variables. Module bindings are `const`. Shared state lives in a `std/store`: a module-level `const` holding a value you read with `get()` and change with `set()`/`update()`. The binding never moves; only the value inside it does, through a method call, so every write is a literal `.set(` or `.update(` you can grep for.

## See it

Declare the store once at module scope. Every function in the module shares it. No `let` threaded through `main`, no capturing closures:

tasks.glyph

```
import std/store { create }
import std/array

type Task = {
  id: number,
  title: string,
}

const tasks = create<Array<Task>>([])
const next_id = create(0)

fn add(title: string) -> Task {
  next_id.update(fn(n: number) -> number {
    return n + 1
  })
  let task = { id: next_id.get(), title: title }
  tasks.update(fn(cur: Array<Task>) -> Array<Task> {
    return array.push(cur, task)
  })
  return task
}

fn all() -> Array<Task> {
  return tasks.get()
}
```

the whole write-surface, in one grep

```
$ grep -rn 'tasks\.\(set\|update\)' src/
src/tasks.glyph:16:  tasks.update(fn(cur...

# every place the task list can change,
# enumerated. nothing hides in an
# ordinary `x = ...` assignment
# somewhere else in the file.
```

## Why a store, and not a mutable global

This is a deliberate design choice. Glyph keeps `mut` restricted to local assignments and module bindings `const`, because a scattered set of writable globals is exactly what makes state hard to reason about and hard to *find*.

- **The binding never moves** A store needs neither rule loosened: the `const s` binding is immutable, and the value inside changes only through `set`/`update`.
- **A library, not syntax** So shared state arrives as a **library, not new syntax** (no linear types, no weakened mutation rules), and the mutation lives in exactly one audited place instead of spread across your program.

One sharp edge, shared with TypeScript: an empty collection can't infer its element type, so seed it with an explicit type argument: `create<Array<Task>>([])`. A scalar store (`create(0)`) infers fine.

## Where it stands

### Shipping today

- `std/store` with `create`, `get`, `set`, and `update`: the single, greppable home for shared mutable state.

### On the way

- Convenience over the same primitive (derived/computed stores, subscriptions), layered on top without changing the language.

Code examples on this page are checked against Glyph 0.1.182 on every build. Last changed 21 Nov 2026.
