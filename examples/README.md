# Glyph examples

The four hard-case example programs locked in step 2 (see `archive/SESSION_1.md`). These are the seed corpus for the transpiler test suite (step 4). Step 6 dogfooding (the fridge shopping list) grows this directory to ~30–50 example programs per the brainstorm Q2 resolution.

| File | Stresses | Pillars |
|---|---|---|
| `01_validator.glyph` | Type system + runtime descriptors + auto-generated schemas | Verifiability |
| `02_async_errors.glyph` | `Result` types + `?` propagation + `par.all`/`par.all_ok` | Verifiability + abstraction |
| `03_react_component.glyph` | JSX sub-grammar + compiler-owned directives (`<if>`, `<for>`, `<match>`, `<case>`, `<else>`) + restricted JSX expressions | Abstraction + greppability |
| `04_cli_tool.glyph` | Program entry + exhaustive subcommand dispatch + file I/O + process exit codes + structured logging | Greppability + diff stability |
| `05_rest_api.glyph` | `std/http` server + typed request/response DTOs + descriptor-validated request bodies (`T.parse`) + auth check, all errors-as-values | Verifiability + greppability |

## V1.0 deviations from the original step-2 corpus

`01_validator.glyph` differs from the version inline in `archive/GLYPH.md §3.1` to reflect the **brainstorm Q1 resolution** (defer mapped types to v1.1). The original used `Schema<infer_shape<Shape>>`; v1.0 uses an explicit `<Out>` type parameter supplied by the caller's type annotation. V1.1 will re-introduce `infer_shape` so the caller's type and the shape's fields stay in sync automatically.

The other three files are faithful transfers, with template literals (D22) used in places where the original used `+` concatenation. The semantics are unchanged.

## `corpus/` — self-contained regression programs

`corpus/` holds small programs that depend on no stdlib or external modules (no `Result`/`Option` imports, no `react`, no `std/*`). Each exercises one emitter feature in isolation, and — because nothing is left untyped — its emitted TypeScript passes `tsc --strict --noEmit` end to end. The four hard-case examples above instead import external/stdlib modules, so their emitted code is type-correct only once those modules' types exist; the corpus is what proves the emitter itself produces fully `tsc`-clean output today.

| File | Stresses |
|---|---|
| `shapes.glyph` | Tagged union + exhaustive constructor-pattern match |
| `maybe.glyph` | Generic union + payload binding |
| `sum.glyph` | `for` loop accumulating into a `mut` binding |
| `list_ops.glyph` | Array-pattern match with a `...rest` binding |
| `classify.glyph` | Value (literal) match with an `else` catch-all |
| `higher_order.glyph` | Records + higher-order functions + lambdas |
| `generics.glyph` | Generic functions + explicit call-site type arguments |
| `tree.glyph` | Recursive tagged union + recursive function |
| `async_chain.glyph` | `async`/`await` functions awaited in sequence |

`repo_examples_emit_typescript_without_diagnostics` (in `glyph-cli`'s integration tests) builds the whole `examples/` tree — these plus the four hard-case files — and asserts every module emits with no diagnostics.
