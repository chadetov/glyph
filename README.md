# Glyph

A statically typed, transpile-to-TypeScript language designed so AI agents can read, write, and modify code safely.

[![npm](https://img.shields.io/npm/v/@glyphlang/glyph.svg)](https://www.npmjs.com/package/@glyphlang/glyph)
[![downloads](https://img.shields.io/npm/dm/@glyphlang/glyph.svg)](https://www.npmjs.com/package/@glyphlang/glyph)
[![CI](https://github.com/chadetov/glyph/actions/workflows/ci.yml/badge.svg)](https://github.com/chadetov/glyph/actions/workflows/ci.yml)
[![install size](https://packagephobia.com/badge?p=@glyphlang/glyph)](https://packagephobia.com/result?p=@glyphlang/glyph)
[![license](https://img.shields.io/npm/l/@glyphlang/glyph.svg)](https://github.com/chadetov/glyph)
[![Socket Badge](https://badge.socket.dev/npm/package/@glyphlang/glyph/0.1.98)](https://badge.socket.dev/npm/package/@glyphlang/glyph/0.1.98)
[![OpenSSF Scorecard](https://api.scorecard.dev/projects/github.com/chadetov/glyph/badge)](https://scorecard.dev/viewer/?uri=github.com/chadetov/glyph)

```sh
npm install -g @glyphlang/glyph
```

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
handle the new case. This is the whole idea in one error:

```glyph
type PaymentStatus =
  | Pending
  | Settled({ cents: int })
  | Refunded({ cents: int })
```

```text
[E0200] non-exhaustive match on `PaymentStatus`: missing variants `Refunded`
   Help: Add an arm for each missing variant, or an `else` arm to catch the rest.
   Note: Tagged unions are sealed (D9): adding a variant forces every match to
         be updated. A `_`/`else` catch-all is allowed but forfeits that
         guarantee.
```

An agent adding a payment state cannot quietly leave a branch unhandled, because
the build stops. In TypeScript the same change compiles and fails later, at a
customer.

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

Every one of those is a property an agent depends on and a human merely
appreciates. That asymmetry is the bet.

## The four pillars

Every design decision is tested against these. If a feature improves one without
harming the others, it ships.

1. **Verifiability.** Anything the type system claims is true at runtime.
2. **Greppability.** Every symbol has one syntactic form at its declaration site.
3. **Abstraction.** Express intent at the level the writer is thinking: pattern
   matching over switch ladders, `Result` over thrown exceptions, named records
   over positional tuples.
4. **Diff stability.** A one-line change produces a one-line diff.

Verifiability and greppability are the wedge. Abstraction and diff stability are
the polish.

## Status

Pre-1.0 and moving. The compiler, standard library, formatter, LSP and MCP
server are usable now: the applications under [`examples/apps/`](examples/apps/)
are real programs written in Glyph with no TypeScript in them, and each carries
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
| Talk to a real database (SQLite, Postgres, Mongo, Redis, MySQL) | [`docs/guide/databases.md`](docs/guide/databases.md) |
| Understand the project's thesis | [`docs/manifesto.md`](docs/manifesto.md) |
| See concrete Glyph programs | [`examples/`](examples/) |
| Read the language specification | [`docs/language/spec.md`](docs/language/spec.md) |
| See the roadmap | [`docs/roadmap/overview.md`](docs/roadmap/overview.md) |
| See the full implementation plan | [`docs/implementation-plan.md`](docs/implementation-plan.md) |
| Compare Glyph against TS, Python, Rust | [`benchmarks/`](benchmarks/) |
| See what the benchmarks show (honestly) | [`benchmarks/FINDINGS.md`](benchmarks/FINDINGS.md) |
| See bugs Glyph catches that `tsc --strict` misses | [`benchmarks/verifiability/`](benchmarks/verifiability/) |

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
