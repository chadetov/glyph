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
  [release notes](https://glyphlang.io/versions/) for that version. This is
  enforced, not just promised: a **spec conformance corpus** (one program per
  language feature, keyed to its D-decision) pins the exact TypeScript the
  compiler emits, so a change to what a feature *means* fails the build and a
  human has to sign off on the diff before it ships. The emit is byte-for-byte
  what it was, or the change is deliberate and reviewed.
- **Diagnostics are addressable.** Every error and warning carries a stable code
  (`E0xxx`) and a one-line fix; `glyph --explain <code>` gives the long form.

## You upgrade on purpose, never by accident

Read the first bullet above again: a 0.1.x release may reject code that compiled
before. That is only safe if you decide when to take one, so a Glyph project
pins the compiler **exactly**.

`glyph init` writes the version of the compiler that scaffolded the project into
`devDependencies` as an exact version, with no `^`. A caret on a `0.x` version
still floats the patch (`^0.1.9` accepts every later 0.1.x), which would mean a
build going red on an `npm install` you ran for an unrelated reason, with no
change to your source. Commit your
`package-lock.json` too, and a fresh clone builds with the toolchain you tested
rather than whatever shipped since.

Pinning has an obvious failure mode: a project can sit on an old compiler
forever without knowing it. Three commands close that.

```sh
glyph doctor     # reports your version against the latest published release
glyph upgrade    # moves a project's pin to it and runs npm install
glyph --update   # moves the compiler itself, not a project's pin
```

`glyph doctor` asks npm (add `--offline` to skip the lookup entirely). Finding a
newer release never changes its exit code, so this is safe in CI. `glyph upgrade`
rewrites the one line and prints the release-notes link; `--dry-run` shows what
would change, and `--to <version>` names a specific one, including an older one
if you need to go back. Build before you commit the result.

`glyph --update` is the other half, and the two are easy to confuse. A flag acts
on the tool, the way `--version` and `--explain` do; a subcommand acts on your
code. So `--update` moves the compiler you invoke and `upgrade` moves the version
a project pins. `--update` only moves an install it can identify against
`npm root -g`: a project's own `node_modules`, an npx cache, or a build from a
source tree gets told what to run instead of being overwritten.


## How we try to make upgrades cheap

- **`glyph fmt` as a migrator.** When a purely syntactic change lands, the goal is
  that running `glyph fmt` rewrites your files to the new form for you. This is an
  aspiration we hold ourselves to per change, not a guarantee for every change.
- **Honest release notes.** Each version at [glyphlang.io/versions](https://glyphlang.io/versions/)
  states what was added, fixed, and — when relevant — what breaks and how to
  adapt.

## Release cadence

Releases are cut when meaningful work lands, not on a fixed calendar, and the
0.1.x line moves quickly (expect several 0.1.x releases between the named
milestones on the [roadmap](roadmap/releases.md)). A minor bump (0.2.0, 0.3.0)
marks a milestone actually reached, not a date. Every release is a `v*` git tag
that triggers the publish workflow, so the npm version and the tagged source
always match (CI hard-fails otherwise). A scaffolded project is already on a
predictable base, since the pin is exact; published versions are immutable and
never yanked out from under a build.

## Deprecation policy

When a construct or standard-library name is going away:

1. It is **marked deprecated** in a release, with a diagnostic (a warning, not an
   error) that names the replacement, and a note in the release notes.
2. It **keeps working for at least one subsequent 0.1.x release** after the
   deprecation warning first appears, so an upgrade never breaks and forces a fix
   in the same step.
3. Where the migration is mechanical, `glyph fmt` or a codemod does the rewrite,
   so acting on the warning is one command.

After 1.0, deprecation windows lengthen and are tied to the semantic-versioning
commitment below.

## Toward 1.0

The pre-1.0 line is: change what needs changing, in the open, with an escape
hatch always available. As the language settles we will tighten this into a
firmer semantic-versioning commitment: after 1.0, a breaking language change
requires a major version, and stable features carry a compatibility guarantee
within a major line. Until then, pin a version if you need reproducibility, and
read the release notes before upgrading.
