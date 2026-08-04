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
- **G5. [FIXED — no longer crashes; lenient form deferred] Hand-edited Option
  JSON.** An `Option` field serializes as `{"tag":"None"}` /
  `{"tag":"Some","value":n}`. A human writing `"quantity": null` or
  `"quantity": 2` used to slip past the cast and crash at the `match` on `.tag`.
  *With G3/G4, a typed `json.parse` now validates the Option's tag shape, so a
  malformed value is rejected as a corrupt file rather than crashing.* Accepting
  the lenient forms (`null` ↔ `None`, a bare value ↔ `Some`) is deliberately not
  in v1: the tagged form is the canonical wire format, and a lenient decode would
  need a normalizing pass (the `match` reads `.tag`) and is ambiguous when `T` is
  itself nullable — a v1.1 ergonomics item.
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

- **G13. [FIXED] `mut` only supported single-level lvalues.** `mut xs[i].field`,
  `mut r.a.b`, `mut r.items[0]` were parse errors, so the most common list update
  ("update field F of item N") couldn't be written. *Fixed: the `mut` target is
  now a general lvalue expression (a name, or any chain of field accesses and
  index subscripts bottoming out at one), unifying the old assign/index/field
  forms. Verified: `mut bag.items[0].qty = 99` updates the nested element in
  place. The immutable-rebuild idiom remains available and is still the
  value-oriented default; the in-place aliasing question is G14.*
- **G14. [DECIDED — documented v1 stance] `mut r.field` mutates the caller's
  record (aliasing footgun).** Records lower to TS objects held by reference, so
  `mut x.field` is an in-place assignment that can mutate a caller's value. *v1
  decision: records have TS reference semantics — `mut` mutates in place, and the
  value-oriented idiom for "produce a changed copy" is immutable rebuild (object
  spread, `array.map`). Compiler-enforced value/copy semantics (clone-on-`mut`)
  is a real design item but a large one (cost + interaction with `owned`), so it
  is deferred; the stance is documented rather than enforced in v1.* Pillar note:
  this trades a little verifiability (the footgun) for diff stability and
  simplicity; revisit if dogfooding shows real bugs from it.
- **G15. `mut` on a `const` is not enforced (D20 says it is).** `mut N = 6`
  against `const N` passes the Glyph typechecker; only `tsc` catches it (TS2588,
  no E-code). *Fix: enforce in the typechecker with a real E-code.*
- **G16. [DECIDED — v1.1 design item] D25 `owned` is unexercised and fights
  `?`.** No stdlib resource type or `open`/`close` exists; `owned`/`resource`
  appear only in negative tests. The natural open→fallible-work→close shape
  fights `?` (a consumption checkpoint) because there is no scoped disposal. *The
  `owned` single-consumption analysis is implemented and tested (it is the
  manifesto's one carve-out), but to carry weight it needs (a) a stdlib resource
  type with `open`/`close` and (b) scoped disposal (`using`/`defer`) so the
  fallible-work shape composes with `?`. Both are v1.1 design work; `owned`
  remains in v1 as the discipline primitive, exercised by the negative suite. No
  code change here — this records the decision to defer the surrounding
  machinery, not the carve-out itself.*
- **G17. `glyph build --out X` never cleans `X`.** A renamed/removed source
  leaves a stale `.ts` that `tsc` and importers still pick up. *Fix: clean the
  out dir, or track + prune.*
- **G18. `glyph fmt` layout nits.** Deletes the blank line between a section
  comment and its declaration; wraps the innermost call's args instead of the
  long method chain.

## Low — expected / cosmetic

- **G19. [IMPROVED] No `T?` sugar over `Option<T>`** (a forward-compatible v1.1
  deferral — adding it later won't change existing parse trees). *The parse error
  now names the fix: `T?` in type position reports "use `Option<T>`" instead of a
  confusing token error.*
- **G20. [IMPROVED] Nested string literal inside `${...}` interpolation** breaks
  the template parser — the lexer ends the outer string at the first inner quote
  (it has no template-literal mode; that mode is the v1.1 fix). *The error now
  explains the cause and names the workaround (hoist the interpolation into a
  `let`) rather than reporting a bare "expected `}`".*

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

Every gap on this list is now either fixed or resolved as a documented decision:

- **Fixed (code):** R1 (`glyph run` build caching, ~2.2s → ~0.6s warm), R2
  (`array.any`/`contains`), R3 (`array.sort`), G12 (`std/record` + keyword module
  segments), G13 (multi-level `mut` lvalues), G15 (`mut` on a `const`, E0212),
  G17 (prune stale emitted `.ts`), G18 (`glyph fmt` preserves author blank
  lines), and the G4 follow-on (union descriptors validate variant payloads).
- **Improved (clearer errors):** G19 (`T?` → "use `Option<T>`"), G20 (nested
  string in `${...}` names the cause + workaround).
- **No-longer-crashes:** G5 (a typed `json.parse` rejects malformed Option JSON
  instead of crashing; lenient decode deferred).
- **Decided (documented stance, no code):** G14 (records have TS reference
  semantics; immutable rebuild is the value-oriented idiom; clone-on-`mut`
  deferred), G16 (`owned` analysis stays; a stdlib resource + scoped disposal are
  v1.1).

The earlier critical/high tier (G1, G2, G11; G3, G4, G6, G7, G8, G9, G10) was
fixed before round 2. Nothing on the gap list is now an open bug; the deferrals
are forward-compatible (adding them later won't change existing parse trees or
semantics).

### Round 3 — adversarial review of the round-2 fixes

A multi-agent adversarial review of the round-2 changes (with independent
verification of each claim) found and confirmed several defects, all now fixed:

- **`glyph run` cache could serve stale output.** The fingerprint omitted
  `<src>/.types/**/*.d.ts` (a real build input, copied + type-checked), so
  editing an ambient declaration didn't bust the cache; and cache validity keyed
  on the target `.ts` merely existing, so a build that errored after writing it
  poisoned the cache. *Fixed: the fingerprint now hashes the `.types` tree, and a
  `.glyph-build-ok` marker (written only on a complete build) gates cache hits.*
  (Concurrent runs of the same program racing the shared cache dir remains a
  known, low-likelihood limitation.)
- **`mut` accepted invalid lvalues.** `mut x?.field = v` (optional chain) emitted
  invalid TS, and `mut foo()` / `mut xs[0]()` (non-method calls) slipped past
  D5's method-call form. *Fixed: optional-chain lvalues are rejected and the
  method-call form requires a `x.method(...)` callee.*
- **`?` in a `mut` RHS errored.** `mut x = g()?` reached the non-hoisting path.
  *Fixed: the RHS now lowers `?` like `let`/`return`.*
- **Descriptor recursion was incomplete.** An inline-record field type
  (`{ a: number }`) and the `is Array<E>` match arm validated only shallowly,
  inconsistent with the rest of G4. *Fixed: both now recurse/element-check.*
- **`json.parse<T>` under-fired.** Only the bare-record namespace form was
  rewritten. *Fixed: `json.parse<Array<T>>` now validates via `T.schema.array()`;
  the doc is corrected (the named-import `parse<T>` form is documented as not
  rewritten — use the `json.parse` namespace form).*
- **`glyph fmt` lost blanks between consecutive comments.** *Fixed.*

## Round 4 — re-dogfooding stdlib logic in real Glyph

Reimplementing stdlib logic in Glyph to exercise the compiler. Option/Result
combinators (`opt_map`, `opt_unwrap_or`, `opt_and_then`, `opt_filter`,
`opt_flatten`, `ok_or`, `res_unwrap_or`, `res_and_then`, `res_ok`) — pure
tagged-union match logic — wrote, built under `tsc --strict`, and ran with
correct output with no friction beyond the already-known G20. Extending to a
recursive tagged-union tree formatter (a `Json` value plus a `render`, mirroring
`json.stringify` logic) surfaced one new gap.

- **G21. [RESOLVED — accepted, won't fix] A bare tail expression that starts a line
  with `[` or `(` glues onto the previous statement.** **Decision:** this is
  consistent with JavaScript's ASI (in JS, `foo()\n[1,2]` is `foo()[1,2]`, and a
  leading `;`/`return` is the standard fix), so it is expected behavior, not a
  bug. Matching it keeps the "looks like TypeScript" stance; the alternative
  (significant newlines inside blocks) would both diverge from JS and break
  multi-line method chains and split expressions, which the current newline model
  deliberately enables. Workaround is idiomatic Glyph anyway: put `return` (or any
  keyword) before the array/paren, which breaks the postfix chain. Documented in
  the D1 spec note. The original analysis follows.

  Inside a block there are
  no newline tokens (D1: newlines are significant only at bracket depth zero, and
  a block's `{` raises the depth), so statement boundaries are found by greedy
  parsing. A statement whose next line begins with `[` or `(` is therefore parsed
  as a postfix index/call on the previous expression, not as a new statement.
  Concretely, this idiomatic recursive tail

  ```glyph
  fn go<T>(xs: Array<T>) -> Array<string> {
    return match xs {
      [] => [],
      [head, ...rest] => {
        let pair = render(head)
        [pair, ...go(rest)]   // parsed as `render(head)[pair, ...go(rest)]`
      },
    }
  }
  ```

  fails to parse (`expected ]`, found `Comma`), and the single-element form
  `let x = y` / `[x]` silently parses as `let x = y[x]` (an unresolved-name error,
  not a "wrong statement" error). `(`-led lines glue the same way
  (`let x = y` / `(g(x))` → `y(g(x))`). The clean workaround is an explicit
  `return` (or any keyword) before the array/paren, which breaks the postfix
  chain: `return [pair, ...go(rest)]` parses correctly. This is not the G20
  template limitation; it is statement-boundary detection.

  *Not fixed here — it is a D1 semantics decision, so it is reported for the
  orchestrator to decide, not implemented.* The options and their tradeoffs:

  1. **Document the current behavior and keep D1 as-is.** A leading-`[`/`(` tail
     needs an explicit `return`. Zero compiler change; costs a footgun in a style
     (implicit tail return of a recursively-built array) the language otherwise
     encourages. Pillar: preserves D1's greppability rationale, cedes a little
     verifiability (a silent wrong-parse in the `let x = y` / `[x]` case).
  2. **Emit newlines inside block `{}` but keep suppressing them inside record
     literals, `()`, and `[]`.** Makes newlines significant statement terminators
     inside blocks, matching most authors' mental model and breaking the postfix
     glue. The cost is real: the lexer's flat `bracket_depth` cannot distinguish a
     block `{` from a record-literal `{`, so this needs the parser to drive
     newline significance (or a lexer brace-kind heuristic), and it revises D1's
     stated rule ("newline terminates only at bracket depth zero, no ASI") and
     touches every block/record/match parse path plus their snapshots.
  3. **Narrowly refuse to extend a postfix chain with `[`/`(` at a statement
     boundary.** Requires the same newline signal inside blocks as option 2, so it
     collapses into it; there is no parser-only fix, because with no newline token
     the parser cannot tell continuation from a new statement.

  Recommendation deferred to the orchestrator. Option 1 is a one-line doc note;
  option 2 is the principled fix but a genuine D1 revision.

## Round 5 — re-dogfooding stdlib logic in real Glyph

Continuing the stdlib-in-Glyph loop. A pure-Glyph IPv4/CIDR module
(`examples/corpus/ipv4.glyph`: dotted-quad parse with canonical-form validation,
32-bit address arithmetic, subnet masking via integer arithmetic rather than the
sign-lossy JS bitwise ops, broadcast/host-count, and containment) wrote, built
under `tsc --strict`, and ran with correct output across the full 0..2^32-1 range
(255.255.255.255 → 4294967295, /8 → 16777214 hosts). The module owns its error
`Result` type and renders it with an in-module `explain(e: ParseError) -> string`.
Writing a *driver in another module* that matched the module's error union
directly surfaced one new gap.

- **G22. [RESOLVED] Matching an imported
  tagged union's no-payload (nullary) variants is rejected.** A second module
  that does `match e { WrongOctetCount(w) => .., EmptyOctet => .., .. }` on an
  `e: ParseError` imported from another module is rejected: every arm after the
  first bare nullary variant (`EmptyOctet`) draws a false E0216 "unreachable match
  arm." Root cause: an imported type annotation lowers to `Ty::Unknown` in the
  consuming module (its declaration lives in a different `Module`, which the
  single-module typechecker cannot reach), so the reachability check has no
  variant set and reads every bare-identifier nullary-variant arm as an
  irrefutable binding catch-all — making the arms below it look dead. Constructor
  arms with a payload (`OctetTooLarge(o) =>`) are unaffected, since a `Constructor`
  pattern is inherently refutable; only bare nullary variants trip it. The emitter
  has the same blindness (its `is_variant` also reads `Ty::Unknown` → None and
  would lower each nullary arm to a `default:`, then reject the multiple
  catch-alls). Importing the variant constructors into the consuming module does
  not help: the block is the reachability/exhaustiveness classification, not name
  resolution. `recover_union_from_arms` cannot rescue it either — it resolves only
  *module-local* `Variant` symbols, and an imported variant is an `ImportNamed`
  whose decl is in another module.

  The 0.1.21 fix solved the sibling problem for *record-payload* variants by
  building a project-wide `record_payload_variants` registry keyed by
  `(module, variant)` and resolving an imported variant through its `ImportNamed`
  symbol — but only on the *emitter* side, which receives that registry through
  `EmitContext`. The typechecker is a pure single-`Module` salsa query with no
  project context, so the reachability/exhaustiveness checks cannot see any
  cross-module variant data today.

  *Not fixed here — it is an architecture decision, so it is reported for the
  orchestrator to decide, not implemented.* The options and their tradeoffs:

  1. **Give the typechecker a cross-module variant registry, mirroring
     `EmitContext`.** Build a project-wide set of union variant names (at least the
     nullary ones) in `build.rs`, thread it into the `type_map` salsa query and the
     `Checker` constructor, and consult it in `check_arm_reachability` and
     `check_match_exhaustiveness`; extend the emitter's `is_variant` to consult the
     same set so the two stay in step. This is the principled fix and follows the
     0.1.21 pattern, but it is a genuine architecture change: it decides that the
     per-module checker gets cross-module *type* knowledge (today it gets none),
     which widens the salsa dependency graph and the checker's inputs. Pillar:
     serves verifiability (exhaustiveness is currently *silently skipped* on any
     imported union, a real hole) and greppability (cross-module matches read
     naturally), at the cost of the checker's single-module simplicity.
  2. **Make the reachability check conservative without cross-module data.**
     Suppress E0216 for a bare-identifier arm whenever the match also contains a
     `Constructor` arm (a variant-style match), since a bare ident there is far more
     likely a nullary variant than a binding. Local and mechanical, and it removes
     the false rejection — but it does *not* fix the emitter (which would still
     mis-lower the imported nullary arms), so on its own it converts a checker error
     into an emit error or a miscompile. Only viable paired with an emitter fix, and
     it forfeits a real dead-arm lint on unusual same-module code. Not a complete
     fix by itself.
  3. **Document that a module should render its own union** (an in-module
     `explain`/`to_string` over the error type) and keep cross-module nullary-variant
     matching unsupported for now. Zero compiler change; this is exactly what
     `ipv4.glyph` does. Costs the ability to `match` a library's error union in the
     caller, which is a natural Result-handling idiom.

  Recommendation deferred to the orchestrator. Option 1 is the principled fix and
  closes the parallel exhaustiveness hole; option 3 is the current shipped stance.

  *Module-local subset closed (E0220).* The related **module-local** hole — a
  PascalCase arm head that is not a variant of a *decidable, same-module* union
  was read as a silent binding catch-all, so a typo like `Loadign` (for `Loading`)
  passed exhaustiveness and swallowed the real missing-variant error — is now
  fixed independently of this fork. `check_patterns_exhaustive` escalates such a
  head to `UnknownVariantPattern` (E0220) with a nearest-variant suggestion for
  all three arm shapes — bare `Loadign`, payload-bearing `Loadign(x)`, and
  qualified `Feed.Loadign` — and no longer marks it a catch-all, so a genuinely
  missing variant still surfaces as E0200. This needed no cross-module data (the
  scrutinee's variant set is already known locally), so it does not touch the G22
  decision: the `is TypeName` guard path and imported/cross-module coverage
  remain the open
  forks above.

## Round 6 — Minesweeper in the terminal (no framework, no dependency)

The improve-glyph loop pointed at an ordinary program instead of an integration:
Minesweeper (`examples/apps/minesweeper.glyph`), a 9x9 grid, lazy first-click
mine placement, flood-fill reveal, a flag/unflag command loop, and a deterministic
seeded RNG so a transcript compares byte for byte. No npm dependency, no server,
no JSX. It builds, passes `tsc --strict`, and plays. Eight gaps came out of
writing it; one is fixed and shipped in 0.1.38, the rest are scheduled or waiting
on a decision in `docs/roadmap/releases.md`.

- **G23. [FIXED] `glyph fmt` relocated any comment written inside a construct.**
  The printer flushed pending `//` comments at declaration and statement
  granularity only, so a comment inside a record body, a union variant list, an
  array or object literal, a call argument list, or above a `match` arm stayed
  pending and was re-emitted above the next declaration or statement. One pass
  over a nine-line file produced three separate corruptions, including an
  array-element comment that escaped its `const` and landed above an unrelated
  `type`, where it reads as that type's documentation. Nothing warned: exit 0,
  `tsc` passed, and the mangled output is a fixed point, so `glyph fmt --check`
  in CI accepts it. The app had to hoist every field comment out of its record
  onto the declaration line to survive a format. *Fixed: `delimited` now takes the
  construct's closing offset and each item's start offset, flushes pending
  comments above the item that followed them in source, drains the rest before
  the closing delimiter, and vetoes the inline form outright when the construct
  holds an interior comment (at any element count and any width). Match arms and
  union variants, which do not route through `delimited`, flush directly. The
  veto is read from spans before the inline candidate is rendered, since that
  candidate is built into a buffer that gets discarded and a flush inside it
  would delete the comment rather than move it. Verified: the app round-trips
  byte for byte through two `fmt` passes, and across the whole `examples/` tree
  exactly one file's output changes, to what its author originally wrote. Ten
  formatter tests; D14 records the guarantee.* The remaining edge is placement,
  not loss: a comment is always emitted on its own line, so one written at the
  end of a code line moves to the line above the next item.
- **G24. `?` is rejected in an expression-form `match` arm.** `=> f(x)?` fails
  while `=> { return f(x)? }` and `=> return Ok(f(x)?)` both compile. One call
  site in the emitter uses `self.expr` where every other statement position uses
  `self.emit_value`. A missed call site, not a design.
- **G25. A value-position `match` cannot host block arms.** A `match` used as a
  sub-expression lowers to an IIFE that rejects block arms, and in that position
  G24 has no workaround. Structural and separate from G24.
- **G26. `std/string` has no `repeat`, `pad_start`, or `pad_end`.** Every program
  that renders a grid or aligned columns needs them; the app hand-rolled all
  three. Three wrappers plus three names in the resolver seed.
- **G27. An unknown stdlib namespace member leaks a raw `tsc` error.** `import
  std/string { repeat }` gives a clean E0105, because `verify_imports` checks
  named imports against the resolver seed. `string.repeat(...)` gives a TS2339
  carrying an absolute build path, because nothing checks member access against
  the same seed. The same typo gets two experiences depending on import style,
  and the absolute path in a remapped TS error is a second, separable defect.
- **G28. There is no `glyph check <file>`.** `build` rejects a non-directory
  source, so the only door into type checking a single file is running it.
- **G29. Two formatter layout complaints, deliberately not bundled with G23.** A
  one-statement `match` arm body is always exploded to three lines, because the
  parser wraps it in a synthetic block and every block prints multi-line; and the
  `INLINE_MAX` check short-circuits the width test, flattening every
  two-argument `array.map(xs, fn(...) { ... })`.
- **G30. Two decisions the trip surfaced, both open.** `for` has nothing in the
  stdlib that produces a counted range, so the most common bounded loop cannot
  use the keyword D21 built for bounded loops and gets hand-rolled from
  `loop`/`match`/`break` instead, which costs greppability. And `xs[i]` types as
  `Ty::Unknown` with `noUncheckedIndexedAccess` off and no `array.get`, so
  `cells[999]` type-checks clean, passes `tsc --strict`, and hands back
  `undefined` where the compiler claimed `Cell`. Both are architecture forks with
  their options and costs written out in `docs/roadmap/releases.md`; neither is
  decided here.

## Round 7 — an expense report CLI (a ledger, a parser, and money)

The loop pointed at a plain command-line tool: `examples/apps/expenses.glyph`
reads a CSV ledger, validates every row, and prints a per-category report with
exact money. Fifteen findings came out of it. Thirteen are "Glyph made me type
more" (a missing `string.repeat`/`pad_start`, no `array.fold`, no
`allow_hyphen_values` on the clap binding). Two are different in kind: the stdlib
returned a value where its own reference docs promised a rejection, and the loop
form that replaces a hand-rolled counter silently binds the wrong kind of index.
The first is fixed here; the second was found while deleting the app's
workaround for the first and is still open.

- **G31. [FIXED] `time.parse_iso` accepted non-ISO text, read it in local time,
  and rolled impossible dates over.** It was a bare `Date.parse`, so
  `"January 5 2026"` parsed, `"2026-1-3"` parsed *in the host's local timezone*
  (which then disagrees with the UTC calendar accessors documented in the same
  file, shifting the day and, near a month boundary, the month), and
  `"2026-02-31"` returned `Some` for March 3. `docs/reference/stdlib.md`
  documented the opposite: "None if invalid". A boundary validator failing open
  while its docs promise it fails closed is the verifiability pillar inverted,
  and an agent that reads the docs and trusts them writes a broken validator.
  The app noticed and hand-rolled the guard at the call site (a regex shape check
  plus a `format_iso` round-trip); when an app writes a correctness guard around
  a stdlib primitive, the guard belongs in the primitive. *Fixed: `parse_iso`
  now gates on an anchored ISO-8601 shape before `Date.parse` sees the string,
  accepting only a bare `YYYY-MM-DD` (UTC midnight per the ECMAScript grammar) or
  `YYYY-MM-DDTHH:MM(:SS)?(.sss)?` with an explicit `Z`/`+HH:MM`/`-HH:MM`; keeps
  the `NaN` check, which is what still rejects `"2026-13-01"`; and then validates
  the year/month/day triple arithmetically, with real month lengths and leap
  years, because `Date.parse` reports rollover as success and no `NaN` check can
  catch it. An offset-less datetime (`"2026-01-03T10:00"`) is rejected rather
  than silently read as local time, which is the deliberate asymmetry that makes
  the file header's UTC guarantee true. Tightened in place, with no lenient
  variant beside it: the name says ISO and the docs promised strict.*
- **G32. [FIXED] The two-binding `for i, x in xs` form was documented nowhere.**
  It parses, it lowers (an array iterand emits `xs.entries()` with a numeric
  index, a record emits `Object.entries`), and two example programs use it, but
  it appeared in no file a user or an agent reads: not D21 in the spec, not
  `AGENTS.md`, not the cookbook. An implemented feature documented nowhere is,
  practically, not implemented, and its absence cost the app its only
  off-by-one-prone code. *Fixed: documented in D21, the agent bootstrap, and the
  cookbook.*
- **G33. [FIXED] D22 overstated the interpolation restriction.** The spec said
  `${...}` interiors were limited to literals, identifier reads, member access,
  `?`, and parens. The parser hands the interior to the full expression grammar,
  so a call inside `${...}` has always worked. The one real restriction is a
  nested string literal, and that is a lexer artifact rather than a rule. A spec
  that forbids more than the compiler does makes people write worse code, so this
  is the same class of bug as G31 pointing the other way. *Fixed: D22 now
  describes what the parser accepts.*
- **G34. `std/string` has no `slice`, and `std/array` has no `fold`.** The
  `fold` gap is the one that costs a pillar: with no fold, every accumulation is
  a `mut` in a loop, which dilutes `grep mut` (the whole point of D5) with
  arithmetic that mutates nothing the reader cares about. Scheduled with G26's
  `repeat`/`pad_start`/`pad_end`.
- **G35. A bare `x = e` gets no mut-teaching diagnostic.** E0006 exists to teach
  the `if` ban; D5 is the second-most-broken rule for a newcomer and there is
  nothing parallel. The parse error names a token, not the rule.
- **G36. The clap binding has no `allow_hyphen_values`.** A negative amount as a
  CLI argument (`--amount -12.50`) cannot be expressed.
- **G37. A two-binding `for` over a call's result binds a *string* index, and
  nothing catches it.** With G32 documented, the app could drop its hand-rolled
  line counter, and the natural spelling was wrong:
  `for i, raw in array.slice(lines, 1)` emits
  `for (const [i, raw] of Object.entries(...))`, so `i` is `"0"`, not `0`.
  `iter_is_array` (`glyph-emit/src/lib.rs:1783`) reads the iterand's type from
  the type map and falls back to the record lowering whenever it is not a known
  `Array`, and a call expression's result is not in the map. This is the
  silent-green class G1 and G2 belong to: `glyph build` is clean, `tsc --strict`
  passes (`"0" + 1` is legal TypeScript), and the loop prints `01:` where it
  should print `1:`. Verified on a three-line program. A `let` binding does not
  rescue it either: `let a = array.slice(...)` still emits `Object.entries(a)`,
  and only `let a: Array<string> = array.slice(...)` emits `a.entries()`, which
  is what the app does today. Fixing it means either
  recording a call's result type at its span or defaulting the unknown case to
  the array lowering, and both change what an unannotated iterand means, so it
  is a decision rather than a patch. Recorded in `docs/roadmap/releases.md`.
