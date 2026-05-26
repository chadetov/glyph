# Glyph Build Plan — Step 5 (Typechecker) Review

## The original proposal

> Build the typechecker (4–6 weeks, overlaps with 4). Start with a
> Hindley-Milner core and add the TS-compatible features you actually need
> (union types, generics, structural-with-nominal-opt-in). Don't try to
> match TS's full type system — match the 80% that real code uses. Skip
> conditional types, mapped types, and template literal types in v1.

## Why this framing is off

### 1. "HM core" is the wrong starting point

Hindley-Milner is built for a tiny ML-like core: everything is a function,
records are named tuples, no flow sensitivity. Glyph's examples need
substantially more than that on day one:

- **`01_validator.txt`** — `Schema<infer_shape<Shape>>` is a type-level
  function over a record of schemas. That is mapped-type territory.
  The manifesto-aligned example does not work without it.
- **`02_async_errors.txt`** — `?` propagation requires unifying the `E`
  of every `?` in a function body with the function's declared error
  type. Doable in HM, but only with `Result` as a privileged type.
- **`03_react_component.txt`** — `<case Loaded bind={users}>` requires
  narrowing the sum type *and* binding the payload. That is
  flow-sensitive narrowing — a different algorithm family from HM.
- **`04_cli_tool.txt`** — `parse_args` matches on array patterns
  (`["add", ...rest]`). Exhaustiveness on ADTs + array patterns is
  its own subsystem.

### 2. The real v1 floor

To make the four existing examples typecheck honestly, v1 needs:

- Bidirectional checking (not pure HM inference — TS users will annotate
  function signatures and expect bodies checked against them; bidirectional
  is also how you make pattern matching and ADTs ergonomic).
- Sum types with exhaustive matching and payload binding.
- Flow-sensitive narrowing for `match` and tagged-union dispatch.
- Structural records with width subtyping.
- Generics with constraints.

This is the floor, not the ceiling.

### 3. The estimate is off by 2–3×

A bidirectional checker with ADTs, narrowing, generics, and respectable
error messages is a 3–4 month job for one strong engineer who has built
one before, longer otherwise. Six weeks ships something whose errors are
worse than `tsc --strict`, which directly undercuts the manifesto's
verifiability pitch.

### 4. "Match 80% of TS" is the wrong target

TS's type system is shaped by JavaScript's runtime — structural typing,
`any`, erasure, and the conditional/mapped/template-literal machinery
exist largely to model libraries that abuse JS dynamism. Glyph's
manifesto explicitly rejects all of that. The right reference points:

- **Rust** — ADTs, exhaustiveness, narrowing.
- **ReScript / ReasonML** — HM-ish inference layered with records and
  variants.
- **Roc** — closest match to Glyph's error-as-value model.

Not TS.

## Proposed restructuring

Split step 5 into three sub-steps:

### 5a. Surface typechecker — ~6 weeks

Bidirectional. ADTs with exhaustive `match`. Structural records.
Generics with simple bounds. `Result` propagation as a typing rule
(not a desugar). **Acceptance criterion:** the four example files
typecheck end-to-end and produce real errors when broken.

### 5b. Inference quality — ~4 weeks

The `infer_shape<Shape>` work — limited type-level computation, just
enough to cover the stdlib patterns. Do *not* generalize. Pick the
half-dozen shapes that matter (object schema inference, array element
extraction, result unwrapping) and special-case them. Generalization
can land later; un-generalization cannot.

### 5c. Narrowing + flow analysis — ~3 weeks

The piece that makes `match` and tagged-union dispatch feel native.
Cheaper than it sounds once ADTs are solid.

**Total: ~13 weeks of focused work**, assuming nothing else slips. If
the rest of the plan slots the checker as a 4–6 week line item, the
plan breaks against it.

## The decision that has to happen now

Is `infer_shape<Shape>` v1 or v2?

- **If v1:** mapped types cannot be skipped — they have just been
  renamed. Step 5b is mandatory.
- **If v2:** the validator example in the project needs to be rewritten
  *now* to use an explicit type parameter, so step 5's scope is honest.

Right now the manifesto and the examples write checks the typechecker
step is not budgeting to cash. This needs to be reconciled before
step 5 starts, not during it.

## Open questions

- What are steps 1–4 and 6+? Right scoping for step 5 depends on whether
  there is a v2 milestone where harder inference can land, or whether
  this is the whole checker forever.
- Who is writing the checker? The 3–4 month estimate assumes prior
  experience building one. Without that, double it.
- What is the error-message bar? "Tsc-quality errors" and "Elm-quality
  errors" are different projects.
