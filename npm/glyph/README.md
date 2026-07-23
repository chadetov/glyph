# Glyph

**TypeScript your AI agents can't quietly break.**

[![npm](https://img.shields.io/npm/v/@glyphlang/glyph.svg)](https://www.npmjs.com/package/@glyphlang/glyph)
[![downloads](https://img.shields.io/npm/dm/@glyphlang/glyph.svg)](https://www.npmjs.com/package/@glyphlang/glyph)
[![license](https://img.shields.io/npm/l/@glyphlang/glyph.svg)](https://github.com/chadetov/glyph)

Glyph is a statically typed language that **transpiles to TypeScript**, designed from the ground up so AI agents can read, write, and modify code safely. It looks almost like TypeScript — you read it on day one, no tutorial — but the differences are deliberate: each one closes a hole an agent falls through and `tsc --strict` waves past.

```sh
npm install -g @glyphlang/glyph
```

> **Website:** [glyphlang.io](https://glyphlang.io) &nbsp;·&nbsp; **Try it in the browser:** [playground](https://glyphlang.io/playground/) &nbsp;·&nbsp; **Pointing an agent at it?** [glyphlang.io/llms.txt](https://glyphlang.io/llms.txt)

---

## The problem it solves

An AI agent will happily write TypeScript that compiles clean and breaks at runtime. It reaches for `any` at a boundary, casts an `unknown`, forgets a `switch` case, and drops a `Promise` on the floor — and `tsc --strict` stays green the whole time. On a large codebase it can't even reliably *find* where something is defined, because overloads, decorators, namespace merging, and barrel re-exports scatter one symbol across many places.

Glyph removes those hazards at the language level. The bugs an agent ships don't compile here.

## See it

Glyph reads like TypeScript, but there's no `any`, `match` must cover every case, and a type you declare is *actually checked at runtime*:

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

## Why teams pick it

- **No `any`, no erasure.** What the types say is true when the code runs. Every record type generates a runtime validator (`User.parse(x)`), so untrusted input is typed only after it's checked.
- **Exhaustive by default.** `match` over a union — or over `number`/`string` — must handle every case, or it doesn't compile. Add a variant and every unhandled site lights up.
- **Errors are values.** `Result` and the `?` operator instead of thrown exceptions; drop a `Result` and you get a warning, because a discarded error is a swallowed failure.
- **Greppable.** One name, one declaration form. `grep "fn parseUser"` finds the definition — always. No overloads, decorators, or barrel files.
- **Stable diffs.** One canonical format, one element per line, trailing commas, no reflow. A one-line change is a one-line diff — so agent edits stay reviewable.
- **Generate, don't hand-write.** `glyph gen openapi spec.yaml --client` emits a typed HTTP client and server stubs; `glyph gen zod` / `gen dts` turn existing schemas into checked Glyph types.

## Quickstart

```sh
glyph init my-app          # scaffold a project
cd my-app
glyph run src/main.glyph   # build + type-check + execute
glyph build --check        # emit TypeScript, verified with tsc --strict
glyph fmt                  # one canonical layout
glyph --explain E0200      # long-form help for any diagnostic
```

Glyph ships as a single prebuilt binary per platform (macOS, Linux, Windows — Intel and ARM). No postinstall download, no toolchain to set up. Running or type-checking uses your local `tsx`/`tsc`.

## Built for agents

Point your coding agent at **[glyphlang.io/llms.txt](https://glyphlang.io/llms.txt)** — a single file that takes it from zero to correct, runnable Glyph: the canonical program shape, the full stdlib surface, the common gotchas, and the complete diagnostic catalogue with one-line fixes. Agents that write Glyph get compile-time feedback precise enough to self-correct.

**Model Context Protocol.** For an agent that speaks MCP, `glyph mcp` runs a server over stdio that hands it Glyph's own analysis as tools — type-check a file for coded diagnostics, the inferred type at a cursor, where a name is defined (following imports), every reference to a symbol across the whole project, and symbol search. It's the same analysis the editor uses, so what the agent sees can't drift from the compiler. Point any MCP client at `glyph mcp <project>`; details at [glyphlang.io/mcp](https://glyphlang.io/mcp/).

**Editors.** `glyph lsp` is a full language server (diagnostics, hover, go-to-definition, completion, symbols, workspace-wide find-references and rename, formatting) that any LSP client can drive over stdio — a VS Code extension ships ready-made.

## Where to go next

| | |
|---|---|
| Your first program in 10 minutes | [Start Here](https://glyphlang.io/start/) |
| The whole language in five minutes | [the tour](https://github.com/chadetov/glyph/blob/main/docs/guide/tour.md) |
| Try it without installing | [playground](https://glyphlang.io/playground/) |
| Straight-talking answers to engineer questions | [glyphlang.io/answers](https://glyphlang.io/answers/) |
| The four pillars, in depth | [glyphlang.io/pillars/verifiability](https://glyphlang.io/pillars/verifiability/) |
| Source, issues, roadmap | [github.com/chadetov/glyph](https://github.com/chadetov/glyph) |

## Status

Glyph is an **early preview** and moves fast. The compiler toolchain — `build`, `run`, `fmt`, `gen`, `regen`, `--explain` — works end to end, and every release is type-checked against `tsc --strict`. It is not yet recommended for production; it's ready for you to try, break, and tell us about. Every version's changes are at [glyphlang.io/versions](https://glyphlang.io/versions/).

**Stability while pre-1.0:** the language can still change between 0.1.x releases. We hold two lines: your code stays runnable (it always compiles to plain TypeScript you own, so there's a permanent escape hatch), and when syntax does change we aim to make `glyph fmt` migrate it for you. Full policy: [docs/stability.md](https://github.com/chadetov/glyph/blob/main/docs/stability.md).

## Verifying your download

Releases are built in GitHub Actions and published with provenance, so you can
confirm an artifact came from this repo's workflow rather than a tampered copy:

```sh
# npm packages: published with npm provenance (OIDC-signed)
npm audit signatures

# GitHub Release archives: SLSA build-provenance attestation
gh attestation verify glyph-<version>-<platform>.tar.gz --repo chadetov/glyph

# ...and a SHA-256 for each archive
sha256sum -c SHA256SUMS
```

## License

Dual-licensed under [Apache-2.0](https://github.com/chadetov/glyph/blob/main/LICENSE-APACHE) or [MIT](https://github.com/chadetov/glyph/blob/main/LICENSE-MIT), at your option.
