# Glyph manifesto

A statically typed, transpile-to-TypeScript language for production systems where AI agents are first-class collaborators on the codebase.

Full original text: `archive/MANIFESTO.md`.

## What Glyph is

It looks almost like TypeScript. A TS developer reads a Glyph file on day one without a tutorial. The differences are deliberate and small in number — every one of them exists to make code that an agent can reason about correctly, edit without breakage, and explain back to a human without lying.

Glyph is not a research language. No effect systems, no dependent types, no macros. The "no linear types" rule has exactly one v1 carve-out: a narrow `owned` modifier for resource handles (file/socket/db-connection) with single-consumption tracking — covering TS developers' real pain (forgotten `.close()`, leaked connections; cf. TC39 `using` stage 3) without committing to a general affine/linear system. See `docs/language/spec.md` D25. Otherwise: TypeScript with the ten footguns removed and four properties enforced — nothing more, nothing less.

## Who Glyph is for

TypeScript developers building with AI agents who feel the daily friction:

- Agents hallucinating APIs because TS's type erasure means runtime and compile-time disagree.
- Agents papering over uncertainty with `any` and `as unknown as T`.
- Agents rewriting whole files when asked to change one line, because the formatter cascades on every edit.
- Agents grepping for a symbol and finding ten unrelated matches because TS overloads syntactic forms.
- Reviewers who can't tell whether a PR is correct because the diff is twelve hundred lines of reflowed whitespace.

Glyph removes those costs. If you are building a Lisp for AI, a DSL for prompts, or a logic language for reasoning systems — Glyph is not for you.

## The four pillars

Every design decision is tested against these. If a feature improves one without harming the others, it ships. If not, it doesn't.

### 1. Abstraction
Express intent at the level the writer is thinking. Pattern matching over switch ladders. `Result` over thrown exceptions. Named records over positional tuples. A small core of orthogonal primitives instead of TS's accreted layers.

### 2. Verifiability
Anything the type system claims must be true at runtime. No `any`. No structural-typing surprises. No type erasure — every Glyph type has a runtime descriptor available when needed. The compiler is the source of truth, and the source of truth is enforceable.

### 3. Diff stability
A one-line change produces a one-line diff. Fixed-width, single-element-per-line formatting — never line-length-based reflow. Explicit, full-path imports. No barrel files. Trailing commas everywhere. Sorted imports.

### 4. Greppability
Every symbol has exactly one syntactic form at its declaration site. No method overloads, no decorators that rename, no implicit `this`, no namespace merging. `grep -n "fn parseUser"` finds the definition. Always.

**Weighting.** Verifiability and greppability are the wedge — they fix problems TypeScript developers actually feel and other languages don't solve. Abstraction and diff stability are the polish that makes daily use pleasant. When pillars conflict, the wedge wins.

## What Glyph deliberately is not

- **Not a research language.** If TypeScript developers don't already wish they had it, it doesn't ship.
- **Not a Lisp or DSL for AI.** General-purpose application language. Agents are the reader, not the runtime.
- **Not a TS replacement.** Glyph compiles to TS, imports from npm, is importable from `.ts` files. Adoption is per-file.
- **Not configurable.** One formatter, no options. One module resolution algorithm. One strictness level (strict). The cost of configurability is paid by every agent that has to reason about which dialect it's editing.
- **Not self-hosting in v1.0.** The compiler is Rust forever, or at least until v1.0 ships and users exist. Self-hosting was promoted from a year-three deferral to a v1.0 non-goal in session 2 (`archive/glyph_step6_session.md`).

## The bet

Within five years, the median line of production code will be written by an agent and reviewed by a human. The languages that win that era will be the ones designed for that workflow, not retrofitted to it. TypeScript will retrofit eventually — it always does. The window between now and then is where Glyph earns its place.

The benchmark: an agent given the same task produces correct code faster in Glyph than in TypeScript, and the reviewer finishes in half the time. Every pillar, every example, every "no" exists to make that benchmark true.
