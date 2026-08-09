# Contributing to Glyph

Thanks for looking. Glyph is an early-preview language that transpiles to
TypeScript, built so AI agents can read, write, and modify code safely. It's
moving fast and pre-1.0, so this guide is short and honest about what's useful
right now.

## Where to start

New here? The [`good first
issue`](https://github.com/chadetov/glyph/issues?q=is%3Aissue+is%3Aopen+label%3A%22good+first+issue%22)
label marks small, well-scoped tasks with enough context to pick up without
deep knowledge of the compiler. [`help
wanted`](https://github.com/chadetov/glyph/issues?q=is%3Aissue+is%3Aopen+label%3A%22help+wanted%22)
is the next step up. If nothing fits, a bug report or a docs fix is always
welcome. Ask questions in
[Discussions](https://github.com/chadetov/glyph/discussions), no question is too
basic.

By participating you agree to the [Code of Conduct](CODE_OF_CONDUCT.md).

## What helps most today

- **Bug reports.** A `.glyph` program that miscompiles, crashes, or produces
  TypeScript that `tsc --strict` rejects is gold. Use the bug template; a
  minimal reproducer plus the exact command and output is ideal.
- **Rough edges and confusing errors.** If a diagnostic didn't tell you how to
  fix it, or a feature surprised you, say so. Elm-quality error messages are a
  goal, not a given.
- **Docs and examples.** Fixes to the guide, the spec, `docs/reference/`, or a
  new example program that exercises a real pattern are very welcome.

## Please open an issue first for language changes

**The grammar is the spec.** Every syntax or semantics rule traces to a numbered
design decision, and the four pillars — verifiability, greppability, abstraction,
diff stability — decide every call. Glyph is *deliberately* stricter than
TypeScript on several axes (no `if`/`else`, `match` must be exhaustive, trailing
commas required, one declaration form per name). Those restrictions earn their
keep; the answer to an annoying one is usually documentation, not loosening the
rule.

So: before writing code that changes the language, open an issue to discuss the
design. A PR that relaxes syntax "to be helpful" will likely be declined — not
because the effort isn't appreciated, but because the constraint is the point.
Toolchain fixes, diagnostics, docs, and examples don't need this — just send them.

## Building and testing

The compiler is a Rust workspace under `glyph-compiler/`.

```sh
cd glyph-compiler
cargo test --workspace        # the full suite, ~1s warm
```

Requires Rust 1.95+ (pinned via `rust-toolchain.toml`). Some tests (`glyph run`,
`@example`/`@doc @run` execution, `--check`) shell out to `tsx`/`tsc`; install
them (`npm install -g tsx typescript`) or those tests skip.

After an intentional AST change, regenerate the parser snapshots:

```sh
INSTA_UPDATE=always cargo test -p glyph-parser --test snapshots
# or review interactively:
cargo insta review
```

New behavior should come with a test: a unit or integration test for the happy
path, and — for a new diagnostic — a case under `glyph-compiler/tests/negative/`
(a program that must fail with the named code). Every example must pass
`tsc --strict` (CI enforces this).

## Repository layout

```
glyph-compiler/   the Rust compiler (lexer -> parser -> resolver -> typechecker -> emit) + CLI
docs/             the guide, language spec, roadmap, and references
examples/         runnable example programs (all type-checked in CI)
web/              the glyphlang.io site (static, deployed via GitHub Pages)
npm/              the published npm launcher + per-platform binary packages
```

## Branching

Trunk-based, with short-lived branches. `main` is always releasable and is
protected: no force pushes, no deletion, linear history, and those apply to
admins too.

Work happens on a branch named for what it is:

| Prefix | For |
|---|---|
| `gap/g39-stdlib-signatures` | an entry in the dogfooding gap list |
| `release/0.1.71` | a version bump and its notes |
| `fix/npx-path-detection` | a defect found outside the loop |
| `docs/reserved-words` | documentation only |

**Keep a pull request to one gap, usually one to three commits.** That is not
tidiness: CI verifies the branch's *head*, not every commit on it, so a long
branch puts commits on `main` that were never built on their own. Short branches
keep that window to a commit or two, which is what makes `git bisect` trustworthy.

**Merges are rebase-only.** Squash and merge commits are disabled on the
repository. Squashing would collapse a planned sequence into one commit and
delete the history this project deliberately writes; merge commits would break
the linear history `main` requires. Rebase keeps both, at the cost of new SHAs
for every commit, which matters when you tag (see below).

## Commits and PRs

- **Imperative, concise subject lines** ("Fix nested match lowering"), under ~70
  characters, no `feat:`/`fix:` prefixes.
- **Explain the why in the body** when it isn't obvious from the subject; note
  what the change deliberately does *not* do.
- Keep each commit a coherent unit; group by theme, not by file.
- Run `cargo test --workspace` before pushing.

Three checks must pass before a pull request can merge, and they run in about two
minutes: **Version consistency**, **Test + type-check examples**, and **Links,
HTML, and sub-nav**. CodeQL and Scorecard run on `main` and on a schedule rather
than blocking a merge, because they take considerably longer than the change
usually deserves.

Reviews are not required, for a reason worth stating rather than leaving as an
apparent oversight: GitHub does not let an author approve their own pull request,
so on a single-maintainer repository a review requirement means either nothing
merges or an admin bypasses it, and the second is theatre. When a second
maintainer arrives, this is the first setting to revisit.

## Sign your commits (DCO)

Contributions are accepted under a [Developer Certificate of
Origin](https://developercertificate.org/): a simple statement that you wrote the
patch (or have the right to submit it) and agree it may ship under the project's
licenses. Certify it by adding a `Signed-off-by` line to each commit, which `git`
adds for you with `-s`:

```sh
git commit -s -m "Fix nested match lowering"
```

This produces a trailer like `Signed-off-by: Your Name <you@example.com>`. It
keeps the project's IP provenance clean without a separate CLA to sign. By
contributing, you agree your contribution is licensed under both the
[MIT](../LICENSE-MIT) and [Apache-2.0](../LICENSE-APACHE) licenses, the same dual
license as the project.

## Stability and scope

Glyph is pre-1.0 and can change between 0.1.x releases; see
[docs/stability.md](../docs/stability.md). Self-hosting is a v1.0 non-goal — the
compiler stays Rust for now.

## Be kind

Assume good faith, keep discussion technical and respectful, and remember this
is early software built in the open. Questions are welcome in
[Discussions](https://github.com/chadetov/glyph/discussions).
