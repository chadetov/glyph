# Glyph in five minutes

Glyph is a statically typed language that compiles to TypeScript. It looks
almost like TypeScript, so you can read it on day one. The differences are few
and deliberate: each one exists to make code an AI agent (or a human) can reason
about, edit without breaking, and explain without lying.

This is the whole language in one read.

## A function

```glyph
module greet

fn hello(name: string) -> string {
  return "hello, ${name}"
}
```

Every file starts with `module <path>`. Functions declare their parameter and
return types. String interpolation is `"${...}"`. There is exactly one way to
declare a function (`fn name(...)`), so `grep "fn hello"` always finds it.

## Records, not loose objects

```glyph
type User = {
  id: string,
  name: string,
}

fn name_of(u: User) -> string {
  return u.name
}
```

Trailing commas are required (a new field is a one-line diff). There is no
object-literal shorthand: you write `{ id: id, name: name }`, never `{ id, name }`,
so the value at a key is always visible.

## Errors are values

There are no exceptions. Fallible functions return `Result<T, E>`, and you
handle both cases with `match`:

```glyph
import std/result { Result, Ok, Err }

fn half(n: number) -> Result<number, string> {
  return match n % 2 {
    0 => Ok(n / 2),
    else => Err("not even"),
  }
}
```

`match` must cover every case. Forget one and the compiler tells you which.
`else` is the catch-all, allowed only as a whole match arm.

## The `?` operator

When you just want to propagate an error upward, `?` unwraps an `Ok` or returns
the `Err` from the enclosing function:

```glyph
fn quarter(n: number) -> Result<number, string> {
  let h = half(n)?
  return half(h)
}
```

`?` only works inside a function that itself returns `Result`, and the error
types must match — no hidden conversions.

## Tagged unions and exhaustive matching

```glyph
type Shape =
  | Circle({ radius: number })
  | Rectangle({ width: number, height: number })

fn area(s: Shape) -> number {
  return match s {
    Circle({ radius }) => 3.14159 * radius * radius,
    Rectangle({ width, height }) => width * height,
  }
}
```

Add a `Triangle` variant and every non-exhaustive `match` becomes a compile
error pointing at the missing case. This is the verifiability pillar: what the
type says is true when the code runs.

## Loops and mutation

There is no `if`/`else` statement — use `match` for branching. Mutation is
explicit and narrow: `let` binds, `mut` reassigns, and `mut` is the only way to
change a binding.

```glyph
fn total(xs: Array<number>) -> number {
  let acc = 0
  for x in xs {
    mut acc = (acc + x)
  }
  return acc
}
```

## Generics and higher-order functions

```glyph
type Maybe<T> =
  | Just({ value: T })
  | Nothing

fn apply_twice(f: fn(n: number) -> number, x: number) -> number {
  return f(f(x))
}

fn demo() -> number {
  return apply_twice(fn(n) { n + 1 }, 10)
}
```

Lambdas are `fn(params) { body }`; the last expression is the return value.

## What you get for the restrictions

- **No `any`.** What the types claim is checked, including at the boundary where
  untyped data enters (`json.parse` validates against a schema).
- **One name, one form.** Every symbol has a single declaration syntax, so
  search is exact.
- **Stable diffs.** Fixed-width, one-element-per-line formatting (`glyph fmt`):
  a one-line change is a one-line diff.
- **Real TypeScript out.** `glyph build` emits `.ts`, type-checked with `tsc
  --strict`. It runs anywhere TS runs and uses any npm package.

## Next

- Install it and run your first program: [`getting-started.md`](getting-started.md).
- Coming from TypeScript? The deltas and gotchas:
  [`for-typescript-developers.md`](for-typescript-developers.md).
- Build something real in 30 minutes: [`tutorial.md`](tutorial.md).
- The full language reference: [`../language/spec.md`](../language/spec.md).
