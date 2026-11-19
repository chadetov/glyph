<!-- Markdown copy of https://glyphlang.io/answers/greppability/, generated from the same source as the page. Last changed 2026-11-19. -->

03 · Navigation by search

# Can an agent (or I) find where anything is defined?

Every construct has **exactly one shape** to search for. A plain `grep` (the way agents actually navigate) lands on the one definition, **not fifty near-misses**.

## Why it matters

An agent doesn't hold your whole repo in its head; it **searches**. If a concept can be written five different ways, search misses cases, and a confident agent that can't find the existing definition invents a second one. One shape per concept means search is complete, so the agent edits what's there instead of duplicating it.

## See it

Glyph restricts mutation to two forms (`mut x = …` and `mut x.method(…)`), so *every* place state changes answers to a single search:

grep -rn "mut " src/ (every mutation in the codebase)

```
src/cart.glyph:14:   mut items.push(item)
src/cart.glyph:22:   mut total = total + price
src/user.glyph:8:    mut session.token = fresh
```

Three mutations, all of them, in one search. In TypeScript a mutation can hide behind `=`, `.push()`, `Object.assign`, a setter, or a spread. There's no single token to grep, so “does anything change this?” has no reliable answer. The same one-shape rule makes whole categories auditable:

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

One canonical form per construct, mutation restricted to two greppable shapes (`mut`), and single-keyword audits (`fn`, `component`, `owned`, `type`).

### Layered on top

The language server adds workspace-wide go-to-definition and find-references: the same guarantee, now interactive in your editor.

Code examples on this page are checked against Glyph 0.1.149 on every build. Last changed 19 Nov 2026.
