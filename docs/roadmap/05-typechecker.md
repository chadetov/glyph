# Step 5 — Typechecker

Status: **🟨 Substep 5a complete (v1 scope).** Full critique of the original framing in `archive/glyph_step5_notes.md`. For day-by-day progress, see `docs/implementation-plan.md`.

## Implementation status

Substep 5a is complete for the v1 scope (the deferrals listed below are v1.1 / later substeps). The salsa-tracked typechecker substrate (the `Q5 hybrid` architecture) is built and end-to-end through the CLI: parse → collect → resolve → assign types → render diagnostics → exit code. The Week-2 and Week-3 task lists are both fully done; the semantic checks are:

- **Match exhaustiveness** for user-defined tagged unions and prelude `Result`/`Option`. Patterns recognized: bare variant ident, `Variant(...)` constructor (any path length), `is Variant` guard (D9), `_`, `else`. Object/literal patterns over a union are conservatively skipped (no false positives).
- **Bool match exhaustiveness** — a `match` over a statically-typed `bool` scrutinee must cover both `true` and `false` or carry a catch-all, else `NonExhaustiveBoolMatch`. Number/string value-match exhaustiveness stays deferred to v1.1 (only an `else` can make those finite, a thin add-on).
- **`?` operator rule** — `expr?` is rejected outside a function that returns `Result` (`QuestionOutsideResultFn`). The operand side is checked too: a decidably non-`Result` operand is rejected (`QuestionOnNonResult`), the `?` expression unwraps to the operand's success type `T`, and the operand's `E` must equal the enclosing function's `E` exactly — no `From` in v1 (`QuestionErrorTypeMismatch`). Each rule gates on decidability so none fires on a type it cannot judge.
- **Call-expression + `await` synthesis** (day 16) — a call's type comes from the callee's signature; `await e` synthesizes `e`'s type (no user-visible `Promise<T>`).
- **Match-arm payload typing** (days 17–18) — `Ok(v) => v` types `v`; object-pattern fields `Variant({ field })` typed from the variant payload.
- **Generic instantiation at call sites** (day 20) — `fn id<T>(x: T) -> T` called with a `number` types as `number`.
- **Return-type mismatch** (day 21) — a `return` of a concrete primitive that differs from the declared primitive return type is flagged (`TypeMismatch`); deliberately narrow so it never fires on a type it can't judge.
- **`owned` single-consumption** (D25) — done. A `let owned h: ResourceType` handle must be consumed exactly once on every path; a consume is a *move* into an `owned` parameter (Model A). Emits `OwnedNotConsumed` (forget / return-while-live), `OwnedUsedAfterMove` (double-consume / use-after-move), and `OwnedRequiresResourceType`. v1 scope: free-function consumers and `match` + `return` + `?` branching — a `?`'s Err-path early return leaks any handle still live, reported like a `return`. Cross-iteration loop double-consume, method/namespaced consumes (`h.close()`, `fs.close(h)`), and lambda/JSX capture remain conservative no-ops deferred to dogfooding (they need member-access type synthesis and stdlib signatures). Manifesto carve-out.
- **Nested-pattern exhaustiveness** (day 25) — done for constructor nesting. A variant reached only through a single payload sub-pattern has that payload checked recursively, so `match r { Ok(Some(x)) => .., Err(e) => .. }` over `Result<Option<T>, E>` now flags the missing `Ok(None)`. Arbitrary depth; works for module-local unions and prelude `Result`/`Option`. Object/array/literal nested payloads and generic module-local unions applied through `Ty::App` are conservatively treated as fully covered (no false positives).
- **Array-match exhaustiveness** (day 26) — done. A `match` over an array scrutinee (`App(Array, [T])`) must cover every length: `[]` covers the empty array, `[a, b]` exactly length two, `[a, ...rest]` every length ≥ its fixed prefix. Coverage is credited only for irrefutable elements, so the `04_cli_tool` idiom (literal-first arms plus a trailing `[other, ..._]` and `[]`) is exact with no false positive. Emits `NonExhaustiveArrayMatch` naming the smallest uncovered length. Object patterns over a record are single-shape, so they need no separate exhaustiveness check.

What is deferred past substep 5a (v1.1 / later substeps):

- **Fuller bidirectional checker** — broader assignability beyond primitives, and a real unifier (generic instantiation today is call-site substitution, not full unification). The unifier also unblocks exhaustiveness for generic module-local unions applied through `Ty::App`, which are skipped today, and the general Maranget matrix over products of independent refutable columns. The operand-side `?` `E`-match is now done (it compares the decidable error types directly).
- **Runtime descriptors** (Q8) — non-generic `record` AND non-generic tagged `union` declarations emit a runtime descriptor in the generated TS: an `is` type guard plus a self-contained `parse` (validates an `unknown` into an inlined `Result` shape, so no `std/result` import is needed) and a `T.schema` member. `is TypeName` match patterns lower to a check that calls the descriptor's `is`, so the descriptor is both emitted and consumed. The guard is shallow (records check each field's presence/`typeof`; unions check the discriminant tag) — the documented v1 validation scope. Remaining (v1.1): descriptors for **generic** records/unions (their type arguments live at the call site), deep/recursive field validation, and wiring `T.parse` into the typechecker so Glyph source can call it.

Substep 5c (narrowing + flow analysis) and substep 5b (deferred to v1.1 per Q1) come after 5a.

## Updates from brainstorm session 1 (2026-05-26)

- **Q1 → v2 (defer mapped types).** Substep 5b (mapped-type-like behavior for `infer_shape<Shape>`) is **no longer mandatory for v1**. It moves to v1.1.
- **Revised time estimate: ~9 weeks** (was ~13): substep 5a (~6w) + substep 5c (~3w). Substep 5b drops out of v1 critical path.
- Composes with Q5 hybrid architecture (`docs/roadmap/04-transpiler.md`): the typechecker is the salsa-backed component, not the visitor.

Remaining step-5 open questions: Q6 (error-message bar), Q7 (`mut` semantics at the typechecker level), Q8 (`is TypeName` runtime descriptors), Q9 (restricted JSX purity classifier). See `docs/open-questions.md`.

## Updates from brainstorm session 3 (2026-05-26)

All four step-5-load-bearing questions resolved, plus the manifesto-touching Q24 owned modifier becomes typechecker scope:

- **Q6 → Elm-quality error messages.** The bar is concrete: when the typechecker rejects code, the message must tell the agent (or human) exactly what to change. Concrete bar replaces "make it fast." This shapes substep 5a from week 1 — every type error gets a structured rejection with the relevant source spans and a one-line suggestion. Budget ~15% of step 5 for error message authoring.
- **Q7 → `mut` is syntactic only.** D5 grammar restricts `mut` to assignments and method calls. The typechecker does NOT verify that called methods mutate. Pure-method annotation on every stdlib method would be too heavy and would make stdlib evolution painful. `mut` reads as documentation; the typechecker trusts the grammar restriction.
- **Q8 → runtime descriptors at every type declaration.** Every `type`/`record` declaration emits a runtime descriptor in the generated TS. The descriptor includes field names, field types, and an auto-generated parse function. This is what makes `is TypeName` runtime checks work, and what `User.parse(input)` calls. **Non-negotiable** — core to verifiability.
- **Q9 → JSX purity via whitelist.** Stdlib functions are pure by convention (their identity is enough). User functions require explicit `@pure` annotation (per D27) to be callable inside JSX `{...}` expressions. No automatic purity inference in v1.
- **Q24 → narrow `owned` modifier (D25).** Typechecker scope grows by ~1 substep. The typechecker tracks single-consumption across every code path for `owned`-modified bindings. Forgetting to consume = compile error. Double-consume = compile error. Returning without consuming = compile error. **Manifesto carve-out** — see `docs/manifesto.md`.

**Time estimate stays at ~9 weeks.** The Q24 owned-tracking substep (~1 week) fits inside substep 5a's existing buffer; substep 5b (mapped types) is still deferred to v1.1.

## What the original strategy said (now rejected)

> Build the typechecker (4–6 weeks, overlaps with 4). Hindley-Milner core + the TS-compatible features you actually need. Skip conditional, mapped, and template literal types in v1.

This framing is wrong on three counts:

1. **HM core is the wrong starting point.** HM is built for an ML-like core: everything is a function, records are named tuples, no flow sensitivity. The four hard-case examples need substantially more on day one — flow-sensitive narrowing for `match` and tagged-union dispatch, sum types with payload binding, mapped-type-like behavior for `infer_shape<Shape>`.
2. **The estimate is off by 2–3×.** A bidirectional checker with ADTs, narrowing, generics, and respectable error messages is a 3–4 month job for someone who has built one before, longer otherwise.
3. **"Match 80% of TS" is the wrong target.** TS's type system is shaped by JavaScript's runtime — structural typing, `any`, erasure, conditional/mapped/template-literal machinery. The right reference points for Glyph are **Rust** (ADTs, exhaustiveness, narrowing), **ReScript / ReasonML** (HM-ish with records and variants), and **Roc** (closest match to the error-as-value model).

## The real v1 floor

To make the four existing examples typecheck honestly, v1 needs:

- **Bidirectional checking** (not pure HM inference — TS users will annotate function signatures and expect bodies checked against them; bidirectional is also how pattern matching and ADTs become ergonomic).
- **Sum types with exhaustive matching and payload binding.**
- **Flow-sensitive narrowing** for `match` and tagged-union dispatch.
- **Structural records with width subtyping.**
- **Generics with constraints.**

This is the floor, not the ceiling.

## Proposed three-substep restructure

| Substep | Scope | Estimate |
|---------|-------|----------|
| **5a Surface typechecker** | Bidirectional. ADTs with exhaustive `match`. Structural records. Generics with simple bounds. `Result` propagation as a typing rule (not a desugar). **Acceptance:** four example files typecheck end-to-end and produce real errors when broken. | ~6 weeks |
| **5b Inference quality** | The `infer_shape<Shape>` work — limited type-level computation, enough to cover stdlib patterns. **Do not generalize.** Pick the half-dozen shapes that matter (object schema inference, array element extraction, result unwrapping) and special-case them. Generalization can land later; un-generalization cannot. | ~4 weeks |
| **5c Narrowing + flow analysis** | The piece that makes `match` and tagged-union dispatch feel native. Cheaper than it sounds once ADTs are solid. | ~3 weeks |

**Total: ~13 weeks of focused work**, assuming nothing else slips. If the rest of the plan slots this as a 4–6 week line item, the plan breaks against it.

## The blocking decision (resolved)

**Is `infer_shape<Shape>` v1 or v2?** → **v2 (Q1 resolution, 2026-05-26 brainstorm).**

- Mapped types deferred to v1.1.
- `01_validator.glyph` was rewritten with an explicit output type parameter before substep 5a began.
- Substep 5b is **not** on the v1 critical path.
- **v1 stand-in (emitter):** the explicit-`<Out>` validator builds a `Record<string, unknown>` and returns it as the caller's `Out`. With no `as` in Glyph, the emitter casts a generic function's return value to its declared return type when that type references one of the function's type parameters (`return { ... } as Schema<Out>`), so the asserted (caller-responsible) value type-checks. Gated to generic returns, so non-generic returns stay precisely checked. v1.1's `infer_shape` makes the assertion unnecessary by deriving `Out` from the shape.

## Tension with step 4 (transpiler)

The step-4 plan ships v0 with annotations-required, no real inference, no advanced generics, and a documented limitation around `infer_shape`. That's consistent with **substep 5a only** — substeps 5b and 5c would come *after* step 4 ships and the dogfooding loop (step 6) starts producing pressure. This reframing makes the timeline workable if we accept that "v1 typechecker" is really substep 5a, and 5b/5c are v1.1.

## `Result`/`Option` combinator methods vs. the `?` operator (resolved)

The examples call `result.map_err(f)` — a method on a `Result` value — so the
prelude `Result` carries `map`/`map_err`. Those methods make `Result` vary in
`T`, which would clash with the `?` operator: `?` propagates an `Err` of
`Result<X, E>` from a function returning `Result<Y, E>`, so an `Err` must be
assignable across success types, and a `map<U>(f: (value: T) => U)` breaks that.

**Resolved by re-wrapping the propagated error.** The `?` lowering emits
`if (__r.tag === "Err") { return Err(__r.value); }` (an aliased `__glyph_err`)
rather than `return __r`, so the propagated value is `Result<never, E>` —
`never` in the success position is assignable to any `Result<Y, E>` regardless
of the methods. A module that uses `?` gets a generated `import { Err as
__glyph_err } from "std/result"`. With this, `04_cli_tool` passes `tsc
--strict`. Still open and coupled to this: the `T.schema` descriptor member
(`User.schema`, `Post.schema.array()`), whose `parse` must return the same
`Result`/`Schema` shape.

## Other open questions

- **Who writes the checker?** The 3–4 month estimate assumes prior compiler-construction experience. Without that, double it.
- **What's the error-message bar?** "tsc-quality errors" and "Elm-quality errors" are different projects. The LSP scoping doc (`07-lsp.md`) needs the answer to set launch gates.
- **`is TypeName` runtime descriptors.** Session 1 calls this "the single biggest implementation cost in the language" — every type needs a descriptor at every callsite. How is it represented and emitted?
- **Restricted JSX expression classifier.** The typechecker rule "only literals, identifier reads, member access, and pure calls inside `{...}`" requires a notion of "pure call." Annotation? Inference? Whitelist?

All tracked in `open-questions.md`.
