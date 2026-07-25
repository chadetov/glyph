# Adopting Glyph in a TypeScript project

You do not rewrite anything. Glyph compiles to plain, readable TypeScript, so a
Glyph module is just another source of `.ts` your existing build already knows
how to consume. Adoption is file by file, and reversible.

## The shape of it

1. **Point Glyph at a directory.** `glyph build src/glyph --out src/generated`
   emits one `.ts` per `.glyph`. Commit the generated `.ts` (or gitignore it and
   build in CI, your call), and import it from your TypeScript exactly like any
   other module.
2. **Start with a leaf.** Pick a module with few dependents, a validator, a small
   service, a set of pure helpers, and write it in Glyph. Its emitted `.ts`
   exports ordinary functions and types your existing code calls unchanged.
3. **Consume npm from Glyph, not around it.** Glyph imports installed packages
   directly (`import zod { z }`); the package's own types are enforced. For
   deep boundary validation, materialize a package's types with `glyph gen dts
   <package>`.
4. **Grow the edge.** Each time you touch a risky module, consider moving it. The
   TypeScript around it never has to know.

## Interop specifics

- **Calling Glyph from TypeScript:** import the emitted `.ts`. A `pub fn` is an
  `export function`; a `type` is an `export type` plus a runtime descriptor.
- **Calling TypeScript from Glyph:** `import` the module by path; add a
  `.types/*.d.ts` stub for anything untyped, or `glyph gen dts` to materialize
  real types. Node builtins work out of the box.
- **Types that Glyph can't spell:** the `extern_ts("...")` escape hatch names a
  raw TypeScript type or expression inline, so no library ever forces a
  hand-written adapter file. Reach for it rarely (see
  [anti-patterns](anti-patterns.md)).

## Keeping generated code fresh

Every file `glyph gen` writes records its own command in a header. `glyph regen`
re-runs them, so a spec or dependency bump flows into the committed Glyph with one
command.

## What not to expect yet

- A runtime stack trace from `glyph run` still points at the emitted `.ts`;
  build-time and `tsc` diagnostics map back to `.glyph`.
- A type imported from a `.d.ts` you only reference (didn't materialize) is
  presence-checked, not deeply validated, until you run `glyph gen dts` on it.

The whole point is that there is no flag day. A Glyph file and the TypeScript it
produces sit side by side, and you move the boundary at whatever pace the risk
justifies.
