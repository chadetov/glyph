# Error codes

Every diagnostic the compiler emits carries a stable code. The code appears in
the rendered error (`[E0200] Error: ...`), and `glyph --explain <code>` prints a
longer explanation with a fix example. Codes are allocated by compiler phase:

| Range | Phase | Source |
|-------|-------|--------|
| `E000x` | Parser | `glyph-parser` |
| `E01xx` | Resolver (collect / resolve / import) | `glyph-resolver` |
| `E02xx` | Typechecker | `glyph-typechecker` |
| `E03xx` | Emitter | `glyph-emit` |

A code, once allocated, is never reused for a different meaning. When a new
error path is added, allocate the next free code in its phase range, give the
error a `code()` and one-line `help()`, and add an `--explain` entry plus a row
below.

## Catalogue

### Parser — `E000x`

| Code | Meaning |
|------|---------|
| `E0001` | Lexical error (unterminated string, invalid escape, stray character) |
| `E0002` | Expected a different token (Glyph is stricter than TS) |
| `E0003` | Unexpected token in this position |
| `E0004` | Expected end of file (likely an unbalanced brace) |
| `E0005` | Construct recognized but not implemented |

### Resolver — `E01xx`

| Code | Meaning |
|------|---------|
| `E0100` | Duplicate top-level name |
| `E0101` | Relative import (use an absolute module path; D15) |
| `E0102` | Barrel file: only imports, no declarations (D15) |
| `E0103` | Unresolved name |
| `E0104` | Unresolved module path |
| `E0105` | Name not exported by the imported module |
| `E0106` | Unused import (warning) |
| `E0107` | Unused variable binding (warning) |
| `E0108` | Unreachable code after `return`/`break`/`continue` (warning) |

`E0106`–`E0108` are the lint tier: warnings, not errors. They surface in the
build output but never fail the build or block emission. `E0107` exempts names
led by `_` (the conventional "intentionally unused" marker).

### Typechecker — `E02xx`

| Code | Meaning |
|------|---------|
| `E0200` | Non-exhaustive `match` on a tagged union |
| `E0201` | `?` used outside a `Result`-returning function |
| `E0202` | `?` applied to a non-`Result` operand |
| `E0203` | `?` error type does not match the function's `E` (no `From` in v1) |
| `E0204` | Type mismatch |
| `E0205` | `owned` used on a non-`resource` type (D25) |
| `E0206` | `owned` resource not consumed on every path (D25) |
| `E0207` | `owned` resource used after it was consumed (D25) |
| `E0208` | Non-exhaustive `match` on an array (length not covered) |
| `E0209` | Non-exhaustive `match` on a `bool` |
| `E0210` | Field access on a record type that has no such field |
| `E0211` | Call argument type does not match the parameter type |
| `E0212` | `mut` reassigns a `const` binding (D20) |
| `E0213` | Wrong number of call arguments |
| `E0214` | Component declared with multiple parameters (use a props record) |
| `E0215` | Aliasing an `owned` handle (D25) |
| `E0216` | Unreachable `match` arm after a total pattern (D9) |
| `E0217` | Discarded `Result` &mdash; **warning**, not an error (its `Err` is silently ignored) |
| `E0218` | Non-exhaustive `match` on `number`/`string` (no catch-all for the unbounded rest) |

### Emitter — `E03xx`

| Code | Meaning |
|------|---------|
| `E0300` | Construct not supported by the v1 TypeScript emitter |
