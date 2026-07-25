# Anti-patterns

Habits that compile but fight the language. Each has a shorter, more idiomatic
form that the four pillars reward.

## Writing TypeScript and translating

The most common one. If you find yourself searching for `if`, a place to hang a
method, or object shorthand, stop and read [how to think in
Glyph](how-to-think.md). The syntax is close enough to mislead you; the model is
different.

## A `mut` where a `match` value would do

```glyph
// Avoid: mutate a placeholder across branches
let label = ""
mut label = classify(x)   // and then more mut...

// Prefer: the branch is the value
let label = match kind(x) {
  Big => "big",
  Small => "small",
}
```

Mutation should be rare and meaningful. A `mut` used only to simulate an `if`
expression is noise the reader has to trace.

## Booleans that encode a state machine

```glyph
// Avoid: flags that can contradict each other
type Order = { paid: bool, shipped: bool, cancelled: bool }

// Prefer: name the states, make the illegal ones unrepresentable
type Order =
  | Pending
  | Paid({ at: string })
  | Shipped({ tracking: string })
  | Cancelled({ reason: string })
```

Three booleans is eight states, most of them nonsense. A tagged union is exactly
the states that exist, and `match` forces you to handle each.

## An `else` that hides a missing case

```glyph
// Avoid: else swallows variants you forgot to handle
match event {
  Click(p) => handle_click(p),
  else => ignore(),
}
```

A catch-all `else` on a tagged union defeats exhaustiveness: add a variant and
nothing tells you this `match` ignored it. Prefer listing the cases so a new
variant is a compile error, and reserve `else` for genuinely open domains
(`number`, `string`).

## Threading state through every signature

If half your functions take and return an accumulator just to share one value,
you are hand-rolling state. Use `std/store`: a module-level `const s =
store.create(...)` gives many functions one shared value, mutated through a
greppable `s.set`/`s.update`, with no `let` snaking through `main`.

## Reaching for `extern_ts` too early

`extern_ts("...")` is the escape hatch for an idiom Glyph can't spell. It is
deliberately a little awkward because it opts out of Glyph's own checking. Before
using it, check whether the thing is expressible: a value-derived type is
`typeof` (`type U = z.infer<typeof s>`), a schema you own is `glyph gen zod`, a
package's types are `glyph gen dts`. Save `extern_ts` for the genuine rare case.

## Ignoring a `Result`

```glyph
// Avoid: the Err is silently dropped (E0217 warns)
save(record)
next()

// Prefer: handle or propagate
save(record)?
next()
```

A dropped `Result` is a swallowed error. Propagate it with `?`, or `match` it.

## `@open` on everything

Records are strict by default: a value with extra keys is rejected at the
boundary, which is the point (mass-assignment protection). `@open` opts out for a
forward-compatible wire type that may gain fields. Use it where the data really
is open, not to silence a validation you didn't want to think about.
