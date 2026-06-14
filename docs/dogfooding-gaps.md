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

- **G3. [FIXED] `json.parse<T>` was a cast, not a validating parse.** The runtime
  was `Ok(JSON.parse(text) as T)` — no shape check. The fridge persistence
  boundary (`json.parse<Fridge>`) trusted on-disk data blindly: exactly the
  failure the manifesto's Example 1 says Glyph exists to prevent. *Fixed: the
  emitter rewrites `json.parse<T>(text)` to `json.parse_with(text, T.schema)` for
  any `T` with a descriptor, routing the decoded value through the validating
  descriptor; a type with no descriptor keeps the casting `parse` as an escape
  hatch. Verified end to end: a malformed `.fridge.json` is now rejected as
  corrupt instead of loaded.*
- **G4. [FIXED] The validating descriptor only checked one level.** `T.is`/`T.parse`
  checked `typeof` for primitive fields and bare `"field" in value` presence for
  everything else — never recursing, so even the "validating" path didn't
  validate the fridge's shape. *Fixed: the record descriptor's `is` guard now
  recurses — a nested record field via `T.is`, an `Array<E>` via `Array.isArray`
  plus a per-element check, and an `Option<E>` by its tag plus the `Some`
  payload's type. Verified: a `Fridge` whose item carries a string where a
  numeric quantity belongs is rejected. The tagged-union descriptor now also
  switches on the tag and validates each variant's payload (record fields, a
  single-value `value`, or nothing for a no-payload variant), so unions are no
  longer tag-only.*
- **G5. Hand-edited Option JSON crashes.** An `Option` field serializes as
  `{"tag":"None"}` / `{"tag":"Some","value":n}`. A human or tool writing
  `"quantity": null` or `"quantity": 2` is rejected by neither the cast nor
  `T.parse`; the value reaches a `match` on `.tag` and `null.tag` throws.
- **G6. [FIXED] The typechecker didn't check field existence or argument
  types.** A typo'd field (`u.naem`) and a wrong-typed argument both built with
  zero Glyph diagnostics; only `tsc --check` caught them, in emitted-`.ts`
  coordinates and TS terms. *Fixed: the typechecker now resolves an object's
  record type and flags an unknown field (E0210), and checks each call argument
  against its (generic-substituted) parameter type with a conservative
  assignability relation (E0211, primitives + nominal named types + generic
  applications; undecidable and cross-shape pairs stay permissive so there are no
  false positives). Both surface as Glyph diagnostics with carets, before `tsc`.
  Verified: the examples still type-check clean; a field typo and a wrong-typed
  argument are now caught at the Glyph level.*
- **G7. [FIXED] The prelude-import trap.** `Option`/`Some`/`None` and
  `Result`/`Ok`/`Err` used without an explicit `import` resolved cleanly (they're
  in the prelude) but the emitter never injected their import, so `tsc` failed
  with a misleading `TS2749`/`TS2304` (the DOM `lib` even shadows `Option` as a
  value). `glyph build` without `--check` emitted broken TS and exited 0. *Fixed:
  the emitter scans the resolution map for prelude tagged-union references and
  injects `import { ... } from "std/result"` / `"std/option"` for the ones used
  without an explicit import. Explicitly imported names resolve to a module
  symbol, not the prelude, so they are never double-imported. Verified end to end
  (a program with no `std/result`/`std/option` import now passes `tsc --strict`).*
- **G8. [FIXED] The resolver's stdlib stubs over-promise the runtime.**
  `StdlibStubs` listed `array.reverse/slice/concat/len/push`,
  `string.split/trim/lower/upper/contains/...`, `std/time.now/sleep/Duration` —
  but the runtime `.ts` implemented only `array.{find,filter,map,zip}`,
  `string.{from,join}`, and `std/time`/`std/http` had no runtime at all (just
  type-only ambient stubs). Those names resolved clean, then failed `tsc` or
  crashed at runtime. *Fixed: the runtime now implements every promised name —
  array `len`/`push`/`concat`/`reverse`/`slice`, the full `string` set, io
  `read_line`/`read_to_string`, fs `exists`/`remove`, process `env`/`cwd`, and
  real `std/time` + `std/http` modules (replacing the type-only declarations). A
  reconciliation test asserts every `StdlibStubs` name is exported by the
  bundled runtime, so the two can no longer drift. Verified end to end.*
- **G9. [FIXED] `glyph run` / `glyph build` skipped `tsc`, so type errors became
  runtime crashes.** Only the opt-in `--check` ran `tsc`. An agent iterating with
  `glyph run` saw `X is not a function` / `Cannot find module`, not a diagnostic.
  *Fixed: `glyph build` and `glyph run` now type-check with `tsc` by default;
  `--no-check` opts out. `glyph run` refuses to run code `tsc` rejects (surfacing
  the error instead of crashing), and a missing `tsc` is a warning, not a block.
  The old `--check` flag is accepted but redundant. Verified: a field typo that
  Glyph's own checker misses is now caught before the program runs.*
- **G10. [FIXED] Multi-file programs didn't run or `--check`.** Sibling-module
  imports emitted bare TS specifiers (`from "helpers"`) with no `./` and no path
  mapping; only `std/*` was mapped. Any second module failed `glyph run` (tsx
  can't resolve) and `tsc` (TS2307). *Fixed: the emitter now emits a relative
  specifier (`./helpers`, `./sub/math`, `../top`) for a project (sibling) module,
  computed from the importer's path; `std/*` stays tsconfig-mapped and external
  npm packages stay bare. Verified end to end: a program spanning a root module,
  a flat sibling, and a nested sibling builds, passes `tsc --strict`, and runs.*
- **G11. [FIXED] `glyph fmt` corrupts string escapes.** The formatter re-emitted a
  decoded string value, turning `\t` into a literal TAB and `\n` into a raw newline
  that split the source line (while `\\`/`\"` were preserved — inconsistent). A
  no-op format rewrote string contents. *Fixed: plain string literals are copied
  verbatim from source by span, so escapes and D12 multi-line strings round-trip
  exactly; the re-escape fallback (template text, JSX attrs, `format_expr`) now
  also escapes `\n`/`\t`/`\r`. A no-op `glyph fmt` no longer touches string
  contents.*
- **G12. [FIXED] Associative collection.** The original gap overstated this:
  `Record<K, V>` already *is* the v1 associative collection — `r[key]` reads and
  writes, `for k, v in r` iterates, and `let r: Record<string, V> = {}` builds
  one up (01_validator does exactly this). What was missing was an absence-aware
  read (a bare `r[key]` yields untyped `undefined` for a missing key) and a clean
  way to query/update. *Fixed: a new `std/record` module adds `get` (returns an
  `Option`), `has`, `keys`, `values`, and value-oriented `set`/`remove`. Module
  path segments now also accept keyword-spelled names, so `import std/record`
  (where `record` is a keyword) parses. Verified: grouping/counting by key works
  end to end (`record.get` to accumulate, `r[k] =` to store, `for k, v` to read).*

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

**Progress:** the critical and high-severity gaps are all fixed — G1, G2, G11
(correctness bugs); G7, G8, G9, G10 (the "silent green" cluster + multi-file);
G6 (typechecker field/arg checking); and G3 + G4 (validating, recursive
`json.parse`). Remaining are medium/low: G5 (hand-edited Option JSON), G12
(Map/dict), G13–G18 (mut, owned, fmt nits), G19–G20 (sugar/parser limits), plus
the tagged-union payload-recursion follow-on noted under G4.

## Round 2 — re-dogfooding after the critical/high fixes

With the critical and high gaps closed, the fridge app was used end to end (every
command) and extended with **merge-on-add** (re-adding an item updates its
quantity instead of duplicating) and a **`summary` footer** (`1/2 checked`). Both
were written cleanly in Glyph, build, pass `tsc --strict`, and ship with
`@example` tests (10 now pass). The persistence boundary correctly rejects a
malformed `.fridge.json` (G3/G4). What the real use surfaced:

- **R1. `glyph run` latency (~2s/invocation).** Every `glyph run` rebuilds,
  type-checks (`tsc`), and starts `tsx` from scratch. For a CLI invoked dozens of
  times a day this is the dominant friction. *Fix candidates: cache/skip the
  build when sources are unchanged; reuse a warm `tsc`/`tsx`; or a persistent
  dev process.* New, and the highest-impact ergonomics gap.
- **R2. No `array.any` / `array.contains`.** Membership tests recur as
  `match array.find(xs, p) { Some(_) => true, None => false }` — a four-line
  dance (`contains_name` in the app). A boolean `array.any(xs, p)` /
  `array.contains` would shorten it. Stdlib gap.
- **R3. No `array.sort`.** A sorted list (the obvious next feature) can't be
  expressed without hand-rolling a sort; `std/array` has no ordering helper.
  Stdlib gap.
- **R4. G12 (Map/dict) re-confirmed as the next real blocker.** Merge-on-add
  works via a linear `array.find` + `array.map` rebuild (fine at small sizes), but
  group-by-category, dedup, and keyed lookup all want an associative collection.
- **R5. `mut` stayed unused (re-confirms G13).** Every list update was an
  immutable rebuild (`array.map`/`filter` + object spread, which works well —
  `{ ...existing, quantity }` in a match arm is clean). `mut` was never reachable
  for "update field F of item N", so it remains decorative for collections.

Net: the toolchain is now trustworthy enough for daily use; the remaining
friction is **ergonomics and stdlib breadth** (R1–R3) plus the **Map** language
gap (G12/R4), not correctness.

### Round-2 fixes landed

R1 (`glyph run` build caching, ~2.2s → ~0.6s warm), R2 (`array.any`/`contains`),
R3 (`array.sort`), G17 (prune stale emitted `.ts`), G15 (`mut` on a `const` is an
error, E0212), and the G4 follow-on (union descriptors validate variant payloads,
not just the tag) are all fixed. Remaining: G12 (Map/dict), G13 (`mut`
multi-level), G5 (Option JSON ergonomics), G18 (`fmt` nits), and the principled
v1.1 deferrals G14/G16/G19/G20.
