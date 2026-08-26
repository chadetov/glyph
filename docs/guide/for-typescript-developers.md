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
| `class` / data `interface` | `type` records + tagged unions | One shape syntax; behavior lives in functions |
| `interface` as a constraint | `interface` (structural, for `<T: Bound>`) | A generic can require capability of its parameter |
| `export` by default | private by default, `pub` to export | The public API is `grep '^pub'`; a helper stays internal |
| `try`/`finally` cleanup | `defer expr` | Cleanup runs on every exit path, greppable |
| backtick templates | `"${expr}"` in normal strings, newlines included | Two forms: `"..."` decodes escapes, `"""..."""` is raw; both interpolate |

The rest of this page expands each.

## Concept map (your word → our word)

A quick lookup when you know what you want in TypeScript and need the Glyph name:

| You want (TypeScript) | Glyph |
|---|---|
| `interface`/`type` for data | `type` record (`type User = { name: string }`) |
| `interface` as a constraint | `interface` used as a bound (`<T: Bound>`) |
| `enum` | tagged union, or a string-literal union (`"a" \| "b"`) |
| `number` for an integer | `int` (validated whole number at the boundary) |
| `T \| null` / `T \| undefined` | `Option<T>` (`Some`/`None`) |
| `throw` / `try`/`catch` | `Result<T, E>` + `match` / `?` |
| `if`/`else`/`switch` | `match` |
| `x.map(f)` (method) | `array.map(x, f)` (module function) |
| `class` with methods | a `type` for data + `fn`s for behavior |
| `readonly` / `const` everywhere | `let` is the default; `mut` to reassign |
| `try`/`finally` | `defer expr` |
| `Promise.all` | `task.all([...])` |
| `z.infer<typeof s>` | `typeof` (`type U = z.infer<typeof s>`) |
| `export` | `pub` (module-private is the default) |
| a raw TS type you can't spell | `extern_ts("...")` (rare escape hatch) |

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

## The prelude: a few names need no import

Most things require an explicit import, but a small prelude is always in scope
(the runtime installs it):

- Values: `number` (`number.to_string`, `number.parse`), `par` (`par.all`,
  `par.all_ok`), `print`, `assert`.
- Types: `number`, `string`, `bool`, `void`, `Array<T>`, `Record<K, V>`, and the
  ambient `Schema<T>` / `Issue`.

Note that `Result`/`Ok`/`Err` and `Option`/`Some`/`None` are **not** prelude —
import them (`import std/result { Result, Ok, Err }`). Also note the boolean type
is spelled **`bool`**, not `boolean`.

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

A payload field can carry a pattern of its own rather than just a name, so an
arm can recognise a shape several levels down: `Click({ x: 0, y })` matches only
a click on the left edge, and `Node({ color: Black, left: Node({ color: Red,
value: v }) })` is one arm of a red-black rebalance. A field that tests a value
can fail, which means the arm no longer counts as covering its variant: if your
only `Click` arm tests `x`, the match is non-exhaustive until another `Click` arm
or an `else` takes the rest. The same holds when there are no variants at all:
`match p { { x: 0, y: y, } => .. }` over a plain record is E0226 until you add an
`else`.

The union does not have to be declared in the file you are matching in. An
imported one behaves the same way, under any of the three import spellings.

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
(`{ x: x, y: y }`). A key that is not an identifier is written quoted, as in
TypeScript (`{ "Content-Type": v }`); an identifier key stays bareword. Trailing
commas are required on every multi-element list, so inserting an element touches
exactly one line.

## The tail expression is the return value

A non-`void` function, lambda, or block evaluates to its final expression, so an
explicit `return` is optional. These are equivalent:

```glyph
fn double(n: number) -> number { n * 2 }
fn double(n: number) -> number { return n * 2 }
```

`return` is **not** mandatory — use whichever reads better. (Most examples here
use explicit `return` for clarity; closures usually drop it.)

## No classes; behavior lives in functions

Glyph has no `class` and no methods on your own types (in v1). Data is records
and tagged unions; behavior is functions, often namespaced through a module:

```glyph
import std/array

fn evens(xs: Array<number>) -> Array<number> {
  return array.filter(xs, fn(n) { n % 2 == 0 })
}
```

## Visibility: private by default, `pub` to export

A top-level declaration is visible only inside its module unless you mark it
`pub`. `pub` sits just before the keyword (`pub fn`, `pub type`, `pub interface`,
`pub const`). Importing a name another module did not make `pub` is an error at
the import site, so the public surface is exactly `grep '^pub'`. `fn main` is
always exported, so a single-file program needs no `pub`.

```glyph
pub fn public_api() -> int { helper() }

fn helper() -> int { 42 }   // module-private; importing it elsewhere is an error
```

## Interfaces are structural, and mainly for bounds

Glyph's `interface` is not a class contract you `implements`; it is a structural
set of member signatures, like a TypeScript `interface`, whose main job is to
constrain a generic. Any value with the members satisfies it, no declaration
needed.

```glyph
interface Named {
  fn name() -> string
}

pub fn label<T: Named>(x: T) -> string {
  return x.name()
}
```

## `defer` for cleanup instead of `try`/`finally`

`defer expr` runs its expression when the enclosing block exits, on every path
(normal, `return`, or a thrown error). It composes with `owned` handles, and
multiple defers run last-in-first-out.

```glyph
fn read_config() -> string {
  defer file.close()
  return file.read_all()   // close() runs after this, on the way out
}
```

## Concurrency joins with `std/task`

`import std/task` gives `all` (run task thunks concurrently, join in order),
`race`, and `all_settled`, over the same `async`/`await` you know.

```glyph
let results = await task.all([fn() { fetch_a() }, fn() { fetch_b() }])
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

`glyph fmt` has one layout: two-space indent, trailing commas, and a list that
stays on one line while it fits inside 100 columns and goes one element per line
when it doesn't. There are no options. The point is diff stability: everyone's
files look identical, so a semantic one-line change is a one-line diff. The LSP
runs it on save.

The width rule is all-or-nothing. Glyph never repacks a list to fill the line,
so a list is either entirely inline or entirely one-per-line, and inserting an
element touches one line. What it does cost you is the threshold itself: rename
a binding so a call crosses 100 columns and the call expands to one argument per
line, which is a four-line diff from a one-token edit.

A long `&&`, `||`, or `??` condition breaks the same way, one operand per line
with the operator leading:

```
fn item_matches(item: Item, noun: string) -> bool {
  return item.id == noun
    || item.name == noun
    || string.contains(item.name, noun)
}
```

The operator goes first so it stays at a fixed column, and only the loosest
operator in the expression breaks: `a && b || c && d` breaks at `||` and leaves
each `&&` pair on its line. A condition that is not inside a `{ ... }` block
stays on one line however long it is, because D1 ends a statement at a newline
outside brackets, so breaking a module-level `const` would change the program.

Comments stay where you put them. A `//` written inside a record body, a union
variant list, an array or object literal, an argument list, or above a `match`
arm is re-emitted above the same item, and a construct holding an interior
comment stays one-element-per-line so the comment has something to sit above.
So `type Shape = { w: int, h: int }` collapses to one line, but the moment you
document `h` the record stays expanded. One placement rule to know: a comment
always lands on its own line, so `w: int, // width in cells` moves to the line
above `h`. Prefer writing the comment on its own line to begin with and the file
is already formatted.

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

That one command builds the whole tree. Each directory under `examples/apps/` is
its own program whose modules import each other by bare name, and each carries a
`package.json` with a `"glyph"` key. That marker makes the directory its own
module-resolution root (D41), so a build over any enclosing tree compiles it in
its own root and its bare-name imports resolve exactly as they do when you build
the app on its own. See `examples/README.md`.

The async-errors example is the best one to start with: the manual `Promise`
error handling and discriminated-union plumbing that Glyph generates is exactly
the boilerplate the language saves you from writing by hand.
