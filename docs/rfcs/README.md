# RFCs (requests for comments)

Language and semantics changes go through a written proposal so the reasoning is
public and contestable, rather than decided in a pull request thread. This is
lightweight on purpose; it should feel like writing a good issue, not filing
paperwork.

## When you need an RFC

- **Yes:** any change to syntax, semantics, the type system, a new language
  construct, a new annotation, or a change to what the standard library
  guarantees.
- **No:** bug fixes, diagnostics, docs, examples, tooling, or new stdlib
  functions that fit existing conventions. Just open a pull request.

If you are unsure, open a normal issue first and a maintainer will tell you
whether it needs an RFC.

## The process

1. **Discuss first.** Open a [discussion](https://github.com/chadetov/glyph/discussions)
   or issue describing the problem and rough idea. Cheap to do, saves you writing
   a full RFC for something that is out of scope.
2. **Write the RFC.** Copy [`0000-template.md`](0000-template.md) to
   `docs/rfcs/NNNN-short-title.md` (use the next free number) and fill it in. Open
   a pull request adding it.
3. **Comment period.** The RFC stays open for public comment for at least one
   week. Objections are addressed in writing on the PR.
4. **Decision.** A maintainer accepts, declines, or asks for changes, and records
   the decision and reasoning on the RFC. An accepted RFC is merged; a declined
   one is merged too (marked declined) so the reasoning is preserved for the next
   person who proposes the same thing.

## The standing criteria

Every proposal is judged against the four pillars in
[`../manifesto.md`](../manifesto.md): **verifiability** and **greppability**
first (the wedge), then **abstraction** and **diff stability**. When pillars
conflict, the wedge wins. "Glyph is deliberately stricter than TypeScript, and
the constraint is the point" is a valid reason to decline a proposal that relaxes
a rule to be convenient.

The bias is toward **not shipping** a contested change: reversing released syntax
is expensive, so "not yet" is cheaper than "oops."
