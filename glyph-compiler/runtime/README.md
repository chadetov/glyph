# Glyph runtime

The runtime prelude and standard-library type surface that emitted Glyph
programs link against. This is what lets `tsc --strict --noEmit` type the output
of `glyph build` against real types rather than `any`.

## Layout

| File | What |
|---|---|
| `std/result.ts` | The `Result<T, E>` type + `Ok`/`Err`, with `map`/`map_err`. Real runtime: the flat `{ tag, value }` wire format the emitter targets. |
| `std/option.ts` | The `Option<T>` type + `Some`/`None`, same wire format. |
| `std/schema.ts` | The `schema()` factory behind a record type's auto-generated `T.schema` member (it carries the recursive `array()`). |
| `glyph-prelude.d.ts` | The names usable without an import: `par`, `print`, the `number` namespace, and the `Schema<T>` / `Issue` types. |
| `glyph-stdlib.d.ts` | Type declarations for the v1 stdlib modules (`std/array`, `string`, `io`, `json`, `fs`, `process`, `http`, `time`). Higher-order functions are generic so callback parameters infer from the call site. |

`result.ts` and `option.ts` are real `.ts` (the prelude ships executable
runtime); the rest are `.d.ts` type stubs until the stdlib is implemented in
Glyph source (Q3). The wire format is single-sourced here and in the emitter
(`glyph-emit`'s `TAG`/`PAYLOAD` constants).

## The wire format, combinators, and the `?` operator

A `Result`/`Option` is a flat object discriminated by a string `tag`, payload
under `value`. `Result` also carries the combinator methods `map`/`map_err`, so
`result.map_err(f)` works directly.

Those methods make `Result` vary in `T`, which would clash with the `?`
operator — `?` propagates an `Err` of `Result<X, E>` from a function returning
`Result<Y, E>`. It is sound because the `?` lowering **re-wraps** the error
(`return Err(__r.value)`, a `Result<never, E>`): `never` in the success position
is assignable to any `Result<Y, E>` regardless of the methods. The emitter
generates the `Err` import this needs. See `docs/roadmap/05-typechecker.md`.

## Typechecking emitted output

`glyph build` does not yet generate a `tsconfig.json` (a later slice). To check
the emitted `.ts` against these types today, point `tsc` at a config that maps
`std/*` to this directory and includes the prelude declarations:

```jsonc
{
  "compilerOptions": {
    "strict": true, "noEmit": true,
    "target": "es2022", "lib": ["es2022", "dom"],
    "module": "esnext", "moduleResolution": "bundler",
    "baseUrl": "<glyph-compiler/runtime>",
    "paths": { "std/*": ["std/*"] }
  },
  "include": [
    "<dist>/**/*.ts",
    "<glyph-compiler/runtime>/*.d.ts",
    "<glyph-compiler/runtime>/std/*.ts"
  ]
}
```

A program with external dependencies also supplies their types; the example
programs' React and `api/users` stubs live in `examples/.types/`.

The self-contained `examples/corpus/` programs (which use no stdlib) pass
`tsc --strict` standalone, and **all four hard-case examples pass** linked
against this runtime (and the React/`api/users` stubs in `examples/.types/`).
That is the Phase 1 Week 4 gate, fully met. (`01_validator`'s `object_schema<Out>`
returns a `Record<string, unknown>` as the caller's `Out`; with no `as` in
Glyph, the emitter casts a generic function's return to its declared type — the
v1 stand-in for infer_shape, Q1.)
