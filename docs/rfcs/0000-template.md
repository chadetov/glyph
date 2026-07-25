# RFC NNNN: <short title>

- **Status:** draft | in comment | accepted | declined
- **Author(s):** <your name / handle>
- **Discussion:** <link to the issue or discussion>
- **Date:** <YYYY-MM-DD>

## Summary

One paragraph: what this proposes, in plain terms.

## Motivation

What problem does this solve? Who hits it, and how often? A concrete example of
the pain is worth more than an abstract argument. If there is a workaround today,
say what it is and why it is not enough.

## Which pillar does it serve?

Name the pillar (verifiability, greppability, abstraction, diff stability) this
serves, and be honest about which pillars it costs. When they conflict, explain
why the wedge (verifiability/greppability) still wins.

## Design

The actual proposal. Grammar, semantics, and how it lowers to TypeScript. Show
Glyph source and the emitted `.ts` side by side. Cover the common case first,
then the edges.

## What it deliberately does not do

The scope you are ruling out, and why. This is as important as the design.

## Alternatives considered

Other shapes you weighed, and why this one won. Include "do nothing" and say what
that costs.

## Drawbacks and open questions

What this makes worse, and what you are unsure about. Naming a weakness early
earns trust and speeds review.

## Migration

If this changes existing code, how does a user's code move forward? Is there an
automated path (a codemod, a diagnostic), or is it a breaking change gated on a
version?
