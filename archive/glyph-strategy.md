# Glyph — Strategy & Delivery Plan

A consolidated summary of decisions made in this session about Glyph: a TypeScript-family language designed for AI agents to read, write, and modify safely.

---

## 1. Positioning

**What Glyph is:** a statically typed, transpile-to-TypeScript language for building production systems where AI agents are first-class collaborators on the codebase.

**Who Glyph is for:** TypeScript developers building AI-agent systems. They have already accepted the core premise — giving up some flexibility for verifiability is worth it — and they are disproportionately represented in the AI tooling ecosystem (LangChain.js, Vercel AI SDK, MCP servers, Claude Code, etc.).

**Why this audience is the right wedge:**

- They already accepted that types matter and that a transpiled language is legitimate.
- They are already in the AI-coding wave.
- Compiling to TypeScript means inheriting npm — the largest package ecosystem in the world.
- LSP, tree-sitter, formatter primitives, and bundler ecosystems are mature and largely reusable.

**The honest risk:** TypeScript developers are the best target *and* the hardest to impress. They already have a good language with great tooling. The pitch cannot be "TypeScript but slightly nicer" — that path leads to CoffeeScript. It must be "TypeScript but agents are measurably more productive and diffs are measurably smaller," proven with benchmarks.

---

## 2. The Four Pillars

Every design decision is tested against these. If a feature improves one without harming the others, it ships. If not, it doesn't.

### Abstraction
Code should express intent at the level the writer is thinking, not the level the runtime requires. Pattern matching over switch ladders, Result types over thrown exceptions, named records over positional tuples, a small core of orthogonal primitives.

### Verifiability
Anything the type system claims must be true at runtime. No `any`. No structural-typing surprises. No type erasure — every Glyph type has a runtime descriptor available when needed. The compiler is the source of truth, and the source of truth is enforceable.

### Diff Stability
A one-line change should produce a one-line diff. Fixed-width, single-element-per-line formatting — never line-length-based reflow. Explicit, full-path imports. No barrel files. Trailing commas everywhere. Sorted imports.

### Greppability
Every symbol has exactly one syntactic form at its declaration site. No method overloads, no decorators that rename, no implicit `this`, no namespace merging. `grep -n "fn parseUser"` finds the definition. Always.

**Weighting:** Verifiability and greppability are the wedge — they fix problems TypeScript developers feel and other languages don't solve. Abstraction and diff stability are the polish that makes daily use pleasant.

---

## 3. TypeScript Complaints Glyph Fixes

Only complaints with real community consensus. Pet peeves and academic improvements are out of scope.

| Complaint | Glyph's Fix |
|---|---|
| Type erasure / runtime gap | Types compile to lightweight runtime descriptors when needed. Opt-in, zero cost when unused. |
| `any` and `unknown` escape hatches | No `any` by default, or require a `// @unsafe` directive that lints loudly. |
| `tsconfig.json` complexity | One mode. No config. Strict by default. |
| Module resolution mess (ESM vs CJS, `.js` extensions) | One resolution algorithm, explicit and boring. |
| Decorators, enums, namespaces | Don't have them. Use const objects and union types. |
| Structural-typing surprises | Nominal types by default, structural as opt-in. |
| Error handling (`throw`, async errors) | Result/Either as first-class builtin. Errors in the type signature. |
| `null` / `undefined` duality | Pick one (likely `null`, since JSON). The other is a compile error. |
| Exhaustiveness checking | Pattern matching as a first-class construct, with exhaustiveness enforced. |
| Compile speed | Compiler in Rust or Go from day one. No bootstrapping in itself for ≥ 3 years. |
| `this` binding footguns | Arrow-method default, or no `this` at all (explicit receivers). |

**Explicitly out of scope:** macros, effect systems, dependent types, algebraic effects, linear types. All cool. None are TS-developer complaints. Adding them turns Glyph from "TypeScript but better" into "an academic language with TS syntax."

---

## 4. The 12-Step Delivery Plan

Ordered so each step de-risks the next. Steps 1–6 get to "I can program in Glyph." Steps 7–12 get to "other people can too."

### Phase 1 — Make it Programmable

1. **Write the manifesto (1 week).** Max 2000 words. What Glyph is, who it's for, the four pillars, three before/after examples. The north star.
2. **Lock the syntax with examples, not a grammar (2 weeks).** 30–50 small Glyph programs by hand: HTTP server, CLI tool, React component, Zod-style validator, error handling, async flows. No compiler yet.
3. **Write the formal grammar and a tree-sitter parser (2 weeks).** Tree-sitter first — instant syntax highlighting and tooling parser. The grammar file is the spec. If tree-sitter can't parse it cleanly, the syntax is too clever.
4. **Build the transpiler to TypeScript (4–6 weeks).** Implement in Rust or Go. Glyph parses → typechecks → emits TS. Let `tsc` handle JS emission. Goal: every example program compiles and runs.
5. **Build the typechecker (4–6 weeks, overlaps with step 4).** The hardest engineering work in the project. Hindley-Milner core plus TS-compatible features (union types, generics, nominal-with-opt-in-structural). Skip conditional, mapped, and template literal types in v1.
6. **Dogfood (2 weeks).** Build a real project in Glyph (JarvisX components are a natural candidate). This surfaces the design mistakes examples didn't.

### Phase 2 — Make it Usable

7. **Ship the LSP (4 weeks).** Non-negotiable. Diagnostics, go-to-definition, hover types, autocomplete, rename, find-references. Use tower-lsp if in Rust. Make it fast.
8. **Ship the formatter and package story (2 weeks).** One formatter, no config, fixed-width wrapping (for diff stability). Package management piggybacks on npm: `glyph.json` wraps `package.json`, `glyph install` calls `npm install`. No new registry.
9. **Ship the installer and playground (2 weeks).** Single curl-pipe-bash installer dropping a `glyph` binary. Web playground showing TS output and run results. The playground is the single most effective marketing tool — every visitor decides in 30 seconds whether to keep reading.
10. **Write the docs and the book outline (4 weeks).** 5-minute tour, 30-minute tutorial that builds something real, complete language reference. Then outline a book — even if it ships in two years, the outline forces gaps to be confronted.
11. **Build the killer demo (6–8 weeks).** Either an agentic coding system that demonstrably writes better Glyph than TS, or a benchmark showing agents complete tasks 2–3× faster in Glyph. Numbers, video, blog post. Without this, "designed for AI agents" is just a claim.
12. **Launch and pick the first 100 users (ongoing).** Show HN, launch post, conference CFPs (Strange Loop, JSConf, AI Engineer Summit). Personally onboard the first 100 users. They define the language's character — treat them as co-designers, not customers.

---

## 5. Realistic Timeline & Traps

**Total focused work:** 6–9 months to ship v0.1. Likely 12–18 months calendar time alongside other things. Anyone promising faster is underestimating tooling.

**The trap at every step:** scope creep. At step 5, the temptation will be to add effect types. At step 7, a custom protocol instead of LSP. At step 11, rewriting the compiler. The discipline that ships a language is saying "v0.2" to every good idea not on this list.

**Premature concerns to defer:** foundation, governance model, logo redesign, corporate sponsor. All distractions until there are users.

---

## 6. Honest Expectations on "Mainstream"

Almost no new language goes mainstream. The last ~20 years gave us roughly Rust, Go, Swift, Kotlin, and TypeScript — each backed by either a billion-dollar company or an existential platform need.

**What "mainstream" actually requires:**

- A wedge (specific painful problem, specific audience) — Glyph has this.
- A platform or paradigm-shift tailwind — the AI-native coding wave is real, early, and narrow.
- A killer app written in the language.
- Full-time engineering investment (solo open-source paths like Python/Ruby took 15–20 years).
- Distribution work (docs, talks, book, community) that exceeds the compiler work.

**The realistic solo outcome:** beloved by 10K developers in a niche — still an excellent result, and a viable business foundation.

**The bet:** within five years, the median line of production code will be written by an agent and reviewed by a human. The languages that win that era will be the ones designed for that workflow, not retrofitted to it. TypeScript will eventually retrofit — it always does — but the window between now and then is where Glyph earns its place.

**The benchmark:** an agent given the same task produces correct code faster in Glyph than in TypeScript, and the human reviewer finishes in half the time. Every pillar, every example, every "no" exists to make that benchmark true.
