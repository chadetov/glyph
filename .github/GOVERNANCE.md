# Governance

This document says who decides what, and how, so that anyone betting on Glyph
knows the project is run predictably rather than by whim.

## Model

Glyph currently uses a **BDFL-style model with a small maintainer group**. The
project lead has final say on language design and direction; day-to-day review,
triage, and merges are shared across the maintainers listed in
[`MAINTAINERS.md`](MAINTAINERS.md). This is honest about the project's stage: it
is early and small, and a lightweight model fits that. The intent is to broaden
toward a steering-council model as the contributor base grows (see Succession).

## How decisions are made

- **Small changes** (bug fixes, docs, diagnostics, examples, tooling) are decided
  by normal pull-request review: one maintainer approval and green CI.
- **Language or semantics changes** go through the RFC process
  ([`docs/rfcs/`](../docs/rfcs/README.md)): a written proposal, public
  discussion, and a maintainer decision recorded on the RFC. The four pillars,
  verifiability, greppability, abstraction, diff stability, are the standing
  criteria, and "the constraint is the point" is a valid reason to decline.
- **Scope boundaries** are set by [`docs/manifesto.md`](../docs/manifesto.md) and
  the parked-ideas list in the roadmap. A proposal that reopens an
  explicitly-abandoned direction (the annotation-heavy sketches) starts from a
  presumption of no.

## Resolving disagreement

Most decisions are reached by discussion. When maintainers cannot agree:

1. The proposal is left open for a defined comment period (at least one week) so
   every objection is heard and addressed in writing.
2. If consensus still isn't reached, the project lead makes the call and records
   the reasoning on the issue or RFC.

The bias is toward **not shipping** a contested language change: reversing a
released syntax decision is expensive, so "not yet" is cheaper than "oops."

## Succession and continuity

Continuity is a first-class concern, not an afterthought:

- The repository, the `glyphlang.io` domain, and the npm org are project assets,
  not personal ones; access is held by more than one maintainer so the project
  survives any single person stepping away.
- If the project lead becomes unavailable, the remaining maintainers select a new
  lead by majority vote.
- Both licenses are permissive ([MIT](../LICENSE-MIT) or
  [Apache-2.0](../LICENSE-APACHE)), so the community can always fork and continue
  regardless of who holds the assets, the ultimate continuity guarantee.

## Becoming a maintainer

Maintainers are added by invitation from the existing maintainers, based on a
track record of good-quality contributions and reviews and alignment with the
project's stance. There is no fixed quota; the goal is enough active reviewers
that no single person is a bottleneck.

## Changing this document

Changes to governance are themselves an RFC. This file is versioned in the repo,
so its history is public.
