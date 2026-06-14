# Step 6 dogfooding — gap list

Findings from building and running the fridge shopping-list app
(`examples/apps/fridge.glyph`) and probing the compiler/stdlib with real-app
patterns. The app itself builds, passes `tsc --strict`, runs end to end, and its
six `@example` tests pass — but writing it surfaced concrete gaps, several of
them silent-miscompile **bugs** (code that passes `glyph build` and `tsc` and
then misbehaves at runtime). Ordered by severity.

## Verdict

The toolchain works for a clean, single-file, primitives-and-tagged-unions app.
The single most important class to fix before v1 is the **"silent green"**
failure mode: `glyph build` (and `glyph run`) report success on code that the
emitter mistranslates or that the runtime can't actually provide, because real
checking is deferred to an optional `tsc --check` — and even `tsc` misses the
two miscompiles below. The verifiability pillar (the project's lead claim) is
the most exposed.

## Critical — silent miscompiles (pass `glyph build` + `tsc`, wrong at runtime)

- **G1. [FIXED] `None` in nested patterns miscompiles.** The match emitter didn't
  treat the prelude `None` as a tag discriminant: a `None` arm became a `default:`
  with a junk `const None = __m0` binding. Flat `Option` matches survived by
  accident (default caught the one remaining case), but nested patterns
  (`Result<Option<T>>`, `Option<Option<T>>` with `Ok(None)`/`Some(None)`) emitted a
  duplicate `case`, compiled the inner `None` as a binding, and `throw`-ed at
  runtime on the `None` value. *Fixed: the prelude constructors `Ok`/`Err`/`Some`/
  `None` are recognized as discriminant tags unconditionally, and nested grouping
  treats a bare prelude variant (`Ok(None)`) as a payload pattern, so it groups
  with `Ok(Some(x))` under one `case`. Verified end to end on `Result<Option<T>>`
  and `Option<Option<T>>`.*
- **G2. [FIXED] `break` inside `match` inside `loop` hangs.** `match` lowered to
  `switch` and emitted an unlabeled `break`, which escaped the switch, not the
  loop. Since `match` is the only conditional, `loop { match cond { true => break,
  ... } }` is *the* idiom for a guarded loop — and it compiled, passed `tsc`, then
  looped forever. (This very bug wedged the gap-audit workflow.) *Fixed: a loop is
  labeled when its body has a `break`/`continue` buried in a `match` arm, and
  those jumps emit the labeled form so they reach the loop past the switch. The
  synthetic switch-`break` is untouched. Verified end to end (a guarded loop now
  terminates).*

## High — verifiability holes and "silent green"

- **G3. `json.parse<T>` is a cast, not a validating parse.** The runtime is
  `Ok(JSON.parse(text) as T)` — no shape check. The fridge persistence boundary
  (`json.parse<Fridge>`) trusts on-disk data blindly: exactly the failure the
  manifesto's Example 1 says Glyph exists to prevent. *Fix: route text → an
  `unknown` decoder → `T.schema.parse` (validating).*
- **G4. The validating descriptor only checks one level.** `T.is`/`T.parse`
  check `typeof` for primitive fields and bare `"field" in value` presence for
  everything else — never recursing into nested records, `Array<T>`, or
  `Option<T>`. So even the "validating" path doesn't validate the fridge's shape.
  *Fix: recurse into named-record / array / option field checks.*
- **G5. Hand-edited Option JSON crashes.** An `Option` field serializes as
  `{"tag":"None"}` / `{"tag":"Some","value":n}`. A human or tool writing
  `"quantity": null` or `"quantity": 2` is rejected by neither the cast nor
  `T.parse`; the value reaches a `match` on `.tag` and `null.tag` throws.
- **G6. The typechecker doesn't check field existence or argument types.** A
  typo'd field (`u.naem`) and a wrong-typed argument both build with zero Glyph
  diagnostics; only `tsc --check` catches them, in emitted-`.ts` coordinates and
  TS terms. For a verifiability-first language, two of the most common mistakes
  are delegated to an optional downstream tool. *Fix: member-access + call-arg
  checking in the typechecker (needs the unifier).*
- **G7. The prelude-import trap.** `Option`/`Some`/`None` and `Result`/`Ok`/`Err`
  used without an explicit `import` resolve cleanly (they're in the prelude) but
  the emitter never injects their import, so `tsc` fails with a misleading
  `TS2749`/`TS2304` (the DOM `lib` even shadows `Option` as a value). `glyph
  build` without `--check` emits broken TS and exits 0. *Fix: the emitter
  auto-imports the prelude std-module values/types it references.*
- **G8. The resolver's stdlib stubs over-promise the runtime.** `StdlibStubs`
  lists `array.reverse/slice/concat/reduce/len/push`, `string.split/trim/lower/
  upper/contains/...`, `std/time.now/sleep/Duration` — but the runtime `.ts`
  implements only `array.{find,filter,map,zip}`, `string.{from,join}`, and
  `std/time` has no runtime at all. Those names resolve clean, then fail `tsc`
  or crash at runtime under `glyph run`. *Fix: make the stubs match the runtime
  (or implement the promised functions).*
- **G9. `glyph run` / `glyph build` skip `tsc`, so G7/G8 become runtime
  crashes.** Only `--check` runs `tsc`. An agent iterating with `glyph run` sees
  `X is not a function` / `Cannot find module`, not a stdlib-coverage
  diagnostic. *Fix: type-check by default, or make the resolver/runtime the
  single source of truth so "resolves" implies "exists".*
- **G10. Multi-file programs don't run or `--check`.** Sibling-module imports
  emit bare TS specifiers (`from "helpers"`) with no `./` and no path mapping;
  only `std/*` is mapped. Any second module fails `glyph run` (tsx can't resolve)
  and `tsc` (TS2307); plain `glyph build` is a false green. *Fix: emit a path
  mapping (or relative specifiers) for project modules.*
- **G11. [FIXED] `glyph fmt` corrupts string escapes.** The formatter re-emitted a
  decoded string value, turning `\t` into a literal TAB and `\n` into a raw newline
  that split the source line (while `\\`/`\"` were preserved — inconsistent). A
  no-op format rewrote string contents. *Fixed: plain string literals are copied
  verbatim from source by span, so escapes and D12 multi-line strings round-trip
  exactly; the re-escape fallback (template text, JSX attrs, `format_expr`) now
  also escapes `\n`/`\t`/`\r`. A no-op `glyph fmt` no longer touches string
  contents.*
- **G12. No associative collection (Map / dict).** No `Map`, no index-signature
  record, no dynamic record indexing. Grouping (e.g. items by category) has
  nowhere to put results — a language-level gap, not just a missing stdlib fn.

## Medium — mutation, resources, tooling

- **G13. `mut` only supports single-level lvalues.** `mut xs[i].field`,
  `mut r.a.b`, `mut r.items[0]` are parse errors — the most common list update
  ("update field F of item N") can't be written. The de-facto idiom is immutable
  rebuild (`array.map`/`filter` + spread); `mut` is decorative beyond a scalar.
- **G14. `mut r.field` mutates the caller's record (aliasing footgun).** Records
  are TS objects by reference; `mut x.field` lowers to in-place assignment, so a
  function silently mutates its caller's value. Surprising for a value-oriented
  language. *Fix: define + enforce value semantics, or document loudly.*
- **G15. `mut` on a `const` is not enforced (D20 says it is).** `mut N = 6`
  against `const N` passes the Glyph typechecker; only `tsc` catches it (TS2588,
  no E-code). *Fix: enforce in the typechecker with a real E-code.*
- **G16. D25 `owned` is unexercised and fights `?`.** No stdlib resource type or
  `open`/`close` exists; `owned`/`resource` appear only in negative tests. And
  the natural open→fallible-work→close shape is rejected because `?` is a
  consumption checkpoint and there's no `defer`/`using`/scoped disposal — so a
  real flow must abandon `?` and duplicate `close` across match arms. The
  manifesto's central carve-out carries no weight in a typical app. *Fix:
  reconsider scope for v1, or add scoped disposal + a stdlib resource.*
- **G17. `glyph build --out X` never cleans `X`.** A renamed/removed source
  leaves a stale `.ts` that `tsc` and importers still pick up. *Fix: clean the
  out dir, or track + prune.*
- **G18. `glyph fmt` layout nits.** Deletes the blank line between a section
  comment and its declaration; wraps the innermost call's args instead of the
  long method chain.

## Low — expected / cosmetic

- **G19. No `T?` sugar over `Option<T>`** (a documented deferral) — but the parse
  error gives no hint that `Option<number>` is the workaround.
- **G20. Nested string literal inside `${...}` interpolation** breaks the
  template parser (known v1 limitation; forces hoisting to a `let`).

## What to fix first (recommended)

The two miscompiles **G1** and **G2** are correctness bugs that `tsc` does not
catch — they should be fixed first. Then the "silent green" cluster **G7/G8/G9**
(close the resolve-vs-runtime gap so a clean build means a working program), and
the verifiability pair **G3/G6**. **G11** (fmt escape corruption) is a quick,
self-contained correctness fix.

**Progress:** G1, G2, and G11 are fixed (the three correctness bugs `tsc` could
not catch). Next up is the "silent green" cluster — G7 (emitter auto-imports the
prelude values it references) and G8 (resolver stubs match the runtime) — then
the verifiability pair G3/G6 (which needs the unifier).
