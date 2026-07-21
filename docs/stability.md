# Stability policy (pre-1.0)

Glyph is an early preview. This page states plainly what may change, what won't,
and how we try to keep an upgrade from costing you a rewrite. It is a policy of
intent, not a contract — until 1.0 we favor getting the language right over
freezing it early.

## What may change between 0.1.x releases

- **Syntax and semantics.** A construct may be renamed, tightened, or replaced if
  it earns its keep against the four pillars. New diagnostics may reject code that
  previously compiled (that is usually the point — catching a latent bug).
- **The standard library surface.** Functions may be added, renamed, or moved
  between modules.
- **Generated TypeScript shape.** The exact emitted `.ts` is an implementation
  detail and may change; what stays stable is that it type-checks under
  `tsc --strict` and behaves the same.

## What we hold stable

- **Your code stays runnable.** Glyph compiles to plain, readable TypeScript that
  you own and commit. If Glyph ever stalls or you want out, the emitted `.ts` is a
  permanent, dependency-free escape hatch — not a lock-in.
- **No silent behavior changes.** A change that alters what your program *does*
  (rather than rejecting it at compile time) is called out in the
  [release notes](https://glyphlang.io/versions/) for that version.
- **Diagnostics are addressable.** Every error and warning carries a stable code
  (`E0xxx`) and a one-line fix; `glyph --explain <code>` gives the long form.

## How we try to make upgrades cheap

- **`glyph fmt` as a migrator.** When a purely syntactic change lands, the goal is
  that running `glyph fmt` rewrites your files to the new form for you. This is an
  aspiration we hold ourselves to per change, not a guarantee for every change.
- **Honest release notes.** Each version at [glyphlang.io/versions](https://glyphlang.io/versions/)
  states what was added, fixed, and — when relevant — what breaks and how to
  adapt.

## Toward 1.0

The pre-1.0 line is: change what needs changing, in the open, with an escape
hatch always available. As the language settles we will tighten this into a
firmer semantic-versioning commitment. Until then, pin a version if you need
reproducibility, and read the release notes before upgrading.
