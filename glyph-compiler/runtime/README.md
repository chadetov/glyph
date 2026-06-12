# Glyph runtime

The runtime prelude and standard-library type surface that emitted Glyph
programs link against. This is what lets `tsc --strict --noEmit` type the output
of `glyph build` against real types rather than `any`.

## Layout

| File | What |
|---|---|
| `std/result.ts` | The `Result<T, E>` type + `Ok`/`Err`. Real runtime: the flat `{ tag, value }` wire format the emitter targets. |
| `std/option.ts` | The `Option<T>` type + `Some`/`None`, same wire format. |
| `glyph-prelude.d.ts` | The names usable without an import: `par`, `print`, the `number` namespace, and the `Schema<T>` / `Issue` types. |
| `glyph-stdlib.d.ts` | Type declarations for the v1 stdlib modules (`std/array`, `string`, `io`, `json`, `fs`, `process`, `http`, `time`). Higher-order functions are generic so callback parameters infer from the call site. |

`result.ts` and `option.ts` are real `.ts` (the prelude ships executable
runtime); the rest are `.d.ts` type stubs until the stdlib is implemented in
Glyph source (Q3). The wire format is single-sourced here and in the emitter
(`glyph-emit`'s `TAG`/`PAYLOAD` constants).

## The wire format and the `?` operator

A `Result`/`Option` is a flat object discriminated by a string `tag`, payload
under `value`. The `?` operator propagates an `Err` by returning it, so an `Err`
of `Result<X, E>` must be assignable to a function returning `Result<Y, E>` for
any `X`, `Y`. That requires the payload to stay a plain value with **no
`T`-dependent methods** — so `Result` carries no `map`/`map_err` methods yet.
Combinator methods are a separate design item (they conflict with `?`'s
cross-success-type propagation unless `?` re-wraps the error); see
`docs/roadmap/05-typechecker.md`.

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

The self-contained `examples/corpus/` programs (which use no stdlib) already
pass `tsc --strict` standalone. The four hard-case examples link this runtime;
their remaining `tsc` errors are documented language gaps — chiefly flow
narrowing (Phase 2) and the `Result` combinator design — not emitter defects.
