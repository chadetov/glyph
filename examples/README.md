# Glyph examples

The four hard-case example programs locked in step 2 (see `archive/SESSION_1.md`). These are the seed corpus for the transpiler test suite (step 4). Step 6 dogfooding (the fridge shopping list) grows this directory to ~30–50 example programs per the brainstorm Q2 resolution.

| File | Stresses | Pillars |
|---|---|---|
| `01_validator.glyph` | Type system + runtime descriptors + auto-generated schemas | Verifiability |
| `02_async_errors.glyph` | `Result` types + `?` propagation + `par.all`/`par.all_ok` | Verifiability + abstraction |
| `03_react_component.glyph` | JSX sub-grammar + compiler-owned directives (`<if>`, `<for>`, `<match>`, `<case>`, `<else>`) + restricted JSX expressions | Abstraction + greppability |
| `04_cli_tool.glyph` | Program entry + exhaustive subcommand dispatch + file I/O + process exit codes + structured logging | Greppability + diff stability |

## V1.0 deviations from the original step-2 corpus

`01_validator.glyph` differs from the version inline in `archive/GLYPH.md §3.1` to reflect the **brainstorm Q1 resolution** (defer mapped types to v1.1). The original used `Schema<infer_shape<Shape>>`; v1.0 uses an explicit `<Out>` type parameter supplied by the caller's type annotation. V1.1 will re-introduce `infer_shape` so the caller's type and the shape's fields stay in sync automatically.

The other three files are faithful transfers, with template literals (D22) used in places where the original used `+` concatenation. The semantics are unchanged.
