# Error codes

Every diagnostic the compiler emits carries a stable code. The code appears in
the rendered error (`[E0200] Error: ...`), and `glyph --explain <code>` prints a
longer explanation with a fix example. `glyph build` and `glyph run` report the
same set on the same tree, warnings included; `glyph build --json` gives you the
machine-readable form. Codes are allocated by compiler phase:

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
| `E0006` | `if`/`else` used where Glyph has none (`match` is the only conditional; D3) |
| `E0007` | Range or comparison pattern (`500..599 =>`) in a `match` arm; not in v1 |
| `E0008` | Assignment without `mut` (`x = e` should be `mut x = e`, or `let x = e` for a new binding; D5) |
| `E0009` | Retired. An object pattern's field takes any pattern, so `Full({ color: Black })` matches the field value; the code is no longer emitted |
| `E0010` | A union variant given more than one positional payload field (`Node(Color, Tree, int)`); a variant carries one payload, and a multi-field payload is a record (D8) |

### Resolver — `E01xx`

| Code | Meaning |
|------|---------|
| `E0100` | Duplicate top-level name |
| `E0101` | Relative import (use an absolute module path; D15) |
| `E0102` | Barrel file: only imports, no declarations (D15) |
| `E0103` | Unresolved name |
| `E0104` | Unresolved import: a local import naming no module under the project root. A local import path resolves from the project root, the nearest directory holding a `package.json` with a `"glyph"` key, else the directory passed to `glyph build`/`glyph run` (D15/D41), not from the importing file's directory. When a file with that name exists elsewhere under the root the message says where, and when it belongs to a different project the message says that instead |
| `E0105` | Name not exported by the imported module (reported for a named import, `import lib { Secret }`, and for a name written through a namespace import, `import lib` plus either a `lib.Secret` annotation or a `lib.secret()` call) |
| `E0106` | Unused import (warning) |
| `E0107` | Unused variable binding (warning) |
| `E0108` | Unreachable code after `return`/`break`/`continue` (warning) |
| `E0109` | A TypeScript reserved word (`class`, `new`, `switch`, `eval`, ...) used as a declaration, parameter, or binding name |
| `E0110` | A top-level declaration whose name shadows a global the emitted module depends on (`Error`, `Number`, `Object`, `Array`, `Promise`, `Record`, or a prelude name such as `number`, `par`, `print`, `string`, `Issue`) |
| `E0111` | `type Key = string \| number`: bare primitive names on the right of `\|` declare tagged-union variants, not a union of those types |

`E0106`–`E0108` are the lint tier: warnings, not errors. They surface in the
build output but never fail the build or block emission. `E0107` exempts names
led by `_` (the conventional "intentionally unused" marker).

`E0109` is an error: Glyph permits these words as identifiers (they are not
Glyph keywords), but they cannot name a binding in the emitted TypeScript, so
Glyph rejects them at the source instead of letting `tsc` fail on generated
code. Only binding positions are checked; object keys, record fields, and
member access (`{ default: v }`, `x.new`) are unaffected.

`E0110` is the other half of the same problem, and the harder half: these names
are legal TypeScript, so `tsc` accepts the emitted module and the mistake is
silent. A variant named `Error` emits `export function Error(...)` at module
top level, and every `new Error(...)` the compiler emits below it resolves to
the variant instead. The list is in
`crates/glyph-resolver/src/reserved.rs`; the full set is tabulated in
[reserved words](reference/reserved-words.md). A module-local `type Issue` is
the same failure one step further down: the emitted descriptors keep writing
`Issue[]`, so the module compiles until `tsc` complains about generated code
you never wrote. `E0111` is the case that reads most like a TypeScript program
and is not one: see the same page.

`E0104` fires when a local import names no module under the project root. Glyph
resolves a local import path from the project root: the nearest directory holding
a `package.json` with a `"glyph"` key, else the directory passed to `glyph build`
(D15/D41). So `import model` from `apps/auth_api/main.glyph` resolves when
`apps/auth_api` carries that marker, or when it is itself the build target.
Without this the import failed silently, the imported type degraded, and what the
user saw was a `E0218` non-exhaustive match on a match that was exhaustive. The
message names the module and, when a `.glyph` file whose module path ends in that
import exists elsewhere under the root, where it actually is.

A project's imports resolve within its own root only (D41). When the file that
would answer to the import belongs to a *different* project in the same tree (a
nested one, an enclosing one, or a sibling), the message says which file it found
and which project holds it, rather than suggesting a spelling fix that cannot
work. That project is named relative to what you asked to build, so the message
is the same on every machine. Reach another project the way TypeScript does, by
package name through npm.

An npm import is not this error. Before reporting, the build collects the module
names it can resolve without a `.glyph` file: every `declare module "X"` in
`<root>/.types/**/*.d.ts` and in the bundled Node shim, plus every package in a
`node_modules` within the project. A name on that list is never reported, so a
declared or installed package is safe even when a local file happens to share its
basename. When the project has no `node_modules` at all the build cannot tell an
uninstalled dependency from a misspelling, and it reports only what it can prove:
an import some `.glyph` file under the root answers to.

### Typechecker — `E02xx`

| Code | Meaning |
|------|---------|
| `E0200` | Non-exhaustive `match` on a tagged union (yours, a prelude `Result`/`Option`, or a stdlib one such as `fs.ErrorKind`), or a string-literal union (`"free" \| "pro"`, D30) missing a literal. Either kind counts whether it is declared in this module or imported from a sibling |
| `E0201` | `?` used outside a `Result`-returning function |
| `E0202` | `?` applied to a non-`Result` operand |
| `E0203` | `?` error type does not match the function's `E` (no `From` in v1) |
| `E0204` | Type mismatch |
| `E0205` | `owned` used on a non-`resource` type (D25) |
| `E0206` | `owned` resource not consumed on every path (D25) |
| `E0207` | `owned` resource used after it was consumed (D25) |
| `E0208` | Non-exhaustive `match` on an array (length not covered) |
| `E0209` | Non-exhaustive `match` on a `bool` |
| `E0210` | Field access on a record type that has no such field, including a record declared in a sibling module under any import spelling, where the message names that record's own type |
| `E0211` | Call argument type does not match the parameter type |
| `E0212` | `mut` reassigns a `const` binding (D20) |
| `E0213` | Wrong number of call arguments |
| `E0214` | Component declared with multiple parameters (use a props record) |
| `E0215` | Aliasing an `owned` handle (D25) |
| `E0216` | Unreachable `match` arm after a total pattern (D9) |
| `E0217` | Discarded `Result` &mdash; **warning**, not an error (its `Err` is silently ignored) |
| `E0218` | Non-exhaustive `match` on `number`/`string` (no catch-all for the unbounded rest; a bounded string-literal union is E0200 instead, including one imported from another module) |
| `E0219` | `@redact` names a field the type does not have (D24) |
| `E0220` | A `match` arm's PascalCase head is not a variant of the scrutinee's union (a typo or wrong-union variant, escalated with a nearest-variant suggestion instead of being read as a silent binding catch-all; covers the bare `Loadign`, payload `Loadign(x)`, and qualified `Feed.Loadign` shapes, for a union declared in the module and for one reached through a namespace import (`model.Loadign`); D9) |
| `E0221` | Unknown `@annotation` (D27); the recognized set is `@example`, `@doc`, `@redact`, `@open`, `@pure`, `@public` |
| `E0222` | `await` outside an `async fn` (the innermost enclosing callable decides, so a sync lambda inside an `async fn` is flagged) |
| `E0223` | A `match` arm produces no value while the `match` is used as a value (an empty block, or a block whose tail is a `let`/`mut`/`for`/`loop`) |
| `E0224` | Reading a key out of a `Record<K, V>` map (`m.name` or `m[k]`), where the key may not be there. Use `record.get`, which returns `Option<V>` |
| `E0225` | A field of a parameter is read before an `await` and written after it, so a concurrent write in between is lost. Move the read after the `await` |
| `E0226` | A `match` whose scrutinee has no variant set to count against, where every arm's pattern can fail and no arm is a catch-all. Add an `else` |

### Emitter — `E03xx`

| Code | Meaning |
|------|---------|
| `E0300` | Construct not supported by the v1 TypeScript emitter |
| `E0301` | An `<else>` that is not the immediate sibling of its `<if>` (D6) |
| `E0302` | `?` in an arm of a `match` nested inside a larger expression (bind the match first) |
| `E0303` | `?` in a position with nothing to hoist the unwrap into, such as a `match` scrutinee (bind the operand first) |
| `E0304` | `parse`/`is` on a record holding a field whose type has no runtime check (a host handle, an `extern_ts` type, a generic tagged union); declaring the record is fine |
| `E0310` | `glyph run` on a module with no `fn main` (it's a library — nothing to run) |
