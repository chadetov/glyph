# Q43: ergonomic TS-library interop (the decision doc)

The make-or-break question for 1.0. Every hands-on tester (Serhiy, Hayk, Adi,
Ashfaq) hit the same wall: using a real npm library today takes a hand-written
adapter. This doc lays out what is actually broken, the concrete options with
their costs, and a recommendation. The point is to choose between costed shapes,
not to argue in the abstract.

## What breaks today

Import paths already pass through verbatim (`import zod { z }` emits
`import { z } from "zod"`), so calling a package is not the problem. Two things
are:

**A. Type availability.** `glyph build` runs `tsc --strict`, and the emitted
tsconfig ships `"types": []` (`runtime.rs:113`). So a package's types are not
loaded unless the user hand-writes a `.types/<pkg>.d.ts` stub. That stub is the
"adapter" testers complained about for simple libraries. This half is a config
and materialization problem, and it is tractable.

**B. Grammar expressibility.** Even with types, Glyph's grammar cannot express
some idiomatic API usage:
- prop spread: `<input {...register("name")} />` (react-hook-form). Glyph has no
  spread-into-JSX-props.
- value-derived types: `type T = z.infer<typeof schema>`. Glyph has `infer_output`
  but it is narrow and does not cover the general `typeof value` case.
- effectful custom hooks: a hook that calls `use_state`/effects is not `@pure`, so
  it cannot be JSX-called (this is Q44).

B is the hard half. It is not about types; the language literally cannot say what
the library's idiom needs. And B concentrates in React UI libraries. Backend and
data libraries (validators, API SDKs, database clients) are mostly data-shaped and
hit only A.

What already exists and helps: `glyph gen dts`, `gen openapi`, and `gen zod`
materialize an external schema into committed Glyph `type` declarations that carry
runtime descriptors. That is the right instinct for A, but it is a manual,
out-of-band CLI step today, not something the import path does.

## Options

### Option 1: auto-materialize types on import
When you import a package, the build finds its `.d.ts` (from `node_modules` or
`@types`) and runs the `gen dts` materializer to produce committed Glyph types and
descriptors for what you use.

- Solves A fully, and it is the only option that extends the verifiability wedge to
  the npm seam: a materialized record type gets a descriptor, so a value crossing
  the boundary is validated, not trusted. This is the core promise, kept at the
  place it currently leaks.
- Does not solve B. A materialized type still cannot express `z.infer` or a prop
  spread.
- Cost: high. `gen dts` handles the wire-correct 80% of schema-shaped `.d.ts`
  (objects, primitives, arrays, refs, optional/nullable). A general package `.d.ts`
  adds generics, conditional types, overloads, function types, and classes, plus
  in-build caching and incrementality.
- Best for data-shaped libraries. Weakest for callback-heavy UI libraries.

### Option 2: trust the installed `.d.ts` at the boundary (`extern`)
Install the package, fix the `"types"` config so its types load, and add a way to
name an external type in a Glyph type position. Glyph applies its rules to your
code and trusts the library's `.d.ts` as opaque.

- Solves A cheaply, and partially helps B (you can call more of the API without
  re-expressing its types).
- Does not extend the wedge. Library outputs are not runtime-validated; the
  boundary stays "trust the `.d.ts`," which is `any`-shaped risk at the exact seam
  the third review said is already the leaky spot. This buys ergonomics by
  spending the core promise.
- Cost: low. Mostly config plus a syntax to reference external types.

### Option 3: hybrid, phased (recommended shape)
- Phase 1, type availability (cheap, near-term): make installed package types load
  (the same `"types"` fix that unblocks Node builtins in 0.1.13 generalizes here),
  so any installed package typechecks with no stub.
- Phase 2, safe data boundaries: auto-materialize types for the record-shaped data
  you cross the boundary with (extend `gen dts`/`zod`/`openapi` to run on import),
  so DTOs, rows, and validated inputs get descriptors. The wedge extends where it
  matters.
- Phase 3, grammar gaps: a narrow, visible escape hatch at the call site (a scoped
  raw-TS expression, or an `extern` call) for the idioms Glyph cannot express, so
  nothing needs a whole adapter file. Plus build the highest-value primitives (a
  JSX prop-spread operator, generalizing `infer_output` toward value-derived types)
  only where the pain concentrates.
- Cost: highest total, but each phase ships value and Phase 1 is immediate.
- Safety: high where it matters, honest escape where the grammar cannot reach.

### Option 4 (scoping, not exclusive): backend-first
Aim Glyph at backend and data code for 1.0, where interop is almost entirely
problem A (materialize types, tractable), and treat React as a separate, later bet
or drop it. This does not replace Options 1 to 3; it shrinks Q43 to the half that
is solvable for 1.0 by removing the grammar-hostile UI cases from scope.

## Comparison

| | Solves A | Solves B | Keeps the wedge at the seam | Cost | 1.0-ready |
|---|---|---|---|---|---|
| 1 auto-materialize | yes | no | yes | high | data libs only |
| 2 trust `.d.ts` | yes | partial | no | low | fast but leaky |
| 3 hybrid | yes | yes (escape + primitives) | yes for data | highest | yes, phased |
| 4 backend-first | n/a | removes most of B | yes | scoping only | makes 1 or 3 tractable |

## Recommendation

Option 3 (phased hybrid) with Option 4 scoping applied. Concretely:

1. Phase 1 in 0.1.13/0.1.14: fix type availability so installed packages typecheck,
   and materialize data types on import so the boundary is validated, not trusted.
   This is the cheap, high-integrity unblock and it keeps the wedge at the seam.
2. Provide a narrow, visible escape hatch for residual grammar-hostile call sites,
   so a library Glyph cannot fully express still does not need an adapter file.
3. Build the specific primitives (JSX prop spread, value-derived types) only where
   the pain is highest, driven by the dogfood app, not speculatively.

The recommendation deliberately does not pick Option 2 alone: it is the cheapest
but it spends the one thing the third review said Glyph must not spend, which is
verifiability at the boundary.

## The decision the owner has to make

The fork that most changes the road is not 1 vs 2 vs 3. It is scope:

- **Backend-first.** Interop is mostly "materialize types + typecheck," which is
  tractable, and a 1.0 is reachable on the plan above. React is deferred or
  dropped, and Q44 goes away.
- **React-included.** You must also build the prop-spread and value-derived
  primitives and answer Q44 (Context, effectful hooks). That is a materially bigger
  1.0 and a longer road.

Pick the scope first. The mechanism (Option 3) follows from it.
