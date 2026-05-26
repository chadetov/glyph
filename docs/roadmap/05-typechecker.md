# Step 5 — Typechecker

Status: **planned, scope contested → partly resolved.** Full critique of the original framing in `archive/glyph_step5_notes.md`.

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

## The blocking decision

**Is `infer_shape<Shape>` v1 or v2?**

- **If v1:** mapped types cannot be skipped — they have just been renamed. Substep 5b is mandatory.
- **If v2:** the validator example (`01_validator.glyph`) needs to be rewritten *now* to use an explicit type parameter, so step 5's scope is honest.

This needs to be reconciled before step 5 starts, **not during it**. The manifesto and the examples currently write checks the typechecker step is not budgeting to cash.

## Tension with step 4 (transpiler)

The step-4 plan ships v0 with annotations-required, no real inference, no advanced generics, and a documented limitation around `infer_shape`. That's consistent with **substep 5a only** — substeps 5b and 5c would come *after* step 4 ships and the dogfooding loop (step 6) starts producing pressure. This reframing makes the timeline workable if we accept that "v1 typechecker" is really substep 5a, and 5b/5c are v1.1.

## Other open questions

- **Who writes the checker?** The 3–4 month estimate assumes prior compiler-construction experience. Without that, double it.
- **What's the error-message bar?** "tsc-quality errors" and "Elm-quality errors" are different projects. The LSP scoping doc (`07-lsp.md`) needs the answer to set launch gates.
- **`is TypeName` runtime descriptors.** Session 1 calls this "the single biggest implementation cost in the language" — every type needs a descriptor at every callsite. How is it represented and emitted?
- **Restricted JSX expression classifier.** The typechecker rule "only literals, identifier reads, member access, and pure calls inside `{...}`" requires a notion of "pure call." Annotation? Inference? Whitelist?

All tracked in `open-questions.md`.
