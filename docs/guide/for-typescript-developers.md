# Glyph for TypeScript developers

You already know 90% of Glyph. It has the same primitive types, the same
function-call syntax, generics with `<>`, `async`/`await`, and JSX. It compiles
to TypeScript you can read.

This page is the other 10%: what is different, and why. Every difference earns
its place against one of the four pillars (verifiability, greppability,
abstraction, diff stability). When a restriction feels annoying, the reason is
in the right-hand column.

## The deltas at a glance

| TypeScript | Glyph | Why |
|---|---|---|
| `if (c) { } else { }` | `match` | One branching construct; every branch is a value, every case is checked |
| `throw` / `try`/`catch` | `Result<T, E>` + `match` / `?` | Errors are values; the type signature tells you what can fail |
| `{ name }` shorthand | `{ name: name }` | The value at every key is visible; no hidden bindings |
| optional trailing comma | required trailing comma | Adding a field is a one-line diff |
| `let`/`const`/`var` + free reassignment | `let` to bind, `mut` to reassign | Mutation is greppable and intentional |
| `enum` / union of string literals | tagged unions (`\| Variant({...})`) | Exhaustive matching with payloads |
| barrel files (`index.ts` re-exports) | full-path imports only | `grep` finds the real definition, not a re-export |
| `any` | (does not exist) | What the type claims is true at runtime |
| `interface` / `class` | `type` records + tagged unions | One shape syntax; behavior lives in functions |
| backtick templates | `"${expr}"` in normal strings | One string syntax |

The rest of this page expands each.

## Files start with `module`

```glyph
module billing/invoice
```

The module path mirrors the file path (`billing/invoice.glyph`). Imports use the
full path; there are no barrel files and no `index.ts` indirection.

```glyph
import std/result { Result, Ok, Err }
import std/array
import billing/customer { Customer }
```

A named import (`{ Ok, Err }`) brings names into scope; a bare import
(`std/array`) is used namespaced (`array.map(...)`).

## No `if` statement — `match` is the only branch

`match` is an expression, so every branch produces a value and the compiler
checks that you covered every case.

```ts
// TypeScript
function sign(n: number): string {
  if (n > 0) return "positive";
  else if (n < 0) return "negative";
  else return "zero";
}
```

```glyph
// Glyph
fn sign(n: number) -> string {
  return match n {
    0 => "zero",
    else => match n > 0 {
      true => "positive",
      false => "negative",
    },
  }
}
```

`match` works on numbers, strings, booleans, arrays (`[]`, `[head, ...rest]`,
`[a, b]`), and tagged unions. `else` is the catch-all and is legal only as a
whole arm.

## Errors are values, not exceptions

There is no `throw`. A function that can fail returns `Result<T, E>`:

```glyph
import std/result { Result, Ok, Err }

fn parse_port(s: string) -> Result<number, string> {
  return match number.parse(s) {
    Ok(n) => match n >= 0 {
      true => Ok(n),
      false => Err("port must be non-negative"),
    },
    Err(_) => Err("not a number: ${s}"),
  }
}
```

To propagate an error without handling it here, use `?`. It unwraps `Ok` or
returns the `Err` from the enclosing function:

```glyph
fn connect(port_text: string) -> Result<Connection, string> {
  let port = parse_port(port_text)?
  return open(port)
}
```

Two rules keep `?` honest: it is only allowed inside a function whose return
type is `Result`, and the error type it propagates must match the enclosing
function's error type exactly. There is no implicit `From` conversion in v1.

## Tagged unions instead of `enum`

```glyph
type Event =
  | Click({ x: number, y: number })
  | KeyPress({ key: string })
  | Close
```

Match a union and the compiler forces every variant (and lets you destructure
the payload):

```glyph
fn describe(e: Event) -> string {
  return match e {
    Click({ x, y }) => "click at ${number.to_string(x)},${number.to_string(y)}",
    KeyPress({ key }) => "key ${key}",
    Close => "close",
  }
}
```

Add a variant later and every non-exhaustive `match` is a compile error that
names the missing case. That is the payoff: the compiler maintains your switch
statements for you.

## `let` binds, `mut` reassigns

`let` introduces an immutable binding. Reassignment requires `mut` — it is the
only form that changes a binding, so every mutation is greppable.

```glyph
fn running_max(xs: Array<number>) -> number {
  let best = 0
  for x in xs {
    mut best = match x > best {
      true => x,
      false => best,
    }
  }
  return best
}
```

`mut` is restricted to assignments and method calls; you cannot use it to
declare.

## Records, no shorthand, trailing commas

```glyph
type Point = { x: number, y: number }

fn shift(p: Point, dx: number) -> Point {
  return { x: p.x + dx, y: p.y }
}
```

Object-literal shorthand (`{ x, y }`) does not exist: you always write the value
(`{ x: x, y: y }`). Trailing commas are required on every multi-element list, so
inserting an element touches exactly one line.

## No classes; behavior lives in functions

Glyph has no `class` and no methods on your own types (in v1). Data is records
and tagged unions; behavior is functions, often namespaced through a module:

```glyph
import std/array

fn evens(xs: Array<number>) -> Array<number> {
  return array.filter(xs, fn(n) { n % 2 == 0 })
}
```

## Async is the same, with one nicety

`async`/`await` work as you expect, and `?` composes with `await`:

```glyph
async fn load(url: string) -> Result<string, string> {
  let response = await http.get(url).map_err(fn(e) { e.message })?
  return Ok(response.body)
}
```

## Formatting is fixed, not configurable

`glyph fmt` has one layout: two-space indent, trailing commas, one element per
line once a list has more than two elements, no line-length reflow. There are no
options. The point is diff stability — everyone's files look identical, so a
semantic one-line change is a one-line diff. The LSP runs it on save.

## What is deliberately missing in v1

- No `if`/`else`, no ternary, no `switch` (use `match`).
- No `any`, no non-null assertion `!`, no `as` casts in source.
- No classes, no `this`, no methods on user types.
- No object/array-destructuring shorthand beyond what patterns provide.
- No barrel files / re-export indirection.
- Resource handles can use a narrow `owned` modifier (files/sockets/db
  connections) for single-consumption; that is the only affine-typing feature
  and it is not a general borrow checker.

## Try it against TypeScript

The fastest way to internalize the mapping is to read a Glyph file and the TS it
produces side by side:

```sh
glyph build examples --out /tmp/out
# compare examples/02_async_errors.glyph with /tmp/out/user_feed.ts
```

The async-errors example is the best one to start with: the manual `Promise`
error handling and discriminated-union plumbing that Glyph generates is exactly
the boilerplate the language saves you from writing by hand.
