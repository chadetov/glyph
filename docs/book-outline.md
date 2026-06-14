# Book outline: *Programming in Glyph*

A working outline, not a manuscript. Its job today is to force the gaps to be
confronted: any chapter that cannot yet be written points at a language or
tooling hole worth tracking. When a chapter is blocked, the blocker is named
inline.

Audience: a working TypeScript developer, and the AI agents that pair with them.
The book teaches the language and the *why* — the four pillars — together, so
the restrictions read as design rather than as friction.

## Part I — The case

1. **Why a language for agents.** The thesis: code is now read and written by
   agents as much as humans; the bugs that result are predictable; a language
   can design them out. The four pillars (verifiability, greppability,
   abstraction, diff stability) and the wedge.
2. **Five minutes of Glyph.** The whole language at a glance, then the promise:
   it compiles to TypeScript you already trust.

## Part II — The language

3. **Modules and the shape of a file.** `module`, full-path imports, no barrel
   files, why discoverability beats convenience.
4. **Values and types.** Primitives, `number`, records (no shorthand, trailing
   commas), arrays, the absence of `any`.
5. **Functions.** One declaration form, parameters and returns, lambdas,
   higher-order functions, greppability as a property you can rely on.
6. **Branching is matching.** No `if`; `match` as the single, exhaustive,
   value-producing construct over literals, booleans, arrays, and unions.
7. **Tagged unions.** Modeling a domain as data; payloads; exhaustiveness as a
   refactoring tool ("the compiler maintains your switches").
8. **Errors are values.** `Result`, `Option`, `match`, and `?`; the exact-`E`
   rule; why no implicit conversions and no exceptions.
9. **Mutation, deliberately.** `let` vs `mut`, `for`, `loop`; computing new
   values over mutating old ones.
10. **Generics.** Type parameters, generic unions and functions, the v1 limits.
11. **Resources with `owned`.** The single affine feature: files, sockets, db
    connections, single-consumption. *Blocker to flesh out:* the remaining
    `owned` consume forms (method/namespaced/loop/closure) are deferred past v1.
12. **Components and JSX.** `component`, the directive lowering (`<if>`/`<for>`/
    `<match>`), how it becomes `React.createElement`.

## Part III — The toolchain

13. **The build.** `glyph build`, the emitted TypeScript, the `tsc --strict`
    gate, what the output looks like and why it is readable.
14. **Running and formatting.** `glyph run`, `glyph fmt` and the one-layout
    stance, diff stability in practice.
15. **Tests next to code.** `@example`, `@doc @run`, property tests; running
    them on every build.
16. **The editor and the agent channel.** The LSP (diagnostics, hover,
    navigation), the canonical view, and the gated structured edit
    (`applyEdit`) that makes "the agent broke the file" impossible.
17. **Packaging.** The `"glyph"` key, audit-currency, `glyph publish`, npm
    distribution. *Blocker:* standalone-library bundling (specifier rewriting)
    is not yet shipped.

## Part IV — Working with agents

18. **Writing Glyph an agent can edit.** Patterns that keep diffs small and
    greppable; what the canonical view buys you.
19. **Reviewing agent changes.** Reading diffs, trusting the type system,
    where human judgment still belongs.
20. **Migrating TypeScript to Glyph.** Strategy, what does not translate,
    interop through TS wrappers. *Blocker:* no automated migration tool in v1
    (Q-deferred).

## Appendices

- **A. The D-decisions.** Every numbered language decision and its pillar.
- **B. Error codes.** The catalogue, with fixes.
- **C. Grammar.** The formal grammar.
- **D. Differences from TypeScript.** The quick-reference table.

## Known gaps this outline surfaces

- `owned` is narrower in v1 than a chapter implies (Ch. 11).
- Standalone library publishing needs bundling (Ch. 17).
- No automated TS→Glyph migration (Ch. 20).
- Type-driven generation (`glyph regen`, Q40) and the dual human/agent view
  (Q32 SSA + position mapper) are research tails, not yet book-ready.
