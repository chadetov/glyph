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

**A `G` entry's marker is the only live status here.** The round and
reconciliation narratives are past tense on purpose: each says what was true
when it was written, not what is true now. A narrative that reads as a
present-tense claim about an entry is a tense bug, not a status, and the fix is
the tense. `scripts/check_gaps.py` reconciles markers and counts; it cannot read
prose, which is why two of these went stale unnoticed.

## Reading the markers

A `G` entry carries its status in brackets after the number. No bracket means
open.

- **`[FIXED]`** — closed, with a note saying what closed it.
- **`[HALF FIXED]`** / **`[IMPROVED]`** — part of the claim is closed; the note
  says what remains.
- **`[DECIDED]`** / **`[RESOLVED]`** — not a defect. Either a documented v1 stance
  or an accepted won't-fix.

Recording a finding here is half the job. The other half is deciding what to do
about it, which happens in `docs/roadmap/releases.md`, and the two drifted:
three entries had been reproduced repeatedly and appeared in the roadmap
nowhere. `scripts/check_findings_scheduled.py` now fails the build when an entry
that is open or partly fixed is not named in the roadmap. Parking it in the
rolling lane with a sentence about why counts; leaving it only here does not.

Reconciled again after 0.1.78 closed G102: of 121 entries, 91 are fixed, 10 are
partly fixed, 9 are decided or resolved, and 11 are open. That round re-ran an assignment the
previous one had quietly substituted its way out of, and found why: `glyph run`
called `process.exit` the moment `main` returned, so no program that outlived a
single pass could run at all (G84). The four it left open are about the
language rather than the build, and G87 is the sharpest of them: `owned`, the
manifesto's one carve-out from "no linear types", turns out not to reach
sockets, which is the case it was written for.

The two most recent entries (G100, G101) did not come from a dogfooding round at
all. They came from reading an application someone outside the project wrote, and
they are recorded here because the file is the backlog regardless of who found
the gap. See "Round 27" at the end.

The round before it, the chat *engine*, added three entries and closed the one
it was about. G81 had been true since `std/io` was written: `read_line`
returned only when stdin closed, so no program a person could talk to was
writable in Glyph. The two it left open, G82 and G83, are what an interactive
program still cannot do: write a prompt without a newline, and tell a terminal
from a pipe.

The reconciliation before it, after the adversarial review of the csvql round,
which added one entry (G77, the match-arm binding that shadowed the `let` it
assigned to) and closed it along with G74 in the same release, then again after
G75 and after the auth_api boundary round (G79/G80), then the papercut batch,
read 80 entries, 53 fixed, 13 partly fixed, 5 decided or resolved, and 9 open.
G75 was the largest thing still open: an imported record type lowered to
`Ty::Unknown`, so field checking and the `for i, x` index type were both wrong
across an import. It carried an identity across the boundary from that release on.

The reconciliation before it, after the csvql trip, which added two entries and
closed one of them in the same release, read 76 entries, 50 fixed, 11 partly
fixed, 5 decided or resolved, and 10 open. G76 is the imported string-literal
union that lost D30's exhaustiveness guarantee, fixed by giving `DeclTyResolver`
the query that reads a sibling module's literal set. G75 is the other one it
added, and it is fixed too, by the general cross-module type query described
above.

The reconciliation before it, after the statechart trip, which added two entries
and closed one of them in the same release, read 74 entries, 49 fixed, 11 partly
fixed, 5 decided or resolved, and 9 open. G73 is the namespace-qualified `match`
that got no exhaustiveness check, fixed by lowering `option.Option<T>` to the
same prelude type the named import produces and by resolving a qualified arm
through its head symbol. G74 is open and is cosmetic: E0200 quotes the missing
variant names for some unions and not others.

The reconciliation before it, after the dependency-resolver trip, which added two
entries and closed one of them in the same release, read 72 entries, 48 fixed,
11 partly fixed, 5 decided or resolved, and 8 open. G71 is the
`record.get` into `match` into `for` miscompile, fixed by modeling `std/record`
and by joining `match` arms one level into their type arguments. G72 is open and
is a `glyph check` scoping behaviour that predates the trip.

The reconciliation before it, after the spreadsheet trip, which added eight
entries (G63–G70) and closed half of two of them with `E0110` and `E0111`, read
70 entries, 47 fixed, 11 partly fixed, 5 decided or resolved, and 7 open. Six of
the seven open ones were new, so the backlog grew for the first time in several
rounds. That is what a trip into a domain the earlier apps never
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
was the one entry left open at that point, and it was the phase-2 half of its own
claim: hard-erroring on the `Unknown`s that remain at the stdlib boundary.

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
and after, so nothing about what those apps print changed. G30 still had its
index-safety half open beside G39 at that point; it closed in 0.1.70.

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
- **G24. [FIXED] `?` is rejected in an expression-form `match` arm.**
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
  module path. The member half is fixed in 0.1.72: a `ns.name` read where `ns` is
  a namespace import is recorded during resolution and held to the same export
  list a named import is, so `string.repeeat(...)` is `E0105` naming the member
  and the module rather than a TS2339. It is keyed on the object resolving to a
  module, which keeps a local binding sharing a namespace's name out of it.
  Turning it on found two things nothing else had: the seed list is now the
  authority for both spellings, so a name it omits turns a working call into an
  error, and a test fixture had been calling `fs.write` where the function is
  `write_text`, passing because it ran under `--no-tsc`. A gate keeps the seed
  and the runtime's exports in step, negative-tested after its first version
  silently checked nothing.*
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
- **G30. [FIXED] Two decisions the trip surfaced.** `for` has nothing in the
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
  index-safety half is closed too, and not the way the options listed. Turning
  on `noUncheckedIndexedAccess` was measured at 428 errors across the examples
  and our own stdlib, and buys a diagnostic that arrives as a mapped `tsc` error
  rather than a Glyph one; making `xs[i]` return `Option<T>` changes how every
  one of 437 index expressions is written. What shipped instead: `xs[i]` keeps
  its `T` and the emitted read is bounds-checked, so out of range it throws a
  `RangeError` naming the index and the length. Glyph had been *worse* than the
  language it borrows that contract from: Rust's `xs[i]` lies in the type and
  panics at the bad index, while Glyph's lay in the type and returned
  `undefined`, which then travelled until something dereferenced it somewhere
  else. `array.get(xs, i) -> Option<T>` is the safe read, modeled so the `Some`
  binding carries the element type. All 323 examples still pass with the check
  on, which says the same thing twice: nothing was relying on an out-of-range
  read, and the check costs nothing in practice. What it does not do is make the
  type honest, so the fuller fixes stay on the table if the runtime failure ever
  proves too late.*

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

  *Two more holes in the same lattice, found by a dependency resolver.* The
  chain `record.get` into a `match` into a two-binding `for` printed `01:x` and
  `11:y` where it should print `1:x` and `2:y`, on a build with no diagnostics
  that `tsc --strict` passed. There were two independent causes and each one
  reproduced the bug on its own. First, `std/record` was not in `stdlib_fn_ty`
  at all (the string `"std/record"` did not appear anywhere in
  `glyph-typechecker/src`), so `record.get(t, k)` typed `Unknown`, the `Some(p)`
  arm bound nothing, and the arm join never got as far as comparing anything.
  Second, `join_match_arms` compared arm types by equality, and an empty array
  literal is `Array<Unknown>` (`infer_array_elem_ty`), so `None => []` was read
  as disagreeing with `Some(p) => p`'s `Array<string>` and sank the whole match
  to `Unknown`.

  Both are fixed. `stdlib_record_fn_ty` models all six of `get`, `has`, `keys`,
  `values`, `set` and `remove`, with the value type riding a `Ty::Param("V")` on
  the record parameter the same way `std/array` carries `T`; the key is always
  `string`, so it is not a parameter. That also types the ordered-walk idiom:
  `array.sort(record.keys(t), cmp)` no longer binds `sort`'s `T` from `Unknown`.
  And the arm join now joins argument-wise underneath an already-agreeing head,
  with `Unknown` absorbing the other side, so `Array<Unknown>` and
  `Array<string>` agree on the container head. The head is the only part
  `iter_is_array` reads. Heads that differ still join to `Unknown`, and an arm
  whose value type is entirely undecidable still sinks the match, because
  projecting one arm's type onto an arm nothing is known about would be a guess.
  The load-bearing `let path: Array<string>` and its three-line comment came out
  of the resolver's `why_lines` and the output is byte-identical.

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
- **G39. [HALF FIXED] Member access and call arguments against `Ty::Unknown` are unchecked.**
  `s.slice(0, 1)` on a `string`, a misspelled `xs.pusj(x)`, and a wrong-arity
  call into a stdlib namespace all compile, because the receiver's type is
  `Unknown` and the checker has nothing to check against. The manifesto promises
  no `any`; this is one, spelled `Unknown`, load-bearing at exactly the boundary
  where the promise is made. Closing it is an architecture decision rather than a
  patch (model the stdlib from its own `.d.ts` sources per Q21/Q40, or keep
  growing the hand-written `stdlib_fn_ty` table), so it is recorded in
  `docs/roadmap/releases.md` and not fixed here.

  *Phase 1 landed; the entry stays open for phase 2.*
  **Scoped for 0.1.71 by reproducing it (2026-08-09), and the danger is much
  narrower than this entry reads.** A misspelled `array.lenn`, `s.slyce` or
  `string.repeeat` does not build: `tsc` catches all three with TS2551. That is
  a diagnostic-quality problem, not a correctness one, and it belongs with G27.
  The correctness half is one shape: `match string.index_of(s, "x") { Some(i) =>
  i, }` with no `None` arm reports `0 error(s)` and throws `non-exhaustive
  match` at run time, because the scrutinee is `Unknown` so D9 never runs. The
  surface is exactly the nine unmodeled functions named below, which makes the
  work bounded and much smaller than modeling the stdlib from its `.d.ts`:
  teach the signature model optional parameters, model the six that take an
  optional trailing argument, then walk one level into a callback's return for
  `map`/`flat_map`/`zip`.*

  *Phase 2 shipped in 0.1.71, and it is the six, not the nine.* `FnParam` gained
  an `optional` flag and the arity check reads a minimum and a maximum instead of
  one number, so `string.index_of`, `string.slice`, `string.pad_start`,
  `string.pad_end`, `array.slice` and `json.stringify` are modeled. The case this
  entry was really about is closed: a `match` on `string.index_of` with no `None`
  arm is `E0200` at compile time rather than a throw at run time on a clean
  build. `map`/`flat_map`/`zip` were attempted and reverted, and the reason is
  worth keeping: a callback parameter modeled as `fn(T) -> U` rejects
  `array.map(items, async fn(n: number) -> number { ... })`, which is legitimate
  and appears in the examples, because assignability compares `is_async` (D40).
  Modeling them needs a callback type that admits both, which is a decision about
  colorless async through a callback rather than more table entries.* The chosen direction is
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
- **G48. [FIXED] `{}` as a match arm is silent green.** `true => {}` parses
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
- **G52. [FIXED] `std/http` cannot bound or observe a request.** `Response` is
  `{ status, body }` with no headers and no final URL, so a redirect is
  invisible; `RequestInit` is `{ method }` with no timeout and no redirect
  policy; there is no `head`. The app's `task.race` timeout workaround leaves the
  loser in flight, which is the exact thing `task.ts`'s own doc comment says the
  scope exists to prevent. A network client that cannot bound a request is not
  shippable. One coherent `std/http` round. *The observe half is fixed:
  `Response` now carries a required `headers` field, the client fills it from the
  fetch response with the names lowercased, and the server writes it to the wire
  (`html`, `redirect`, and `with_header` build it; `form` reads a form-encoded
  request body). The bound half is fixed in 0.1.69.
  `Response` carries `url`, the address the response actually came from, so a
  followed redirect is visible as landing somewhere other than where you asked.
  `http.send(f: Fetch)` takes the whole request as one record carrying
  `timeout_ms` and a `redirect` policy (`"follow" | "manual" | "error"`, a D30
  union), because an optional trailing argument is the one shape the checker
  cannot model. The timeout aborts through an `AbortController` rather than
  racing a timer, so the loser does not stay in flight, which is what the
  `task.race` workaround got wrong. `http.head` is there too. Verified against a
  local server: a 300ms budget against a 3s endpoint returns the abort message
  and the whole program finishes in 1.7s, a `manual` redirect reports 302 with
  its `location`, and a followed one reports the final URL.*

  *Revisited the apps afterwards, which the loop asks for and this round nearly
  skipped.* `linkcheck` carried the `task.race` workaround this entry named; it
  now sends one bounded request, and the timeout aborts instead of leaving the
  loser in flight. That surfaced a second thing: the app could not tell a slow
  site from a dead one, because both arrived as `status: 0`. `HttpError` grew a
  `kind` (`"timeout" | "network" | "status"`) on the `FsError.kind` model, and
  the checker models the field, so `match e.kind` is held to D30 exhaustiveness
  rather than reporting E0218 and advising a catch-all. The app also reports
  where a redirect landed now, instead of the literal `"?"` it had to print
  while `Response` carried no URL.
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

- **G63. [FIXED] A top-level declaration silently shadowed a global the
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
  carries `Cellerr` instead of the `Error` its domain wanted. **Closed in 0.1.70, by the approach settled in 0.1.69 and not by mangling.**
  Renaming the user's declaration would trade one greppability loss for
  another. The narrow fix keeps the name the author wrote and aliases the
  *compiler's own* references instead: a module that declares a colliding
  name emits `const __glyph_Error = globalThis.Error;` at the top, and the
  emitter's internal uses go through it. Both halves were checked against
  `tsc --strict`: the value capture works, and `globalThis.Array<T>` is
  legal in *type* position too, which was the half in doubt. The cost is
  53 emission sites across five globals that have to route through a
  helper reading module state, in the crate where a mistake is a silent
  miscompile, so it earned its own release, which is this one. Every
  emitter-internal reference now goes through one accessor, and the drift test
  follows that accessor rather than the literal `X.member` text routing removed.
  One thing the investigation did not anticipate: of the five, only four can be
  freed. `Array` is also a Glyph *prelude type*, so `type Array` does not shadow
  a global the compiler happens to use, it redefines how the rest of the module
  spells an array, and no capture can help because the name that changed meaning
  is the one the author writes. It stays reserved, and E0110 now names the
  prelude origin, which is the accurate reason. `Record` is reserved for the
  same reason. The app that reported this was updated in the same
  pass: `sheet`'s value union reads `Number | Text | Empty | Error` instead of
  `Num | Text | Empty | Cellerr`, and the comment explaining that the rename was
  forced is gone because it no longer is. That module alone needed 162 captured
  references, which is the 139 `new Error(...)` and 23 `Number.isInteger` the
  entry counted, and its output is unchanged. Closing it the old way meant
  mangling Glyph names in the emitter, which changes what every
  emitted identifier looks like and therefore what a stack trace, a `grep` over
  `dist/`, and a hand-written `extern/` shim see. That is an architecture
  decision and it has not been made.

- **G64. [DECIDED] `type Key = string | number` built clean and meant
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

- **G65. [FIXED] `==` means `deepEqual` in an `@example` and `===` in the program.** The
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

- **G66. [RESOLVED] An optional record field is declarable but cannot be read.** `field?:
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

- **G67. [HALF FIXED] A `for` binding carries no element type, so D30 exhaustiveness
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

- **G68. [FIXED] `json.parse<T>` reports one issue where `T.parse` reports paths.** The
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

  *Reproduced against 0.1.68.* A `Cfg` of `host: string, port: number` fed
  `{"host": 5, "port": "nope"}` reports 1 issue through `json.parse<Cfg>` and 2
  through `json.parse<unknown>` then `Cfg.parse`. **Fixed in 0.1.69** by giving
  the schema factory the descriptor's own `parse` alongside its guard, so the
  one-step form reports what the two-step form does; an array threads the element
  index, so a bad second row reports `1.host`. The `sheet` app's two-step
  workaround, and the comment explaining it, are gone.

- **G69. [FIXED] `glyph run` and `glyph check` never run `@example` blocks.** Both
  `run_examples` call sites are inside the `Build` arm, the text path and the
  JSON path (`glyph-cli/src/main.rs:284` and `:344`). So a colocated test that
  fails turns `glyph build` red and leaves `glyph run` and `glyph check` green
  on the same source. `docs/guide/getting-started.md` tells a reader the two
  never disagree. During the trip that meant a fast edit-run loop reported
  success on code whose own examples were failing, and only the slower `build`
  found it.

- **G70. [FIXED] `E0109` and `E0110` can report the same declaration twice.**
  `is_reserved_ts_word` is called from both `collect.rs` and `resolve.rs`, so a
  reserved name looked like it could be counted once per pass. It cannot: the
  two call sites check disjoint name sets (collect checks top-level declaration
  and variant names, resolve checks local bindings, and `walk_decl` never
  re-binds a declaration's own name), and when collection reports anything the
  resolve pass is skipped and its output dropped, which 0.1.54 added to stop
  exactly this doubling. Both codes now report once per declaration; a counting
  assertion in `glyph-cli/tests/integration.rs` pins it, and
  `tests/negative/reserved_word_decl_name.glyph` gives E0109 its first fixture.

  One residual, deliberately left open: because a collect error skips resolve
  entirely, a reserved-word *parameter* (`fn f(class: number)`) stays invisible
  until an unrelated duplicate-name error in the same file is fixed. That is
  cascade suppression rather than duplication, and narrowing it (running resolve
  against the partial table) trades one kind of noise for another. Undecided.

## Round 15 — a dependency resolver, and a loop index that was a string

The loop pointed at `examples/apps/depsolve/`: read a project manifest and a
registry of published versions from JSON fixtures, parse every version and
constraint string into a typed value, then expand requirements, pick the highest
published version satisfying every constraint gathered on a package so far, and
backtrack when a later requirement invalidates an earlier choice. An unknown
package, an unsatisfiable constraint set, and a cycle are all values of one error
union carrying the requirement path that reached them. Two modules, 1,122 lines,
and a domain where a `Record` keyed by package name is the central data
structure.

That last part is what made the trip worth taking. Every earlier app used
`std/record` incidentally. This one leans on it, and the three most ordinary
things in the language sit next to each other in `why_lines`: read a `Record`,
`match` the `Option` it hands back, walk the array inside with an index. The
chain miscompiled, silently, on a build with no diagnostics that `tsc --strict`
passed. The app had already worked around it before the round started: the
resolver carried a `let path: Array<string>` with a three-line comment saying the
annotation was load-bearing, which is the shape of a workaround for a defect
nobody had filed.

Two findings came back. The miscompile is fixed in this release and is G71. The
second is a `glyph check` scoping behaviour, unrelated to the fix and confirmed
pre-existing on a stashed build; it is G72 and it is open. The app also hit G20
twice, once in `format_version` and once at line 712, both of them the same
hoist: a string literal cannot appear inside a `${...}` interpolation, so the
separator gets its own `let`. That is a known and already-numbered limit and it
did not need a new entry.

- **G71. [FIXED] `record.get` into a `match` into a two-binding `for` bound the
  index as a string.** The program printed `01:x` and `11:y` where it should
  print `1:x` and `2:y`, because the `for` took the `Object.entries` lowering and
  bound `i` as a record key. Nothing reported anything: the emitted TypeScript is
  well typed either way, which is what makes the D21 lowering choice a place
  where the checker's inference is load-bearing semantics with no `tsc` backstop
  behind it. There were two independent causes and each one reproduced the bug on
  its own. First, `std/record` was not modeled anywhere in the typechecker (the
  string `"std/record"` did not appear in `glyph-typechecker/src` at all), so
  `record.get(t, k)` typed `Unknown` and the `Some(p)` arm bound nothing. Second,
  `join_match_arms` compared arm types by equality and an empty array literal is
  `Array<Unknown>`, so `None => []` read as disagreeing with an `Array<string>`
  arm and sank the whole `match`. *Both closed in 0.1.55.
  `stdlib_record_fn_ty` models all six of `get`, `has`, `keys`, `values`, `set`
  and `remove`, with the value type riding a `Ty::Param("V")` on the record
  parameter the way `std/array` carries `T`; the key is always `string`, so it is
  not a parameter. The arm join now goes argument-wise underneath an
  already-agreeing head, with `Unknown` taking the other side. The knock-on is
  the ordered-walk idiom: `array.sort(record.keys(t), cmp)` was binding `sort`'s
  element type from `Unknown` and now keeps `string`. The annotation and its
  comment came out of the resolver and every emitted `.ts` line is otherwise
  identical.* What this does not close is G37's residue, which is the iterand
  whose type is honestly unknown; that one is a decision, not a patch.

- **G72. [FIXED] `glyph check` on one file compiles every `.glyph` under that file's
  directory.** `glyph check examples/apps/bracket.glyph` reports "13 module(s)",
  and `glyph check examples/corpus/calendar.glyph` reports 57: the walk is
  recursive from the file's directory rather than the file plus what it imports.
  The visible cost is a diagnostic about a program you did not ask about. Because
  the module root stays at the top of that walk, `depsolve/main.glyph`'s
  `import wire` has nothing to resolve against, so checking any single file in
  `examples/apps/` reports `TS2307 Cannot find module 'wire'` pointing into a
  different app. Checking `examples/apps/depsolve` directly is clean, and so is
  `examples/apps/sheet/main.glyph`, whose directory holds one module. Confirmed
  identical on a build with this release's changes stashed, so it predates them.
  It also means the cost of checking one file scales with the directory it
  happens to live in.

## Round 16 — a statechart replay engine, and the import spelling that turned the check off

The loop pointed at `examples/apps/workflow/`: read a hierarchical statechart and
an event log from JSON fixtures, resolve each event against the active
configuration, evaluate guards over a typed context, run entry and exit actions in
the right order, and print the replay. Nine modules. A domain built almost
entirely out of tagged unions, since a condition, an action, a slot value, and a
transition verdict are all sum types, and half the app is a `match` over one of
them.

That is why the trip found what it found. The app's own import lists were the
tell. Every module carried an eighteen-name variant import so its matches could be
written on bare constructors, and `interp.glyph` had a comment explaining that a
union's constructors do not arrive with its type. The comment was accurate about
the syntax and wrong about the reason: the author had not chosen the named form
for readability, they had been pushed into it because the namespace form silently
skipped the exhaustiveness check.

The hole was total, not partial. `match c { model.Yes(_) => …, model.No(_) => … }`
over a three-variant union reported no diagnostics, passed `tsc --strict`, and
threw `Error: non-exhaustive match` with a raw JavaScript stack trace at run time.
Writing the identical match with `import model { Yes, No }` was E0200. It reached
the prelude unions too, which is the part that matters: `option.Option<T>` lowered
to `Unknown`, so the most-used union in the language lost D9 to a one-token change
in how it was imported.

One thing turned up while proving the fix on the real app rather than on unit
tests. Making `result.Result<T, E>` decidable also made it *classifiable*, and
`type_expr_is_result` recognized only the prelude and named-import spellings, so
every `?` in a function returning the qualified type became E0201. Nothing shipped
in that state, because the qualified return type had been `Unknown` and therefore
permissive before this release, but it is the same defect as the one being fixed:
a predicate that knows two of the three legal spellings of a type. It is a third
arm in `type_expr_is_result` and a regression test now.

The rewrite is the evidence. `interp.glyph` and `main.glyph` are in the namespace
form with their import lists deleted, and the remaining seven modules stayed on
the named form, so the app carries both spellings and builds clean on both. Every
run mode is byte-identical to the pre-rewrite baseline, and deleting any single
arm from either rewritten module is now an E0200.

The trip also put a number on what G72 costs once it reaches CI. The examples
gate runs `glyph build ../examples` over one root, so a multi-module app nested
under `examples/apps/` has its own siblings out of scope and every cross-module
import comes back `TS2307`. That gate has been red since the app directories
started arriving; this app makes the failure larger without changing its shape.
All four multi-module apps build clean when the root is the app directory. The
fix is a call about the walk or about the gate, not about the apps.

- **G73. [FIXED] A namespace-qualified `match` over an imported union got no
  exhaustiveness check at all.** `import model` plus `model.Yes(_)` arms, or
  `import model as m` plus `m.Yes(_)`, was accepted with any subset of the
  variants covered: no diagnostic, `tsc --strict` green, `Error: non-exhaustive
  match` at run time. The named-import spelling of the same match was E0200. It
  applied to project-sibling unions and to `std/option` and `std/result` alike.
  *Fixed in 0.1.56, in two places that had the same shape. `stdlib_path_ty` now
  falls back to `imported_prelude_container` when the two-segment stdlib type
  table misses, so `option.Option<T>` and `Option<T>` lower to one `Ty` and the
  ordinary exhaustiveness path runs on both. For project modules,
  `imported_union_variants_from_arms` resolves a qualified arm through its head
  symbol (`ImportNamespace` or `ImportAlias`) instead of looking the variant name
  up as a symbol, which under a namespace import it never is. A misspelled
  qualified head used to enter the covered set unexamined and come back from
  `tsc` as a `TS2678` against a literal union type; it is E0220 on the arm now,
  with the nearest-variant hint. Seven integration tests pin the spellings and the
  two ways the check could over-fire, an eighth pins the `?` regression, and there
  was no coverage for any of it before.*

- **G74. [FIXED] E0200 quotes the missing variant names for some unions and not
  others.** A module-local union reports ``missing variants `B` `` and so do the
  prelude unions, but a union imported from another Glyph module reports `missing
  variants Maybe` with the names bare. The two lists are built in different places
  and only one of them quotes. Cosmetic, and it splits by where the union is defined
  rather than by how it was imported, so it is not a residue of G73; confirmed on
  the named-import spelling as well. *Fixed in `check_imported_union_coverage`,
  which now backticks each missing name the way `check_patterns_exhaustive`
  already did, so E0200 has one shape for one rule. The string-literal path keeps
  its double quotes: those are the literal values, not identifiers.*

## Round 17 — csvql, and the module boundary that eats types

The loop pointed at `examples/apps/csvql/`: a relational query engine over CSV.
A catalog of table specs, a CSV reader, a SQL parser, a binder that resolves
column references against the catalog, a planner, an executor with grouping and
aggregation, and a renderer. Eleven modules, and that count is the whole story.
This is the first app in the loop where the interesting types are declared in one
file and consumed in another, and three guarantees turned off the moment it
split.

The one that reached the shipped source was D30. `catalog.glyph` declares
`pub type ColType = "text" | "int" | "real" | "bool"`; `table.glyph` imports it
and matches on all four literals; the compiler answered E0218 with

    Help: Add an `else` arm. A `number`/`string` match with only literal arms
    can never be exhaustive.

That help is false about the code in front of it, and following it converts a
compile error into a runtime fallthrough. The author followed it, and the dead
`else => None` shipped with a comment explaining what it cost. The asymmetry is
what makes it indefensible rather than merely incomplete: D8's half of the same
guarantee already crosses a module boundary through
`DeclTyResolver::imported_union_of_variant`, so the seam was built, proven, and
salsa-memoized. Nobody had written the second query.

- **G75. [FIXED] Imported record fields lowered to `Ty::Unknown`, so field
  checking and `for i, x` were both wrong across an import.** `record_fields_of` and
  `named_record_fields` resolve a `Ty::Named` only against `self.module.items`,
  which is module-local, and an imported type never becomes `Ty::Named` in the
  first place. Three consequences, all silent: a typo'd field on an imported
  record draws no `UnknownField` error at all, member access is permissive
  because the field set is `None`, and `for i, x in s.rows` on an imported record
  field emits `Object.entries(...)` because `iter_is_array` cannot decide, which
  binds `i` to the string `"0"` rather than the number `0`. `tsc` catches most
  numeric consumers of that index (`total + i`, `i > 9`, passing it to a `number`
  parameter) and reports them against a variable Glyph bound for the author; it
  is silent where a string works where a number was meant, which is string
  interpolation, concatenation, and `record.get` keys. csvql worked around it
  with a `let` hoist before each such loop, three of them: `raw_rows` and `cols`
  in `table.build`, and `cols` again in `bind.fields_of`. All three are deleted
  now, along with the four-line comment that explained why they had to be there.
  `table.build` loops straight over `sheet.rows` and `spec.columns`, `fields_of`
  over `spec.columns`, and the app builds with no diagnostics, passes `tsc
  --strict`, and prints byte-identical output for all twelve queries plus
  `--explain` and `--limit`. The old binary cannot compile the hoist-free
  version: it emits `Object.entries` for both loops and `tsc` rejects `i + 1`,
  and with `--no-tsc` the `BadCell` reads `row 11` where it should read `row 2`.
  *Fixed by giving an imported type an identity. `Ty` grew an `Imported { module,
  name }` variant, keyed on the source module's registry path and the name that
  module declares, so the named, namespace-qualified and aliased spellings all
  produce the same type. It deliberately carries no `SymbolRef`: a foreign
  module's ids index an unrelated symbol in the consumer's table. Lowering emits
  it without consulting any query, which is why `type Node = { next: Option<Node>
  }` and a two-module type cycle both terminate with no cycle guard. Resolution
  happens in one place, `record_fields_of`, through a single general
  `DeclTyResolver::imported_type_decl(module_path, type_name)` with a `None`
  default; `SalsaDeclTy` answers it from a new tracked `exported_type(db, file,
  decl_idx)` query, which lowers the declaration on the source side. That is the
  part the consumer cannot do: `Lowerer::lower` resolves paths through
  `self.resolved.resolutions.get(span)`, and an imported declaration's spans
  belong to another file. Keying the query on the source declaration rather than
  on the consumer means one lowering is shared by every consumer and every import
  spelling. A sibling type named inside another sibling type is itself a
  `Ty::Imported`, resolved on demand, so nesting costs nothing extra; a generic
  sibling record substitutes its type arguments the way a local one does; and a
  string-literal union reached through an imported record's field keeps D30's
  exhaustiveness. Ten integration tests pin the three spellings, the E0210 that
  now names `Sheet` rather than `record`, the nested and self-referential and
  generic cases, and a stdlib type as the negative; seven unit tests hold the
  export view, the stdlib fall-through, and the trait default. Emitted TypeScript
  for every app under `examples/apps/` is byte-identical.*

  Giving an imported type a field set exposed a hole on the way in, so the same
  release closed it. `import lib { Secret }` on a non-`pub` type had always been
  E0105; `import lib` plus a `lib.Secret` annotation reported nothing, and once
  the checker could resolve the declaration that silence started handing out a
  private type's fields and its array lowering, with only `tsc`'s TS2694 left to
  object. The resolver's type walk now records every `ns.Name` annotation it
  passes, and `import_diagnostics` runs the same export check over that list, so
  a declaration answers the same way whichever spelling names it. Value access
  through a namespace (`lib.helper()`) is unchanged and still a `tsc` error.

  Three things this deliberately did not do. Cross-module assignability stays
  permissive (`ty_is_decidable` returns false for `Ty::Imported`); applying the
  local rule across a file edge is mechanical, but whether that rule is nominal
  or structural is Q15. A sibling `interface` (D34) still has no cross-module
  member set, and that one really is a language decision: giving its members the
  shape a record's get would redefine D34's structural satisfaction rule. And the
  two per-shape queries, `imported_string_literal_union` and
  `imported_union_of_variant`, were left exactly as they are: folding them onto
  `imported_type_decl` is the natural follow-up, kept out of the release that
  introduced the general query so it carried one risk instead of two.

- **G76. [FIXED] An imported string-literal union lost D30's exhaustiveness
  guarantee, and the compiler's help text told the author to delete it.** A
  `match` covering every literal of an imported union was E0218 "no catch-all for
  the other values", with help advising an `else` arm. The identical `type`
  declaration moved into the consuming module compiled clean with no `else`.
  *Fixed by mirroring the seam G22/G73 already built for tagged unions.
  `DeclTyResolver` grew `imported_string_literal_union(module_path, type_name)`
  with a `None` default, so db-less callers are unchanged, and `SalsaDeclTy`
  implements it by finding the sibling in `project_files_input`, parsing it, and
  reading the `TypeExpr::StringLiteralUnion` body. `Lowerer` grew `with_imports`,
  used at the two construction sites where an annotation's type has to be right
  for an imported name (the Assigner's walk, which lowers param and `let`
  annotations, and the `decl_ty` query, so a `fn f(k: ColType)` signature carries
  the union); the other seven sites keep `new`. It returns the ordinary
  `Ty::StringLiteralUnion`, not a `Ty::Named` pointing into a foreign module, so
  `string_literal_union_values` handles it on its first line and the
  exhaustiveness check needed no change. All three import spellings are covered.
  Five integration tests pin them, plus two unit tests holding the trait default
  at today's behaviour so it cannot become load-bearing. The E0218 help text was
  left alone: it is correct once the scrutinee is typed.*

- **G77. [FIXED] A destructured match-arm binding shadowed the `let` the match
  assigns to, and the value was dropped.** `let text = match t { TPunct({ text })
  => text, ... }` emitted

      let text;
      const __m0 = t;
      switch (__m0.tag) {
        case "TPunct": {
          const text = __m0.text;
          text = text;

  an assignment to a `const`. `tsc` rejects this exact shape (TS2588, once per
  arm, every one pointing at the `let` line rather than the arm), but a collision
  TypeScript happened to accept would drop the value silently. The statement form
  of `let x = match` declares `x` outside the `switch` and has each arm assign it,
  while `emit_arm_binds` emits `const <binder> = ...` inside the case; neither
  consulted the other and the emitter had no uniquing at all. The adversarial
  review of the csvql round named the emitter, but the app had already hit it: at
  `sql.glyph:518` the natural binding for the punctuation text of a token is
  `text`, the same name `TPunct({ text, at })` destructures, and the author
  renamed it to `symbol` with three lines of comment recording why. That rename
  and its comment came out with the fix; `parse_op` reads the way it wanted to,
  the app builds and passes `tsc --strict`, and all twelve queries plus `--explain`
  print byte-identically. *Fixed by routing
  the assignment through a synthesized `__aN` temporary, but only when an arm
  actually binds the name: `match_binds_name` walks the arm patterns (identifier,
  constructor args, object fields, array elements and rest), a top-level `let` in
  a block arm body, and a nested `match` that is the whole arm body, since that
  one lowers into the same case block. Each pattern reports exactly the name it
  binds, so a renamed field `{ text: p }` binds `p` and never `text`; a `for`
  binder is not a source at all, since it lowers to `for (const i of ...)` and is
  scoped to the loop. The `mut <lvalue> = match` twin is guarded the same way
  against every identifier the rendered lvalue mentions, so
  `mut s.count = match r { Ok(s) => s, ... }` no longer assigns through the arm's
  `s`. Rebuilding every app in `examples/apps/` emits byte-identical TypeScript to
  the 0.1.57 binary, all 65 files, which is what keeps the fix off every existing
  program's diff.*

- **G78. [HALF FIXED] A multi-module app cannot be built as part of an enclosing tree.** Glyph
  resolves a local import from the module root (D15: no relative imports, paths
  are slash-separated from the root), so an app at `examples/apps/csvql/` writing
  `import catalog` resolves only when its *own* directory is the build root.
  Building the enclosing `examples/` tree looks for `examples/catalog.glyph`,
  fails to resolve it, and then reports something that points away from the
  cause: the imported type degrades to a primitive and the build fails
  `[E0218] non-exhaustive match on \`string\`` in a match that covers every value
  of an imported string-literal union, or, with a catch-all present,
  `[TS2307] Cannot find module 'catalog'`. Neither error mentions the build
  layout. Every app in `examples/apps/` builds and passes `tsc --strict` on its
  own; only the whole-tree build fails. This turned CI red the moment the first
  multi-module app landed (depsolve, 0.1.55) and it stayed red through 0.1.57,
  because the repo-wide example gate ran exactly the build that cannot work.
  Worked around in `.github/workflows/ci.yml`: each app directory is built as its
  own root, and everything else is built from a copy of `examples/` that excludes
  `apps/` (kept inside the repo so the react example still resolves
  `node_modules` by walking up). The real question is a design decision and is
  not made here: either nested apps move out of the tree that gets built as a
  unit, or Glyph grows a way for a directory to be its own resolution root. A
  third option, writing the imports root-relative as `import apps/csvql/catalog`,
  is worse: it would fix the tree build and break building the app on its own,
  which is how the app is actually used.

  *The compiler half is answered by D41: a directory holding a `package.json`
  with a `"glyph"` key is its own module-resolution root, nearest marker wins.
  It is the marker `glyph init` already writes and `glyph publish` already reads;
  resolution simply did not read it. `glyph build` over an enclosing tree walks
  it and builds each project it finds, writing each project's output under
  `<out>/<dir relative to the target>`, so a marked app compiles the same whether
  you build it or the tree above it. With no marker anywhere the build target is
  the sole root, which is the pre-D41 behaviour, so nothing existing changed. A
  project's imports resolve within its own root only, in both directions: a
  nested project's files are not part of the enclosing project's compilation.
  E0104 keeps its code and gains a clause naming the other project when the
  module it would answer to lives across a boundary. `glyph run` and `glyph check`
  on a file find its project by climbing to the nearest marker, and they find the
  same one whether the path was typed relative or absolute.*

  *This repo's own tree proves it. Each of the six directories under
  `examples/apps/` carries a `package.json` with a `"glyph"` key, and
  `glyph build examples --out /tmp/out` now compiles all seven projects (105
  modules, 222 `@example`s) in one invocation with `tsc --strict` passing, where
  it used to report 88 E0104s. Each app still builds standalone with
  byte-identical output. `.github/workflows/ci.yml` lost its per-app loop and the
  `.ci-examples` copy that excluded `apps/`; both collapsed into a single
  `glyph build ../examples`.*

- **G79. [FIXED] A boundary rejection said which field was wrong but never
  which rule, and a record accepted an array.** Every failing field in a record
  descriptor's `parse` pushed the same string, whatever went wrong:
  ``field `password` is missing or has the wrong type``. Absent, present-but-failing-its-`where`-predicate, and
  present-with-the-wrong-type were byte-identical, so a handler could not tell a
  400 from a 422 without re-deriving the answer the validator already had. The
  refinement descriptor was no better: it said only `expected Password` and never
  named the predicate it had just rendered verbatim one line above, which is the
  half of D39 that promises the constraint is greppable from the rejection. And
  the object test was `typeof value !== "object" || value === null`, which an
  array passes, so a record with no required fields accepted an array outright
  (`Empty.parse([])` returned `Ok`, and the emitter's own comment called that
  payload "a checked cast") while a posted `[1, 2, 3]` came back as one
  misleading issue per declared field, none of them saying "you sent an array".
  The auth_api app paid for it in shipped code: two record types and two `parse`
  calls per password-bearing payload to recover one bit, and before that
  workaround a signup with no password field at all answered `weak_password`.
  *Fixed in the descriptor emitter. The object test now excludes arrays in both
  `is` and `parse`, and `parse` answers an array by name. Each field is tested in
  order — absent first (`code: "missing"`), then wrong (`code: "type"`, naming
  the declared type as the declaration spells it) — and a field whose type has
  its own descriptor delegates to that type's `parse`, splicing the nested issues
  in with the field name prepended to each `path`. That delegation is what
  carries a refinement's message out to the caller, and the refinement descriptor
  now emits `expected Password (string where value.length >= 8)`. `Issue` gained
  an optional `code` (`"missing" | "type" | "refinement" | "unexpected"`) so a
  handler branches on the classification instead of matching message text.
  Optional fields (`f?: T`) are unchanged: absence stays legal exactly where the
  type says it is. Leaf types, unconstrained type parameters, and imported
  `.d.ts` types have no descriptor to delegate to and keep the flat inline check
  — the emitter still does not synthesize descriptors, which is its documented
  soundness limitation. The auth_api app dropped both loose record types, both
  extra `parse` calls, and the hand-written restatement of the length rule; it
  now branches on `code == "refinement"` and reports the boundary's own message,
  and its transcript answers a posted JSON array with `expected SignupBody (an
  object), got an array`.*

  The split this entry exists for is proved in the app, not only in unit
  assertions. Re-running the transcript in 0.1.72: `POST /signup short password`
  answers **422** `weak_password` with `expected Password (string where
  (value.length >= 8) && (value.length <= 128))`, and `POST /signup password 42`
  answers **400** `bad_request` with `expected Password (string)`. Two statuses
  chosen off the classification rather than off message text. The entry had gone
  stale on its own last paragraph: the transcript step was added, and nothing
  came back to reconcile the claim that it was missing.

- **G80. [FIXED] A module-local type named `Issue` shadows the prelude one and
  breaks every descriptor in that module.** A descriptor's `parse` annotates its
  error array as `Issue[]`, which resolves to the module's own `Issue` when one
  is declared. The emitted issues carry `path`, `message`, and `code`, so a user
  type spelled `{ path, message }` failed `tsc` with TS2353 on `code`, at a span
  pointing somewhere else entirely.

  Fixed as what it is: a declaration shadowing a name the emitted module depends
  on, which is what E0110 already covers for JavaScript globals. `Issue` is now
  in `PRELUDE_GLOBALS` and `Record` in `JS_GLOBALS`
  (`crates/glyph-resolver/src/reserved.rs`), so declaring either is an error at
  the declaration naming the type. `Record` sits with the JavaScript globals
  because it is a TypeScript built-in; filing it under the prelude made E0110
  state a falsehood about where the name comes from. The
  drift scan that guards the JS-global list now covers ambient prelude types too
  (it also had to learn to see `Issue[]`, which is how this shipped in the first
  place). `Schema`, `Component` and `Option` stay legal on the `Date` precedent:
  the emitter only writes them because the author wrote them in an annotation,
  so declaring one shadows nothing. The injected-alias route (`__GlyphIssue`)
  was not taken; it needs a runtime public-surface change and an emitter change
  to buy the same thing.

  The proof: `examples/01_validator.glyph` and `examples/corpus/infer_output.glyph`
  each hand-declared `type Issue = { path: Array<string>, message: string }`.
  Both are deleted, both files use the prelude `Issue`, and
  `glyph check examples/corpus/infer_output.glyph` goes from 15 TS2353 errors to
  clean under `tsc --strict`.

## Round 18 — a chat client, and a stdin that only answered after you hung up

The loop pointed at `examples/apps/chat/`: a chat server you can talk to. Six
modules, a wire format, a room engine with nicks, joins, topics, direct messages
and scrollback, a renderer, and a protocol parser. The engine came together the
way the last few apps have. The client did not, and the note the author left in
the source says why: the app shipped as a session replayer over a recorded JSON
file, "because `io.read_line` blocks until stdin EOF".

That is the whole finding, and it had been true since `std/io` was written. Every
previous app that read stdin read a piped file, so nobody had noticed that
`read_line` was not a line reader at all. It called
`readFileSync(0, "utf8")` once, which returns when the writer closes the stream,
split the result on newlines and handed them out from an array. Piping a file
works. Typing into it does not: the program sits silent while you type, and every
answer arrives at once after Ctrl-D. `docs/reference/stdlib.md` said "one line
from stdin (None at EOF)", which reads as a line reader and is what the app was
written against.

The cost is bigger than one function. It made a whole category of program
impossible to write in Glyph, and three apps already in this repository had been
quietly shaped around it: `minesweeper.glyph` could not redraw between moves,
`adventure.glyph` could not answer `look`, and `minilang --repl` was a REPL that
evaluated nothing until you hung up. All three are interactive now, on a real
pty, with no change to any of them. That is the tell that this was one defect and
not three apps' worth of design.

- **G81. [FIXED] `io.read_line` returned only at end of input, so no interactive
  program could be written.** `read_line` went through `ensure_loaded`, which
  called `readFileSync(0, "utf8")`: a single synchronous slurp of fd 0 that
  returns when stdin closes. A prompt/read/respond loop compiled, type-checked
  and ran, and answered nothing until the person typing pressed Ctrl-D, at which
  point every response printed at once. The documented contract ("one line from
  stdin") described the reader nobody had written. *Fixed by reading stdin
  incrementally: module state holds a decoded `pending` string, an `eof` flag,
  one reused 64 KiB `Buffer` and one `StringDecoder`, and a private `fill()` does
  one `readSync(0, ...)` per call. `read_line` returns the text before the first
  `"\n"` and fills only when no newline is buffered, so it returns as soon as a
  line arrives rather than when the stream closes. A single trailing `"\r"` is
  stripped, so CRLF input yields the same lines as LF; input that ends without a
  newline hands back that last partial line once and then `None` forever. A read
  that reports `EAGAIN` on a non-blocking tty backs off about 10ms through
  `Atomics.wait` and retries rather than spinning, and any other read failure
  degrades to empty input so a program started with no stdin at all still
  terminates. `read_to_string` drains the same buffer instead of re-reading fd 0,
  so `read_line` followed by `read_to_string` returns the rest rather than
  losing what the other call had buffered. The bundled Node shim gained the three
  declarations this needs (`fs.readSync`, `Buffer.alloc` plus
  `GlyphBuffer.subarray`, and a `string_decoder` module) so the runtime still
  type-checks with no `@types/node` installed.*

  The regression test is a timing test, not a pipe-a-file test, because a
  pipe-a-file test passes against the broken implementation:
  `read_line_returns_a_line_before_stdin_closes` builds a Glyph echo loop, runs
  it with stdin held open, writes one line and requires the echo back within
  20 seconds, then writes a CRLF line and only then closes the stream. Against
  the old `io.ts` it fails with a timeout. Measured by hand on
  `(printf 'a\n'; sleep 2; printf 'b\n')`, the two lines now print 1ms and
  1405ms in; before, both printed at the end. 200,000 piped lines are counted in
  20-21ms, multi-byte UTF-8 split across a chunk boundary round-trips, and
  idling 15 seconds at a pty prompt costs 0.00s of child CPU.

- **G82. [FIXED] `std/io` cannot write without a newline, so a prompt cannot share a line
  with the answer.** `println` and `eprintln` are the whole write surface, and
  both append `"\n"`. The `> ` prompt every REPL and every interactive CLI opens
  with is therefore unwritable: the cursor is always on the line below the
  prompt, and the transcript of a session reads as alternating full lines rather
  than as a conversation. The chat app works around it by printing a one-line
  banner instead of a prompt, and `minilang --repl` prints nothing at all before
  each read. This is not the shape of the interactive programs G81 just made
  possible. An `io.print`/`io.eprint` pair is the obvious answer and needs a
  decision about flushing, since `process.stdout.write` on a pipe is buffered
  where `console.log` is not.

- **G83. [FIXED] A program cannot tell whether stdin is a terminal or a pipe.** `std/process`
  exposes `args`, `env`, `cwd` and `exit`, and nothing reports `isTTY`. So an app
  that wants to behave one way when a person is typing and another way when a
  file is piped in has to be told which by a flag: the chat app takes `--stdin`,
  which the person running it must remember to pass, and passing it when nothing
  is piped hangs the program rather than falling back. Every CLI that colours
  output, draws a progress line, or prompts needs this predicate, and the
  workaround is a flag whose default is wrong half the time.

This trip closed the thing it found, which is the second time a round has done
that. G82 and G83 are the two it left open, and both are the same shape as G81:
they are the difference between a language that can pipe a file through a program
and a language that can write the program a person talks to. No release carries
the Next marker now; the next trip picks from what is open above.

## Round 19: the chat server, for real this time

The previous round built a chat *engine* and shipped it as a session replayer
over a recorded JSON file. The assignment had been a multi-client server. This
round ran the same assignment again with one rule: a fallback is a finding, to
be reported with the exact error, not a design choice to be made quietly.

There was no design choice available. `glyph run` could not run a server at all,
and the failure was invisible: exit 0, no output, nothing on stderr.

The engine from the previous round was kept whole and three modules were added
around it (`framing`, `audience`, `daemon`), plus `.types/net.d.ts` and a
`--serve PORT` flag. It was then run with three concurrent TCP clients and
checked against what a chat server is actually supposed to do: a room post
reaches that room's members and nobody else, a direct message reaches exactly
two clients, `/who` answers only the client that asked, a `/nick` is visible to
the client that sent it, a message split across three TCP packets arrives as one
line, two messages in one packet arrive as two, and three clients dropping at
once are each announced under the right name.

- **G84. [FIXED] `glyph run` killed any program that was still doing something
  when `main` returned.** The generated entrypoint called `process.exit(code)`
  as soon as `main` came back. Node honours that immediately, while the event
  loop still holds live handles, so a program that created a TCP server bound
  its port and died in the same tick. `glyph run app.glyph --serve 4100` printed
  nothing, not even the line inside the `listen` callback, and exited 0. Every
  long-lived program was affected: a server, a watcher, a bot, a REPL, anything
  driven by events rather than by a single pass. Nothing about it was visible
  from the source, which type-checked and read correctly, and the exit code said
  success. Fixed by assigning `process.exitCode` instead of calling
  `process.exit`, which leaves Node's own rule in place: exit when there is
  nothing left to wait for. A program that only computes still exits
  immediately with the same code. The failure path still terminates, but now
  waits for stderr to drain first, because `console.error` is asynchronous when
  stderr is a pipe and the old code could truncate the diagnostic it had just
  written.

  Why it survived this long is the interesting part. `std/http.serve` returns a
  promise that never resolves while the server listens, and its own comment
  names the reason: "a Glyph `main` that does `await http.serve(...)` never
  returns, so the process stays alive without any keep-alive hack." The one
  server path in the stdlib had a private workaround built into it, so HTTP
  serving worked and nothing else did. A raw TCP server, a WebSocket server, a
  bot holding a gateway connection, a file watcher: none had a workaround, and
  nothing in the docs said one was needed. The stdlib had routed around a
  compiler defect and left no note that it was one.

- **G85. [FIXED] A nested project's `.types/` ambient declarations were dropped
  in a tree build.** Since D41 a directory carrying a `package.json` with a
  `"glyph"` key is its own resolution root, and `glyph build <tree>` builds each
  project it finds. The generated `tsconfig.json` includes `**/*.ts`, which
  reaches down into nested projects' emitted output, while `.types/**/*.d.ts`
  only ever covered the outer project's own directory. So the outer `tsc` run
  type-checked the inner project's files under the outer project's
  configuration, without the declarations they depend on. `examples/apps/chat`
  built clean on its own and failed as part of `examples/` with `Cannot find name
  'net'` and four implicit-`any` errors, which is the worst shape a build error
  can have: correct code, rejected only in the configuration CI uses. Fixed by
  giving each project's config an `exclude` naming the output directories of the
  projects nested inside it, so every project is checked exactly once, by its own
  config. The exclusion is derived from output paths, not source paths: a
  project's output directory comes from its package directory while its sources
  may sit in `src/` below it, and deriving it from sources produced
  `apps/inner/src`, which matches nothing and silently excluded nothing. The
  flat layout every app under `examples/` uses hid that, because there the two
  paths coincide.

- **G86. [FIXED] Nothing sets the exit code after `main` returns, so a program that
  fails later reports success.** This is the hole G84's fix opened, and it is
  worth stating plainly because it is the same silent-success shape. Once `main`
  has returned, its return value is spent. The chat daemon's `listener.on("error")`
  fires on `EADDRINUSE` well after that, and without intervention the loop
  drains and the process exits 0: a server that never bound its port reports
  success. The app now calls `process.exit(1)` there itself, which is correct
  and which every server will have to remember to do. What is missing is a way
  for the language to make it hard to forget. `main -> number` is documented as
  "the exit code" in four guide pages, and that is now only true of programs
  that finish inside `main`.

- **G87. [RESOLVED] `owned` (D25) is unusable for sockets, the case it was specced for.**
  The manifesto grants exactly one carve-out from "no linear types" and names
  its justification: files, sockets, database connections, locks, the forgotten
  `.close()`. This round wrote the canonical version of that workload, a server
  holding N live sockets each of which must be closed exactly once, and `owned`
  appears nowhere in it. `owned` requires a type declared with `resource`, and a
  socket arrives from an ambient `.d.ts` as an opaque foreign type that cannot
  be declared `resource` in the consuming project. So `Conn` holds a raw
  `net.Socket`, `drop` removes it from the registry without closing it, and the
  only thing that reclaims the descriptor is the peer going away first. A
  server-side eviction leaks it, and Glyph's dedicated leak-prevention feature
  has nothing to say. Either D25 cannot reach the case it was written for, in
  which case the spec is wrong, or it can and this needs rewriting; nobody has
  established which, and this entry stays open until somebody does.

- **G88. [FIXED] A record holding an opaque external value gets a `parse` that lies.**
  Descriptors are emitted for every record type. For a field whose type the
  emitter has no descriptor for, the generated check is `field !== undefined`
  and the generated message is ``field `socket` must be Socket``. So `Conn.parse`
  accepts `{ id: 1, nick: "a", socket: "hello", buffered: "" }` and reports
  success. A boolean that is always true would merely be useless; a boolean that
  is always true under a message naming a type it never checked is worse,
  because `parse` is exactly what a boundary is told to trust. The verifiability
  pillar should not ship this. The options are to refuse the descriptor for such
  a record, to keep it with a message that says only what is true, or to have
  `parse` report the unverifiable field. All three are better than the current
  one.

  *Reproduced against 0.1.68.* A record with a field typed
  `extern_ts("{ handle: number }")` accepts a string in that field and returns
  `Ok`. Measuring the whole examples tree found the always-false branch eight
  times, from three causes rather than one: a field typed by a **stdlib named
  type** (`amount: Decimal`, so money at a boundary is a presence check), a
  field typed by an **imported string-literal union** (`kind: ColType`, the D30
  membership check lost across a module, the same hole G76 closed for `match`),
  and a field whose type genuinely has no runtime check (`Socket`, an
  `extern_ts` type, `unknown`). The first two are bugs with one correct answer
  and are planned for 0.1.69. The third was the decision this entry recorded,
  and it is made: a record with an unverifiable field may exist, but calling
  `parse` or `is` on it is a compile error naming the field. Holding a socket in
  a record is ordinary; being told at a boundary that the socket was validated
  is what cannot ship. **Closed in 0.1.69.** Causes 1 and 2 by resolving an
  imported descriptorless alias to its leaf, so a field typed by an imported
  string-literal union keeps its membership check. Cause 3 by `E0304`: the
  emitter's field check now answers one of three things rather than producing a
  string either way (a real predicate, presence-only, or nothing), and `parse`
  and `is` are refused at the call site when a field falls in the last bucket.
  Declaring such a record stays legal, which is what keeps a socket in a record
  ordinary. `unknown` turned out not to belong in the last bucket: it claims
  nothing, so presence is the whole check and there is no lie to refuse. All
  eight always-false branches are gone from the emitted examples tree.

- **G89. [FIXED] A program cannot say that it does not terminate.** `daemon.serve` is a
  `pub fn serve(port: int)` whose doc comment has to explain in prose that the
  process is driven by socket events from there on. `main` has a `return 0` that
  is never reached in the normal case, and `main.glyph` carries a dead match arm
  that exists only to keep a later `match` exhaustive. `std/process.exit` is
  typed `-> never`, so the concept exists in the stdlib and is not spellable in
  user code. A `-> never` return type would delete the dead arm, delete the
  unreachable `return`, and let the compiler state what the comment currently
  asserts.

  *Correction, made when the apps were revisited after `never` shipped.* The
  entry mis-read its own example. `daemon.serve` installs a listener and
  **returns**; the process outlives it because the event loop still has work,
  not because the function diverges, and the file's own comment says so ("`main`
  has already returned by the time this fires"). So `main`'s `return 0` is
  reached, and annotating `serve` as `-> never` would state something false.
  `never` is right and shipped as D43, and `std/process.exit` is a genuine one;
  the chat daemon is simply not a place to spend it. What a non-returning
  function looks like is a `loop` with no break, or a call to another `-> never`.

The round found two compiler defects and fixed both, and left four entries open
that are about the language rather than about the build. The uncomfortable one
is G84: it had been true since the runner was written, it made an entire
category of program impossible, it is one line deep, and the round before this
one hit it and shipped a replayer without reporting it. It could not have
reported it easily, which is the actual lesson. A `glyph run` that exits 0
having produced no output and consumed no measurable time is indistinguishable
from a program that worked, and that is the signal the loop was missing.

## Round 20: a Discord bot

A gateway client: connect over a WebSocket, identify, heartbeat on the interval
the server dictates, track a sequence number so a dropped connection resumes
instead of starting over, notice when the gateway has stopped answering, back
off between reconnects, and answer commands. It is the first app here that
speaks a protocol Glyph did not design, against a server Glyph does not control.

It was verified against a mock gateway speaking the real opcodes rather than by
inspection, and then against an adversarial one written from Discord's
documentation rather than from the client: an unprompted opcode 1, a close with
code 4004, and a gateway that greets you and then silently stops acknowledging
heartbeats. All three are real Discord behaviours and the first two broke the
bot as originally written.

**What the live runs found that reading did not.** Four things, and the order
matters, because each was found only after the previous one was fixed.

1. One `RECONNECT` opcode scheduled two reconnects, since the socket close that
   follows one arrives as its own event.
2. The fix for that latched: the flag was cleared only when a HELLO arrived, so
   a gateway that was simply *down* left it set forever. Against a dead port the
   bot made exactly two attempts and then went silent permanently, which is the
   ordinary outage and the one case a reconnect loop exists for. It is cleared
   when an attempt *begins* now, and the same dead port produces attempts at
   2s, 4s, 8s, 16s, 32s toward the ceiling.
3. Opcode 1 is a heartbeat *request*, not only something the client sends. It
   was parsed as an unknown opcode and ignored, which is how Discord decides a
   bot is unresponsive and hangs up on it.
4. Close codes were thrown away and a constant reported in their place, so a
   rejected token retried forever instead of stopping. 4004 and its five
   siblings are terminal now, and the process exits non-zero.

The first was caught by the cooperative mock. The other three were not, and
could not have been: a mock written by the author of the client only tests the
parts of the protocol the author read. That is the lesson worth keeping from
this round, and it is why the adversarial gateway exists.

  *Reproduced against 0.1.68.* `pub fn spin() -> never` is `[E0103] unresolved
  name never`, so the type the stdlib uses is not spellable in user code.
  **Fixed in 0.1.69** as D43: a real bottom type, assignable to everything with
  nothing but itself assignable to it. A `-> never` function owes no returned
  value and a `never` arm drops out of the match join. See the correction above:
  the chat daemon was the wrong example for it.

- **G94. [FIXED] A match arm whose last statement produced no value fell
  through into the compiler's own "non-exhaustive match" throw.** The worst
  class of bug this project has found: code that compiles clean, passes
  `tsc --strict`, and throws at run time on a `match` that is exhaustive. A
  lambda body is a value block in return position, so an arm ending in a `mut`
  (or a `let`, `for`, `loop`, all of which yield nothing) emitted no `return`,
  because there is no value to return, and no `break` either, because the
  emitter only added one in statement position. The `switch` case then ran
  straight on into the generated
  `default: throw new Error("non-exhaustive match")`. Twelve lines of Glyph
  reproduce it. The same code inside a top-level `fn` was correct, which is why
  it survived: nothing in the test suite put a valueless arm inside a lambda,
  and the bot's socket callbacks are nothing but lambdas containing matches. A
  nested match in the same position had the identical hole one level down.
  Fixed by making the `break` depend only on being inside a `switch` case and
  not on the arm's position, which is the rule the empty-block case next to it
  already used and documented. Two emitter tests now cover it, both verified to
  fail against the old lowering.

- **G90. [FIXED] Ambient *global* declarations in `.types/` are invisible to the
  resolver, so reaching the platform meant writing TypeScript.** `.types/*.d.ts`
  is documented as the way to give the type-checker types for something
  external, and it works for `declare module "x"` blocks. A `declare var
  WebSocket` or `declare function setInterval` in the same file is not read at
  all: naming either is `[E0103] unresolved name`, from Glyph's own resolver,
  before `tsc` is consulted. Installing `@types/node` does not help, because the
  resolver matches module names and a global is not one. The sharp end was D37:
  `new` was added so class-based clients would not need an `extern_ts("new ...")`
  string, and `new WebSocket(url)` is E0103, so for every global class the string
  was still the only route.

  Fixed by removing the reason to reach for a global, rather than by teaching
  the resolver to read one. Two stdlib modules now cover what the apps actually
  wanted: **`std/timers`** (`after`, `every`, `cancel`, `unref`, `sleep`) and
  **`std/websocket`** (`connect`, `on_open`, `on_message`, `on_close`,
  `on_error`, `send`, `close`, `is_open`). Both are typed Glyph, both work on
  any host that can schedule or connect, and neither has an event-name string to
  misspell: each event is its own function taking exactly what it carries, so
  `on_close` is handed the code and the reason rather than an event object to
  narrow. The bundled Node shim also grew `net`, `timers`, `events`,
  `child_process`, `dns/promises` and `zlib`, so a program that imports a
  builtin directly type-checks with nothing installed.

  The measure was that both apps had to lose their TypeScript entirely, and both
  did. The chat server dropped `.types/net.d.ts`; the bot dropped
  `.types/timers.d.ts` and all six `extern_ts` escapes, and its socket layer is
  now ordinary Glyph. Every app was re-run afterwards, not just rebuilt: three
  concurrent TCP clients against the chat server, and the bot against a
  cooperative gateway and all three adversarial ones. `scripts/check_apps_are_glyph.py`
  fails CI if any app under `examples/apps/` ever again contains a `.d.ts`, a
  `.ts`, or an `extern_ts`, so the answer to the next missing capability is to
  extend the stdlib rather than to write the TypeScript Glyph exists to replace.

  What is *not* fixed is the underlying resolver behaviour: an ambient global in
  `.types/` is still invisible, and a program that needs a host global the
  stdlib does not wrap still has no way to name it. That is now a smaller hole
  with a clear remedy, and it is recorded as G95 rather than being closed here.

- **G95. [DECIDED - the resolver stays module-only] A host global the stdlib does not wrap is still unnameable.** The
  general form of G90, left open deliberately. `declare var`/`declare function`
  in `.types/` is not read by the resolver, so a global that Glyph ships no
  wrapper for can only be reached through `extern_ts`, which is `unknown` at the
  seam. Two ways to close it, and they are different decisions: teach the
  resolver to read ambient global declarations, which makes `.types/` mean what
  its documentation implies and would also make D37's `new` work on a global
  class; or keep the resolver module-only and treat every unwrapped global as a
  stdlib gap to be filled on demand, which is what was done for timers and
  WebSocket. The first is more general and reopens the question of what a
  program is allowed to reach without the compiler knowing; the second keeps the
  guarantee that anything reachable is typed Glyph, at the cost of the stdlib
  being the bottleneck for every new host capability.

  *Reproduced against 0.1.68.* `declare var GLOBAL_TOKEN: string;` in
  `src/.types/globals.d.ts` and a use of `GLOBAL_TOKEN` is `[E0103] unresolved
  name GLOBAL_TOKEN`. **Decided: the narrow answer.** The resolver stays
  module-only, and a host global Glyph ships no wrapper for is a stdlib gap to
  be filled on demand, as was done for timers and WebSocket. Everything
  reachable stays typed Glyph that the stdlib vetted, and the cost is accepted:
  D37's `new` on a global class stays unavailable. What remains is to stop the
  `.types/` documentation implying ambient declarations resolve, and to say that
  in the diagnostic rather than leaving a bare E0103. Planned for 0.1.69.

- **G91. [HALF FIXED] An `Option<T>` field cannot be read from ordinary JSON.** G5 recorded
  this and deferred the lenient forms deliberately, on the grounds that the
  tagged encoding is "the canonical wire format". That holds while Glyph owns
  both ends. It does not survive contact with somebody else's API, which is
  what this round supplies. Measured against
  `type Frame = { op: int, s: Option<int> }`: the field absent is ``field `s` is
  required``, `"s": null` is ``field `s` must be Option<int>``, and a bare
  `"s": 1` is the same error. Only `{"tag":"Some","value":1}` parses, and no
  third-party service sends that. Discord puts `"s": null` in every HELLO, so
  the natural spelling of a gateway frame is unparseable and `.parse` is
  unusable at exactly the boundary boundary-validation is for.

  The app works around it with `@open` records that declare only the fields a
  given opcode carries, which is a good idiom and should be documented as one
  whatever else is decided. There are two ways forward and they are not the
  same decision: loosen `Option<T>.parse` to accept `null` and bare values,
  which keeps one type and inherits G5's real objection that it is ambiguous
  when `T` is itself nullable; or treat "a JSON field that may be null" as a
  distinct boundary type that decodes *into* `Option`, which has no ambiguity
  and costs a new concept. Reopened as its own entry because the case that
  motivates it is not the case G5 was written about.

- **G92. [FIXED] Locally bound closures cannot call each other.** `let a = fn() { b() }`
  followed by `let b = ...` is `[E0103] unresolved name b`: a `let` is in scope
  from its own line down and there is no local `fn` that hoists. Event-driven
  code is full of mutual reference and this one is not exotic: `connect`
  schedules a reconnect and the reconnect calls `connect`. The way out is to
  lift them to top-level functions and thread the shared state through as a
  record. That is arguably the better structure and it is what this app does,
  but the language forces it without saying so and the diagnostic points at a
  name rather than at the rule.

  *Reproduced against 0.1.68.* `let a = fn() { b() }` before `let b` is
  `[E0103] unresolved name b`. **Fixed in 0.1.69**: each block registers the
  `let` names it will declare before walking its statements, and a reference
  reaches one only from deeper inside a nested function, where by the time it
  runs the binding exists. A direct forward reference out of an initializer stays
  the error it was. The hole TypeScript also has remains: a forward-referencing
  closure *called* before the target's `let` runs throws at run time, and
  `tsc --strict` does not catch that either.

- **G93. [FIXED] An `@example` has to fit on one line.** Wrapping one is
  `[E0003] unexpected token: EqEq` pointing at the continuation. Examples of
  anything with a real payload are long, so this app names a helper function per
  example several times purely to get under a line length.

Two smaller notes. `@redact` (D24) had never been used by any example in the
repository; this is the first app to hold a credential, and marking
`Session.token` with it is now checked by the replay, which asserts the token
does not survive being printed. And the replay mode asserts at all only because
the first version of it could not fail: it printed frames and returned 0
whatever it read, which is the same vacuous-success shape as the silent
`glyph run` that Round 19 ended on.

Three of the five entries are about reaching outside Glyph: a global, a foreign
JSON shape, an escape hatch that costs its type. That is the same seam the
interop work was about, and it is still where the sharp edges are. G94 is not
about the seam at all, and is the most serious thing found in twenty rounds.

## Round 21: closing the backlog

Not an app trip. The owner's instruction was to close the open gaps and re-check
the applications, so this round works the list rather than looking for new
entries. Six closed below, plus two defects the work itself turned up, one of
which was `glyph fmt` moving a comment into somewhere it did not belong.

Everything here was re-verified against the applications afterwards: the chat
server still holds three concurrent TCP clients with correct room scoping, and
the bot still passes its offline replay and all three adversarial gateways.

- **G96. [FIXED] `glyph fmt` relocated a comment written between two
  annotations into the parameter list.** A comment above a declaration is
  flushed at the declaration's start, which is the *first annotation's* offset,
  so a comment sitting between two annotations was never flushed there. It
  stayed pending and surfaced in the next construct that flushes comments, which
  is the parameter list: a note between two `@example` lines came out inside
  `fn f(...)`, expanding the parameters to one per line to make room for it.
  Found while testing G93 and reproduced with no wrapping involved, so it
  predates that work. A formatter that runs on save must never move a comment
  into unrelated syntax. The annotation block now flushes its own comments,
  before each annotation and again before the declaration keyword.

- **G97. [FIXED] `let _ = expr` could not appear twice in one scope.** `_` is
  the spelling the unused-binding lint tells you to use, so a function ignoring
  two results writes it twice, and two `const _` in one scope is a raw `tsc`
  redeclaration error naming a variable the author never meant to declare. A
  bare `_` now emits its initializer as a statement: the effect happens, nothing
  is bound, and nothing can collide. A named `_foo` still binds. Found while
  writing a two-timer test for G86.

The six from the list:

- **G69** closed by running the `@example` gate in `check` and `run`, each in
  its own project root (D41), with `--no-test` to opt out. `build` had always
  run it, so a failing colocated test turned one command red and left the other
  two green on the same source, and the fast edit-run loop was the one that
  missed it. Both now exit non-zero.
- **G72** closed by building only the file's own project when the target is a
  file. A directory target still builds every project under it, because there
  the nested projects are the point. Checking a loose file under `examples/`
  went from 119 modules to its project's 72; an app file was already down to 2
  from D41, which had closed the half of this entry about `TS2307` pointing into
  a different app.
- **G82** and **G83** closed by `io.print`, `io.eprint`, `io.is_terminal` and
  `io.stdin_is_terminal`. A prompt can share a line with its answer, and a
  program can tell a person from a pipe instead of being told by a flag whose
  default is wrong half the time. Verified in both directions: under a pty the
  predicates report true, under a pipe false.
- **G86** closed by `process.set_exit_code`, which records the code the process
  will leave with and lets it shut down on its own terms. `exit` stops
  immediately and can truncate output still queued on a pipe, so a late failure
  had to choose between reporting itself and finishing cleanly. An uncaught
  error thrown after `main` returns was checked and already exits non-zero, so
  what was missing was only the deliberate case.
- **G93** closed by continuing an annotation onto a line that begins with an
  operator. Nothing about it is ambiguous: a line starting with `==`, `&&`, `.`
  and the rest cannot begin a declaration or another annotation. `-` and `!` are
  deliberately excluded because both can begin an expression. The captured slice
  splices out exactly the line breaks it crossed, never the text between them,
  so a string literal in the argument is untouched.

## Round 22: a durable job queue

An HTTP service with a SQLite store and workers: submit a job, have it run,
retry it with backoff, give up on it, and survive the process dying. Chosen
because nothing in the repository had ever run `http.serve`, and because a queue
is a state machine whose correctness is checkable rather than a matter of taste.

Five modules. The transitions are pure and carry 25 `@example` rows; routing is
a `match` with seven more; persistence and the socket are the only impure parts.
It was run rather than inspected: three jobs submitted over HTTP, workers on a
timer picking them up, `noop` reaching `done`, `poison` reaching `dead` on its
first attempt, `fail` reaching `dead` after exactly five attempts, then the
server killed and a **separate process** reading the same database and seeing
the same state.

- **G65. [FIXED] `==` meant a deep comparison in an `@example` and reference
  equality in the program.** Found in the first module written, inside ten
  minutes: `last_error(j) == Some("bad payload")` inside a `fn` was false while
  the identical expression as an `@example` passed. A test reporting success on
  code that does not work is the worst artifact the example gate can produce,
  and reaching it needed nothing more than writing the same expression twice.

  `==` is now value equality on every type (**D42**). Records, tagged unions and
  arrays compare by structure; primitives are unchanged, and so is the emitted
  TypeScript for them, since `===` is still what a comparison of two known
  primitives lowers to. Reference identity is deliberately not expressible: a
  language whose values are records and unions has no use for it, and offering
  both would mean every `==` needed a reader to work out which was meant.

  Worth noting how narrow the blast radius was. All 959 existing tests passed
  unchanged and no conformance snapshot moved, which says no test in the
  repository had ever compared two aggregates with `==`: the operator was broken
  in exactly the place nobody had looked.

**What this round could not fix, and what it cost.**

- **G39 is the most serious thing open, and this round makes it concrete.** A
  `sqlite.Row` is `Record<string, unknown>`, so every column read is a member
  access against `unknown`. `row.naem` for `row.name` compiles clean, passes
  `tsc --strict`, and evaluates to `undefined`, which `string.from` then renders
  as the text `"undefined"` and the app stores. There is no diagnostic at any
  stage. Every database read in every application is that surface, and it is the
  precise failure Glyph's front page says it prevents. The store works around it
  by naming each column once in a `const` and routing every value through a
  checked conversion, which is a discipline the compiler should be enforcing
  rather than a convention an author has to remember.

- **G20 cost three separate edits in one sitting.** A nested string literal
  inside `${...}` is a parse error, so a message interpolating `string.join`
  has to become a helper function with two `let`s. It is marked improved and the
  diagnostic is genuinely good (it names the limitation and the fix), but three
  hoists in one app is friction rather than a papercut.

- **G98. [IMPROVED] An `is` pattern narrows the scrutinee's *binding*, and using it any
  other way fails as a `tsc` error rather than a Glyph one.** Writing
  `match row[col] { is string => Some(row[col]), else => None }` reports
  `Type 'Option<unknown>' is not assignable to type 'Option<string>'` pointing
  at the whole match, which does not say that the narrowing applies to a binding
  and that the fix is to bind it with `let` first. The rule is right; the
  diagnostic makes an agent reconstruct the compiler's model before it can act.

- **G99. `array.map` with an `async` callback compiles clean and prints
  `[object Promise]`.** `array.map(xs, some_async_fn)` type-checks, passes `tsc
  --strict`, and produces an `Array<Promise<T>>` that `string.from` will happily
  render, because `map`'s result was `Unknown` and `string.from` takes an
  `unknown`. The five predicate-taking array functions do not have this problem:
  their callbacks return a concrete `boolean` or `number`, so `tsc` rejects a
  promise there, and as of 0.1.72 their callback parameters are modeled so the
  rejection is `E0211` at the argument rather than a TS2322 about
  `Promise<boolean>`. It is exactly `map`, `flat_map`, and `zip` that are
  exposed, because their callback's return is a free type variable that a
  promise satisfies. Modeling the callback as a synchronous `fn(T) -> U` closes
  it and was written and tested, but it also rejects
  `par.all(array.map(items, async fn(n: number) -> number { ... }))`, which is
  Glyph's own concurrency idiom and has an integration test. The idiom depends
  on an `Array<Promise<T>>` that Glyph has no way to spell: `await e` types as
  `e`, and there is deliberately no user-visible `Promise<T>` (D40). So closing
  this needs a decision about the type of a pending value, not a table entry.

  *Reproduced against 0.1.80, verbatim: `array.map([1, 2,], slow)` over an
  `async fn` builds clean, passes `tsc --strict`, and prints
  `[object Promise],[object Promise]`. The premise is unchanged, including the
  reason it is not a table entry: closing it rejects
  `par.all(array.map(items, async fn ...))`, which is Glyph's own concurrency
  idiom, so it still needs a decision about the type of a pending value rather
  than a signature.* Re-run when the staleness gate flagged the
  entry: `array.map([1, 2, 3], double)` over an `async fn` still builds with no
  diagnostics and a clean `tsc --strict`, and still prints
  `[object Promise],[object Promise],[object Promise]`. The premise holds
  unchanged, and the three modeled-callback fixes since have not touched it,
  because they covered the predicate-taking functions whose callbacks return a
  concrete type.

## Round 23: reading a key that may not be there

Working G39 directly rather than through an app. Most of what the entry named
had already been closed by phase 1 and by `tsc`: a misspelled method on a
`string` or an `Array` is TS2339, a wrong argument type into the stdlib is
TS2345, and a wrong arity is E0213. Four of the cases the entry lists as open
are caught today, so the entry was stale.

What is genuinely silent is **reading a key out of a map**. A
`Record<K, V>` has arbitrary keys, so `m.name` and `m["name"]` cannot be checked
and were typed `V` regardless: absent, the value is `undefined` under a type
saying it is a `V`, and nothing downstream reports it. This is the failure that
bit the job-queue round, where a mistyped column name compiled clean, passed
`tsc --strict`, and rendered as the text `"undefined"`.

**E0224** now rejects the read and points at `record.get`, which is the same
lookup with the absent case in the type where a `match` can reach it.

Two things the first cut got wrong, both caught by measuring rather than
reasoning:

- **Writes were flagged.** `mut m[k] = v` lit up twenty sites across the
  examples, every one of them building a map, which is safe and is how a map is
  built. An lvalue index is not a read. Excluding it dropped the count to zero.
- **Excluding it dropped the type-map entry** for that node, breaking the
  invariant that every expression carries a type. The workspace test for it
  failed immediately, which is the gate doing its job.

Array indexing is deliberately untouched. `noUncheckedIndexedAccess` was
measured first and produces 589 errors across the examples, almost all of them
`argv[i]` in argument parsers that have just measured `array.len`; and `T |
undefined` is not expressible in Glyph, so a program could not have fixed them
even in principle. A bound is a value a program can check. A map key is not.

Verified across 124 modules: zero E0224, so the check fires on the mistake and
nowhere on correct code.

**What is still open, and why the entry stays HALF FIXED.** The check sees a
direct `Record<K, V>` annotation and a module-local alias
(`type Headers = Record<string, string>`). It does *not* see a type that arrives
from another module or from the stdlib: `sqlite.Row` is `Record<string,
unknown>` and is still read unchecked, which is exactly the shape that motivated
the work. Closing that means modelling stdlib named types as more than a field
set, which is the architecture decision phase 1 of this entry already named. The
job queue's `store.glyph` still reads `row[column]` and is still relying on its
own discipline rather than the compiler's.

## Round 24: three entries that had outrun their evidence

Working the backlog rather than an app. All three turned out to be closable by
checking what the compiler does today rather than by changing it, which is worth
recording as its own result: a gap list is a snapshot, and three of these had
been overtaken by fixes made for other reasons.

- **G48 closes.** Both halves are done. The silent-green half went with E0223,
  which reports a value-position arm that produces no value. The spelling half
  is closed too, and nobody noticed: `({})` compiles, and `glyph fmt` **keeps
  the parentheses**. The entry's complaint that "the obvious workaround does not
  survive the toolchain" was true when written and was fixed by one of the
  formatter batches. A formatter test now pins it, because a formatter that
  un-spells a workaround puts the file back into the error it was formatted out
  of, and nothing was stopping that from regressing.

- **G64 is decided, not fixed.** What remained was that Glyph cannot spell a
  union of two primitive types. It should not: D8's tagged unions are sealed so
  that a `match` over one is verifiable, and untagged unions would put a hole in
  exactly that. There are two Glyph-native answers and both are checked all the
  way through. When you own the type, name the cases
  (`| Text(string) | Count(number)`). When the value arrives from somewhere
  Glyph does not own, take it as `unknown` and narrow with `is`, which was
  verified to compile and run. `extern_ts` remains for a type that must cross
  into TypeScript by name, and is opaque, so it is the last resort rather than
  the answer. E0111 now says all of this.

- **G66 is resolved by an idiom the entry did not know about.** The claim was
  that an optional field is "writable and unreadable". Reading one is fine; what
  is not fine is reading it *into a non-optional `T`*, and `tsc` draws that line
  exactly right, rejecting the unsafe use and accepting the safe one. The safe
  one is what `workflow` does: optional fields live on the **wire** type, where
  they mirror the JSON and are consumed by the record's own `parse`, and the
  domain type they decode into has `children: Array<string>` and
  `initial: Option<string>`. `check.glyph` reads `s.children` straight into
  `text_list(raw: unknown)`, which validates it.

  A Glyph-level error for reading an optional field was written and then
  reverted: it fired on eight sites across the examples and every one was the
  safe idiom. `tsc` was already right, and duplicating a correct check in the
  frontend only to make it wrong is not an improvement. What is left is message
  quality on the rejecting path, which is real but small, and is the same
  complaint as G27 and G98 rather than a fact about optional fields.

The pattern across all three: two were stale, and the third wanted a language
feature that would cost the guarantee it was asking to be exempted from.

## Round 25: null is absence, and a loop keeps its type

G91, G67 and G98 were scheduled together. Two are fixed and the third was
assessed and deliberately left alone.

- **G91's practical half is closed, and it turned out not to need the design
  decision the entry was holding.** The entry framed this as `Option<T>` versus
  ordinary JSON, with a fork between loosening `Option.parse` and inventing a
  boundary nullable. Neither was needed. Measuring first showed that the wire
  spelling `field?: T` already accepts an **absent** key and a **present**
  value, and rejected only an explicit `null` — which is what every real API
  sends, and what a Discord gateway frame carries in every HELLO.

  An optional field now treats `null` as absence. That is the only coherent
  reading: the field's declared type is `T`, and `null` is not a value of `T`,
  so a key holding null is a key holding no value. `glyph gen openapi` had
  already documented exactly this mapping (a `nullable` schema field becomes an
  optional one, "a literal JSON `null` is treated as absent") while the runtime
  descriptor did not implement it, so a generated type rejected the payload it
  was generated from.

  `Option<T>`'s own encoding is untouched: the tagged form stays canonical, and
  G5's deliberate stance on it stands. What changed is the *wire* spelling, which
  is what a boundary type should have been using. The entry stays half fixed
  because an `Option<T>` field still does not decode from a bare value, which is
  the part G5 decided and this round did not revisit.

- **G67 is half fixed.** A `for` binding now carries the iterand's element type,
  so D30 exhaustiveness survives a loop: a `match` over a string-literal union
  inside `for t in ts` went from E0218 ("a string match can never be exhaustive,
  add an `else`") to E0200 ("missing variants `pro`"). The first is advice to
  switch the check off; the second is advice to satisfy it.

  Single-binding only. `for i, x in xs` gives both names the statement's span as
  their def-site key, because the AST carries no per-binding spans, so typing one
  would type the other. That is the AST change G37 is already about, and the two
  entries should be closed together.

- **G98 was assessed and left open on purpose.** The confusing message is real
  and still reproduces. But the detectable condition — an `is` arm whose
  scrutinee is an expression rather than a binding — has a legitimate
  counterexample: `match f() { is string => "yes", else => "no" }` tests the type
  without using the value, and is correct. A diagnostic that fires on correct
  code is worse than the tsc message it replaces. Doing this properly means
  attaching a note during the tsc remap, when the error is known to involve a
  match with an `is` arm, which is real plumbing rather than a check. Recorded
  rather than forced.

## Round 26: verifying the list instead of trusting it

Three entries had already turned out stale this week, so the rest were checked
against the compiler rather than read. Ten were exercised; two more were wrong.

**Wrong, and now closed:**

- **G24 is fixed.** `?` inside an expression-form `match` arm builds clean. The
  entry has been half-fixed since the batch that added E0008/E0222/E0223, and
  the remaining half closed at some point without anyone re-running it.

- **G87's premise is false, which resolves it.** The entry, and the adversarial
  review that sharpened it, both held that `owned` cannot reach a socket because
  it requires a `resource` type and a socket arrives from an ambient `.d.ts` as
  an opaque foreign type that "cannot be declared `resource`". It can:

  ```
  resource type Conn = Socket

  fn shut(owned c: Conn) -> void { return void }
  ```

  builds, and the discipline is enforced end to end over that imported handle. A
  handle that is never consumed is **E0206** ("not consumed on every path"), one
  consumed twice is **E0207** ("used after it was consumed"), and the correct
  program compiles clean. D25 works for the case the manifesto wrote it for.

  So the fork the entry left open ("either D25 is unusable, or the app took the
  easy road") resolves to the second. By the entry's own standard that makes
  `examples/apps/discord` wrong to ship as it is: an example that ignores the
  language's one carve-out from "no linear types" teaches every reader that the
  carve-out is optional. Rewriting its socket layer onto `owned` is the work
  this closes into, and it is app work rather than compiler work.

**Confirmed still real**, each reproduced: G20 (a nested string literal in
`${...}` is E0002), G27 (an unknown stdlib member still leaks TS2551), G68
(`json.parse<User>` reports one issue where `User.parse` reports two, with
paths), G88 (a record holding an opaque field still emits ``field `socket` must
be Socket`` under a check that is only `!== undefined`), G89 (`never` is an
unresolved name), G92 (a `let`-bound closure cannot call one declared below it),
G95 (`structuredClone` is an unresolved name), and G98.

Five of roughly ten entries examined this week were stale or mis-scoped. That is
the finding worth keeping: a gap list is a record of what was true when it was
written, and an entry that has not been re-run is a claim, not a fact. Anything
scheduled off this list should be reproduced first.

  *Reproduced against 0.1.68.* `match record.get(row, col) { is string =>
  Some(record.get(row, col)), ... }` is `[TS2322] Type 'Option<Option<unknown>>'
  is not assignable to type 'Option<string>'`, a `tsc` error rather than a Glyph
  one. **Improved in 0.1.69, not closed.** A note attached during the `tsc` remap
  states the rule, so an agent no longer has to reconstruct the compiler's model,
  which is what the entry complained about. The title's claim stands: it is still
  a `tsc` error. A Glyph *check* was assessed in 0.1.68 and declined, because the
  detectable shape has a legitimate counterexample and would fire on correct code.
## Round 27: an app from outside the project

Not a dogfooding round. `github.com/canpolatoral/glyph-hello` is a tic-tac-toe
game written by someone with no connection to the project: the whole engine in
Glyph (rules, minimax, position evaluation, board rendering), the DOM in 301
lines of hand-written vanilla JS, an HTTP server and a terminal client in Glyph
on top of the shared engine. 1,485 lines, one commit, and **no TypeScript
anywhere** in it. `src/.types/` holds the scaffold README and nothing else.

It builds green on 0.1.72 (14 `@example`s pass, four modules, no diagnostics,
`tsc --strict` passes, `glyph fmt --check` reports four already formatted), and
green was not taken as the finding. Bundling the engine and searching it
exhaustively: playing the `Perfect` level as O against every legal X line is 593
terminal positions and **zero losses**, as X is 94 positions and zero losses, and
across all 5,478 reachable positions none of the three difficulty levels ever
returned an illegal move. The engine is correct, first commit, by an outside
author.

What it found is two stdlib gaps and one documentation gap, and one measurement
worth keeping.

- **G100. `std/array` has no `max`, `min`, `max_by`, `min_by`, or `sum`, so
  argmax is hand-written at every search.** `std/math` has the scalar `min`/`max`
  only. This app writes the same fold five times (`src/xox.glyph` lines 219, 230,
  254, 289 and 319): once to take the maximum of the child scores, once for the
  minimum, once to find the best-scoring move, once to find the best score before
  filtering ties. Picking the highest-scoring element of an array is the core
  operation of every search, every ranking and every scheduler, and the fold that
  does it is four lines of `match acc` ceremony each time. `max_by`/`min_by`
  taking a key function is the shape that closes it; `max`/`min`/`sum` over
  `Array<number>` are the trivial cases of the same thing.
  *Reproduced against 0.1.78: all four are `[E0105] not exported by std/array`,
  and `max` is answered with `(did you mean `map`?)`.*

- **G101. `array.fold` cannot stop early, so every short-circuiting accumulation
  is hand-written index recursion.** The app's requirements ask for alpha-beta
  pruning, which is exactly a fold that stops when the window closes. Alpha-beta
  *is* expressible today: a pair of mutually recursive functions threading an
  index (`ab_max(kids, i, alpha, beta)` calling `ab_max(kids, i + 1, a, beta)`
  only when the cutoff does not fire) compiles, passes `tsc --strict`, and
  returns the textbook answer on the standard test tree. But it takes four
  functions and an explicit cursor to say what `fold_while` says in one call, and
  the version that reads naturally (`array.fold` over the moves) silently
  evaluates every branch, which for a search is the difference between pruning
  and not pruning. A `fold_while`/`try_fold` whose callback returns a continue-or-
  stop is the missing piece. Not a soundness bug: the hand-written form is
  correct, just long.
  *Reproduced against 0.1.78: `array.fold_while` is `[E0105] not exported by
  std/array`.*

**The documentation gap, which is not a `G` entry because nothing is broken.**
The author's `specs/requirements.md` records, as a decision taken before the
build, that "the emitted code uses bare `std/*` specifiers that a build step must
rewrite." That is wrong, and the way it is wrong is our fault. `glyph build`
emits `dist/tsconfig.json` carrying `"paths": { "std/*": [...] }`, and any
bundler that reads a neighbouring tsconfig resolves the specifiers with no
rewriting at all: `esbuild dist/xox.ts --bundle --format=esm` produces 20.8 kb of
ESM with zero `node:` imports and zero `process` references, and it runs in a
bare realm. So an engine in a Web Worker works today. But `docs/guide/deployment.md`
says only that "a front-end build (via React interop) bundles like any other
TypeScript", which does not cover a plain module bundled for a worker, and an
outside developer read the emitted imports, drew the pessimistic conclusion, and
wrote it into his spec as settled. The fix is a worked browser/worker example in
the deployment guide that names the tsconfig and shows the esbuild line.

**The measurement.** 31 of this app's 70 `match` expressions are
`match <bool> { true => ..., false => ... }`, which is 44%. That is the first
number we have for what D9 costs at the keyboard from someone who did not choose
the restriction, and `terminal_score` (`src/xox.glyph:198`) is the shape it
produces at its worst, a nested boolean `match` standing in for a two-branch
conditional. Recorded as evidence, not as a proposal: the pillar case for one
branching construct is unchanged.

## Round 28: five apps at once, and two of them stopped on the same missing type

Five rounds run concurrently, each on an app shape no existing app covers, each
told to stop at the first thing Glyph could not express. Every finding below was
re-reproduced by the orchestrator against 0.1.72 before being written down; one
finding the rounds reported was checked and rejected (see the end).

Three of the five never reached a running program, which is the loop working.
The two that reached furthest, `sitegen` (marked + gray-matter) and `logmerge`
(a k-way streaming merge), both got real work done before stopping.

**The convergence is the result.** `pngmeta` (a PNG chunk reader) and `totp` (an
RFC 6238 authenticator) were assigned different apps, ran in different processes,
and stopped on the same sentence: Glyph has no bytes.

- **G102. [FIXED] There is no byte type, so no binary file, no real HMAC, and no
  bytes/text bridge.** Every external boundary in the standard library is
  string-in, string-out, so there is no spelling anywhere for "these octets".
  `std/fs` is `read_text`/`write_text`/`append_text`, all `"utf8"`;
  `fs.read_bytes` is `[E0105] not exported by std/fs`. `std/encoding`'s six
  functions are all `string -> string`, so `hex_decode` returns UTF-8 text and
  neither direction crosses to bytes. `std/crypto` is
  `hmac_sha256(key: string, input: string) -> string`, and there is no SHA-1 at
  all, which is the algorithm RFC 6238 defaults to and every authenticator ships.
  `std/websocket` documents its own version of this ("bytes is not served by this
  module yet"). A PNG's first byte is `0x89`, not valid UTF-8 alone, so the file
  cannot be read at all; a TOTP key is base32-decoded arbitrary octets and its
  message is an eight-byte counter that is mostly NUL, so neither argument can be
  formed even if SHA-1 existed. The compiler's own suite confirms the only route
  is the escape hatch: `buffer_byte_boundary_typechecks_with_the_shim`
  (`glyph-cli/tests/integration.rs:2668`) crosses it with three
  `extern_ts("Array.from(Buffer.from(...))")` calls, which is what an app under
  `examples/apps/` may not contain. **What is not the problem:** the arithmetic.
  D36's operators all work, and `((d[0] << 24) | (d[1] << 16) | (d[2] << 8) |
  d[3]) >>> 0` returning `4294967295` was verified, as were RFC 4648 base32
  decode and RFC 4226 dynamic truncation against published vectors, both written
  in ordinary Glyph over `Array<int>`. Only the octets are missing. `std/bytes`
  is already scheduled beside `std/net`/`std/dns`/`std/tls`/`std/url`; these two
  rounds say it does not belong in that bundle, because it is the one item that
  blocks whole classes of program rather than one host boundary. **It now carries
  the roadmap's Next marker**, and a third reason arrived after those two: 0.1.79
  cannot ship "WebSocket binary messages" without it, which `websocket.ts` states
  in its own header. The shape needed is wider than a `Bytes` alias: a file read
  as octets, a bytes/text bridge in `std/encoding` (whose six functions are all
  `string -> string`), and a `std/crypto` taking and returning bytes, including
  the SHA-1 RFC 6238 defaults to. What is *not* needed is arithmetic — D36's
  operators, base32 decode and RFC 4226 dynamic truncation were all written in
  ordinary Glyph over `Array<int>` and verified against published vectors. Adjacent and
  already-documented: hex literals still do not parse (`0xff` is `[E0002]`),
  which is a known deferral but unusually painful here, since a 256-entry CRC32
  table written in decimal is unreadable.
  *Reproduced against 0.1.72.*

  **Fixed in 0.1.78.** `std/bytes` is a new module: `Bytes` is an immutable
  sequence of octets (a `Uint8Array` at run time, so it hands to a host API with
  no unwrapping), with the sequence operations named after their peers in
  `std/array` and `std/string`, and hex, base64, base64url and base32 codecs.
  `fs.read_bytes`/`write_bytes`/`append_bytes` read and write a file undecoded,
  and `std/crypto` gained a `_bytes` form of every digest and HMAC plus SHA-1,
  `random_bytes`, and `timing_safe_equal`. Both stopped apps were then written
  as a compiler test: the PNG signature survives a write-read round trip, and
  the RFC 6238 vector (ASCII secret `12345678901234567890` at T=59) returns
  `94287082`.

  Four decisions worth keeping, because each was the more expensive option:

  - **Every decode returns a `Result` that names the position.** node's `Buffer`
    is silent on malformed input: `Buffer.from("zz", "hex")` is an empty buffer
    and no error, base64 decoding skips any character outside the alphabet (so a
    base64url string decodes to quietly wrong bytes), and `toString("utf8")`
    substitutes U+FFFD and reports success. Every codec here is written out
    rather than delegated, refuses all three, and reports `index`. `to_text`
    scans on the error path to find the first byte that cannot start or continue
    a valid sequence, so the answer is "not valid UTF-8 at 2", not "not valid
    UTF-8".
  - **`from_array` rejects anything outside 0..255** rather than masking. A
    silent `& 0xff` turns 256 into 0 and a typo into data.
  - **The module reaches for no host API.** `Uint8Array`, `TextEncoder` and
    `TextDecoder` are the whole of it, and base64/base32 are hand-written rather
    than delegated to `Buffer`, so a bundle touching only `std/bytes` still runs
    in a Web Worker. That is the same property round 27's author wanted and
    assumed he did not have.
  - **base32 was added after the fact**, because writing the `std/crypto`
    documentation produced a TOTP example whose first line was
    `bytes.from_base32(secret)` and the function did not exist. `otpauth://`
    URIs carry the shared key in base32, so an authenticator starts by decoding
    one. Round 28 had proved it writable in ordinary Glyph and it was left out
    on that basis; the doc example is what showed the surface was incomplete.

  **What the test vector could not catch on its own, recorded because it nearly
  shipped.** The RFC 6238 secret is ASCII, so it survives a trip through a
  string unchanged: deliberately breaking `hmac_sha1_bytes` to decode its key to
  text left `totp=94287082` passing. The test now also asserts an HMAC over a
  key containing `0xff`, where the string route gives `4ab779f0…` instead of
  `c543ef42…`. A published vector is only evidence for what its inputs exercise.

  Two things named above are still open and are not part of this fix: hex
  literals (`0xff` remains `[E0002]`), and `std/websocket` binary frames, which
  this unblocks but does not deliver.

- **G103. [FIXED] `glyph gen dts` reports success for a file that cannot compile,
  because its only uniqueness check runs before the step that creates
  duplicates.** First seen as a namespace collision (`namespace Tokens {
  interface List }` emitted alongside a top-level `TokensList`), and that framing
  was wrong: namespaces are incidental. No namespace is needed at all.

      export interface tokens_list { a: string; }
      export interface TokensList { b: number; }

  Two distinct, legal TypeScript types. `gen` prints `2 type(s) written`, exits
  0, emits `type TokensList` twice, and the next `glyph build` is `[E0100] name
  TokensList declared more than once`. The chain: `ts-to-schema.mjs:338` collects
  each type under its dotted TypeScript identity (`Tokens.List`, `tokens_list`,
  `TokensList`) and the dedup guard keys on *that*, where there is correctly no
  collision. The dotted names travel through the JSON schema unchanged. Then
  `sanitize_type` (`gen.rs:1479`) drops every non-alphanumeric character and
  upper-cases the next letter, mapping all three onto `TokensList`. It is
  many-to-one and it is the last thing to touch the name, so **every uniqueness
  check in the pipeline happens upstream of the only step that can create a
  duplicate.** The guard is not weak, it is in the wrong place, and it is in a
  different language and process from the transform it would have to see.

  The fix is a check on the *emitted* names rather than a better namespace
  branch. What to do on a genuine collision is a design call and not the
  round's to make: disambiguate with the dotted scope (`TokensList` and
  `TokensListNs`), keep first-wins with a note the way the cross-file case
  already does, or refuse to write the file. What is not defensible is the
  current behaviour, since a generator reporting green for output it never
  compiled is the class this language exists to remove, and the exit code
  matters as much as the collision.

  **Fixed in 0.1.74.** The check moved to the emitted names, where duplicates
  can first exist, and a collision is now an error that names every colliding
  source and writes nothing. `--rename Source=GlyphName` resolves it, and the
  choice is recorded in the generated header so `glyph regen` replays it rather
  than failing on the same collision the original run was told how to resolve.
  Erroring rather than auto-renaming is the pillar call: an invented
  `TokensList2` appears in no source (greppability) and could renumber when the
  package gains a type (diff stability), and both of those outrank the
  abstraction cost of asking a developer for one name once.

- **G104. [FIXED] `glyph gen dts` silently ignores a relative import that carries
  a file extension.** Three sibling files differing only in the specifier:
  `export * from "./a.ts"` and `export * from "./a.js"` each materialize **zero**
  types and fail with the OpenAPI generator's message ("expected
  `components.schemas` (OpenAPI 3), `definitions` (Swagger 2)...") against a
  TypeScript input, while `export * from "./a"` materializes cleanly. It is not
  barrel-specific: a type *reference* through an extension-carrying import
  degrades to a note, writes a file, and exits 0, leaving output that will not
  compile. The blast radius is the point: `.js` in a relative specifier is
  **mandatory** under `moduleResolution: nodenext`, so every ESM-authored typed
  package is in this class, and `date-fns` uses `.ts` under
  `allowImportingTsExtensions`. Both fail, so `glyph gen dts date-fns` produces
  nothing. Pointing `gen dts` at a leaf file works but requires knowing the
  package's internal layout, which is what the by-name form exists to spare you.
  The diagnostic naming OpenAPI keys for a `.d.ts` input is a second, smaller
  defect on the same path, still open.

  **Fixed in 0.1.74.** `resolveModuleFile` maps a runtime extension to the
  declaration file that carries its types and tries that too. The mapping is a
  lookup rather than a strip, because it is not uniform: `.mjs` takes its types
  from `.d.mts`, not `.d.ts`. Measured on the package that produced the entry,
  `glyph gen dts date-fns` went from **0 types to 280**.

- **G105. A file can only be read whole, and there is no async iteration.** A
  streaming merge that never holds more than one line per source cannot be
  written. `std/fs` has no open/read-at-offset/close and no line iterator.
  `std/io.read_line` does exactly the right thing (chunked `readSync` into a
  `StringDecoder`) but is hardwired to fd 0 through one module-level
  `pending`/`eof`/`chunk` triple, so it cannot be pointed at a file or
  instantiated twice, which is what k-way merging needs. The Node builtin is not
  a way round it either: the shims declare `readSync` but not
  `openSync`/`closeSync` (`[TS2305]`), and `readSync` is unreachable regardless
  because its buffer needs `Buffer` (`[E0103] unresolved name`, per G95) and its
  `position` needs a `null` the language deliberately has no spelling for. There
  is no `for await`, no generator, no `yield`, and nothing matching
  `AsyncIterable`/`asyncIterator`/`createReadStream` anywhere in the runtime.
  **`std/stream` is not related to this**: it is the property-testing sampler
  (`Stream<T> = { sample: (i: number) => T }`, with `ints`/`bools`/`from`), so
  the obvious name for the I/O abstraction is already taken and any design here
  has to decide what to call it. The only expressible version of the app is
  `read_text` plus `array.sort`, which is the memory shape the round existed to
  avoid.
  *Reproduced against 0.1.78: `fs.open` and `fs.read_line_at` are both `[E0105]
  not exported by std/fs`, and the runtime still contains no `asyncIterator`,
  `AsyncIterable`, `createReadStream` or generator. One clause of the premise has
  changed and the finding survives it: `readSync`'s buffer no longer needs a
  `Buffer` the language cannot name, because 0.1.78 shipped `std/bytes`. What is
  still missing is `openSync`/`closeSync` in the shim, a `position` that can be
  null, and any iteration protocol at all, so the streaming shape stays
  unwritable. `std/stream` is still the property-testing sampler, so the naming
  problem is unchanged.*

- **G106. E0106 calls an import dead when only an `@example` uses it.** A module
  whose `@example` annotations reference `Some`/`None`, a union's constructors,
  or `math.floor` gets `[E0106] unused import` for each, in the same build where
  those examples compile, run, and pass. Removing the import to satisfy the lint
  makes the examples fail with `[E0103] unresolved name`, so there is no
  warning-free spelling. This contradicts a documented requirement: `glyph llms`
  states that an `@example` must import the constructors it compares against.
  Four lines reproduce it:

      module repro
      import std/option { Option, Some, None }
      @example wrap(1) == Some(1)
      pub fn wrap(n: number) -> Option<number> { Some(n) }

  *Reproduced against 0.1.78: `import std/option { Some, None }` used only by
  two `@example` lines is two `[E0106] unused import` warnings, in a build whose
  examples pass. The same module with the constructors also used in the body is
  warning-free, which is what pins the finding to the `@example` reference not
  counting as a use.*

  Two independent rounds hit it, which is a fair signal of how often a test-only
  import occurs in practice. The lint's own justification is greppability ("no
  dead imports"), and an import an `@example` needs is not dead.
  *Reproduced against 0.1.72.*

**Checked and rejected, so it does not become an entry.** A round reported that
`int / int` yields a fraction: a function declared `-> int` returning
`59 / 30` gives `1.9666666666666666` with no diagnostic. It reproduces, but D31
says in as many words that `int` "is a boundary-validated refinement, not a
static type" and that `let x: int = 3.5` is deliberately not a compile error.
Documented v1 behaviour, correctly labelled as such by the round that found it.
Recorded here rather than silently dropped, so the next round that trips over it
finds the answer instead of re-filing it.

**One infrastructure blocker, which is not a compiler gap.**
`scripts/check_apps_are_glyph.py` walks `APPS.rglob("*")` with no `node_modules`
exclusion, and the root `.gitignore` does not cover
`examples/apps/*/node_modules/`. Installing a single npm package produced 3,977
gate failures, all of them vendored library files. The gate's intent is that an
app must not carry *hand-written* TypeScript, and a vendored dependency is not
that. Consequence to decide before changing it: the examples tree is built as a
whole by `repo_examples_emit_typescript_without_diagnostics`, so an
npm-dependent app makes CI need `npm install` per app or an exclusion. That is a
scope decision for the owner, not a fix, and it currently blocks the round the
roadmap says is next.

## Round 29: the wrong project's file

Not an app round. Found while wiring CI to let an app carry npm dependencies,
which is the infrastructure round 28 left blocked.

- **G107. In a multi-project build, a `tsc` error is reported against a
  same-named module in a *different project*.** `glyph build examples` builds
  each `apps/<name>/` as its own project (D41). When `tsc` reports an error in
  one project's `main`, the remap resolves the module name against the first
  project that has a module of that name, so the diagnostic lands on unrelated
  source. Reduced to two projects, each with a `main.glyph`:

      /tmp/mp/alpha/main.glyph   32 lines, entirely correct, line 3 is `import std/io`
      /tmp/mp/beta/main.glyph     line 3 is `import totally-not-installed { thing }`

  `glyph build /tmp/mp` reports:

      [TS2307] Error: tsc: Cannot find module 'totally-not-installed' ...
         ╭─[ main:3:1 ]
       3 │ import std/io

  The message is right, the line number is right, and the **file is wrong**: it
  quotes `alpha`'s source for `beta`'s error. Seen first in the real tree, where
  an error in `apps/zzprobe/main.glyph:3` was reported against
  `apps/auth_api/main.glyph:25` (`import clock`), which is a different app
  entirely. Someone acting on that opens a file with nothing wrong in it, and
  the quoted line looks plausible enough to try to "fix". This is the class
  0.1.60 closed for single-project builds ("the compiler stops blaming the wrong
  line"); the multi-project path kept it, and it only shows when two projects
  share a module name, which for `main.glyph` is every app in the tree.
  *Reproduced against 0.1.78, verbatim: two projects each with a `main.glyph`,
  the error in `beta`, and the diagnostic quoting `alpha`'s `import std/io`.*

## Round 33: a Linus review of the server lifetime, before it shipped

Four claims verified against the compiler and node before any were acted on; the
full review is in `feedbacks/linus/04-server-lifetime-and-std-net.md`. Most of it
was fixed in 0.1.80. Two were not, and are here.

- **G120. [FIXED] `http.read_request` has no body size cap and never settles when a
  client disconnects mid-body.** It accumulates `raw += chunk` with no limit and
  listens for `data` and `end` only, with no `aborted` and no `error`. A client
  that POSTs forever exhausts memory. A client that disconnects mid-body means
  `end` never fires, so the promise never settles, `respond` never returns, and
  the request's whole closure is retained for the life of the process: a leak
  with nothing in the log. One `curl` killed in a loop is enough.
  *Reproduced against 0.1.79.*

  **Fixed in 0.1.80.** A body over 8 MB is answered `413` and the read stops
  there rather than continuing to accumulate, and `aborted`/`error` now settle
  the read so a client that leaves mid-body ends the request instead of
  stranding it. `respond` distinguishes the three outcomes: a request to answer,
  a body too large, and a peer that is gone, which is written to rather than
  answered only in the first case. The size is counted in **bytes**
  (`Buffer.byteLength`), not in `chunk.length`, because a body of three-byte
  characters would otherwise be allowed three times the limit.

  The cap is a constant rather than a setting. A program that genuinely needs
  more than 8 MB in one request wants a streaming read, not a larger buffer, and
  there is no streaming read yet (G105); when that lands the limit belongs to its
  design rather than to a constant here. Verified both ways: a 9 MB POST is 413
  and the server survives, an interrupted upload leaves it healthy, and removing
  the cap makes the test fail.

- **G121. Network `Bytes` are zero-copy views onto node's pooled buffers.**
  `net.on_data` and `websocket.on_binary` both build
  `new Uint8Array(chunk.buffer, chunk.byteOffset, chunk.byteLength)`, and the
  reference calls `Bytes` "an immutable sequence of octets" without saying that
  one is a window onto a shared 8 KiB pool. The octets are correct; the cost is
  that retaining a 20-byte frame pins 8 KiB, so a chat server holding one frame
  per connection pays 8 KiB per connection. The comment in `websocket.ts` notes
  the view "is over the same memory rather than a copy" as though that were
  purely a benefit. Either say so in the reference, or copy when the slice is a
  small fraction of its backing buffer.
  *Reproduced against 0.1.79.*

## Round 32: the npm round finally ran, and three of its blockers were already gone

The 0.1.79 plan said this round could not be committed at all: the apps gate had
no `node_modules` exclusion, the root `.gitignore` did not cover a vendored
dependency, and CI building the examples tree as one meant an owner's decision
about running `npm install` per app. All three were already done, in CI
(`ci.yml` installs dependencies for any app whose `package.json` declares them),
in the gate (`check_apps_are_glyph.py` skips `node_modules`), and in
`.gitignore`. Both `gen dts` prerequisites, G103 and G104, were already fixed
too. Nothing gated the round. That is five stale premises across one plan, and
the lesson is the one already written down: re-check before implementing.

**The interop gate passes.** `examples/apps/feeds` reads an RSS feed with
`fast-xml-parser`, an ordinary typed npm dependency. It is imported by name,
constructed with `new` (D37), and returns an `any` that `Document.parse` turns
into a checked value. No adapter, no hand-written `.d.ts`, no `extern_ts`. That
is the first application in the tree to use a real npm package, so the 1.0 gate
("can a working engineer use their existing npm dependencies without a
hand-written adapter") now has an app behind it rather than only a guide.

- **G118. A client cannot say "this response body is text".** `http.get` returns
  a `Response` whose `body` is `unknown`, which is right, and there is no
  accessor that narrows it to a string. The client parses JSON when it can and
  keeps the raw string when it cannot, so for XML, HTML, CSV or plain text the
  string is already sitting in `body` and `string.from(response.body)` is the
  identity on it. That is the spelling `feeds` uses and it works, but it is the
  wrong shape: on a JSON response the same line renders `[object Object]` and
  reports nothing, so the correctness of the call depends on knowing what the
  server sent. `http.raw` exists and is the server-side counterpart, taking a
  `Request`. The missing piece is its client-side twin, `http.text(response) ->
  Result<string, string>`, failing when the body was parsed rather than kept.
  *Reproduced against 0.1.79.*

- **G119. `url.join`'s `Err` branch is nearly unreachable, and nothing says so.**
  Against a valid base the WHATWG parser treats anything that is not a URL as a
  relative path, so `url.join("https://x.test/feed.xml", ":::")` is
  `Ok(https://x.test/:::)` rather than an error. Only an invalid *base* fails.
  This is not a defect, and the signature cannot be tightened without lying
  about the base, but a caller writing an `Err` arm reasonably expects it to
  catch a malformed link and it never will. The fix is documentation: say which
  argument the failure comes from. `feeds` carries the case as an `@example` so
  the behaviour is pinned rather than assumed.
  *Reproduced against 0.1.79.*

## Round 30: what is left between `gen dts` and a usable `marked`

Closing G103 and G104 made the two silent failures loud, and running the result
against the package that produced them showed what is genuinely still missing.
`glyph gen dts marked --rename Tokens.List=ListToken` now writes 46 types and
exits 0, and the file still does not build: 14 `[E0103] unresolved name`.

The distinction matters and is the reason this is a new entry rather than a
reopened one. **Every one of those failures was disclosed by `gen` in its own
notes** ("reference to `Lexer` could not be resolved to a materialized type ...
`glyph build` will report it as an unresolved name"), nine of them, naming
exactly the nine names that then failed. That is the honest floor doing its job;
G103 was the case where nothing was said at all.

- **G108. The `.d.ts` reader materializes interfaces and type aliases, so a
  package whose surface is classes and TypeScript utility types is unusable
  through `gen dts` even when generation succeeds.** Against `marked`, the nine
  unresolved names fall into three groups, and each wants a different answer:
  **classes** (`Lexer`, `Parser`, `Renderer`, `Tokenizer`, `Hooks`) are the
  package's actual API and the reader walks only `interface`/`type`
  declarations, though D37 `new` already exists for constructing them;
  **TypeScript utility types** (`Omit`, `Pick`) are computed types with no
  JSON-Schema form, so materializing them means evaluating them rather than
  reading them; and **host types** (`RegExp`, `Promise`) have no Glyph spelling
  at all, with `Promise` deliberate under D40. A field typed by any of them is
  emitted as a reference to a name that was never written, and `glyph build`
  reports E0103. The workaround for a *value* is real (import the package
  directly and let `tsc` check it, which is the path that already works and has
  no adapter), so this bites specifically when a generated record has a field of
  such a type. Whether a class should materialize as an opaque type with a
  descriptor that only checks presence, or be skipped with its dependent fields
  widened, or make the whole type unmaterializable, is a design call with a real
  verifiability trade in it.
  *Reproduced against 0.1.80: `gen dts marked --rename Tokens.List=ListToken`
  writes 46 types and exits 0, and building the result is 14 `[E0103]`s across
  eight names, still in the same three groups: classes (`Lexer`, `Parser`,
  `Renderer`, `Tokenizer`, `Hooks`), utility types (`Omit`, `Pick`) and host
  types (`RegExp`). Worth noting for whoever fixes it that the generated file
  lands in `src/.types/`, which `glyph build` does not walk for `.glyph`, so the
  failure only appears once it is moved next to the source; a first pass at this
  reproduction read the silence as the gap having closed.*

## Round 31: four apps, and a loop index that was a string

Four rounds at once on shapes the tree does not cover: a static site generator
on real npm packages, a resilient HTTP client, a generic collections library, and
a localized message formatter. Three of the four produced a working app; every
finding below was re-reproduced by the orchestrator before being written down,
and two were fixed in the same pass.

The severe one is not the one that looks severe. `sitegen`'s finding blocks a
whole class of npm package, which is loud. `collections` found a program that
computes the wrong number while both Glyph and `tsc` report success, which is
the class this language exists to remove.

- **G109. [FIXED] A `for k, v` over an iterand whose type the checker had not
  settled silently took the record protocol, so the index arrived as a string.**
  An array's pairs are `it.entries()` (index is a **number**); a record's are
  `Object.entries(it)` (key is a **string**). The emitter chose by the iterand's
  static type and, per its own comment, defaulted "to a record when it is
  unknown". Guessing wrong there is not a style choice, it changes what the
  program computes:

      type Wire<V> = { keys: Array<string>, values: Array<V> }
      match Wire.parse<number>(raw) {
        Ok(w) => { for index, key in w.keys { io.println("next=${index + 1}") } },
        ...
      }

  prints `next=01` and `next=11` instead of `1` and `2`, from a build reporting
  `no diagnostics` and `tsc --strict passed`. The same loop over the same
  declared `Array<string>` emits `w.keys.entries()` when the value came from a
  non-generic `parse`, so two spellings of one idiom disagreed at run time.
  **Fixed in 0.1.74:** `iter_shape` now answers Array, Record, or Unknown as
  three distinct cases, and Unknown emits `__glyph_pairs(it)`, a bootstrap helper
  that reads `Array.isArray` at run time. The compiler cannot always know the
  shape; the runtime always can. A settled type keeps its direct emit, so no
  typed loop pays for this. `Ty::Imported` counts as unsettled, since a type
  crossing a module boundary carries no shape.

- **G110. [FIXED] The `Ok` payload of a *generic* record's `parse` is opaque to
  the checker.** The cause behind G109 rather than a duplicate of it, and still open.
  `descriptor_member_ty` returns `None` when `td.generics` is non-empty, with the
  reason recorded in its own doc comment: a generic record's descriptor takes one
  runtime checker per type parameter, so its arity differs from the non-generic
  form. The consequence is broader than the loop: a field typo on the parsed
  value produces **no Glyph diagnostic** (it falls through to a `tsc` TS2339
  mapped to the whole enclosing function), where the non-generic path gives
  `[E0210] type `PlainWire` has no field `keyz`` at the field. That is the G75
  decision's problem one layer in. Typing it means modelling the per-parameter
  checker arity that the emitter already writes, so the two would agree by
  construction the way the non-generic pair already does.
  *Reproduced against 0.1.74.*

- **G111. [FIXED] A stdlib type imported by name lost the field table the
  namespaced spelling gets, switching off two checks.** `import std/http {
  HttpError }` and `import std/http` + `http.HttpError` are both legal, and they
  disagreed:

      | spelling                | `e.nope`        | match over all 3 literals |
      | named                   | no Glyph error  | E0218, "add an `else`"    |
      | namespaced              | E0210           | compiles, exhaustive      |

  `stdlib_type_path` keys the field tables on a two-segment path, and a named
  import has one segment, so `HttpError.kind` typed `Unknown` and the D30
  string-literal union that makes the match exhaustive was never found. The
  advice E0218 then gives is to add a catch-all, which is advice to switch the
  check off. This is the class CLAUDE.md records as settled twice already
  (0.1.56, 0.1.57) and calls wrong on arrival, and `lower.rs` states the rule in
  its own doc comment three functions above the bug: a guarantee must not depend
  on which legal spelling brought the type into scope. **Fixed in 0.1.74:** the
  `ImportNamed` arm consults `stdlib_modeled_type` before falling through, so
  both spellings lower to the same `Ty`.

- **G112. [FIXED] Glyph has no default-import form, so a CommonJS `export =`
  callable package is unreachable.** The single widest interop gap found so far. A package
  whose export *is* a function (`module.exports = f`) cannot be called at all;
  all three D15 import spellings fail:

      import pkg { pkg }        -> [TS2595] can only be imported by using a default import
      import pkg as p           -> [TS2349] This expression is not callable
      import pkg { default as p } -> [E0002] parse: expected `}`, found As

  Verified against a minimal `export =` package and against **express**, where
  `import express { express }` is `[TS2724] '"express"' has no exported member
  named 'express'`. The reach is most of the pre-ESM registry: express, lodash,
  debug, chalk@4, minimist, commander, and `gray-matter`'s documented
  `matter(text)` entry point. **The gap is exactly the default binding**: a
  *named* export reached through the same `export =` namespace
  (`import gray-matter { read }`) compiles and runs, which is what
  `examples/apps/sitegen` uses, with a source comment saying why.

  **Fixed in 0.1.74**, and D15 now names four import forms:
  `import express { default as app }`. The `as` is legal only after `default`,
  never for an arbitrary imported name, so general renaming stays closed and a
  name in the file still matches the name at its source; `grep 'default as'`
  finds every default import. Binding it through the *aliased* form was rejected
  because it would give one spelling two meanings depending on the package's
  module format, which is exactly the class G111 was fixed to remove. Verified
  end to end: `matter("---\ntitle: hi\n---\nbody text")`, gray-matter's own
  documented entry point, prints `content: body text`.

- **G113. [FIXED] `Intl` is unreachable, so CLDR plural data has no route.** A host
  global, and Glyph resolves names from modules, so `new Intl.PluralRules(loc,
  {})` is `[E0103] unresolved name `Intl``. That much is the documented D-stance
  (G95). What the round adds is which side of the line each thing falls:
  **method forms pass through and are type-checked** (`value.toLocaleString(loc,
  { style: "currency", currency: c })` and `a.localeCompare(b, loc)` work today
  and a bogus option is a real `[TS2769]` against `Intl.NumberFormatOptions`), so
  locale-aware number, percent, currency and collation are all available and
  **undocumented**; nothing under `runtime/std/` mentions them. What has no
  method form has no route at all: `Intl.PluralRules`, `NumberFormat` (as a
  reusable formatter), `ListFormat`, `RelativeTimeFormat`, `Collator`,
  `DateTimeFormat`, `Segmenter`. The app hand-wrote `en` and `pl` plural rules;
  a real one needs ~200 rule sets. The npm answer works as a direct import
  (`intl-messageformat` builds and runs under `tsc --strict`) but **cannot be
  materialized**: `glyph gen dts intl-messageformat` writes 28 types and the
  result does not compile, because its declarations reference the `Intl.*`
  globals Glyph has no types for. So any package whose types touch `Intl` is
  import-only, never boundary-validated.

  **Fixed in 0.1.74** by the answer this repo already documents for a host global
  the stdlib does not wrap: `std/intl` wraps it, the way timers and WebSocket
  were. Twelve functions covering plurals, ordinals, numbers, fixed decimals,
  currency, percent, lists, relative time, dates, collation and locale
  negotiation. The wrapping earns its keep rather than just forwarding:
  `plural_category` returns the **string-literal union** of the six CLDR
  categories, so a match over it is exhaustive with no catch-all and a missing
  one is `[E0200] ... missing variants "zero"`. Exposed as a bare `string` it
  would have been E0218, whose advice is to add an `else`, and an `else` over a
  plural category is how a locale's `few` silently renders as `other`. Verified
  against real CLDR data: Polish 1/3/5 select one/few/many. The `gen dts`
  half stays open and is folded into G108, since a package whose declarations
  reference `Intl.*` still cannot be materialized.

**Refining G108 with evidence from this round.** The entry says `gen dts` fails
on marked because the reader handles only `interface`/`type` declarations. That
is right for `Lexer`/`Parser`/`Renderer`/`Tokenizer`/`Hooks` and for
`Omit`/`Pick`, but **`RegExp` is neither**: it is a TypeScript global reached from
an ordinary `interface` the reader does handle
(`interface Rules { block: Record<string, RegExp> }`). So the gap is wider than
"classes and computed types" and includes host types referenced from shapes that
otherwise materialize cleanly. `Promise` degraded to `unknown` rather than
erroring, which is D40 working as designed.

**Two small ones, neither blocking.** `string.from` over an `Array<Issue>`
renders `[object Object],[object Object]`, so a validation failure has to be
mapped field-by-field to read. And `path.join` takes an array
(`path.join([dir, name])`) where `string.join` takes a separator
(`string.join(parts, sep)`); `glyph llms` does not list `std/path` at all, so the
shape is only discoverable by reading the runtime.

## Round 32: the outside app came back, and it shipped

`github.com/canpolatoral/glyph-hello` added Ultimate Tic Tac Toe: 3,377 lines of
Glyph across a rules engine and an AI (depth-limited minimax with alpha-beta,
iterative deepening under a time budget, three difficulty levels), driving a Web
Worker so the search never blocks the main thread. **198 `@example` tests pass,
six modules, `tsc --strict` clean** on 0.1.74. Still no TypeScript in `src/`, and
still no hand-written `.d.ts`.

Two things are worth saying before the gaps. Alpha-beta is what round 28 recorded
as *expressible but awkward* (G101, no early exit in `fold`); an outside developer
wrote it anyway, at scale, and it works. And the app is now genuinely
client-side, which is the deployment shape none of our own apps have.

What it cost them is a **487-line build tool** (`tools/build-web.mjs`) to get
Glyph output into a browser. They took the no-npm-dependencies path deliberately,
so a bundler was not on the table; every step in that file is a thing the
compiler did not do for them. Three are real.

- **G114. [FIXED] The emitter puts type-only names in a value import list,
  which is a hard ESM link error once types are stripped.** `import std/option { Option,
  Some, None }` emits `import { Option, Some, None } from "std/option"`, and
  `Option` is `export type` in the runtime, so it has no runtime binding.
  `import std/option { Option }` alone emits an import whose every name is a
  type. `tsc` elides such names, which is why `glyph build` is green and why
  their pipeline routes through `tsc --outDir` on purpose; their own comment
  says a bare type-stripper "is a hard ESM link error in a browser". Confirmed
  both ways:

      $ node consumer.mjs        # import { Option, Some, None } from "./std/option.js"
      import { Option, Some, None } from "./glyph/std/option.js";
               ^^^^^^   SyntaxError: does not provide an export named 'Option'

      $ node consumer_ok.mjs     # same file, type-only name removed
      linked Some None

  It also makes the output ill-formed under `verbatimModuleSyntax`, which is
  `[TS1484] 'Option' is a type and must be imported using a type-only import`
  against **our own `runtime/std/*.ts` sources as well as the emitted user
  code**. The fix is to emit `import type` for a name the module exports as a
  type, and to split a mixed import. `docs/guide/deployment.md` currently tells
  a reader the output "bundles like any other TypeScript", which holds for a
  bundler that elides unused type imports (esbuild does) and not for the
  stripper-based toolchains that are now common.

  **Fixed in 0.1.75.** Every emitted import now marks a name with no runtime
  binding using the inline `type` modifier, which is the spelling a tool with no
  type information can act on. Two populations needed it, and the second was
  missed on the first pass: the hand-written standard library, whose 25
  `export type` names across 16 modules are listed in `stdlib_types.rs` and
  reconciled against the runtime by a gate that fails in **both** directions (a
  name missing from the table emits unmarked and will not link; a value name
  wrongly in it would be elided and lose a binding); and **a Glyph plain alias**,
  which emits `export type Board = Array<Cell>` with no descriptor `const`,
  unlike a record or tagged union which ships one under its own name. The second
  came from checking the fix against the app that found the gap rather than
  against a repro. The runtime's own sources were fixed too, so
  `verbatimModuleSyntax` is now clean over the whole emitted tree, including
  `glyph-hello`'s 3,377 lines.

- **G115. `glyph build` materializes the whole standard library, under a
  directory name a static host hides.** The engine imports five std modules;
  the output carries **31**, including `sqlite`, `http`, `fs` and `process` —
  modules a browser worker must not contain at all. Tree-shaking answers this
  when a bundler is in the pipeline, and `docs/guide/deployment.md` says so, but
  a no-bundler deployment has to prune the graph itself, which is step 3 of their
  487 lines. Step 4 renames `.glyph-runtime` to `glyph`, because a path component
  starting with a dot is hidden by most static hosts, so the emitted layout
  cannot be uploaded as-is. Neither is a miscompile; both are the difference
  between "the output is portable JavaScript" and "the output is deployable".
  A `--target browser` that emits pruned, relative-specifier ESM is the shape
  that would remove the file.
  *Reproduced against 0.1.80, and it has grown: a program importing three
  modules (`array`, `io`, `option`) now emits **36**, up from the 31 recorded
  here, and the browser-hostile set is `dns`, `fs`, `http`, `net`, `process`,
  `sqlite`, `tls`. Three of those (`net`, `tls`, `dns`) are 0.1.79's own work, so
  each release that adds a host module makes this entry worse rather than
  leaving it flat. The output directory is still `.glyph-runtime`.*

**Their pin is still `^0.1.72`**, from the scaffold before the exact-pin change,
so this app is exactly the population `glyph upgrade` reads a caret for.

**Re-verified against the published 0.1.75, and the result is worth keeping.**
A fresh `npm install` in their checkout resolved that caret straight to 0.1.75,
silently, which is the floating-pin hazard happening to a real project rather
than to a test case. Nothing broke: 198 `@example` tests pass, `tsc --strict` is
clean, and their own browser build tool runs both paths end to end, worker smoke
test included (`AI chose board 4 cell 4 (legal: yes, depth 4, 3061 nodes)`). So
three releases moved under an outside application with no regression.

**One assumption checked and found false, before it reached them.** G114 looked
like it must unblock their `--strip` fallback, since their comment says a bare
type-stripper "is a hard ESM link error in a browser". Running that path against
**0.1.74**, before the fix, it works: their tool prunes exports itself, so they
had already solved it. The fix is still right, and it was not the favour to them
it appeared to be. Worth recording because the next person reading G114 will make
the same inference.

## Round 33: reading the outside author's session log

The author of `glyph-hello` shared the agent session that built Ultimate Tic Tac
Toe: 9,090 lines covering 14 agents and roughly 9.4M tokens, producing 3,377
lines of Glyph. It is the first record we have of what writing Glyph *feels like*
from outside, in real time, rather than what the result looks like.

**The headline number is how little went wrong.** Across the whole build the
compiler produced **eleven** diagnostics: eight `E0105`, two `E0106`, one
`E0103`. For 3,377 lines of a rules engine and an alpha-beta search, written by
agents that had never seen the language, that is the manifesto's bet paying off
rather than a list of complaints. What follows is the friction that remained.

- **G116. [FIXED] `E0105` says the name is wrong and never says what is right,
  so an agent guesses.** All eight of the session's `E0105`s are one agent hunting a
  single function in `std/random`, in order: `int`, `next`, `float`, `number`,
  `range`, `int_range`, `between`, `shuffle`. The module exports exactly one
  name, `seeded`. Eight build round-trips to find it. The message is accurate
  every time ("`int` is not exported by `std/random`") and its help is "Check the
  spelling, and that the module actually exports this name" — advice that cannot
  be acted on without the list. **The resolver is holding that list already**;
  producing the error is what proves it. Naming the module's exports, or the
  nearest matches, collapses eight round-trips into one. For a human this is an
  annoyance solved by opening the docs; for an agent, which is who this language
  is for, each guess is a whole build cycle.

  **Fixed in 0.1.76.** The error now carries the answer it was already holding.
  A near miss gets the intended name (`string.repeeat` is ``(did you mean
  `repeat`?)``), and anything else gets the export list, capped at eight with a
  count of the rest so a wide module does not bury the message. Every one of the
  session's eight guesses now ends on the first build: ``is not exported by
  `std/random` (exports: Rng, seeded)``. The edit budget scales with name length,
  so a three-letter guess cannot "match" an unrelated three-letter export. It
  went in the message rather than the help because the help is the same sentence
  for every instance and the useful part is not.

- **G117. [FIXED] The most-recommended loop idiom is the slowest one, and
  nothing says so.** The session's own benchmark concluded that idiomatic
  `array.map`/`filter`/`fold` closures cost 4.5x to 20x in the search hot path,
  and it recommended rewriting the scanners as `for i in array.range(n)` with
  `mut` accumulators. Measured here over the same scanning shape (81 cells,
  200k rounds, one build, warm):

      for c in cells (direct)                40 ms
      array.filter + closure                 72 ms   1.8x
      for i in array.range(n) + cells[i]    168 ms   4.2x

  **The recommendation is the slowest of the three.** Closures are not the
  problem. `array.range(n)` allocates an n-element array on every call, and
  `cells[i]` emits `__glyph_index(cells, i)`, a helper doing `Array.isArray` plus
  a bounds check per element, so an index loop pays an allocation and a call the
  other two forms do not. Substituting `<` for `==` changed nothing, so the
  `__glyph_eq` helper is inlined by V8 and is not a cost here.

  Two things follow. `docs/guide/performance.md` says function calls, records and
  closures "are ordinary JS values and operations", which is true and is not the
  guidance a reader needs; it says nothing about iteration form, and there is no
  benchmark in the repo covering these idioms. And `for x in array.range(a)` is
  an obvious lowering target: a counting `for` allocates nothing and is the shape
  every reader expects it to already be. Until then the fastest advice is the
  plainest one, `for c in cells`, which nothing currently tells anyone.

  Their measurement and this one disagree in direction, and both can be right:
  theirs timed whole engine functions doing more per element. What is not in
  dispute is that the performance of the three idioms is not predictable from
  the docs, and that an outside team spent a benchmark harness finding out.

  **Fixed in 0.1.76.** A `for` that iterates `array.range(n)` or
  `array.range_from(a, b)` **directly** lowers to a counting `for` with both
  bounds hoisted into the initializer, so nothing is allocated and neither bound
  is re-evaluated per step. Only a direct call qualifies: bound to a `let` it is
  a real array something else may hold, and the loop keeps walking it. The same
  benchmark after the change: 62 ms filter, **61 ms** index loop (from 168), 33
  ms direct. The trap is gone; direct iteration stays fastest because indexing
  still costs a bounds check per element, and `docs/guide/performance.md` now
  carries the table and says so, which it did not before.

**Already recorded, and now confirmed from outside.** The session hit `E0106`
twice on imports its `@example`s needed, which is G106. Their own reference
notes it as a limitation ("an `@example` that compares against a prelude
constructor must import it"), so an outside reader has written our lint's
contradiction into their notes as a known cost of using the language.
