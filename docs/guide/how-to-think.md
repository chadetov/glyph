# How to think in Glyph

The fastest way to be unproductive in Glyph is to write TypeScript and translate
it line by line. The syntax is close enough that this almost works, and then you
hit `if` and get stuck. This page is the mental model that makes the rest feel
obvious instead of restrictive.

## One shape per idea

TypeScript gives you five ways to say most things. Glyph gives you one, on
purpose. There is one way to branch (`match`), one way to bind (`let`), one way
to reassign (`mut`), one way to declare data (`type`), one string syntax, one
formatter layout. The constraint is the feature: when there is one shape, a
`grep` finds every occurrence, a diff shows the real change, and an agent that
searches your code finds what exists instead of inventing a second copy.

So when you reach for a second way to do something, that is the signal you are
still thinking in TypeScript. There isn't one. Use the shape that exists.

## Errors are data, control flow is a value

Nothing throws. A function that can fail returns `Result<T, E>`, and its type
signature is the honest list of what can go wrong. You don't wrap calls in
`try`; you `match` on the result, or you write `?` to pass the error up. This
means the compiler can see every failure path, and so can you.

`match` is not a statement you run for effect. It is an expression that produces
a value, and the compiler checks that you covered every case. Branch by
returning the value of a `match`, not by mutating a variable inside an `if`.

## Data is inert; behavior is functions

There are no classes and no methods on your own types. A `type` is a record or a
tagged union, and that's all it is: a shape. Behavior lives in functions,
usually reached through a module (`array.filter(xs, f)`, not `xs.filter(f)`).
Once you stop looking for a place to hang methods, the code organizes itself:
data in `type`s, transforms in `fn`s.

## Make illegal states unrepresentable

Tagged unions plus exhaustive `match` are the whole toolkit. Instead of a
`User` with a nullable `subscription` and a boolean `isActive` that can disagree,
model the states directly (`Free | Trialing({...}) | Paid({...})`) and let the
compiler force you to handle each. The payoff is that a bug becomes a compile
error: add a state, and every `match` that forgot it stops compiling.

## Mutation is rare and visible

Default to `let` and immutable data. When you genuinely need to change something,
`mut` says so, and it is the one token to search for. If a function threads a
`let` through everything to accumulate state, reach for `std/store` instead. The
point isn't purity for its own sake; it's that "what changes here" always has a
one-search answer.

## When a rule annoys you

It will, early. No `if`, required trailing commas, no object shorthand. Each of
those earns its place against one of the four pillars (verifiability,
greppability, abstraction, diff stability). The answer to an annoying rule is to
learn why it's there, not to look for a way around it, because there usually
isn't one, and that predictability is the thing you're actually buying.

Next: [Glyph for TypeScript developers](for-typescript-developers.md) for the
line-by-line mapping, or the [cookbook](cookbook.md) for task recipes.
