# Step 6 dogfooding — gap list

Findings from building and running the fridge shopping-list app
(`examples/apps/fridge.glyph`) and probing the compiler/stdlib with real-app
patterns. The app itself builds, passes `tsc --strict`, runs end to end, and its
six `@example` tests pass — but writing it surfaced concrete gaps, several of
them silent-miscompile **bugs** (code that passes `glyph build` and `tsc` and
then misbehaves at runtime). Ordered by severity.

Later trips appended their own rounds below, so the file is both a backlog and a
record of what each app found. The numbered `G` entries are the backlog; the
per-round sections are history and stay as written.

## Reading the markers

A `G` entry carries its status in brackets after the number. No bracket means
open.

- **`[FIXED]`** — closed, with a note saying what closed it.
- **`[HALF FIXED]`** / **`[IMPROVED]`** — part of the claim is closed; the note
  says what remains.
- **`[DECIDED]`** / **`[RESOLVED]`** — not a defect. Either a documented v1 stance
  or an accepted won't-fix.

Reconciled against the source after the spreadsheet trip, which added eight
entries (G63–G70) and closed half of two of them with `E0110` and `E0111`: of 70
entries, 47 are fixed, 11 are partly fixed, 5 are decided or resolved, and 7 are
open. Six of the seven open ones are new, so the backlog grew for the first time
in several rounds. That is what a trip into a domain the earlier apps never
touched does: a spreadsheet wants a value union spelled `Number | Text | Empty |
Error`, and every name in it is a name the emitted module already owns. The two
halves that shipped are the silent ones, `E0110` for a top-level declaration that
would shadow a global the emitted module depends on and `E0111` for `type Key =
string | number`. What is left on both is expressiveness, not detection.

The reconciliation before it, after the stdlib-modeling batch (G45, G47, G61,
G50, and phase 1 of G39/G37), read 62 entries, 47 fixed, 9 partly fixed, 5
decided or resolved, and 1 open. That batch taught the typechecker the
return types of the fixed-arity half of `std/string` and `std/array` and the
*shape* of a stdlib named type, so `for i, part in string.split(text, ",")` binds
a number and a `match e.kind` on an `fs.FsError` is held to E0200; added the
`async fn(...) -> T` function type as D40; rewrote D12 to describe both string
spellings the lexer actually accepts; and closed the codepoint half of G50 as a
decision (Glyph indexes UTF-16 code units and ships no codepoint accessor). G39
is the one entry left open, and it is the phase-2 half of its own claim:
hard-erroring on the `Unknown`s that remain at the stdlib boundary.

The reconciliation before it, after the descriptor, range, and formatter batch
(G41, G30, G62), read 62 entries, 44 fixed, 10 partly fixed, 4 decided or
resolved, and 4 open. That batch made a descriptor's `.parse` return the
real `Result`, put `array.range` and `array.range_from` in `std/array`, and
stopped `glyph fmt` from collapsing an interpolating multi-line string back into
`\n` escapes. The workarounds came out of the apps in the same pass: the three
`upto`/`span` definitions and their 16 call sites are gone from `bracket.glyph`
and `minesweeper.glyph`, the two identity re-wrap `match`es around
`Bracket.parse`/`SeedFile.parse` are now `.map_err(...)`, and the five
`\n`-escaped HTML builders in `shortlink.glyph` are D12 multi-line strings that
survive `glyph fmt --check`. Every emitted `.ts` file is byte-identical before
and after, so nothing about what those apps print changed. G30 keeps its
index-safety half open beside G39.

The reconciliation before it, after the CLI and docs batch (G28, G42, G36, G55)
and the adversarial review of it, which added G61 and G62, read 62 entries, 42
fixed, 9 partly fixed, 4 decided or resolved, and 7 open. That batch
shipped `glyph check`, moved the build's green summary below the stage that can
turn it red, let hyphenated arguments reach a program through
`glyph run`, and closed G55 as the docs round it asked for. G62 was the sharp edge
left on G55: the multi-line string form that round documented is one
`glyph fmt` rewrote back into `\n` escapes whenever the string interpolates, so
the docs ran ahead of the formatter until that path was fixed. The reconciliation
before it, after the formatter batch (G60, G54, G29), read 60 entries, 38 fixed,
9 partly fixed, 4 decided or resolved, 9 open. The batch before it, which added
E0008, E0222, and E0223, closed G35 and G44 outright and took G24 and G48 to
`[HALF FIXED]`: the `?` scrutinee and the empty-record spelling are each still
open, and `linkcheck` still carries the
`no_cache()` workaround G48 is about. It also turned up G60, a formatter
round-trip that breaks a working program, which the formatter batch then closed
along with G54 and G29.

The reconciliation before it, after the `std/fs` breadth, `regex.captures_all`,
and `task.pool_settled` batch and the `linkcheck` rewrite that consumed it, read
59 entries, 33 fixed, 7 partly fixed, 4 decided or resolved, 15 open. Three of
that batch's four entries are `[FIXED]`, and the fourth stops
short: G47's error taxonomy is spellable in a pattern but nothing checks that the
`match` covers it, so an omitted kind is a run-time throw rather than an E0200.
G51 spent one release at `[HALF FIXED]` on the belief that `captures_all` could
not discriminate an alternation, and closed when rewriting the app showed that
wrapping each branch's group around the whole construct answers the question
without an offset. The item reported by
five consecutive trips, `std/string` breadth, is now mostly closed: `slice`,
`index_of`, `repeat`, `pad_start`, `pad_end`, `replace_all`, `trim_start`, and
`trim_end` ship, along with `array.fold`, `index_of`, and `flat_map`, and the
hand-rolled copies came out of the seven apps that carried them. One thing keeps
it off `[FIXED]` in G50: codepoint-aware iteration, blocked on deciding whether
`std/string` indexes code units or codepoints. A stdlib function that ships and
a workaround that survives it are different claims, which is why G26 and G34
only closed once the apps stopped hand-rolling.

## Verdict

The toolchain works for a clean, single-file, primitives-and-tagged-unions app.
The single most important class to fix before v1 is the **"silent green"**
failure mode: `glyph build` (and `glyph run`) report success on code that the
emitter mistranslates or that the runtime can't actually provide, because real
checking is deferred to an optional `tsc --check` — and even `tsc` misses the
two miscompiles below. The verifiability pillar (the project's lead claim) is
the most exposed.

G9 and G10 closed that class in the emitter and the `tsc` pass. It came back one
layer up: `glyph run` computed a full build report and then read only the list of
emitted files out of it, so a program that ran fine reported fewer diagnostics
than `glyph build` on the same tree, including warnings on the file the user
named. That is G38 below, and it is fixed. The lesson for the next fix in this
class is that "verified through `glyph build`" is not the same as verified.

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
- **G15. [FIXED] `mut` on a `const` is not enforced (D20 says it is).** `mut N = 6`
  against `const N` passes the Glyph typechecker; only `tsc` catches it (TS2588,
  no E-code). *Fixed: the typechecker raises `MutateConst` (E0212) with a test, so
  the rule is enforced in Glyph rather than left to `tsc`.*
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
- **G17. [FIXED] `glyph build --out X` never cleans `X`.** A renamed/removed source
  leaves a stale `.ts` that `tsc` and importers still pick up. *Fixed:
  `prune_stale_outputs` removes emitted `.ts` left behind by an earlier build.*
- **G18. [HALF FIXED] `glyph fmt` layout nits.** Deletes the blank line between a
  section comment and its declaration; wraps the innermost call's args instead of
  the long method chain. *The blank line is now preserved, with a test. A long
  method chain still wraps the innermost call's arguments: there is no
  chain-aware path in the printer at all, so the only breakable point in
  `a.b(x).c(y).d(z)` is one of the argument lists and the innermost is reached
  first. Adding one needs the layout rule decided first (break before every `.`
  once the chain overflows, or break only as many links as it takes), plus a
  guard for D1: a break outside `()`/`[]`/`{}` ends the statement, so a chain in
  a top-level `const` initializer must stay on one line whatever the rule.*

  The G54 width fix makes this worse to look at, not better. While the
  `INLINE_MAX` exemption stood, a long chain simply sat on one over-wide line and
  the eye read it as one thing. Now the width test runs at every element count,
  so the chain still does not break but the innermost argument list inside it
  does, and the result is a break in the middle of a chain that continues after
  the closing paren. Three sites in the reformatted `examples/apps` are that
  shape, all `||` chains: `adventure.glyph:521`, `:586`, and `:999`, where
  `string.contains(` or `string.starts_with(` is the last thing on the line and
  the rest of the chain resumes two lines down. It is not only method chains:
  any parent expression that has no break of its own hands the decision to its
  innermost child, so a binary operand or a JSX attribute value does the same
  thing.

  *The operator half is fixed. A `&&`/`||`/`??` chain that does not fit breaks
  one operand per line with the operator leading the continuation line, indented
  one level, so those three `||` sites get the chain break instead of an
  exploded argument list. Leading rather than trailing: the operator lands at a
  fixed column where `grep` finds it, it matches the leading `|` of union
  syntax, and adding an operand touches one line instead of two. Both forms
  re-parse identically, so nothing but style rode on the choice. Only the
  top-level run of one operator flattens, so `a && b || c && d` breaks at `||`
  and keeps each `&&` group whole. The D1 guard is `in_block`, set only under a
  `{ ... }` the printer opened: a block is always brace-delimited, so it implies
  bracket depth of at least one, and a module-level `const` initializer or a
  `where` predicate stays on one line whatever it measures. The guard is
  deliberately one-sided, so a bracketed expression outside any block loses a
  break the parser would have accepted rather than risking one it would not.
  `examples/apps` is reformatted, and all three sites now read as a broken `||`
  chain: `grep -rn '^\s*)\.' examples/` returns nothing, every emitted `.ts` in
  the tree is byte-identical to the pre-reformat build, and a second `glyph fmt`
  pass over `examples/` changes no byte. The `.`-chain half stays open on the
  receiver question above: `xs.filter(f).map(g).reduce(h, 0)` still comes back
  with `.map`'s argument on its own lines and `).reduce(` resuming the chain.*

  A narrower guard was considered and not taken (a list does not take its
  multi-line form when its immediate parent is an expression that cannot itself
  break). That is a new layout rule with its own edge cases and it belongs with
  the chain decision, not ahead of it.

  Six code lines in `examples/apps` are still over 100 columns after the
  reformat, and three of them are `fn` signatures: `minesweeper.glyph:289`,
  `expenses.glyph:281`, `bracket.glyph:434`. Those are a different residual, not
  the chain one. The width check measures the parameter list up to its closing
  `)` and never sees the ` -> Result<Command, CommandError> {` tail, so a
  signature whose return type pushes it over reads as fitting. The other three
  are `schedule.glyph:461` and the two `map_err` lambdas at `bracket.glyph:819`
  and `:863`, which are the same miss on a different shape: `lambda_block`
  inlines a one-statement body whenever the captured statement holds no newline,
  without checking what the inlined form does to the column.

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
- **G24. [HALF FIXED] `?` is rejected in an expression-form `match` arm.**
  `=> f(x)?` fails while `=> { return f(x)? }` and `=> return Ok(f(x)?)` both
  compile. One call site in the emitter uses `self.expr` where every other
  statement position uses `self.emit_value`. A missed call site, not a design.
  *The arm body is fixed: an expression arm of a `match` that is the whole value
  of a `let`, a `mut`, or a `return` emits through the same hoisting path as any
  other statement value, so `None => f(b)?` lowers to the unwrap plus the
  assignment inside its own `case` and passes `tsc --strict`. An arm of a `match`
  nested inside a larger expression is still rejected, now as E0302 naming the
  positional rule instead of the old false "not implemented yet". What remains is
  the scrutinee: `match load(p)? { ... }` is still refused, because the scrutinee
  is rendered through `expr`, which has no statement slot to hoist the unwrap
  into. That refusal no longer lies about itself either: it is E0303 ("`?` cannot
  be used in this position") with a bind-first fix. But the position is not
  supported, so the gap is half closed, not closed.*
- **G25. [FIXED] A value-position `match` cannot host block arms.** A `match`
  used as a sub-expression lowers to an IIFE that rejects block arms, and in that
  position G24 has no workaround. Structural and separate from G24. *Fixed in
  round 10 as part of G43: a `match` that is the whole value of a `let` or a
  `mut` assignment no longer goes through the IIFE at all. A `match` nested
  inside a larger expression still rejects block arms, because a function-level
  `return` there genuinely cannot be captured by an arrow.*
- **G26. [FIXED] `std/string` has no `repeat`, `pad_start`, or `pad_end`.**
  Every program that renders a grid or aligned columns needs them; the app
  hand-rolled all three. Three wrappers plus three names in the resolver seed.
  *Fixed: all three ship, and the six apps that hand-rolled them (settle,
  expenses, bracket, schedule, linkcheck, and minesweeper as `repeat_text`/
  `pad_left`) now call the stdlib. `repeat` clamps a negative count instead of
  throwing the way TS does, which is what makes `repeat(pad, width - len(s))`
  safe, and `pad_start`/`pad_end` leave a string that is already at least `width`
  long alone.*
- **G27. [HALF FIXED] An unknown stdlib namespace member leaks a raw `tsc` error.**
  `import std/string { repeat }` gives a clean E0105, because `verify_imports`
  checks named imports against the resolver seed. `string.repeat(...)` gives a
  TS2339 carrying an absolute build path, because nothing checks member access
  against the same seed. The same typo gets two experiences depending on import
  style, and the absolute path in a remapped TS error is a second, separable
  defect. *The path half is fixed: a `tsc` error now remaps onto a relative Glyph
  module path. Member access is still unchecked against the resolver seed, so the
  typo still surfaces as a TS2339 rather than an E-code.*
- **G28. [FIXED] There is no `glyph check <file>`.** `build` rejects a non-directory
  source, so the only door into type checking a single file is running it.
  *`glyph check [path]` ships. It takes a `.glyph` file or a directory, reuses
  `build`'s pipeline into a temp dir it deletes on the way out, runs `tsc
  --strict` over the emitted TypeScript by default (`--no-tsc` stops after the
  Glyph stages), and exits 0/1/2 the way `build` does. `--json` emits the same
  keys `build --json` uses, minus `emitted` and `examples`. Nothing is written to
  your tree and nothing is executed: the regression test asserts an empty stdout
  and an absent sentinel file for a program whose `main` would have written one.
  Two consequences are documented rather than hidden. A file is checked in the
  context of its directory, so a sibling's error is reported and fails the check,
  exactly as `build` and `run` see that tree. And the `@example` / `@doc @run`
  gate does not run here, because running it would run your code, so a green
  `check` promises the types and not the colocated tests.*
- **G29. [FIXED] Two formatter layout complaints, deliberately not bundled with
  G23.** A one-statement `match` arm body is always exploded to three lines,
  because the parser wraps it in a synthetic block and every block prints
  multi-line; and the `INLINE_MAX` check short-circuits the width test,
  flattening every two-argument `array.map(xs, fn(...) { ... })`. *Both fixed. An
  arm body that is a synthetic one-statement block now prints as `X => { break }`
  through the same helper a one-statement lambda body already used, and the
  `INLINE_MAX` exemption is gone (see G54).*
- **G30. [HALF FIXED] Two decisions the trip surfaced.** `for` has nothing in the
  stdlib that produces a counted range, so the most common bounded loop cannot
  use the keyword D21 built for bounded loops and gets hand-rolled from
  `loop`/`match`/`break` instead, which costs greppability. And `xs[i]` types as
  `Ty::Unknown` with `noUncheckedIndexedAccess` off and no `array.get`, so
  `cells[999]` type-checks clean, passes `tsc --strict`, and hands back
  `undefined` where the compiler claimed `Cell`. Both are architecture forks with
  their options and costs written out in `docs/roadmap/releases.md`; neither is
  decided here. *The range half has its stdlib entry: `array.range(count)` and
  `array.range_from(start, end)` are in `std/array`, so `for i in
  array.range(n)` is the counted loop and the hand-rolled `upto(n)` built from
  `loop`/`match`/`break` goes away. It is a stdlib function, not range syntax:
  `..` would be language surface that costs grammar and forecloses later
  decisions, while a function costs nothing and reads beside `slice`. `range`
  takes a count and clamps it the way `string.repeat` does, so `range(-1)` is
  `[]`. `range_from`'s second argument is an exclusive end bound, matching
  `array.slice`, `string.slice`, and the `span(lo, hi)` in `bracket.glyph` it
  replaces, so `range_from(2, 5)` is `[2, 3, 4]` and the port is textual; it
  was written first as `(start, count)`, which returned a different array from
  the same call with no type error to catch it. The typechecker models
  both as `Array<number>` so the loop variable binds as a number rather than
  falling back to `Unknown` — without that entry, replacing a typed
  `upto(n) -> Array<int>` would have traded a hand-rolled loop for a typing
  regression. The apps were ported in the same pass: `upto` and `span` are
  deleted from `bracket.glyph` and `minesweeper.glyph`, all 16 call sites read
  `array.range(n)` or `array.range_from(lo, hi)`, and the emitted `.ts` for both
  apps is byte-identical to what the hand-rolled helpers produced. The
  index-safety half is untouched and stays open beside G39.*

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
- **G34. [FIXED] `std/string` has no `slice`, and `std/array` has no
  `fold`.** The `fold` gap is the one that costs a pillar: with no fold, every
  accumulation is a `mut` in a loop, which dilutes `grep mut` (the whole point of
  D5) with arithmetic that mutates nothing the reader cares about. Scheduled with
  G26's `repeat`/`pad_start`/`pad_end`. *Fixed: `string.slice` behaves like
  `array.slice` (exclusive `end`, negative indices counting back from the end),
  and `array.fold` takes the collection, then the seed, then the callback, so it
  reads like the rest of the module. The callback gets `(acc, x)` and no index.
  The accumulation loops the apps wrote before `fold` existed are folds now, and
  `grep mut` over `examples/apps/` went from 192 sites to 161.*
- **G35. [FIXED] A bare `x = e` gets no mut-teaching diagnostic.** E0006 exists
  to teach the `if` ban; D5 is the second-most-broken rule for a newcomer and
  there is nothing parallel. The parse error names a token, not the rule.
  *Fixed: a bare `x = e` (and `r.field = e`, `xs[0] = e`) is E0008, "assignment
  requires `mut`", with the D5 help line and an `--explain` body carrying the
  before/after. It fires in a block and in a `match` arm, where the old message
  was "expected `,` after match arm". Like E0006 it aborts the parse, so a file
  with several bare assignments reports them one build at a time.*
- **G36. [FIXED] The clap binding has no `allow_hyphen_values`.** A negative amount as a
  CLI argument (`--amount -12.50`) cannot be expressed. *The surface is `glyph
  run`'s trailing argv passthrough, and only that: a built program run under node
  was never affected, and no other subcommand takes program arguments, so the
  original sentence is true of the dev loop rather than of Glyph programs. Before
  the fix clap rejected `--amount` as an unknown argument and read a bare
  `-12.50` as a short-flag cluster; `#[arg(trailing_var_arg = true,
  allow_hyphen_values = true)]` lets both through untouched. clap starts the
  trailing var-arg on unknown flags only, so a flag glyph itself knows still
  binds to glyph wherever it appears (`glyph run x.glyph --no-check` is
  unchanged, and pinned by a test); `--` remains the answer for a program flag
  that collides with one of glyph's.*
- **G37. [HALF FIXED] A two-binding `for` over a call's result binds a *string* index, and
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

  *Partly closed.* A `match` expression used to type as `Ty::Unknown` no matter
  what its arms produced, which meant a binding taken out of the only branching
  construct in the language reached `iter_is_array` with no type. The typechecker
  now joins the arms by equality, so `let w = match get() { Ok(v) => v, Err(_) =>
  return 1, }` types as the success type and `for i, row in w.rows` gets the
  numeric-index lowering with no annotation.

  The arm join alone was not enough for the site it was written for. `settle`
  reads its ledger through `WireLedger.parse(decoded)`, and the checker had no
  signature for the `parse` a type declaration's runtime descriptor emits, so the
  scrutinee was undecidable, the `Ok(w)` arm bound nothing, and the join had
  nothing to join. `T.parse` now types as `Result<T, Array<Issue>>` for the
  non-generic record, union, and refined-primitive types that emit a descriptor,
  read off the same shape the emitter writes. With both pieces in place the
  annotation came out of `examples/apps/settle.glyph` and the loop still binds a
  number: a corrupt third entry reports `expense 3`, not `expense 21`.

  The half that remains is the one that was always a decision: an iterand whose
  type is honestly unknown, such as a call into an unmodeled stdlib function
  (`array.slice`, `string.split`), still falls back to the record lowering.
  Closing that means either modeling the stdlib return types or hard-erroring on
  an unknown-typed iterand. See Round 12.

  *The first of those landed, which is what moves this to `[HALF FIXED]`.*
  `stdlib_fn_ty` now models the fixed-arity half of `std/string` and
  `std/array`, so `for i, part in string.split(text, ",")` and `for i, x in
  array.filter(xs, keep)` emit `.entries()` and bind a number with no
  annotation. The element type rides on a `Ty::Param` bound from the first
  argument, so `array.filter` over an `Array<string>` is an `Array<string>` and
  the type survives into whatever reads the loop body.

  What is left is a smaller residue than the entry describes, and it is G39's,
  not this one's. Two classes of call still have no type. The six stdlib
  functions with an optional trailing argument (`array.slice`, `string.slice`,
  `string.index_of`, `string.pad_start`, `string.pad_end`, `json.stringify`) are
  out because the call-arity check compares one number against one number, so
  modeling them would report a false E0213 on every call that omits the last
  argument; that check needs a range first. And `array.map`, `array.flat_map`
  and `array.zip` are out because their element type comes from the callback's
  return, which the unifier does not walk. An iterand from either class keeps
  the record lowering, so `for i, x in array.slice(xs, 1)` still needs its
  `Array<T>` annotation.

  *Measured on the apps.* Ten `Array<string>` annotations over `string.split`
  came out of `examples/apps/`: six in `shortlink.glyph`, three in
  `linkcheck.glyph`, and one in `bracket.glyph`, where the deleted line also
  carried a two-line comment saying the annotation was load-bearing. `bracket`'s
  `put` now reads `for i, c in string.split(text, "")` and emits `.entries()`;
  its ASCII bracket renders byte-identically. `examples/apps/expenses.glyph:139`
  is the one site that had to keep its annotation, because it slices, and its
  comment was narrowed from "the type of a std-module call" to `array.slice`
  specifically. `shortlink.glyph:348` keeps its annotation for the same reason
  (`array.map`).

## Round 8 — a text adventure, and the return of "silent green"

The loop pointed at `examples/apps/adventure.glyph`, a text adventure: rooms, an
inventory, a command parser. Thirteen findings came out of it. Twelve are the
compiler not knowing something (member access and call arguments against a
`Ty::Unknown` receiver go unchecked at the stdlib boundary) or the stdlib not
having something (`string.slice`, `string.lines`, `array.fold`, a `compare`).
One is different in kind, and it is the one fixed here.

- **G38. [FIXED] `glyph run` computed a build report and threw its diagnostics
  away.** On the success path `run_file` read `report.emitted` and nothing else,
  so `report.diagnostics`, `report.structured` and `report.error_count` never
  reached the user. `glyph run solo.glyph` printed the program's output and
  exited 0 while `glyph build .` on the identical tree printed E0204 on a sibling
  and E0106 on `solo.glyph` itself. The eaten warning was on the entry file the
  user pointed the command at, so this was not the documented "sibling modules
  are best-effort" stance: nothing decided to suppress it, it was never wired up.
  *Fixed: `run_file` returns a `RunResult` carrying the outcome plus every
  diagnostic the build computed, and `glyph run` prints them before dispatching
  on the outcome, followed by a `glyph run: N error(s), M warning(s) in the
  source tree` summary on the path where the program ran. The warm run cache
  carries them too: a build writes `.glyph-diagnostics.json` into its staging
  directory (so it moves into place atomically with the rest), and a cache hit
  whose sidecar is missing, unreadable, or rendered for a different color setting
  is treated as a miss and rebuilt rather than reported as a clean tree. The
  regression test that matters is the second run: reporting a warning once and
  then falling silent reads as a warning that went away. Exit codes are
  unchanged; whether a sibling error should also make `glyph run` exit non-zero
  is a separate call, tracked in `docs/roadmap/releases.md`.*
- **G39. Member access and call arguments against `Ty::Unknown` are unchecked.**
  `s.slice(0, 1)` on a `string`, a misspelled `xs.pusj(x)`, and a wrong-arity
  call into a stdlib namespace all compile, because the receiver's type is
  `Unknown` and the checker has nothing to check against. The manifesto promises
  no `any`; this is one, spelled `Unknown`, load-bearing at exactly the boundary
  where the promise is made. Closing it is an architecture decision rather than a
  patch (model the stdlib from its own `.d.ts` sources per Q21/Q40, or keep
  growing the hand-written `stdlib_fn_ty` table), so it is recorded in
  `docs/roadmap/releases.md` and not fixed here.

  *Phase 1 landed; the entry stays open for phase 2.* The chosen direction is
  the hand-written table, grown: `stdlib_fn_ty` now models the fixed-arity half
  of `std/string` and `std/array` (returns, plus `Array<T>` where the element
  type has to travel), and there is now a shape model for a stdlib *named* type
  — `fs.FsError`, `fs.FileInfo`, `fs.ErrorKind` — hooked into `record_fields_of`,
  `required_variants` and `variant_payload`. So `e.mesage` on an `fs.FsError` is
  E0210 and a `match e.kind` missing a kind is E0200, where both were silent
  before. Most `Unknown`s at the stdlib boundary are gone.

  Phase 2 is the hard-erroring half, and it is what this entry still names.
  Nothing yet rejects member access, call arity, or an argument type against a
  receiver that is *still* `Ty::Unknown`: `s.pusj(x)` on a `string`, a
  wrong-arity call into a namespace the table does not model. Deciding that also
  means deciding whether an iterand whose type is unknown is an error or
  defaults to the array lowering (the residue of G37), and whether modeled
  stdlib parameters get real scalar types — today only `Array<T>` and `T` are
  typed, so `string.len(42)` is still not an E0211. Recorded in
  `docs/roadmap/releases.md`.

## Round 9 — a scheduler, and a boundary that was open the whole time

The loop pointed at a scheduling app: time ranges, blocks, a JSON boundary, types
split across modules. The report's headline was that a `where` refinement stopped
working the moment the type was used as a field. Probing one step further found
the same hole on a second axis, and the two are one defect.

- **G40. [FIXED] Descriptor resolution scanned only the emitting module and knew
  nothing about refinements.** `has_local_descriptor` recognized a module-local
  non-generic record and a module-local non-generic tagged union, and nothing
  else. So a D39 refined alias in field position dropped its predicate
  (`Instant.parse("no")` -> Err, but `Block.parse({ start: "no" })` -> Ok, and
  `json.parse<Instant>` validated nothing), and a field typed by a record
  **imported from another project module** was checked by `!== undefined`, which
  covers every non-generic cross-module composition in every multi-file Glyph
  program: `Outer.parse({ i: 42 })` returned Ok. Both built clean and passed
  `tsc --strict`. The cross-module machinery already existed for imported
  *generic* descriptors (`generic_descriptor_arity` resolves through
  `ImportNamed` and a project registry, shipped in 0.1.23); the hard version was
  built and the easy one was not. *Fixed in the resolver, not at the symptoms:
  `has_descriptor` accepts a refined alias and resolves an imported name the same
  way the generic path does, backed by a new `plain_descriptors` project
  registry. Four call sites read it, so the one change closes the record-field
  drop, the `Array<Refined>` element drop, the `Option<Refined>` payload drop,
  the union-variant payload drop, the synthesized checker for a generic
  descriptor, `is T` narrowing, and `json.parse<T>` for both refined and imported
  types. The namespaced form (`import types`, then a field typed `types.Inner`)
  is handled too; it previously was not handled at all. A regression test pins
  the value import (`import { Inner } from "./types"`), because an "emit `import
  type` for type-only uses" optimization would erase the binding `Inner.is`
  depends on with `tsc` still clean.*
- **G41. [FIXED] A descriptor's `.parse` result is not assignable to `Result`.** TS2322,
  confirmed; the cookbook recipes that thread `T.parse(x)` into a function
  returning `Result<T, E>` do not compile. Separate from G40 and tracked in
  `docs/roadmap/releases.md`. *An impedance bug between two stages, not a design
  question. The emitter wrote `parse` as a bare `{ tag, value }` union so a
  descriptor would compile in a module that never imports `std/result`, but
  `Result<T, E>` intersects the `map`/`map_err` combinators, so the bare union is
  not assignable to it: `return User.parse(v)` was TS2322 and
  `User.parse(v).map_err(f)` was TS2339, while Glyph's own checker reported
  `parse` as `Result<T, Array<Issue>>` the whole time. The same silent-green
  family as G38 and G42, one stage later. All three descriptor kinds (record,
  refined alias, union) now annotate `parse` as the real `Result` and build both
  arms with the prelude constructors, under an injected aliased import
  (`import { Ok as __glyph_ok, Err as __glyph_err, type Result as __GlyphResult }
  from "std/result"`). `?` and a descriptor share that one import line, since
  two of them would redeclare `__glyph_err`. Two costs worth stating exactly.
  The import is a value import, so every module declaring any record, union, or
  refined type now carries a runtime edge to `std/result` even if it never
  mentions `Result`; the old inlined wire format carried none. That is not the
  same bargain `?` and `T.schema` strike, since those are paid only by the
  modules that use them. And every `T.parse` allocates the two combinator
  closures the constructors build, which is what an `Ok(...)` costs, but `parse`
  runs once per inbound record per request where an `Ok(...)` runs once per
  function return, so it lands on the boundary path rather than on an occasional
  return. `?` on a parse result still works;
  its lowering only reads `.tag`/`.value`, and `infer_output<S>` still reduces,
  because `Extract` over the `Result` union still selects the `Ok` member.*
- **G42. [FIXED] `glyph build` prints "no diagnostics" above its own `tsc` errors.** The
  Glyph-stage summary is printed before the TypeScript stage runs, so a red build
  is introduced by a green line. The same "silent green" family as G38, one stage
  later. *The summary now prints after the `tsc` gate and the example gate, next
  to the `tsc --strict passed.` line that was already held back for the same
  reason: one rule, nothing signs off until every stage has run. A red build
  never prints it at all. Wording is unchanged, so a green build's transcript
  still reads summary then `tsc --strict passed.`, and both orders are pinned by
  tests. The `--json` path was never affected (`emit_build_json` runs `tsc`
  itself and folds the result into one object before printing). One thing the
  reorder does not settle: under `--no-check` the line still says "no
  diagnostics" on a build where no TypeScript stage ran. It is honestly about the
  Glyph stage, and rewording it is a separate call.*

## Round 10 — a Markdown link checker, and the emitter as the source of truth

The loop pointed at `examples/apps/linkcheck.glyph`: walk a directory, scan text
for Markdown links, fan out HTTP requests, report. It could not do any of those
three things without reaching past the language, so the round produced a long
stdlib list. The headline is not on that list. Five separate findings ended with
the same sentence — Glyph reported no diagnostics and only `tsc` caught it — and
a sixth turned up while checking them. The compiler is supposed to be the source
of truth. On the async path it was a preprocessor with opinions.

- **G43. [FIXED] Value-position `match` picked its lowering on the wrong
  condition.** The emitter has two lowerings for `match`, a flat statement
  `switch` and a value IIFE, and `Stmt::Let` chose between them by asking "is any
  arm a block?" instead of "is this match the whole initializer?". Three symptoms
  came out of that one guard: an `await` in an arm landed inside a synchronous
  arrow and `tsc` rejected it (TS1308); a self-referential accumulator
  (`mut on = match on { ... }` in a loop) went through an untyped IIFE and tripped
  circular inference (TS7024); and `Stmt::Mut` had no `Expr::Match` path at all,
  so a block arm was a hard `EmitError` under `mut` where the identical `let`
  compiled (that is G25). All three built clean under `glyph build`. *Fixed by
  deleting the special case rather than adding one: a `match` that is the whole
  value of a `let` or a `mut` assignment always lowers to the flat `switch`,
  which declares (or reuses) the binding and assigns it per arm, with the
  existing `default: throw` keeping `tsc`'s definite-assignment analysis happy.
  The IIFE now fires only for a `match` nested inside a larger expression, and
  there an `await` in an arm makes it an awaited async arrow. Two follow-ons that
  the wider path exposed: a `break`/`continue` in a `mut`-bound or `let`-bound
  match arm now labels its loop (an unlabeled `break` would have escaped only the
  `switch`), and an empty array literal in an arm is pinned to `never[]`, because
  a bare `[]` assigned to an unannotated `let` starts TypeScript's evolving-array
  inference and every later read becomes an implicit `any[]` (TS7034/TS7005).*
- **G44. [FIXED] `await` in a non-`async fn` is not checked by Glyph.** `fn
  nope() -> int { return await slow() }` builds with no diagnostics and fails at
  `tsc` with TS1308. There is no Glyph-side check of async context anywhere; the
  whole async story is delegated. Same family as G43 and the reason the
  async-arrow fallback above is no regression. *Fixed: E0222, "`await` is only
  valid inside an `async fn`". The innermost enclosing callable decides, which
  matches TypeScript and means a synchronous lambda inside an `async fn` is
  flagged while the same lambda written `async fn(...)` is not. One thing is
  deliberately left permissive: an `await` in a module-level `const` initializer
  has no enclosing callable, and the emitted ESM accepts top-level `await`, so it
  is not reported. Whether Glyph wants implicit async module initialization with
  nothing in the source marking it is a spec question, not a checker bug; it is
  in `docs/roadmap/releases.md`.*
- **G45. [FIXED] An `async` function type is unspellable.** `parse_atom_type` has exactly
  one function-type entry (`Token::Fn`) and `TypeExpr::Fn` carries no async bit,
  so a parameter that takes an async callback cannot be typed. Closing it is a
  fork — `async fn() -> T` emitting `() => Promise<T>`, versus `fn() -> T`
  emitting `() => T | Promise<T>` — and that is an orchestrator call, not an
  agent's. Deliberately out of scope for the G43 fix.
  *Fixed by the first form, written down as D40. `TypeExpr::Fn` carries an
  `is_async` flag, `parse_atom_type` takes a leading `async`, and the emitter
  renders the return as `Promise<T>` (`async fn()` with no return type becomes
  `() => Promise<void>`). `glyph fmt` prints the `async` back, so a signature
  using it is a fixed point. The flag defaults to `false`, so no existing parse
  tree or emitted type changed. The checker enforces the distinction:
  `definitely_incompatible` compares `is_async`, so a sync function where an
  `async fn` type is expected is E0211 at a call argument and E0204 at a return,
  and the reverse direction is caught the same way. It stays
  permissive when either side returns `void`, matching the existing return-type
  guard, because TypeScript lets any function stand where a `void`-returning one
  is expected. One limit worth knowing: a plain `fn() -> T` still emits `() =>
  T`, so an async value does not fit one. In the app, `linkcheck`'s `task_for`
  dropped its "a Glyph function type cannot spell an async thunk" comment and now
  reads `fn task_for(url: string) -> async fn() -> Fetched`, which emits
  `function task_for(url: string): () => Promise<Fetched>`; writing a plain
  `fn()` there instead is E0204. A handler map keyed
  by route (`type Handler = async fn(string) -> string`) is covered end to end by
  `async_fn_type_annotates_a_handler_map` in the CLI integration tests.*
- **G46. [FIXED] `std/fs` has no `read_dir`, `is_dir`, or `stat`.** Six exported
  functions and nothing that enumerates a directory. The app discovered
  directories by reading every path in a tree and inspecting the `errno` it got
  back, which is as bad as it sounds. Blocking for any CLI that takes a path.
  *Fixed: `fs.read_dir(path) -> Result<Array<string>, FsError>` (entry names, one
  level, not recursive), `fs.is_dir(path) -> bool` (no `Result`, mirroring
  `exists`: a missing or unreadable path is `false`), and
  `fs.stat(path) -> Result<FileInfo, FsError>` where
  `FileInfo = { is_dir, is_file, size, modified }` with `size` in bytes and
  `modified` in epoch milliseconds, so it feeds `time.format_iso` directly. Both
  Result-returning functions are modeled in the typechecker, so `?` on them
  decides its error type the way `read_text` does. Two things deliberately did
  not ship. There is no recursive `walk` or glob: a walk is `read_dir` + `is_dir`
  + `path.join` in about ten lines, and a walk primitive is surface the gap did
  not ask for. And `read_dir` returns entries in OS order, which differs across
  platforms and filesystems, so a program that wants a reproducible report still
  sorts them itself; whether the stdlib should sort is a behaviour call left
  open. There is no `is_file` either, since that is `let info = stat(p)?`
  followed by `info.is_file` (the `?` operator does not chain into a field
  access, so the binding is not optional).*
- **G47. [FIXED] `FsError.kind` is `{ tag: string }` with one constant.**
  `NotFound` is
  the entire taxonomy, so an fs error cannot be matched exhaustively. That is the
  errors-as-values promise leaking. It batches with G46: one change to `fs.ts`
  that names the errnos a filesystem program recovers from (`NotFound`,
  `IsADirectory`, `NotADirectory`, `PermissionDenied`, `AlreadyExists`) and keeps
  an `Other { code }` tail.
  *The taxonomy shipped, the checking did not. `ErrorKind` is now the closed set
  `NotFound`, `IsADirectory`, `NotADirectory`, `PermissionDenied`,
  `AlreadyExists`, and `Other({ code })` carrying the raw errno for anything
  unnamed. Every kind is spellable in a pattern, so the app's
  `e.kind.tag == "EISDIR"` errno comparison becomes `fs.ErrorKind.IsADirectory`.
  EACCES and EPERM collapse into `PermissionDenied`, so those are the two raw
  codes that do not survive. This is `[HALF FIXED]` rather than `[FIXED]` because
  the gap's word was "exhaustively", and nothing checks the match: the typechecker
  models stdlib function return types and has no field model for a stdlib named
  type, so `e.kind` types as unknown, a `match` over it resolves no required
  variants, and omitting `PermissionDenied` is not an E0200. It emits a
  `default: throw` and dies at run time, so keep an `else` arm. Spellable is real
  progress; checkable is what the entry asked for. Teaching the compiler the
  taxonomy means a stdlib named-type table, which is the general "model the
  stdlib's types, not just its signatures" question G39 and Q21 already own, and
  deciding it is an orchestrator call rather than something to settle inside a
  stdlib-breadth round. The names are load-bearing even unchecked. `linkcheck`
  now recovers by kind in `fs_reason`, and on a tree containing a `chmod 000`
  directory the rewritten app prints `permission denied` and counts the path as
  unreadable, where the pre-batch app (which could only ask whether the errno was
  `EISDIR`) dropped that directory from the report with no row and no mention.*

  *Now fixed: the checking shipped too.* The typechecker has a shape model for a
  stdlib named type — `stdlib_type_fields`, `stdlib_union_variants` and
  `stdlib_variant_payload` in `glyph-typechecker/src/assign.rs`, hooked into
  `record_fields_of`, `required_variants` and `variant_payload`. A written
  `fs.FsError` annotation lowers to the same synthetic named type the stdlib
  return table produces, which is what was missing: two-segment type paths used
  to lower to `Ty::Unknown` regardless. So `e.kind` resolves to the closed
  `fs.ErrorKind`, a `match` over it covering all six kinds needs no `else` arm, a
  match missing one is `E0200: non-exhaustive match on 'fs.ErrorKind'`,
  `fs.ErrorKind.Other({ code })` binds `code` as a `string`, and a typo'd
  `e.mesage` is E0210 instead of a `tsc` error. The negative case is pinned in
  `tests/negative/fs_error_kind_not_exhaustive.glyph`. The model is hand-kept
  against `runtime/std/fs.ts`; a kind or field added there has to be added to
  both. The `else` arm and its four-line "required" comment are gone from
  `examples/apps/linkcheck.glyph`, replaced by
  `fs.ErrorKind.Other({ code }) => "${e.message} (${code})"`; deleting the
  `PermissionDenied` arm from that app now fails `glyph build` with
  `E0200 ... missing variants 'PermissionDenied'`.
- **G48. [HALF FIXED] `{}` as a match arm is silent green.** `true => {}` parses
  as an empty
  block, emits `case true: { break; }`, and the function falls out of its own
  switch returning `undefined` while claiming a record type. No Glyph diagnostic;
  `tsc` catches it as TS2366. Narrower than first reported: an unquoted `{ a: 1 }`
  arm parses fine, so only `{}` and string-keyed literals are affected. Two
  halves, both needing a decision rather than a patch: `X => {}` as a deliberate
  no-op *statement* arm is meaningful, so `{}` cannot simply be reread as a
  record; and the deeper half is that `check_return_type` never asks whether a
  value-position arm produces a value at all. The next verifiability item after
  G43. The obvious workaround does not survive the toolchain: `=> ({})` compiles
  and passes `tsc`, and then `glyph fmt` takes the parentheses back off and the
  formatted file reproduces the error. Until this is decided, an arm that means
  "the empty map" needs a named constructor.
  *The silent-green half is closed and the spelling half is not. E0223 now
  reports a value-position arm that produces no value (an empty block, or a block
  whose tail is a `let`/`mut`/`for`/`loop`) wherever the position is decidable: a
  `let`, a `mut`, a `return`, or the tail of a callable with a declared non-`void`
  return type, recursing into nested value-position arms. A statement-position
  `X => {}` is untouched, and none of the 9+ such arms across `examples/apps/`
  fires. The string-keyed literal half of the parse is also fixed: `A => {
  "Content-Type": "application/json", }` now parses as an object literal, since a
  block cannot begin with `"str" :`. What is not fixed is the thing the gap's
  last sentence is about. There is still no way to spell an empty record in arm
  position, so `examples/apps/linkcheck.glyph:738` still carries `fn no_cache()
  -> Record<string, Outcome> { return {} }` and line 1017 still calls it with a
  comment naming this gap. E0223 makes the workaround's absence a hard error
  instead of a runtime `undefined`, which is progress, not closure. The `=>
  ({})` round-trip is worse than the entry says and is now tracked on its own as
  G60.*
- **G49. [FIXED] `@example` execution was opt-in behind `--test`, contradicting
  D23.** D23 is tagged verifiability precisely so an agent rewriting a body
  cannot bypass the examples, and a flag is a bypass by default. The `--json`
  half was worse: `emit_build_json` diverges and was called before the example
  block, so `glyph build --test --json` printed `"ok": true, "tsc": "passed"` on
  a project whose own `@example` asserted something false. The agent-facing
  channel could not report a failing colocated test even when asked to.
  *Fixed: the `@example` and `@doc @run` checks run on every `glyph build`;
  `--no-test` opts out and prints how many tests it skipped, and `--test` is
  accepted and ignored for compatibility. Under `--json` the checks run before
  the JSON is emitted, and the result is an `examples` object
  (`total`/`ran`/`skipped`/`failures`) folded into `errors` and `ok`, so the two
  channels agree on the exit code. A missing `tsx` on a project that has
  examples is treated the way a missing `tsc` already was: no success line,
  `"ok": false`, exit 2, rather than a build that looks verified. A project with
  no examples pays nothing (the runner returns before it copies anything), which
  a test now pins.*
- **G50. [DECIDED] `std/string` and `std/array` are still short of the basics.**
  `string.slice`, `index_of`, `replace`, `repeat`, `pad_start`, `pad_end`,
  `trim_start`, `trim_end`; `array.fold`, `index_of`, `flat_map`. This is G26's
  third sighting and G34's second, and `array.fold` was already written up as
  "the one that costs a pillar". They are abstraction chores, which is exactly
  why they keep getting picked over harder work. Batch them in one stdlib round,
  and settle `iter.take_while` at the same time: D21's prose cites it and it does
  not exist. *Half fixed: eleven of the twelve names ship. Replacement landed as
  `replace_all`, which replaces every occurrence and has no first-only form, so
  it cannot be confused with TS `String.prototype.replace`. Both `index_of`
  functions return `Option` rather than a `-1` sentinel, and `string.index_of`
  takes an optional start index. Two things remain. Codepoint-aware `chars` and
  `char_at` are still missing, because shipping them means deciding whether
  `std/string` indexes UTF-16 code units (what `len` and `split` do today) or
  codepoints, and the two answers disagree on any non-BMP string. And
  `iter.take_while` is untouched: there is no `std/iter` at all, and the nearest
  module, `std/stream`, is a test-data generator rather than a lazy sequence, so
  it needs a design rather than a wrapper.*

  *Decided, for the codepoint half only.* Glyph strings are sequences of UTF-16
  code units, and they will stay that way: `len`, `split`, `slice`, `index_of`,
  `pad_start` and `pad_end` all count and cut in that space today, exactly as
  TypeScript does. `chars` and `char_at` are not shipping, because an accessor
  that can hand back half of a surrogate pair is worse than no accessor at all —
  the half reads as a character right up to the point where it is written out,
  and the failure lands far from the call. A program that has to walk codepoints
  converts to bytes first, with `encoding.hex_encode` and two hex digits at a
  time, which is what `examples/apps/shortlink.glyph` does in its slug encoder.
  Written down in D12, `docs/reference/stdlib.md`, and the `std/string` runtime
  header. Be plain about what this decision buys: **no workaround comes out of
  any app.** The hex walk stays; it is now the documented answer rather than a
  gap.

  This decision covers the codepoint question and nothing else. The
  `iter.take_while` / `std/iter` item in the note above is still undecided and
  still lives here — whether it becomes its own `G` entry or an open question is
  a call nobody has made.*
- **G51. [FIXED] `regex` cannot iterate captures.** `regex.find_all` maps
  each match to
  `m[0]` and drops the groups, so a scanner that needs the capture text has to be
  hand-rolled. It turned a 15-line link extractor into a 180-line character
  scanner. *Added: `regex.captures_all(pattern, text) -> Array<Array<string>>`,
  one inner array per match holding groups 1 onward, symmetrical with the
  existing `captures` (first match only). It inherits `captures`'s two
  conventions, both of which differ from JavaScript's `matchAll`: the whole match
  is not in the array, and a group that did not participate is `""` rather than
  `undefined`. The second convention was the reason this entry sat at `[HALF
  FIXED]` for one release: an empty capture and an absent capture read the same,
  so a scanner that asks which group is non-empty to learn which alternation
  branch fired looked unserviceable, and `linkcheck`'s character scanner stayed
  hand-rolled. Rewriting the app closed it. Write each branch's group around the
  whole construct rather than around its payload, and a group that fired can
  never be empty, because it starts with `` ` ``, `[`, `!`, or `<`. The
  possibly-empty capture (the link target) is then nested inside a discriminator
  group and never asked whether it fired, so `[]()` still reports an empty
  target. Ordering the alternation with the code span first reproduces the "a
  link inside backticks is not a link" rule without tracking an offset, which is
  what made the offset unnecessary too. The scanner is now two patterns and a
  12-line dispatch: `scan_inline`'s stepping loop, `type Step`, `skip_code_span`,
  `autolink_at`, `looks_like_autolink`, `bracket_link_at`, and the `index_of`
  chain in `reference_definition` are all deleted, 81 lines of scanner plus 16 of
  reference-definition parsing, and both the app's own header and a hostile
  fixture (nested brackets, an unterminated code span, angle-bracketed targets,
  autolinks containing bracket pairs) produce byte-identical output before and
  after. What `Array<Array<Option<string>>>` would still buy is a scanner whose
  discriminator genuinely can match empty; that is a narrower case than this
  entry claimed, and it stays parked against `captures` agreeing with it.*
- **G52. [HALF FIXED] `std/http` cannot bound or observe a request.** `Response` is
  `{ status, body }` with no headers and no final URL, so a redirect is
  invisible; `RequestInit` is `{ method }` with no timeout and no redirect
  policy; there is no `head`. The app's `task.race` timeout workaround leaves the
  loser in flight, which is the exact thing `task.ts`'s own doc comment says the
  scope exists to prevent. A network client that cannot bound a request is not
  shippable. One coherent `std/http` round. *The observe half is fixed:
  `Response` now carries a required `headers` field, the client fills it from the
  fetch response with the names lowercased, and the server writes it to the wire
  (`html`, `redirect`, and `with_header` build it; `form` reads a form-encoded
  request body). The bound half is untouched: still no timeout, no redirect
  policy, no `head`, and no final URL after a redirect, so a client still cannot
  see that it was redirected, only that it landed somewhere.*
- **G53. [FIXED] `task.pool` is fail-fast with no settled variant.** `pool` is
  `Promise.all` over workers, so one rejection abandons the rest, and
  `all_settled` is unbounded. `pool_settled` is a few lines and turns a
  convention into a check. *Fixed: `task.pool_settled(limit, tasks)` is `pool`'s
  worker loop with each call guarded, returning one `Settled<T>` per task in
  order and never rejecting, with the same "a `limit` below 1 is treated as 1"
  clamp. A run test pins the behaviour `pool` cannot deliver: task 2 of 4 throws
  and the other three still produce values. `pool`'s doc comment now points at
  it. `Settled<T>`, `all_settled`, `all`, and `race` are untouched, and the error
  is still `unknown` (read it with `string.from(e)`); a typed task error is a
  different question. `linkcheck` moved to it, and the difference was measured on
  two copies of the app differing only in that call, with a throw injected into
  one of three fetches: `pool_settled` printed all three rows and named the
  failing URL, `pool` printed nothing and died on an unhandled rejection, losing
  both surviving results.*
- **G54. [FIXED] Two formatter defects, both cheap.** The `items.len() <=
  INLINE_MAX` branch short-circuits past both the width check and the newline
  check, so a two-argument call with a nested lambda body is emitted at any
  length (137 columns observed). And D27 asks for canonical ordering of
  annotation *kinds*, not of repeated arguments within one kind, so the
  `raw_args` tiebreaker sorts `@example` arguments and costs the author's
  sequence for nothing. Note the reflow complaint underneath this is
  doc-versus-doc, not doc-versus-code: the formatter's own module comment says it
  keeps a list inline while it fits `PRINT_WIDTH`, and the guide is the document
  that overpromises. *Both fixed. `INLINE_MAX` is deleted and the width test runs
  at every element count, so the formatter's own fixed-point output no longer
  holds 142-column lines. The `raw_args` tiebreaker is gone; `sort_by` is stable,
  so repeated `@example`s keep the order they were written in. A prerequisite bug
  came with it: the inline candidate is rendered into a detached buffer, and the
  buffer used to start at column zero, so any list nested inside a candidate
  measured its own width from the wrong column. The printer now carries the real
  starting column into the capture.*

  Two residuals stay open. The width test measures only up to the list's closing
  delimiter, so a suffix printed after it (`?` on a `try`, ` -> Result<T, E> {`
  on a signature) does not count and a line can still land a few columns over.
  And a list whose inline candidate is intrinsically multi-line now explodes
  one-argument-per-line rather than letting a trailing lambda keep hugging the
  call, which is more vertical than Prettier's rule for the same shape.

  "No line exceeds 100 columns" is still not true, and the number belongs here.
  `examples/apps` is reformatted and `glyph fmt --check` is clean on it, and it
  holds 62 lines past 100 columns. Fifty-seven of them carry a string literal:
  `@example` raw argument text, which is copied verbatim and unformattable by
  design, and long interpolated messages, which the formatter does not reflow.
  The five that do not are three `fn` signatures the ` -> T {` residual above
  explains, one chain break, and one `match` arm. They are listed under G18.
  Formatting the rest of the tree (`examples/corpus` and the numbered examples)
  is a separate reformat that has not landed; those files were not `fmt`-clean
  before this batch either, for unrelated reasons such as redundant parentheses.
- **G55. [FIXED] Three findings that were not gaps, and what they have in common.**
  Multi-line strings work (D12, with a regression test), `math.max` exists, and
  the two-import rule for `std/time` is documented behaviour. In all three cases
  the author reimplemented or routed around something that already shipped. Two
  of the three are discoverability failures, not surface failures, which is a
  docs round of its own: a shipped feature nobody can find is not a feature.
  *Closed as docs, each at the address where the reader was actually looking.
  Multi-line strings were described only in the D12 decision line; AGENTS.md's
  "Template strings" section now says a raw newline inside `"..."` is kept
  verbatim and means the same as `\n`, with a runnable example, and the
  TypeScript-developer table row for backtick templates finishes with "newlines
  included". `math.max` was reachable only through a slash-grouped line
  (`math.min / max / pow / imul (...)`), so grepping the reference page for it
  found nothing; every grouped line on the page is now one call per line
  (`std/math`, `std/encoding`, `std/log`, the `Deque` methods), and `pow`'s
  parameters are named `(base, exponent)` to match the runtime. The two-import
  rule was stated once in the page preamble and nowhere in `## std/time`, and the
  preamble was wrong besides: it claimed a static factory is reached "through its
  named import", when `import std/time` alone gives you `time.Duration.ms(5)` and
  `x: time.Duration`. Both spellings were verified against the compiler; both
  files now show the two import lines and say which name each buys.*

## Round 11 — a URL shortener, and a green build running yesterday's code

The loop pointed at `examples/apps/shortlink.glyph`: shorten a URL, redirect a
visitor, count the hits. Most of what came out of it is stdlib shape (`std/http`
has no headers, so a 302 and an HTML page are both unspellable), and that is
recorded with the trip in [`roadmap/releases.md`](roadmap/releases.md). One
finding belongs here because it is a different class from the rest: it is the
only false *green* in the batch.

- **G56. [FIXED] `glyph run`'s build cache did not hash `<src>/extern/**`.**
  `source_fingerprint` hashed every `.glyph` file and every `<src>/.types/**/*.d.ts`,
  with a comment above the `.types` block spelling out the rule: a file that is
  copied into the out dir and type-checked is a build input, so a change to it
  must bust the cache. `<src>/extern/**/*.ts` satisfies that sentence word for
  word (`runtime.rs` stages it into `<out>/extern`, the generated tsconfig checks
  it) and was not hashed. Staging only runs on the rebuild path and the output
  prune deliberately skips `extern/`, so the stale staged copy survived the cache
  hit: you edited your hand-written shim, ran `glyph run`, and the compiler
  printed a clean type-checked build while executing the previous version of your
  TypeScript. Not a stale error, a stale program, and nothing on screen
  distinguished it from a correct build. Compounding it, the recursive `.glyph`
  walker skips symlinks outright, and this app ships a symlinked shim in
  `extern/`, so the same false green had a second door. *Fixed: the `.d.ts`
  collector is now suffix-parameterized and `source_fingerprint` hashes
  `<src>/extern/**/*.ts` and `.tsx` the same way it hashes `.types`: relative
  path as well as contents, so a rename or a deletion busts the fingerprint too.
  The extern walk follows symlinks (reading the target's contents while hashing
  the link's own path) with a canonical-path set so a symlink cycle terminates.
  Five tests pin it: editing, deleting, adding, the symlink case, and a non-`.ts`
  file under `extern/` that must NOT bust the cache. Whether symlinked `.glyph`
  sources should be walked at all is a separate question and is still open.*

## Round 12 — a group expense splitter, and a `match` with no type

The loop pointed at `examples/apps/settle.glyph`: read a ledger of shared
expenses, split each one, and compute the smallest set of payments that squares
the group up. Sixteen findings came back and twelve of them already carried a
G-number, mostly stdlib shape. The one below is new, and it is the reason the app
was carrying an annotation with a comment above it explaining why the annotation
could not be removed.

- **G57. [FIXED] A `match` expression always typed as `Ty::Unknown`.** Glyph has
  no `if`, so `match` is the branching construct, and the typechecker's
  `Expr::Match` arm walked the arms and then recorded `Ty::Unknown` for the whole
  expression, in every program. Anything taken out of a branch was therefore
  untyped from that point on. Two failures came out of it. A field access on the
  binding was left to `tsc`, so a typo in a field name came back as a `TS2339` on
  generated TypeScript instead of Glyph's `E0210` on the `.glyph` line. And a
  two-binding `for` over one of its fields picked the wrong lowering: this is
  G37's mechanism, `iter_is_array` falling back to `Object.entries` when it
  cannot see an `Array`, so `for i, row in w.rows` bound `i` to the string `"0"`,
  `glyph build` was clean, `tsc --strict` was clean, and the program printed
  `01:a` instead of `1:a`. *Fixed: a `match` now takes its arms' type through an
  equality join. Each arm contributes the type of its value, an arm ending in
  `return`/`break`/`continue` diverges and contributes nothing, and if the
  contributing arms disagree or any one of them is undecidable the result is
  `Unknown` exactly as before. No widening, no union, no bottom type. One
  prerequisite came with it: `bind_arm_payloads` resolved payloads for
  module-local unions only, so `Ok(v)` over the prelude `Result` bound `v` to
  nothing and the arm had nothing to contribute; it now reads prelude payloads
  off the scrutinee's type arguments.*

- **G58. [FIXED] The `parse` on a type's runtime descriptor had no signature.**
  The arm join above did not remove `settle`'s annotation on its own. The app
  gets its ledger from `WireLedger.parse(decoded)`, and the checker knew nothing
  about the `parse` that a `type` declaration's descriptor emits, so the call
  typed `Unknown`, the scrutinee was undecidable, the `Ok(w)` arm bound nothing,
  and the join had nothing to join. This is the boundary between untrusted input
  and typed data, which makes it the worst place in a program to lose a type: it
  undoes every inference downstream of it. *Fixed: `T.parse` types as
  `Result<T, Array<Issue>>`, read off the same shape the emitter writes, for the
  types that actually get a descriptor. Eligibility mirrors `emit_type_decl`:
  a non-generic record, a non-generic tagged union whose name no variant shadows,
  and a refined primitive. A plain alias (`type Cents = int`) emits no descriptor
  and gets no signature. With both fixes in, the annotation and its comment are
  gone from the app, `for i, w in wire.expenses` still binds a number, and a
  ledger whose third entry has three decimal places reports `expense 3` rather
  than `expense 21`.*

### Still open from this trip

- **The unknown-typed iterand** (G37, what remains). An iterand whose type is
  honestly unknown, a call into a stdlib function `stdlib_fn_ty` does not model,
  still takes the record lowering and binds a string index with no diagnostic.
  The two ways out are modeling stdlib return types or hard-erroring on an
  unknown-typed iterand, and picking between them is a decision, not a patch.
- **A generic type's `parse` stays `Unknown`.** A generic record's descriptor
  threads one runtime checker per type parameter, so its `parse` has a different
  arity than the non-generic one and typing it needs the checker to know what the
  caller passed.
- **Arms that disagree stay `Unknown`.** The join is equality, so a `match` whose
  arms produce different types gives the rest of the compiler nothing, same as
  before. Doing better means widening or a union type, which Glyph does not have
  in its checker today.

## Round 13 — the URL shortener again, with the shim taken away

Round 11 wrote the shortener with a hand-written `extern/web.ts` because
`std/http` could not spell a `Location` or a `text/html` page. This trip deleted
the shim and asked for the same app in plain Glyph. The blocking gap is G52,
recorded above and fixed by this release. What is new here came out of running
the fixed version against input a user controls.

- **G59. [FIXED] A header value outside Latin-1 killed the server.** The response
  header fix stripped CR and LF from every value on the way out, which closes
  response splitting. Node rejects more than that: `writeHead` throws
  `ERR_INVALID_CHAR` for any byte outside `/[\t\x20-\x7e\x80-\xff]/`, and it
  throws from a call site outside `respond`'s `try`, so the rejection was
  unhandled and the process exited. Shortening a URL with an emoji in it and then
  following the short link took the whole server down, and the emoji came from a
  form field, so any visitor could do it. *Fixed: `sanitize_header_value` strips
  every character Node rejects rather than only CR and LF, so an unencodable
  character is dropped and the response still goes out. The integration test
  redirects to a path containing an astral character, asserts the stripped
  `location`, and then asserts a second request still gets an answer, which is
  the half that would have caught this.*

### Still open from this trip

- **No percent encoding in the standard library.** The app needs to put an error
  message into a query string, so it hand-writes `url_encode` and
  `percent_decode` over `encoding.hex_encode`/`hex_decode`. Both are 60 lines of
  the kind of code the stdlib exists to own, and encoding a URL component is
  about as common as string formatting. A `std/url` with `encode_component` and
  `decode_component` closes it.
- **`string.split(s, "")` splits UTF-16 units.** Splitting on the empty string is
  the only way Glyph has to get at characters, and it breaks a surrogate pair, so
  a non-BMP character comes apart into two lone halves. Encoding `🎉` per
  character that way yields `%EF%BF%BD%EF%BF%BD`, which builds clean, passes
  `tsc --strict`, and is the wrong answer. The app works around it by converting
  to hex first and walking bytes. This is the same root as the `std/string`
  breadth item: no `slice`, no `index_of`, no codepoint-aware iteration.
- **`std/string` breadth, fifth sighting.** `slice` and `index_of` have now been
  reported by five consecutive trips. *The stdlib round for G26 and G34 shipped
  both, along with `repeat`, `pad_start`, `pad_end`, `replace_all`, `trim_start`,
  `trim_end`, and `array.fold`/`index_of`/`flat_map`, and the apps now call them.
  Codepoint-aware iteration is decided against; see G50.*

## An adversarial review of the E0222/E0223/E0008 batch

The batch that closed G35 and G44 was reviewed from outside, against a binary
rebuilt from the working tree. Three findings were bookkeeping (this file said
all four gaps were open; `docs/error-codes.md` cited D18 for E0222, which is the
postfix-`?` precedence rule; three catalogued codes had no `--explain` body) and
are fixed in the same session. The fourth is a defect in its own right.

- **G60. [FIXED] `glyph fmt` turns a building program into a non-building one.**
  An arm that means "the empty record" is written `=> ({})`, which parses as a
  parenthesized object literal, builds clean, and passes `tsc --strict`. `glyph
  fmt` reprints it as `=> {}`, which is an empty *block*, and the formatted file
  no longer builds: E0223, the arm produces no value. Verified end to end on a
  three-arm program. The formatter has no grouping node, so the parentheses are
  not in the AST it reprints and their meaning is lost. That the round-trip
  changes meaning is G48's problem; that the *formatter* is what changes it is
  this one, and it is the more serious half. D14 promises `fmt` round-trips, and
  a formatter a program cannot survive is worse than a missing feature, because
  it damages code that already worked. The narrow fix is a grouping node in the
  AST so `({})` reprints as itself; that also removes `linkcheck`'s `no_cache()`
  workaround rather than blessing it. Whether Glyph instead wants a different
  spelling for the empty record in arm position is G48 and stays open; this entry
  is closed by the formatter preserving what it was given, whichever way G48
  goes.

  *Fixed in the printer rather than in the AST. Arm-body position is the only
  place in the grammar where a leading `{` is ambiguous (`=>` occurs nowhere
  else), and the parser resolves it by requiring `key :` or `...` right after the
  brace, so the empty object literal is the only shape that loses its meaning.
  The printer wraps exactly that shape: `X => ({})` reprints as `X => ({})`,
  which re-parses to the same object, is a fixed point, and emits identical
  TypeScript. A general `Expr::Paren` node was considered and not taken: about
  fifteen structural `matches!` sites across the emitter, typechecker, and
  formatter test on `Expr` shape (arm-body `match` flattening, `contains_await`,
  `try_span`, `expr_has_captured_jump`, `is_atom`), and a wrapper each one failed
  to see through would be a silent miscompile rather than a compile error. It
  would also either preserve redundant parens, so two spellings of one program
  survive `fmt`, or print the node only when needed, in which case it buys
  nothing over the predicate. That option stays open.*

## An adversarial review of the CLI and docs batch

The batch that shipped `glyph check` (G28), moved the build's green summary below
the stages that can turn it red (G42), let hyphenated arguments through
`glyph run` (G36), and closed G55 as a docs round was reviewed from outside. Two
findings were about the batch's own reach and are fixed in the same session:
`glyph build one.glyph` still ended at "source path is not a directory" without
naming the command that answers it (it now names `glyph check <file>`), and
`build --json` reported `ok: true` and exit 0 on a machine with no `tsc` while
`check --json` reported `ok: false` and exit 2 (`build` now matches `check`, and
both match their own text paths). The `--no-check` / `--no-tsc` split the batch
introduced is settled the same way: `--no-tsc` is the one name for the stage on
`build`, `check`, and `run`, with `--no-check` kept as a hidden alias on `build`
and `run`. The third finding is a divergence the batch surfaced but did not
cause.

- **G61. [FIXED] Two string syntaxes, one of them undocumented.** D12 says "One string
  syntax: `"..."`", and the lexer has two. `lex_string`
  (`glyph-lexer/src/lexer.rs:114-119`) dispatches `"""` from the general string
  path, not from an `@doc`-only path, so a triple-quoted literal is reachable
  from any expression position and arrives at the parser
  (`glyph-parser/src/expr.rs:417`) as an ordinary `Token::String`. The two forms
  do not mean the same thing: the triple path does no escape decoding, so
  `"""a\nb"""` keeps the literal backslash where `"a\nb"` does not, and because
  the parser cannot tell them apart it still runs `split_template_parts` over the
  triple's content, so `"""x ${y}"""` interpolates despite the lexer's own
  comment promising raw content. The formatter already names `"""..."""` in its
  comments. Two spellings of a string literal that differ only in escape
  semantics, both written with double quotes, is a greppability and verifiability
  problem, which is why it is filed rather than left as trivia. Three ways to
  close it and only the last is wrong: the parser rejects `"""` outside a `@doc`
  annotation, restoring D12; or D12 and the reference document the second raw
  form honestly, including what interpolation does inside it; or it stays
  reachable and undocumented. Which of the first two is a spec call, not a patch.
  *Closed by the second: the spec was what was wrong.* Removing a form would
  break working code, and the lexer is the normative implementation, so D12 now
  describes both spellings — `"..."` decodes escapes, `"""..."""` does not, both
  interpolate, and `"""` is legal in any expression or type position rather than
  only after `@doc`. D12 also records the formatter's behaviour, which is real
  and was undocumented: a `"""` literal is copied through verbatim, and a
  single-line one that interpolates is reprinted in the ordinary `"..."`
  spelling because the verbatim path is gated on a raw newline. Content and
  meaning survive that reprint; only the spelling changes. The lexer comments
  that claimed a `@doc`-only path and inert raw content are corrected, the
  migration table in `docs/guide/for-typescript-developers.md` no longer says
  "one string syntax", and AGENTS.md documents the raw form. No compiler
  behaviour changed. Whether the formatter should also make the triple form a
  fixed point (gate the verbatim path on the `"""` prefix as well as on a raw
  newline) is left open beside G62's related call.
- **G62. [FIXED] `glyph fmt` collapses a multi-line string that interpolates.** G55
  closed by documenting the multi-line form in AGENTS.md, and the formatter
  undoes it for the case a real program writes. `string_literal`
  (`glyph-formatter/src/lib.rs:1051`) copies a literal verbatim from source by
  span, and its comment says so: that is what preserves a D12 multi-line string.
  A literal that interpolates is not a `Expr::String`, it is a template, and
  `template` (`:1072`) rebuilds the text through `escape_string` (`:1555`), whose
  `'\n' => out.push_str("\\n")` arm turns every raw newline back into an escape.
  Verified on a two-function file: the function whose string has no `${...}`
  round-trips unchanged, the one beside it comes back as `"hello ${name}\nsecond
  line\n"`. So the documented spelling survives only until someone saves the
  file, and under format-on-save it never survives at all. That is why
  `examples/apps/shortlink.glyph` still writes all five of its HTML builders with
  `\n` escapes: the multi-line rewrite was made and checked (`glyph check` clean,
  all five pages byte-identical), then reverted, because `glyph fmt --check`
  fails on it and shipping a curated example the formatter reformats is worse
  than the escapes. The fix is to give `template` the same verbatim-by-span path
  `string_literal` already has, which needs the template's own span to cover the
  quotes; the alternative, teaching `escape_string` to pass newlines through, is
  wrong because the same function escapes strings that were written on one line.
  *The formatter half is fixed. The same family as G60: formatting must not
  change what a program means or how it prints. The template's span already covered the quotes (the
  lexer spans from the opening `"` to just past the closing one and the parser
  hands that span straight to `Expr::TemplateString`), so no parser change was
  needed — `template` now takes its span and copies the literal verbatim, sharing
  one `verbatim(span)` helper with `string_literal`. `escape_string` is untouched,
  since it still serves single-line rebuilds and the no-source `format_expr` path.
  The verbatim path is gated on the source slice containing a raw newline, which
  is exactly the corrupting case, so a single-line template still gets its
  `${...}` interiors normalized (`"${ a+b }"` still becomes `"${a + b}"`).
  Whether to drop that normalization and copy every template verbatim, the way
  `Expr::String` already does, is a separate open call: one rule and a guaranteed
  fixed point against one less formatting service. Note that under either answer
  the escapes inside a multi-line template stop being canonicalized, because
  `TemplatePart::Text` carries the whole-literal span rather than its own.
  `examples/apps/shortlink.glyph` now writes all five HTML builders as real
  multi-line strings: the rewrite that was reverted before is back, the emitted
  `shortlink.ts` is byte-identical to the `\n`-escaped version, and
  `glyph fmt --check` reports the file already formatted.*

## Round 14 — a terminal spreadsheet, and the names a module already owns

The loop pointed at `examples/apps/sheet/`: load a grid from JSON, parse and
evaluate formulas with cell and range references, detect reference cycles, apply
scripted edits, and render the result as a terminal table. It is the largest app
in `examples/` and the first one whose domain vocabulary collides head-on with
the emitted module's own vocabulary. A spreadsheet cell holds a number, a label,
nothing, or an error, which in Glyph wants to be spelled
`Number | Text | Empty | Error`. Two of those four names are already bound in
every module the compiler emits.

Fourteen frictions came back. Eight are recorded below; the rest were already
carrying a G-number or were stdlib shape, and those are with the trip in
[`roadmap/releases.md`](roadmap/releases.md). Two shipped a fix in the same
release and are half closed. The other six are open, which is the first time in
several rounds that the backlog grew.

- **G63. [HALF FIXED] A top-level declaration silently shadowed a global the
  emitted module depends on.** Every top-level Glyph name reaches the emitted
  module verbatim, including a tagged union's variant constructors, and nothing
  checked it against the names that module already uses. `type Value = |
  Num(number) | Error(string)` emits `export function Error(...)` at module top
  level, so every `new Error(...)` the compiler writes below it (the `?`
  lowering, `match` fallthrough, the descriptor `parse` throw paths) called the
  variant instead. The build did not fail there; it failed at an unrelated
  `match` with a `tsc` type error, in the wrong place and with the wrong
  explanation. `Number` was harmless until `int` shipped and the boundary check
  started emitting `Number.isInteger`, which is how the defect reached a release
  without anyone writing a program that hit it. *Half fixed by `E0110`: a
  top-level `fn`, `type`, `const`, `component`, or variant whose name is one of
  the JavaScript globals the emitter references (`Object`, `Array`, `Promise`,
  `Number`, `Error`) or one of the prelude globals in scope without an import
  (`par`, `print`, `assert`, `number`, and the primitive type names) is now a
  Glyph error at the declaration span, which is where the rename goes. The list
  is derived from `glyph-emit` rather than from a general list of JavaScript
  globals, and a test greps the emitter and fails when a new global reference
  appears without a matching entry, so the `Number` mechanism cannot repeat.
  `Date` is deliberately absent: nothing the emitter writes mentions it, so
  rejecting `type Date` would make the diagnostic's claim false and would break
  `examples/corpus/calendar.glyph`.* What remains is the whole reason the entry
  was filed: you still cannot name a type or variant `Error`, `Number`,
  `Object`, `Array`, or `Promise`. `E0110` turns a silent miscompile into a
  clear rename request, which is the verifiability half, and the app still
  carries `Cellerr` instead of the `Error` its domain wanted. Closing it for
  real means mangling Glyph names in the emitter, which changes what every
  emitted identifier looks like and therefore what a stack trace, a `grep` over
  `dist/`, and a hand-written `extern/` shim see. That is an architecture
  decision and it has not been made.

- **G64. [HALF FIXED] `type Key = string | number` built clean and meant
  something else.** D8's `A | B` is a tagged union whose members are variant
  *names*, so bare primitives on the right-hand side declare variant
  constructors called `string` and `number`. The line looks like ordinary
  TypeScript, it passed `glyph build` and `tsc --strict`, and it emitted
  `export const string` and `export const number` that shadowed the prelude
  namespaces of the same name. Nothing failed until a later `number.to_string`
  call resolved to the wrong thing. *Half fixed by `E0111`, which is a separate
  code from G63 because rejecting the line teaches nothing on its own: the
  message says what the line actually parsed as, points at named variants
  (`| Text(string) | Count(number)`), and names `extern_ts("string | number")`
  as the boundary escape hatch.* What remains is that Glyph still has no way to
  spell a union of two primitive types except `extern_ts`, which its own checker
  treats as opaque `unknown`. Adding untagged unions touches exhaustiveness,
  descriptors, and `is`, and D8's tagged unions exist because a sealed variant
  set is what makes a `match` verifiable. It is a type-system decision, not a
  patch.

- **G65. `==` means `deepEqual` in an `@example` and `===` in the program.** The
  worst finding of the trip. `@example make(1) == { x: 1 }` reports a passing
  example, because the example harness compiles `==` through its own
  `deepEqual` (`glyph-cli/src/examples.rs:292`). The same `==` inside a function
  body emits `===`, so the program compares two record literals by reference and
  prints NOT EQUAL. Both halves pass `tsc --strict` and nothing warns. A test
  that passes while the code it tests is wrong is worse than no test: it is the
  exact "silent green" class this file exists to track, and it undermines
  `@example` as a verification surface. The emitter's own comment claims value
  equality, so one of the two spellings is wrong and picking which one is a
  language decision (structural `==` everywhere, or an explicit
  `record.equals`/`array.equals`, or `@example` dropping to reference equality
  and forcing an explicit comparison). Not this release's scope, but it should
  not sit long.

- **G66. An optional record field is declarable but cannot be read.** `field?:
  T` parses, and `RecordTypeField::optional` reaches the emitter, which writes
  `field?: T` into the TypeScript. The two checkers then disagree about what a
  read of it produces. Glyph's member-access path
  (`glyph-typechecker/src/assign.rs:508`) propagates `f.ty` and ignores
  `f.optional`, so the read types as `string`; `tsc --strict` types the same
  read `string | undefined`, and Glyph has no construct that narrows one to the
  other, because there is no `if` and a `match` on a string does not consider
  the `undefined` case. So the field is writable and unreadable. The app worked
  around it by making the field required and documenting that a `clear` command
  ignores it (`examples/apps/sheet/main.glyph:50`). Either optional fields get a
  narrowing story or the parser should stop accepting `?`; accepting a
  declaration whose values cannot be used is the worst of the three.

- **G67. A `for` binding carries no element type, so D30 exhaustiveness
  evaporates inside a loop.** `Stmt::For` in the typechecker's assigner
  (`glyph-typechecker/src/assign.rs:452`) walks the iterand and the body and
  binds nothing for the loop variable. Iterating `Array<CommandSpec>` where
  `CommandSpec` has an `op: "set" | "clear"` field, the read of `cmd.op` inside
  the body types as `string`, the `match` over it is no longer exhaustive over
  two cases, and `E0218` asks for an `else` arm. Adding that `else` forfeits
  exactly the guarantee the string-literal-union type was declared to provide:
  add a third command later and the `match` silently routes it to the catch-all
  instead of failing to compile. This is a quiet downgrade of a checked property
  to an unchecked one, with the compiler suggesting the downgrade in its own
  help text. The app keeps a one-line annotation and a comment saying why it
  cannot be removed (`examples/apps/sheet/main.glyph:1202`). Same family as G37
  and G57: a type that stops existing at a boundary takes every check downstream
  of it with it.

- **G68. `json.parse<T>` reports one issue where `T.parse` reports paths.** The
  emitter rewrites `json.parse<T>(text)` to `json.parse_with(text, T.schema)`
  (`glyph-emit/src/lib.rs:3230`), which is what made the call validating in the
  first place (G3). The cost was never recorded: `T.schema` is a single runtime
  checker, so every field-level failure collapses to one issue reading
  `expected T`. The same fixture through `json.parse<unknown>` followed by
  `T.parse(decoded)` names the field that is wrong and its path. A malformed
  config is the case where a precise error matters most, and the one-step form
  is what `docs/guide/typed-apis.md:51` teaches, calling out that it "parses the
  string *and* validates in one step" without saying what it gives up. The app
  writes the two-step form with a comment explaining the choice
  (`examples/apps/sheet/main.glyph:1434`). Either the rewrite threads the
  descriptor's issues through, or the guide has to say which form to reach for
  when the error message is going to a human.

- **G69. `glyph run` and `glyph check` never run `@example` blocks.** Both
  `run_examples` call sites are inside the `Build` arm, the text path and the
  JSON path (`glyph-cli/src/main.rs:284` and `:344`). So a colocated test that
  fails turns `glyph build` red and leaves `glyph run` and `glyph check` green
  on the same source. `docs/guide/getting-started.md` tells a reader the two
  never disagree. During the trip that meant a fast edit-run loop reported
  success on code whose own examples were failing, and only the slower `build`
  found it.

- **G70. `E0109` and `E0110` can report the same declaration twice.**
  `is_reserved_ts_word` is called from both `collect.rs` and `resolve.rs`, so a
  reserved name is counted once per pass. The new shadow check runs only in
  `collect`, and the rendered diagnostic still appears twice for a single-file
  build because the collect pass itself is reached more than once. Cosmetic: it
  inflates the reported error count without changing the verdict or the spans,
  and a reader who trusts the count sees two problems where there is one.
