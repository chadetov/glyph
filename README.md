# Glyph

A statically typed, transpile-to-TypeScript language built so code an AI agent writes can be read, checked and changed without guessing.

[![npm](https://img.shields.io/npm/v/@glyphlang/glyph.svg)](https://www.npmjs.com/package/@glyphlang/glyph)
[![downloads](https://img.shields.io/npm/dm/@glyphlang/glyph.svg)](https://www.npmjs.com/package/@glyphlang/glyph)
[![CI](https://github.com/chadetov/glyph/actions/workflows/ci.yml/badge.svg)](https://github.com/chadetov/glyph/actions/workflows/ci.yml)
[![install size](https://packagephobia.com/badge?p=@glyphlang/glyph)](https://packagephobia.com/result?p=@glyphlang/glyph)
[![license](https://img.shields.io/npm/l/@glyphlang/glyph.svg)](https://github.com/chadetov/glyph)
[![Socket Badge](https://badge.socket.dev/npm/package/@glyphlang/glyph/0.1.155)](https://badge.socket.dev/npm/package/@glyphlang/glyph/0.1.155)
[![OpenSSF Scorecard](https://api.scorecard.dev/projects/github.com/chadetov/glyph/badge)](https://scorecard.dev/viewer/?uri=github.com/chadetov/glyph)

```sh
npx -y @glyphlang/glyph init hello && cd hello && npm install && npx glyph run
```

That needs Node 18 or later and installs nothing globally: the project pins the
compiler, `typescript` and `tsx` in `devDependencies`. To have `glyph` on your
PATH instead, `npm install -g @glyphlang/glyph tsx typescript`. Prebuilt binaries cover macOS
and Linux on x64 and ARM, and Windows on x64.

**[glyphlang.io](https://glyphlang.io)** · **[playground](https://glyphlang.io/playground/)** · **[AGENTS.md](AGENTS.md)** if you are pointing an agent at this

## What it looks like

```glyph
module main

import std/array
import std/option { Some, None }
import std/result { Result, Ok, Err }

type User = {
  id: string,
  name: string,
}

pub fn find_user(users: Array<User>, id: string) -> Result<User, string> {
  let found = array.find(users, fn(u: User) -> bool {
    return u.id == id
  })
  return match found {
    Some(u) => Ok(u),
    None => Err("no user with id ${id}"),
  }
}
```

If you write TypeScript you can read that on day one. That is the point: Glyph
compiles to TypeScript, runs anywhere TypeScript runs, and uses npm packages
directly. The differences are few and each one exists so that a program stays
correct when something other than its author changes it.

## What the compiler catches

Add a variant to a union and every `match` over it stops compiling until you
handle the new case:

```glyph
type PaymentStatus =
  | Pending
  | Settled({ cents: int })
  | Refunded({ cents: int })
```

```text
[E0200] non-exhaustive match on `PaymentStatus`: missing variants `Refunded`
   Help: Add an arm for each missing variant.
   Note: Tagged unions are sealed (D9): adding a variant forces every match to
         be updated. A `_`/`else` catch-all is allowed but forfeits that
         guarantee.
```

An agent adding a payment state cannot quietly leave a branch unhandled, because
the build stops. In TypeScript, `tsc --strict` catches the same change only in a
`switch` that returns a value from every branch under a declared return type
(TS2366). A `switch` that only logs or updates state, or one with a `default:`,
compiles and skips the new case unless the project turns on typescript-eslint's
`switch-exhaustiveness-check`.

## Why not just TypeScript

TypeScript is the thing to beat, and mostly Glyph does not try to. It keeps the
ecosystem and changes the layer above it. What changes, and why:

| TypeScript | Glyph | Why it matters when a machine is editing |
|---|---|---|
| `any` erases what the type says | no `any` | an agent cannot satisfy the checker by escaping it |
| exceptions are invisible in a signature | errors are values in the return type | what can fail is readable without running anything |
| a `switch` can silently miss a case | `match` must be exhaustive | adding a variant surfaces every site that must change |
| several ways to declare the same thing | one form per declaration | `grep "fn parse_user"` finds the definition, always |
| formatting is a matter of taste | formatting is fixed | a one-line change makes a one-line diff |
| types are erased before runtime | types carry a runtime descriptor | untrusted input is validated at the boundary, not cast |

The checks are the same whoever wrote the code. They matter most when an agent
is editing, because none of them is a setting the agent can turn off to get a
build green.

## The four pillars

Every design decision is tested against these. If a feature improves one without
harming the others, it ships.

1. **Verifiability.** In code written in Glyph, a typed value has been checked.
   There is no `any` and no cast, and a value crossing a boundary is checked
   against its type on the way in: `json.parse<User>(body)` is `Ok` only if the
   body is a `User`, and a type Glyph cannot check at runtime is refused rather
   than trusted. The known exceptions are listed at
   [glyphlang.io/catches/#not-yet](https://glyphlang.io/catches/#not-yet), and a
   type imported from an npm package's `.d.ts` has no runtime check.
2. **Greppability.** Every symbol has one syntactic form at its declaration
   site: no overloads, no renaming decorators, no implicit `this`, no
   declaration merging. `grep` finds the definition.
3. **Abstraction.** Express intent at the level the writer is thinking: pattern
   matching over `if`/`else` ladders, `Result` over thrown exceptions, named
   records over positional tuples.
4. **Diff stability.** A one-line change produces a one-line diff. `glyph fmt`
   has one layout and no options, and the layout depends only on the code, not
   on how the input happened to be wrapped.

Verifiability and greppability are the wedge. Abstraction and diff stability are
the polish.

These are properties of the language rather than of a project's settings.
Strict TypeScript with lint rules and Prettier gets part of the way in one
repository; Glyph makes them true of every file, so an agent opening an
unfamiliar codebase does not have to find out which settings it chose. The
manifesto (`docs/manifesto.md`) goes through each pillar against that
objection.

## Status

Pre-1.0 and moving. The compiler, standard library, formatter, LSP and MCP
server are usable now: the applications under [`examples/apps/`](examples/apps/)
are programs written in Glyph with no TypeScript in them, and each carries
a README saying what writing it found in the compiler.

What that means for you: the language works, and 0.1.x releases can still reject
code that compiled before. Read the [release notes](https://glyphlang.io/versions/)
before moving between versions, and pin the compiler in `devDependencies`, which
`glyph init` does for you.

## Editor support

The TextMate grammar and a VS Code extension live in
[`editors/vscode/`](editors/vscode/). Neither is on the Marketplace yet, so
install it from source:

```sh
cd editors/vscode && npx @vscode/vsce package && code --install-extension glyph-vscode-*.vsix
```

The grammar is generated from the lexer's keyword table, so a keyword the
compiler knows is a keyword the editor colours. Any editor with an LSP client
can drive `glyph lsp` over stdio for diagnostics, hover, go-to-definition,
completion, find-references and rename.

## Where to start

| If you want to | Read |
|---|---|
| See the whole language in five minutes | [`docs/guide/tour.md`](docs/guide/tour.md) |
| Install Glyph and run your first program | [`docs/guide/getting-started.md`](docs/guide/getting-started.md) |
| Map your TypeScript knowledge onto Glyph | [`docs/guide/for-typescript-developers.md`](docs/guide/for-typescript-developers.md) |
| Build something real (a todo CLI) | [`docs/guide/tutorial.md`](docs/guide/tutorial.md) |
| Talk to a database (SQLite, Postgres, Mongo, Redis, MySQL) | [`docs/guide/databases.md`](docs/guide/databases.md) |
| Understand the project's thesis | [`docs/manifesto.md`](docs/manifesto.md) |
| See concrete Glyph programs | [`examples/`](examples/) |
| Read the language specification | [`docs/language/spec.md`](docs/language/spec.md) |
| See the roadmap | [`docs/roadmap/overview.md`](docs/roadmap/overview.md) |
| See the full implementation plan | [`docs/implementation-plan.md`](docs/implementation-plan.md) |
| Compare Glyph against TS, Python, Rust | [`benchmarks/`](benchmarks/) |
| See what the benchmarks show and what they don't | [`benchmarks/FINDINGS.md`](benchmarks/FINDINGS.md) |
| See bugs Glyph catches that `tsc --strict` lets through | [`catches/`](catches/) |

## Building

```sh
cd glyph-compiler
cargo test --workspace
```

Requires Rust 1.95 or later (pinned via `rust-toolchain.toml`).

## Verifying a release

Releases are built in GitHub Actions and published with provenance:

```sh
npm audit signatures                                                   # npm packages (OIDC provenance)
gh attestation verify glyph-<version>-<platform>.tar.gz --repo chadetov/glyph   # SLSA attestation
sha256sum -c SHA256SUMS                                                 # per-archive checksums
```

## License

Dual-licensed under either of

* [Apache License, Version 2.0](LICENSE-APACHE)
* [MIT License](LICENSE-MIT)

at your option.
