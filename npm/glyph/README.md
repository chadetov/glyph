# Glyph

**TypeScript your AI agents can't quietly break.**

[![npm](https://img.shields.io/npm/v/@glyphlang/glyph.svg)](https://www.npmjs.com/package/@glyphlang/glyph)
[![downloads](https://img.shields.io/npm/dm/@glyphlang/glyph.svg)](https://www.npmjs.com/package/@glyphlang/glyph)
[![CI](https://github.com/chadetov/glyph/actions/workflows/ci.yml/badge.svg)](https://github.com/chadetov/glyph/actions/workflows/ci.yml)
[![install size](https://packagephobia.com/badge?p=@glyphlang/glyph)](https://packagephobia.com/result?p=@glyphlang/glyph)
[![license](https://img.shields.io/npm/l/@glyphlang/glyph.svg)](https://github.com/chadetov/glyph)
[![Socket Badge](https://badge.socket.dev/npm/package/@glyphlang/glyph/0.1.188)](https://badge.socket.dev/npm/package/@glyphlang/glyph/0.1.188)
[![OpenSSF Scorecard](https://api.scorecard.dev/projects/github.com/chadetov/glyph/badge)](https://scorecard.dev/viewer/?uri=github.com/chadetov/glyph)

Glyph is a statically typed language that **transpiles to TypeScript**, built so code an AI agent writes can be read, checked and changed without guessing. It looks almost like TypeScript, so you read it on day one with no tutorial. Where it differs, each difference closes a hole an agent falls through and `tsc --strict` waves past.

## Try it without installing anything

```sh
npx -y @glyphlang/glyph init my-app
cd my-app
npm install
npx glyph run
# hello from glyph
```

That needs Node 18 or later. It scaffolds seven files, pins the compiler, `typescript` and `tsx` in `devDependencies`, and runs the program. Anyone who clones the project builds it with `npm install` and nothing global. Two of the seven are for coding agents rather than for you: an `AGENTS.md` pointing at the offline language reference, and an `.mcp.json` registering the analysis server below. A `.vscode/extensions.json` makes VS Code offer the Glyph extension when you open the folder.

Prefer it on your PATH (a global `glyph` runs and type-checks with the `tsx` and `tsc` on your PATH, so they come along):

```sh
npm install -g @glyphlang/glyph tsx typescript
glyph init my-app && cd my-app && glyph run
```

> **Website:** [glyphlang.io](https://glyphlang.io) &nbsp;·&nbsp; **Try it in the browser:** [playground](https://glyphlang.io/playground/) &nbsp;·&nbsp; **Pointing an agent at it?** [glyphlang.io/llms.txt](https://glyphlang.io/llms.txt)

---

## The problem it solves

An AI agent can write TypeScript that compiles clean and breaks at runtime. It reaches for `any` at a boundary, casts an `unknown`, or leaves a case out of a `switch` that only logs, and `tsc --strict` accepts all three. On a large codebase it can't always find where something is defined, because overloads, decorators, namespace merging, and barrel re-exports scatter one symbol across many places.

Glyph removes those escape hatches at the language level, so many of the bugs an agent ships don't compile here. The ones that still do are listed at [glyphlang.io/catches/#not-yet](https://glyphlang.io/catches/#not-yet).

## See it

Glyph reads like TypeScript, but there is no `any`, `match` must cover every case, and a record type you declare comes with a runtime validator:

```glyph
type User = {
  id: number,
  name: string,
}

// A boundary value is `unknown` until you prove its shape.
// `User.parse` exists because every type carries a runtime descriptor.
fn handle(body: unknown) -> string {
  return match User.parse(body) {
    Ok(user) => string.upper(user.name),
    Err(_) => "invalid",
  }
}
```

That compiles to clean, readable TypeScript you can commit, run anywhere TS runs, and mix with any npm package.

## What's different

- **No `any`, no casts.** Every record type generates a runtime validator (`User.parse(x)`), so untrusted input is typed only after it has been checked, and a failure names the field and its path.
- **Exhaustive by default.** `match` over a union, or over `number`/`string`, must handle every case or it does not compile. Add a variant and every `match` that does not handle it fails to compile.
- **Errors are values.** `Result` and the `?` operator instead of thrown exceptions. Drop a `Result` and you get a warning, because a discarded error is a swallowed failure.
- **Greppable.** One name, one declaration form. `grep "fn parseUser"` finds the definition every time. No overloads, decorators, or barrel files.
- **Stable diffs.** One canonical format, one element per line, trailing commas, no reflow. A one-line change is a one-line diff, so agent edits stay reviewable.
- **Generate, don't hand-write.** `glyph gen openapi spec.yaml --client` emits a typed HTTP client and server stubs. `glyph gen zod` and `gen dts` turn existing schemas into checked Glyph types.
- **Talks to databases.** Import any npm client by name. Class-based clients (`new Pool`, `new MongoClient`) construct with `new`, and SQLite is built in via `std/sqlite`. A row comes back untrusted, so you `Row.parse` it into a typed record and the schema-versus-code mismatch a cast would hide is caught at the boundary. See the [databases guide](https://github.com/chadetov/glyph/blob/main/docs/guide/databases.md).

## The commands

```sh
glyph init my-app          # scaffold a project
glyph run                  # build + type-check + execute
glyph build --check        # emit TypeScript, verified with tsc --strict
glyph fmt                  # one canonical layout
glyph --explain E0200      # long-form help for any diagnostic
```

Glyph ships as a single prebuilt binary per platform (macOS and Linux on x64 and ARM, Windows on x64), with no postinstall download. Running and type-checking a program use `tsx` and `tsc`, which `glyph init` pins in the project's `devDependencies`, so `npm install` and `npx glyph run` in the project need nothing else. A globally installed `glyph` looks for `tsx` and `tsc` on your PATH, so install those globally too (`npm install -g tsx typescript`).

## Built for agents

Point your coding agent at **[glyphlang.io/llms.txt](https://glyphlang.io/llms.txt)**, a single file with the canonical program shape, the full stdlib surface, the common gotchas, and the complete diagnostic catalogue with one-line fixes. Agents writing Glyph get compile-time feedback precise enough to self-correct.

**Model Context Protocol.** `glyph init` writes an `.mcp.json` that registers this automatically, and `glyph agents` adds it to a project you already have. `glyph mcp` runs a server over stdio with eleven tools: a file's coded diagnostics, the type at a cursor, where a name is defined (following imports), every reference to a symbol, every `match` site over a union, what one change to a declaration breaks (`glyph_impact`), everything known about one symbol, symbol search, whether one type is assignable to another, what a declaration depends on, and what a module exports. It runs the same analysis the editor uses, so the agent's answers match the compiler. Point any MCP client at `glyph mcp <project>`; details at [glyphlang.io/mcp](https://glyphlang.io/mcp/).

**Editors.** `glyph lsp` is a full language server (diagnostics, hover, go-to-definition, completion, symbols, workspace-wide find-references and rename, formatting) that any LSP client can drive over stdio.

## Where to go next

| | |
|---|---|
| Your first program in 10 minutes | [Start Here](https://glyphlang.io/start/) |
| The whole language in five minutes | [the tour](https://github.com/chadetov/glyph/blob/main/docs/guide/tour.md) |
| Try it without installing | [playground](https://glyphlang.io/playground/) |
| Answers to the questions engineers ask | [glyphlang.io/answers](https://glyphlang.io/answers/) |
| The four pillars, in depth | [glyphlang.io/pillars/verifiability](https://glyphlang.io/pillars/verifiability/) |
| Source, issues, roadmap | [github.com/chadetov/glyph](https://github.com/chadetov/glyph) |

## Status

Glyph is an **early preview** and moves fast. The compiler toolchain (`build`, `run`, `fmt`, `gen`, `regen`, `--explain`) works end to end, and every release is type-checked against `tsc --strict`. It is not ready for production yet. It is ready for you to try and to tell us where it breaks. Every version's changes are at [glyphlang.io/versions](https://glyphlang.io/versions/).

**Stability while pre-1.0:** the language can still change between 0.1.x releases. Two lines hold. Your code stays runnable, because it always compiles to plain TypeScript you own and can keep if you stop using Glyph. And when syntax does change we aim to make `glyph fmt` migrate it for you. Full policy: [docs/stability.md](https://github.com/chadetov/glyph/blob/main/docs/stability.md).

## Verifying your download

Releases are built in GitHub Actions and published with provenance, so you can
confirm an artifact came from this repo's workflow rather than a tampered copy:

```sh
# npm packages: published with npm provenance (OIDC-signed)
npm audit signatures

# GitHub Release archives: SLSA build-provenance attestation
gh attestation verify glyph-<version>-<platform>.tar.gz --repo chadetov/glyph

# ...or against the provenance bundle attached to the release, with no network
gh attestation verify glyph-<version>-<platform>.tar.gz \
  --bundle v<version>.intoto.jsonl --repo chadetov/glyph

# ...and a SHA-256 for each archive
sha256sum -c SHA256SUMS
```

## License

Dual-licensed under [Apache-2.0](https://github.com/chadetov/glyph/blob/main/LICENSE-APACHE) or [MIT](https://github.com/chadetov/glyph/blob/main/LICENSE-MIT), at your option.
