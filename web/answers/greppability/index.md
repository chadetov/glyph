<!-- Markdown copy of https://glyphlang.io/answers/greppability/, generated from the same source as the page. Last changed 2026-11-21. -->

03 · Navigation by search

# Can an agent (or I) find where anything is defined?

Every construct has **exactly one shape** to search for. A plain `grep` (the way agents actually navigate) lands on the one definition, **not fifty near-misses**.

grep -rn "mut " src/ (every mutation in the codebase)

```
src/cart.glyph:14:   mut items.push(item)
src/cart.glyph:22:   mut total = total + price
src/user.glyph:8:    mut session.token = fresh
```

Glyph restricts mutation to two forms (`mut x = …` and `mut x.method(…)`), so *every* place state changes answers to a single search. Three mutations, all of them, in one search. The compiler refuses an array write that skips the `mut`.

## Why it matters

An agent doesn't hold your whole repo in its head; it **searches**. One shape per concept means search is complete, so the agent edits what's there instead of duplicating it.

If a concept can be written five different ways, search misses cases, and a confident agent that can't find the existing definition invents a second one.

**Details why the same question has no answer in TypeScript **

In TypeScript a mutation can hide behind `=`, `.push()`, `Object.assign`, a setter, or a spread. There's no single token to grep, so “does anything change this?” has no reliable answer.

## The compiler keeps the search complete

A method that changes its array (`push`, `pop`, `sort`, `splice` and five more) only stands as the whole call of a `mut` statement. Anywhere else it is `E0240`, including inside an expression, where a helper could sort its caller's array while reading one element:

glyph check

```
[E0240] `xs.sort(cmp)` changes `xs` in place, and its value is used here
 4 │   return xs.sort(cmp)[0]
   │ Help: `array.sort(xs, cmp)` returns a sorted copy and leaves `xs` alone
```

`xs.push(4)` on a line of its own gets the same code, and `glyph fix` writes the `mut` for you. The other direction is `E0241`: `mut xs.concat(ys)` changes nothing and drops the new array, so the audit never lists a line that writes nothing.

## Whole categories in one search

The same one-shape rule makes whole categories auditable:

one search = one complete answer

```
# every UI component
grep -rn "^component " src/

# every function
grep -rn "^fn " src/

# the entire resource-ownership surface
grep -rn "owned" src/
```

There's no object-literal shorthand, no second way to declare a component, no overloaded syntax to disambiguate. Fewer ways to write a thing is a real constraint, and it's the point: it's what makes an agent's search, and yours, trustworthy.

The constructs Glyph leaves out so that a search can be trusted, and why a lint rule is not the same guarantee, are on the [greppability pillar page](https://glyphlang.io/pillars/greppability/).

## Where it stands

### Shipping today

- One canonical form per construct.
- Mutation restricted to two greppable shapes (`mut`), and an array write without one refused (`E0240`).
- Single-keyword audits (`fn`, `component`, `owned`, `type`).

### Layered on top

- Writes through the stdlib `Set`, `Store` and `Deque` are not checked for `mut` yet

  **Details**

  Glyph's checker does not type those receivers yet, so `seen.add(x)` and `mut seen.add(x)` are both accepted, and the corpus uses both spellings. Typing them and marking their writing methods is the next planned step. A value from an npm package is not judged either way: `mut` before one of its calls is allowed and not required.

- The language server adds workspace-wide go-to-definition and find-references: the same guarantee, now interactive in your editor.

Code examples on this page are checked against Glyph 0.1.160 on every build. Last changed 21 Nov 2026.
