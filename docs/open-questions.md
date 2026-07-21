# Open questions

The live unresolved decisions, organized by what they block. This is the input list for the upcoming brainstorm.

A question lives here when:
- It has been deferred from a prior session/proposal and is now load-bearing for a future step, **or**
- Two existing decisions conflict and one of them must give, **or**
- A pillar implication has been claimed but not pinned down.

---

## Resolved decisions — Session 1 of brainstorm (2026-05-26)

Five step-blocker decisions resolved. Originals preserved below with `[RESOLVED]` markers; this section is the canonical record.

### Q1 → **shipped in 0.1.10 (D28).**
Originally deferred to v1.1; `01_validator.glyph` shipped a stand-in `<Out>` parameter that the caller had to keep in sync by hand (unchecked). 0.1.10 shipped `infer_shape<Shape>` as a narrow built-in type-level operator (D28), not the full TS mapped-/conditional-type machinery: `object_schema<Shape: Record<string, Schema<unknown>>>(shape) -> Schema<infer_shape<Shape>>` now **derives** the output type from the shape, and `tsc` checks the caller's `Schema<User>` annotation against it. This closed the last silent unsoundness the "Linus" review found and let the blanket generic-return `as` cast be narrowed to `infer_shape`-returns only. See spec D28.
**Reflected in:** `docs/roadmap/04-transpiler.md`, `docs/roadmap/05-typechecker.md`.

### Q2 → **Fold corpus generation into step 6 dogfooding.**
No synthetic-examples-by-hand phase. Step 4 ships against the existing 4 hard cases plus whatever step 6 (fridge shopping list and successors) generates as real production code. Saves ~3–4 weeks; trades coverage breadth for ergonomic-realism.
**Reflected in:** `docs/roadmap/04-transpiler.md`, `docs/roadmap/06-dogfooding.md`.

### Q5 → **Hybrid compiler architecture.**
Typechecker and name resolver are built around salsa-style demand-driven queries from day one (~1–2 extra weeks in step 4). AST→TS emission remains a dumb visitor. Step 7 LSP timeline stays at 4 weeks because the load-bearing queries (diagnostics, hover, go-to-def) already exist as compiler queries.
**Reflected in:** `docs/roadmap/04-transpiler.md`, `docs/roadmap/07-lsp.md`.

### Q20 → **Two loop constructs: `for x in iter` + `loop { }`.**
`for x in iter { ... }` for bounded iteration. `loop { ... break ... continue }` for unbounded retry/server loops. Matches Rust. No `while` (use `loop { match cond { false => break, true => ... } }` or `iter.take_while(...)`). Becomes **D21** in the spec.
**Reflected in:** `docs/language/spec.md` (new D21).

### Q21 → **Stdlib migration pattern, no new syntax.**
Stdlib provides `Migration<From, To>` with `migrate.from<Old, New>((old) => new)` and a `Schema.parse_versioned(input)` that walks migrations. The shopping list app's persistence boundary is the first stress test. Forward-compatible to language-level migrations later if needed.
**Reflected in:** `docs/roadmap/06-dogfooding.md`.

## Resolved decisions — Session 2 of brainstorm (2026-05-26)

The two manifesto-touching questions are resolved **without requiring manifesto rewrites**. The current "looks almost like TypeScript" stance survives.

### Q32 → **Option C: LSP exposes virtual agent view.**
On-disk file remains the human view (current Glyph syntax). The LSP exposes a virtual document `agent://file.glyph.canonical` with stable line numbers and SSA-like tokens (`L001`, `$0`, `$1`, ...). Agents query the LSP RPC for the canonical form when they need it. No language commitment; no parser fork; the diff-stability pillar is served by *tooling*, not by bifurcating the language.
**Composes with:** Q29 (structured edit API) — both are LSP RPCs.
**Reflected in:** `docs/roadmap/07-lsp.md`.

### Q40 → **Option B: stdlib metadata + external `glyph regen` CLI.**
`@generate by:` is documentation metadata, not a language primitive. Bodies are normal Glyph code, written by anyone (human or agent). A separate `glyph regen <fn>` CLI command regenerates a body given the spec block; the user runs it explicitly. Q11 (executable `@example` tests) + Q40 metadata together give 90% of the value with 10% of the commitment. Forward-compatible to language-level `@generate` if v2 needs it.
**Composes with:** Q11 (testing model — `@example` and `@property` carry the contract; Q40's `glyph regen` reads them).
**Reflected in:** `docs/roadmap/08-09-packaging.md`.
**Advanced in 0.1.3:** the first type-driven generators ship as external `glyph gen` CLI commands (the same "generate real, committed code" stance): `glyph gen openapi` maps an OpenAPI/JSON Schema document to Glyph records, and `glyph gen dts` materializes a TypeScript `.d.ts`. Output is canonical, descriptor-bearing, and idempotent to regenerate. See `docs/guide/typed-apis.md`.

### What changed in the roadmap (sessions 1 + 2)

- **Step 4 (transpiler):** salsa-for-typecheck adds ~1–2 weeks, no synthetic-corpus phase, validator example rewritten as prerequisite. New estimate **6–8 weeks** (was 4–6).
- **Step 5 (typechecker):** substep 5b (mapped types) deferred to v1.1; substep 5a + 5c remain. New estimate **~9 weeks** (was ~13).
- **Step 6 (dogfooding):** now also produces the test corpus for step 4's CI. Migration stdlib drops alongside the shopping list.
- **Step 7 (LSP):** stays at 4 weeks (Q5 hybrid). **Scope expanded** to include the `agent://` virtual-document RPC (Q32) and structured-edit RPC (Q29 if it resolves toward Option B). May push to 5–6 weeks depending on Q29's resolution.
- **Step 8 (packaging):** ships a `glyph regen <fn>` CLI (Q40) alongside `glyph build` / `glyph fmt` / `glyph publish`.

### What's now load-bearing

For **step 4**: Q3 (stdlib minimum bootstrap), Q4 (tree-sitter scaffolding).
For **step 5**: Q6 (error-message bar), Q7 (`mut` typechecker semantics), Q8 (`is TypeName` runtime descriptors), Q9 (restricted JSX purity classifier).
For **step 7**: Q6 (still — same bar), Q29 (structured edit API as LSP RPC).
For **step 11 (killer demo)**: Q10 (token-count benchmarks — the manifesto's empirical claim still has no measurement plan).

**Session 3** will speed-triage the remaining 30+ questions (Q3, Q4, Q6–Q19, Q22–Q31, Q33–Q42 minus already-resolved-or-folded) into v1 / v2 / reject buckets.

---

## Resolved decisions — Session 3 of brainstorm (2026-05-26)

The remaining ~30 questions resolved in one sweep. Three required user sign-off (Q11, Q23, Q24); the rest had strong-enough recommendations to apply directly. Six new D-decisions added to the spec (D22–D27). One manifesto carve-out (Q24's narrow `owned`).

### V1 — ships in v1.0

- **Q3 → stdlib bootstrap.** v1 ships: `result, option, array, string, io, json, fs, time` + prelude (`Result`, `Option`, `Ok`, `Err`, `Some`, `None`, `par`). Everything else (`http`, `process`, `crypto`, React bindings) is v1.1.
- **Q6 → Elm-quality error messages.** Concrete bar replaces "make it fast." Reflected in step-5 typechecker scope and step-7 LSP launch gates.
- **Q7 → `mut` syntactic only.** D5 restricts at grammar level; typechecker does not enforce that called methods actually mutate. Pure-method annotation on every stdlib method is too heavy.
- **Q8 → runtime descriptors at every type decl.** Core to verifiability; non-negotiable. Schema parser generated alongside.
- **Q9 → JSX purity via whitelist.** Stdlib functions pure by convention; user functions require explicit `@pure` annotation to be JSX-callable.
- **Q10 → benchmarks track starts in step 4.** `benchmarks/` directory with 5–10 functions in Glyph vs TS vs Python vs Rust. Token count, line count, diff size tracked over time.
- **Q11 → `@example` as language primitive, `@property` as stdlib.** `@example` becomes **D23**; lands with `@doc @run` (D26, from Q31).
- **Q12 → discoverability via LSP.** Workspace indexing.
- **Q18 → structured concurrency via stdlib only.** `par.all` / `par.all_ok`. No `parallel { }` block in v1.
- **Q19 → errors-as-runbooks as stdlib convention.** Stdlib error types ship with a `remediation` field of a sum type.
- **Q22 → content-addressed imports via `"glyph"` key in `package.json`.** Audit metadata (`stdlib`/`internal`/`third-party`) and `last_reviewed` dates. `glyph publish` checks audit currency.
- **Q23 → split.** `@redact` is **D24** (v1 language); tracing/metrics ship as v1 stdlib (`trace.span`, `metrics.counter`); first-class `@trace`/`@metrics` deferred to v2.
- **Q24 → narrow `owned` modifier.** Becomes **D25**. ⚠️ **Manifesto carve-out** added to `docs/manifesto.md`. Resource-discipline only; not a general affine/linear system.
- **Q29 → structured edit API via LSP RPC.** Agents call `applyEdit`; LSP returns `{ ok, rejected, reason }`. Not a language construct. Composes with Q32.
- **Q31 → `@doc @run` executable docs.** Becomes **D26**. Folded with Q11 — same machinery.
- **Q33 → taint via stdlib newtype.** `Tainted<T>` / `Trusted<T>` with `sanitize()` discipline. No flow analysis in v1.
- **Q34 → budgets via stdlib helper.** `withBudget({wallTime, llmTokens, usdCost}, () => {...})`. Language-level `@budget` is v2.
- **Q41 → FFI minimal: TS wrappers only.** Non-TS interop via npm packages (`node-ffi`, `neon`, etc.). `@ffi target:` syntax deferred to v2. *0.1.3 eased the ergonomics:* `glyph gen dts` materializes an external TypeScript `.d.ts` into a first-class, descriptor-bearing Glyph type, so a hand-written wrapper is no longer the only way to bring an external shape across the boundary.
- **D12 re-litigated → template literals adopted.** Becomes **D22**. `"hello ${name}"` joins the spec.

### V2 — explicitly deferred

- Q4 (tree-sitter scaffolding) — Rust Pratt parser is the runtime; tree-sitter is editor tooling.
- Q14 (design-by-contract `requires`/`ensures`) — needs SMT or symbolic execution.
- Q15 (refinement types via `where`) — typechecker work; v1 uses nominal newtypes.
- Q17 (capabilities incl. time/randomness) — big stdlib redesign.
- Q25 (compiler-enforced semver) — no Glyph registry in v1.
- Q27 (bidirectional functions `bifn`) — demand not proven.
- Q28 (typestate) — protocol-correctness wins, but four examples don't exercise it.
- Q36 (units of measure) — v1 uses nominal newtypes.
- Q41 extension (`@ffi target: c/rust/python` syntax) — when demand surfaces.

### Rejected (closed door)

- Q13 (stable IDs `@gid`/`@fid`) — LSP rename refactor + Q32 canonical view carry the load.
- Q16 (versioned signatures `.v3`) — only meaningful if Q13 had landed.
- Q26 (complexity annotations) — static Big-O verification is research-grade.
- Q35 (feature flags as language primitives) — infrastructure concern; LaunchDarkly etc. exist.
- Q39 (policy-as-types) — enterprise feature; revisit when enterprise customers ask.

### Folded into other decisions

- Q37 (content-preserving refactors) → category of edit within Q29 LSP RPC.
- Q38 (differential typing across versions) → extension of Q25 (deferred together).
- Q42 (change propagation graph) → UX layer on top of Q5 salsa queries + Q29 LSP RPC.

### Re-litigation appendix outcomes

- D3 (match-as-only-conditional): **keep.** Re-litigate via step 6 dogfooding if 2000-line files surface the pain.
- D10 (no object literal shorthand): **keep.** Greppability earns its keep.
- D12 (no template literals): **CHANGED.** Now D22 — template literals added to v1.

### New D-decisions added to spec.md

| # | Decision | Pillar |
|---|---|---|
| D21 | Two loop constructs (`for x in iter`, `loop { }`) | Greppability |
| D22 | Template literals (`"hello ${name}"`) | Abstraction |
| D23 | `@example expr == expr` inline compile-checked tests | Verifiability |
| D24 | `@redact fields: [...]` PII enforcement on type declarations | Verifiability |
| D25 | Narrow `owned` modifier for resource handles | Verifiability |
| D26 | `@doc """ ... @run ... """` executable documentation | Verifiability |
| D27 | `@<name>` annotations as a meta-rule (umbrella) | Abstraction |

### Manifesto change

Single carve-out added to "no linear types": narrow `owned` modifier for resource handles only (file/socket/db). For resource discipline, not for a general affine/linear system. See `docs/manifesto.md`.

### Total scope at end of session 3

**v1.0 spec: 27 D-decisions** (D1–D17 from session 0; D18–D20 from session 0; D21–D27 from sessions 1+3).
**Open questions remaining: 0 blocking; all soft questions deferred as before.**

---

---

## Reopened by feedback — Serhiy's React session (2026-07-12)

A working React developer built a todo app in Glyph (AI-authored), wired in
`react-hook-form` + `zod`, and gave a hands-on "would not use it" verdict. Full
record in `feedbacks/serhiy.md`. His session re-litigates one resolved decision
(Q41) and surfaces one genuinely new gap (Q44). It also produced a short list of
grammar accidents that are bugs, not open questions.

### Bugs to fix (no pillar cost — not open questions, just work) — FIXED 2026-07-12

These collide with universal web/npm conventions and serve no pillar. They are
listed here only so the reopened questions below don't get conflated with them.
All three are now fixed in the compiler (parser + emitter); spec D6 and D15 note
the two language-surface refinements.

- **Hyphenated JSX attribute names** (`aria-label`, `data-*`) parsed as `Minus`.
  Fixed: the JSX name reader joins byte-contiguous `ident-ident` runs, and the
  emitter quotes non-identifier prop keys (`"aria-label": ...`). (D6.)
- **Hyphenated/scoped npm import specifiers** (`react-hook-form`,
  `@hookform/resolvers/zod`) were rejected (`E0002: found Minus` / `found At`).
  Fixed: import-path segments accept hyphens and an optional leading `@scope`.
  (D15.)
- **Runtime bootstrap not injected into the browser bundle.** `number.to_string`
  depended on a global that `glyph run` installs but a Vite browser build never
  loaded, producing a live `ReferenceError` on first run. Fixed: every emitted
  module now side-effect-imports `.glyph-runtime/glyph-bootstrap`, so the ambient
  globals exist no matter which module an external bundler treats as the entry.

### Q43. Ergonomic TS-library interop (re-litigates Q41)

**Q41 resolved that TS interop is "hand-written TS wrappers" and treated that as
solved.** Serhiy's session is evidence it is not solved *ergonomically*: every real
React dependency (react-hook-form, zod, and by his own extension date-fns, UI
libraries, formatters) required a bespoke hand-written TS adapter. A per-library
adapter tax is fine for one library and absurd for the thirty a real app imports.
Three Glyph rules drive the adapter requirement:

- **Prop/argument spreading** (`{...register("title")}`, `{...props}`) is banned
  (greppability/diff-stability). This is the genuinely hard one — deliberate *and*
  mandatory for RHF-style APIs. Sub-question: is a narrow spread allowance
  defensible for spreading a *statically typed record whose fields are known at
  compile time* (greppability preserved via the type), while still banning
  spread of untyped/`any` values?
- **Value-derived types** (`z.infer<typeof schema>`) — deriving a type from a
  runtime value. Glyph's stricter typing has no expression for this. Sub-question:
  is a `typeof`/`infer`-style operator worth a v1 carve-out, or does the adapter's
  `.d.ts` remain the boundary?
- **Hyphenated/scoped package names** — a pure grammar accident, listed as a bug
  above; fixing it removes one of the three adapter drivers for free.

**The strategic question:** should v1 offer a first-class "import a TS module and
use its API directly" path, so a Glyph file inside an existing React/Node codebase
consumes libraries without a bespoke adapter per library? This is the make-or-break
for living inside existing codebases (echoes Hayk, Adi, README theme 3). Options
range from "keep adapters, document the pattern well, ship a codegen tool that
scaffolds the adapter + `.d.ts` from a TS module" (cheapest) to "a real `extern`
mechanism that imports a TS declaration and applies Glyph's rules only at the Glyph
call site" (largest). **Not a unilateral call — brainstorm input.**

### Q44. React Context + effectful custom-hook composition

**New gap, not covered by any prior question.** Big React apps are built on custom
hooks and Context. Two concrete holes:

- **Context has no story in the spec or docs.** No `createContext` / provider /
  `useContext` equivalent is documented. Serhiy: "big projects use a lot custom
  hooks and contexts, I have no idea how it would be feasible."
- **Effectful custom hooks vs the `@pure` JSX-callable rule (Q9 → D9).** Components
  and built-in `use_state` work, but a user function must be `@pure` to be
  JSX-callable, and a custom hook that calls `use_state`/effects is by definition
  not pure. How does an effectful custom hook compose and get called? In Serhiy's
  session a custom hook (`use_task_form`) only existed because the TS adapter built
  it in TypeScript.

Needs a spec answer **before step 6 dogfooding** touches any Context-using or
custom-hook-heavy app, or the dogfood will stall on it. Relates to the React
bindings deferred to v1.1 under Q3.

### Positioning note (framing, tied to Q32)

Serhiy's two headline objections split cleanly along Glyph's own AI-first bet:

- **"Not feasible manually / like notepad"** is about the *human review-and-maintain*
  path. Non-negotiable regardless of who authors, because humans review agent
  output in an editor. This is Q32 (dual human/agent view) made concrete and argues
  for pulling editor syntax highlighting forward off the existing tree-sitter
  grammar, ahead of the full LSP (step 7).
- **"Changes how you think in React"** is a *human-authoring* migration cost. An AI
  agent has no muscle memory to unlearn; it writes whatever the grammar dictates.
  This is the least applicable complaint to Glyph's actual target author, and
  "fixing" it by making Glyph more React-like would erode the pillars.

**Decision owed:** does Glyph court human React authors at all, or is it
agent-authored + human-reviewed? That call determines which of Serhiy's complaints
are bugs versus working-as-designed.

---

## Blockers for step 4 (transpiler)

### Q1. Is `infer_shape<Shape>` v1 or v2?

The validator example (`01_validator.glyph`) uses `Schema<infer_shape<Shape>>` — a type-level function over a record of schemas. That is mapped-type territory. The original strategy says "skip mapped types in v1." The example demands them.

- **If v1:** mapped types cannot be skipped — they have just been renamed. Substep 5b of the typechecker is mandatory; step 4 needs an emission strategy for type-level computation.
- **If v2:** rewrite `01_validator.glyph` *now* to use an explicit type parameter, so step 4 and 5 scope is honest.

Source: `archive/glyph_step5_notes.md`. **Must be decided before step 4 starts.**

### Q2. 4 examples vs 50 examples

Step 2 was scoped as "30–50 small Glyph programs by hand." Only the 4 hard cases were written. The grammar was written against 4. The step-4 plan tests against 50. The dogfooding plan in step 6 doesn't include "write more examples" as a phase.

- **Option A:** Write the missing 26–46 examples before starting step 4. Adds ~3–4 weeks.
- **Option B:** Start step 4 with the 4 we have; let the transpiler corpus grow during week 5–6 of step 4.
- **Option C:** Conflate this with step 6 — write the missing examples as part of dogfooding.

Source: `archive/SESSION_1.md`, `archive/glyph-transpiler-plan.md`. **Must be decided before step 4 starts.**

### Q3. Stdlib minimum bootstrap

The 4 examples import `std/result`, `std/http`, `std/json`, `std/array`, `std/fs`, `std/process`, `std/io`, `std/string`, `std/time`, `react`. None of these exist. Which is the **smallest set** the transpiler can ship against?

`Result`, `Option`, and `par` helpers are in the planned runtime prelude (<200 lines). Everything else is open. Sequencing question: does step 4 ship the rest of stdlib, or does step 6 (dogfooding) drive what gets built?

Source: `archive/glyph-transpiler-plan.md`, `archive/glyph_step6_session.md`. **Should be decided before step 4 starts.**

### Q4. Tree-sitter scaffolding now or v1.1?

The tree-sitter grammar exists in `archive/grammar.js` but has never been run through `tree-sitter generate`. The scaffolding (`package.json`, `binding.gyp`, `src/`, `examples/`) is missing. Step 4 doesn't depend on tree-sitter at runtime (it uses a Rust Pratt parser). But editor syntax-highlighting for the dogfooding loop in step 6 *does* benefit from tree-sitter.

- **Option A:** Finish tree-sitter scaffolding now (~3–5 days) so dogfooding gets highlights.
- **Option B:** Defer to v1.1; dogfood without highlights or with a hand-written VS Code TextMate grammar.

Source: `docs/language/grammar-status.md`.

---

## Blockers for step 5 (typechecker)

### Q5. Compiler architecture: salsa-style queries or dumb visitor?

The step-4 plan says **"dumb AST→TS visitor, no IR."** The step-7 LSP plan says **"compiler must be built around incremental, demand-driven queries (salsa-style) or the LSP timeline collapses."** These positions are incompatible.

- **Option A:** Adopt salsa from day one. Step 4 ships later but step 7 is plausible at 4 weeks.
- **Option B:** Dumb visitor in step 4. Step 7 becomes 8–12 weeks (first weeks are a compiler refactor).
- **Option C:** Hybrid — visitor for codegen, salsa-style facts for typechecking. Probably the right answer but most work.

Source: `archive/glyph-transpiler-plan.md`, `archive/glyph-lsp-discussion.md`. **The single biggest risk to the launch date.**

### Q6. Error-message bar: tsc-quality or Elm-quality?

Two different projects. Affects (a) how much effort step 5 spends on diagnostics, (b) the LSP launch gates in step 7, (c) the `--explain E0042` doc-writing budget.

Source: `archive/glyph_step5_notes.md`.

### Q7. `mut` semantics — type-system feature or syntactic convention?

D5 restricts `mut` to assignments and method calls at the grammar level. But: how does the compiler know `push` mutates and `map` doesn't?

- **Option A:** Every method has a mutation annotation in its signature; typechecker enforces `mut` at call sites. **This is a serious type-system feature not yet in any session log.**
- **Option B:** `mut` is purely syntactic — a convention. **This is a lie the language tells.**

Pick (a) or drop call-site `mut` for methods.

Source: `archive/glyph-day-0-parser.md §Part 2`. Settled at the grammar level (D5) but **not** settled at the typechecker level.

### Q8. `is TypeName` runtime descriptors — what's the model?

Session 1 calls this "the single biggest implementation cost in the language" — every type needs a runtime descriptor available at every call site for the verifiability pillar to hold. What does this descriptor look like? When is it emitted? What's the per-call-site cost?

Source: `archive/SESSION_1.md`.

### Q9. Restricted JSX expression purity classifier

The typechecker rule "only literals, identifier reads, member access, and pure calls inside `{...}`" requires a notion of "pure call."

- **Option A:** Annotation — `pure fn name(args)` or similar marker.
- **Option B:** Inference — compiler walks the call target's body and decides.
- **Option C:** Whitelist — stdlib functions are pure by convention, user functions are not.

Source: `archive/SESSION_1.md`.

---

## Soft questions to settle during dogfooding (step 6)

These don't block the next step but will demand answers within weeks.

- **`T?` sugar over `Option<T>`.** Deferred in session 1; step 6 watchlist predicts "three weeks in, the deferral will be felt." Decide based on real frequency, not first-week feeling.
- **String interpolation.** Currently `+` concatenation only (D12). Day-0 parser file recommends adding `"foo ${bar}"`. Forward-compatible to add later.
- **`mut` on method calls feeling awkward at scale.** Session 1 flagged this. Step 6 will test.
- **Unified vs split `match`** (patterns + `is TypeName` guards). Provisional in session 1.
- **Tuple destructuring for non-reactive returns** (`let a, b = ...`). Never came up in 4 examples.
- **`panic` syntax for unrecoverable errors.** Deferred; not yet in corpus.
- **Async resource primitives.** Library or language feature? Lean library.

---

## Cross-cutting gaps surfaced by the earlier session (archive/glyph-session.md)

These aren't bound to a particular step but the project ships incomplete without each of them.

### Q10. Token-count benchmark track — when does it start?

The manifesto's central empirical claim is that "an agent given the same task produces correct code faster in Glyph than in TypeScript, and the reviewer finishes in half the time." There is **no benchmark plan anywhere in the roadmap.** The closest hit is step 11 ("killer demo, numbers, video, blog post"), which is far too late — by then the language is locked and the numbers can only validate or embarrass.

A pre-current-direction session (`archive/glyph-session.md`) suggested a `benchmarks/` directory with token-count comparisons of equivalent functions across Glyph vs Python vs Rust vs TypeScript, **measurable from day one**. This proposal predates the TS-family direction but the underlying point survives: the empirical claim should be instrumented as soon as the first Glyph→TS output exists, not as a marketing artifact at launch.

- **Option A:** Add a benchmarks track as a cross-cutting concern starting at step 4 (first transpiler output). Track token count, line count, and diff size per example over time.
- **Option B:** Defer to step 11 as currently planned. Risk: the language is locked and the numbers either rationalize or undermine decisions already made.
- **Option C:** Lighter weight — write a one-off benchmark after step 4 ships, before step 5 starts. Sanity-check the claim early without building infrastructure.

**Worth deciding in the brainstorm.** If the claim is load-bearing, the measurement should be too.

### Q11. Testing model — colocated, property-style, or out-of-band?

The current spec (D1–D20) says nothing about how tests work. None of the four hard-case examples include tests. The step-4 plan mentions "~15 negative examples that test 'this code must fail with this specific error'" but treats that as a CI suite for the compiler, not a language-level testing story.

Two abandoned-direction sketches in `archive/` both put tests inside the function declaration, but with different syntax:

- **`glyph-session.md`** — `@test` blocks colocated with the function, named with intent strings ("no-op when within tolerance"), property-style assertions (`expect.plan.sells.all(.tax_cost < .alternative_lot_tax_cost)`) asserting relationships not magic numbers.
- **`glyph-annotation-sketch.md`** — sharper version: `@example slugify("Hello, World!") == "hello-world"` as one-line compile-checked tests, plus `@property forall s: String . slugify(s).matches(/^[a-z0-9-]*$/)` as first-class property-based tests verified at compile time. The framing: "the agent can rewrite the body and the compiler will verify all @examples still pass without the agent ever running a separate test harness."

Both also flag **test/invariant unification** — invariants run at runtime, tests in CI. Should there be one verification model?

Concrete options now:
- **Option A:** Inline compile-checked examples (a la `@example`) as the only built-in testing primitive. Property tests live in a stdlib `test.property(...)` function.
- **Option B:** Full sketch direction — `@example` + `@property` as first-class language constructs, both verified by the compiler.
- **Option C:** Tests are entirely a stdlib concern; the language stays out of it (current de facto position).
- **Option D:** Tests are out-of-band (`*_test.glyph` files), no inline tests.

Whichever direction is chosen, the "agent rewrites body, compiler verifies tests still pass" workflow is the load-bearing argument — it's a tighter feedback loop than any TS testing setup.

**Pt3 adds a sharper version of the argument:** machine-readable counterexamples. If property tests are first-class and the compiler runs them on every build with shrinking, the failure output looks like:

```
FAIL parse_csv_line @property[1]  counterexample: `"a"",b`  shrunk_from: 18 chars
```

The agent reads the input that broke the property — no stack-trace archaeology, no "the test is flaky," no human translation. That's the verifiability pillar's strongest agent-facing payoff in any of the three sketches. Worth weighing in the brainstorm: is the compile-time PBT loop achievable on a transpiler that just emits TS?

Pt3 also adds **`@fuzz` corpora**:
```glyph
@fuzz corpus: "data/csv/rfc4180-corpus" runs: 100_000 shrink: true
@fuzz adversarial: utf8.malformed | utf8.mixed_endian | length.huge
```

Real fuzzers, not just shrunk property tests. Adversarial corpus categories are first-class (UTF-8 malformations, length attacks). This is heavier weight — probably v2 — but the *direction* (the build is also the fuzz run) deserves an explicit position.

### Q12. Discoverability — how does an agent know what's importable?

D15 covers import *syntax* (three forms, no barrel files, no re-exports, no relative imports). It does not cover how an agent learns *what exists to import*. In TypeScript this is partly solved by IDE indexing of `node_modules` and partly by convention. Glyph's greppability pillar implies a sharper answer is possible: if every symbol has exactly one syntactic form at its declaration site, the index is trivial to build — but who builds it, when, and how does an agent query it?

The earlier session flagged this as RFC-worthy. Currently unanswered.

- **Option A:** LSP responsibility — discovery is workspace-indexing in step 7.
- **Option B:** A dedicated `glyph index` command that emits a structured manifest of all module exports.
- **Option C:** Encode the answer in the formatter/import-sorter at step 8 — imports already canonical, the act of writing an unknown import is what surfaces missing modules.

## Cross-cutting gaps surfaced by archive/glyph-annotation-sketch.md

This second abandoned-direction sketch contains four ideas that current Glyph has not committed to, three of which sharpen existing pillars. Worth surfacing as brainstorm material.

### Q13. Stable IDs (`@gid`, `@fid`) for rename-cascade-free refactors

The boldest idea in the sketch. Every function carries a globally unique, immutable identifier (`@gid:fn.auth.verify_token.v3`). Every record field carries one (`@fid:001`). Callers can reference symbols by either textual name *or* `@gid`. An agent renaming `verify_token` → `validate_token` doesn't cascade through call sites because the IDs are invariant. Field updates use field IDs (`p with { @fid:007 = true }`) so position changes in the record don't break callers. Retired field numbers are never reused.

Current Glyph's diff-stability pillar tackles cascade *within a file* (fixed-width formatting, single-element-per-line). Stable IDs would tackle cascade *across files*, which is a structurally different — and stronger — claim. This is closer to Protocol Buffers' field numbering than to anything TS-family.

- **Option A:** Adopt stable IDs as a first-class language feature. Big implementation tax: every symbol gets one, the compiler maintains an ID-to-name index, the formatter renders names but the AST is keyed by IDs. Diff-stability pillar becomes structurally enforced rather than formatter-enforced.
- **Option B:** Opt-in stable IDs for public APIs only (`@gid` is a decorator on exported declarations, not internal ones). Lower cost, smaller benefit.
- **Option C:** Reject. The "looks almost like TypeScript" stance survives. Cascade is a real problem but lived with via the LSP's rename refactor (step 7). Accept that the LSP carries the load.
- **Option D:** Defer to a v2 syntax extension once the textual-name version of Glyph ships. Forward-compatibility: `@gid:` is currently illegal syntax, so adding it later doesn't break existing parses.

This question deserves a real conversation in the brainstorm. **The single most novel idea Glyph has access to**, and the easiest to dismiss for the wrong reasons.

### Q14. Design-by-contract (`requires` / `ensures` clauses)

The sketch puts pre/post-conditions in the function signature as compiler-checked proof obligations:

```glyph
fn safe_divide(numerator: Int, denominator: Int) -> Int | DivByZero
    requires denominator != 0 or returns DivByZero
    ensures  result is Int implies result * denominator <= numerator
```

Current Glyph has Result types and runtime descriptors (verifiability pillar) but no contract clauses. The verifiability pillar pushes toward this direction; the manifesto explicitly stops short. The sketch's framing: "the compiler emits proof obligations; the AI agent reads them as feedback" — agents can verify their edits against the contracts without running anything.

- **Option A:** Adopt as a first-class verification mechanism. Major implementation cost (SMT or symbolic execution backend), real verifiability win.
- **Option B:** Adopt as runtime assertions only, not statically verified. Lighter weight; less of a verifiability claim.
- **Option C:** Reject in favor of the existing Result<T, E> + runtime-descriptors stance. Contracts are a v2 conversation.

### Q15. Refinement types — `where` clauses, angle brackets, or nominal newtypes?

The first sketch used angle-bracket annotations: `String<jwt>`, `String<1..64>`. Pt3 uses a **much cleaner shape** — a `where` clause attached to a type alias:

```glyph
type Email       = String where matches(/^[^@\s]+@[^@\s]+\.[^@\s]+$/)
type Percentage  = Float  where 0.0 <= self <= 100.0
type NonEmpty<T> = List<T> where self.length > 0
type Port        = Int    where 1 <= self <= 65535
```

The compiler proves the refinement at call sites. Passing `[]` where `NonEmpty<T>` is expected is a compile error, not a runtime check. Defensive `if recipients.empty()` boilerplate disappears.

The `where`-clause shape sidesteps D7 entirely (no overload of `<>`) and reads naturally with current Glyph's `type X = ...` alias form.

- **Option A:** Adopt `type X = Y where condition` refinement types. The verifiability pillar's strongest available expression. Major implementation cost (the typechecker must prove refinements at call sites — SMT or symbolic execution).
- **Option B:** Achieve the same outcome via nominal newtypes — `type Jwt = String` with `Jwt.parse(s)` returning `Result<Jwt, ParseError>`. Current `record` semantics already cover this minus the inline-predicate sugar. *Lower-cost, weaker.*
- **Option C:** Angle-bracket refinements (`String<jwt>`). Collides with D7 and requires context disambiguation. **Worse than Option A.**
- **Option D:** Reject. The verifiability pillar is served by runtime descriptors at I/O boundaries; refinement types are a v2 conversation.

Option A is the winning shape if any refinement story ships. The brainstorm should decide whether v1 ships A or B.

### Q16. Versioned signatures (`.v3`, `.v4` in `@gid`)

Lower priority. The sketch encodes signature versions in the `@gid` itself, so callers can pin a specific version of a function. This is forward-compatibility-as-a-language-feature. Probably out of scope for v1 even if Q13 (stable IDs) is accepted. Note: only meaningful if Q13 lands.

## Cross-cutting gaps surfaced by archive/glyph-annotation-sketch-pt2.md

The continuation sketch (examples 6–10) adds five more ideas. Two of these reveal **real gaps in the current spec**, not just rejected-direction syntax.

### Q17. Capability-based effects vs effect annotations vs nothing

The current manifesto explicitly rejects effect systems ("no effect systems, no dependent types, no linear types"). But the sketch demonstrates a different shape: **capabilities as values**, passed as parameters with a `use stripe: cap:stripe.charges.write` clause. The compiler tracks capability flow transitively. No ambient authority — a function cannot accidentally hit the network because the capability must be passed in.

This is closer to Effekt or Wyvern than to Haskell's `IO`. It buys the verifiability win (agent-auditable: `grep "cap:network"` finds every network-touching function) without an effect lattice or row polymorphism.

- **Option A:** Adopt capability tokens as values. Real verifiability win for agent code review. Big stdlib redesign — every I/O function takes a capability parameter.
- **Option B:** Reject and rely on the existing `Result<T, E>` + module boundary approach. The manifesto's stance survives.
- **Option C:** Lightweight version — capability declarations are documentation-only (not type-system-enforced). Doesn't help the verifiability pillar at all; probably not worth the syntax cost.

The manifesto's "no effects" was written against effect *annotations*, not capability *values*. The distinction matters and deserves a brainstorm-level reconsideration.

**Pt3 extends the capability argument to non-determinism sources** — time, randomness, IO. A function takes `use clock: cap:time.read` rather than calling an ambient `Date.now()`. The payoff:

- **Tests pass `cap:time.read.fake(at: "2026-01-01T00:00:00Z")`** — flaky time-dependent tests become impossible by construction.
- **Production traces can replay bit-exact** via `cap:time.read.from_trace(trace_id)`.
- The `@deterministic given (clock, entries)` annotation declares same-inputs-same-outputs when capabilities are pinned.

The non-determinism case is arguably stronger than the I/O case. "Agents write flaky time-dependent tests" is a real, measurable problem. If capabilities ship at all, time/randomness/IO are first-class candidates alongside network/DB.

### Q18. Structured concurrency primitive

Current Glyph has `async fn`, `await`, and the stdlib helpers `par.all` and `par.all_ok` (per `archive/SESSION_1.md`). It does **not** have a `parallel { ... }` block — a structured-concurrency primitive where nothing escapes, errors propagate, and cancellation is automatic. The sketch's example:

```glyph
parallel {
    let profile  = api.fetch_profile(user_id)
    let orders   = api.fetch_orders(user_id, last: 30d)
} on_error (e) => return Dashboard.degraded(e)
  on_timeout   => return Dashboard.partial(...)
```

The agent-safety claim: there is no way to fire-and-forget. No dangling tasks, no race conditions, no `Promise.resolve().then(() => {})` orphans. This is the Trio/Anyio/Kotlin-coroutineScope approach.

- **Option A:** Adopt `parallel { }` as a language construct (not a stdlib function). Couples it with `@timeout` / `@cancellation` annotations from the sketch.
- **Option B:** Keep `par.all` / `par.all_ok` as stdlib only. The dogfooding loop (step 6) will tell us whether stdlib helpers are enough.
- **Option C:** Adopt structured concurrency but as stdlib functions with strong type signatures, not language syntax. `nursery.run(|| { ... })` returns only when all children resolve.

### Q19. Errors-as-runbooks (errors carry remediation data)

Current Glyph's tagged-union errors describe *what* went wrong (`type FetchError = | Timeout | NotFound | InvalidPayload({ field: String })`). The sketch's `RateLimitError` carries *what to do about it*: a `remediation: Remediation` field with structured action data (`Backoff { jitter: true }`, `RefreshToken { endpoint }`, `FixPayload { hints }`).

The agent reads the remediation and acts on it programmatically. No string parsing of error messages, no stack-trace archaeology — the error encodes the runbook.

This is a real verifiability + agent-actionability win and current Glyph is silent on it. Not a syntactic change — just a stdlib convention: every error type carries a `remediation` field of a sum type encoding the recovery action.

- **Option A:** Adopt as a stdlib convention. All stdlib errors have a `remediation` field. User code is encouraged to follow.
- **Option B:** Adopt as a language-level expectation — every `type` declared as an error variant *must* carry a remediation field. Stronger; more rigid.
- **Option C:** Treat as a v2 stdlib refinement. Doesn't block step 4.

### Q20. Loop construct — genuine spec gap

The current spec (D1–D20) has **no `loop`, `for`, or `while`**. The four hard-case example files contain no loops. This was missed in earlier reviews because the four-file corpus didn't surface it.

The sketch uses `loop attempt in 1..=3 { ... continue ... }` for retry logic. Real production code needs *something* — recursion is one answer but it's the worst answer for agent-readable code at line-count.

- **Option A:** `for x in iter { ... }` only — explicit iteration, no general loop. Idiomatic in Rust/Swift; pattern-matches well with the rest of Glyph.
- **Option B:** `loop { ... }` with `break` / `continue` only — general loop, no `for` sugar. Forces explicit termination conditions but verbose for simple iteration.
- **Option C:** Both — `for x in iter { }` for iteration, `loop { }` for unbounded retry/server loops.
- **Option D:** Functional only — `iter.fold(...)`, `iter.for_each(...)`. No statement-level loops. Forces map/reduce idiom even when iteration is the natural shape.

This is **not a "settled but worth re-litigating" item**. It is unsettled. **This must be decided before any meaningful step-4 transpiler work.**

### Q21. First-class migrations / type evolution

The sketch's `@migrates_from type.order.v4` + compiler-generated forward and reverse migrations with `@verify roundtrip: ...` clauses. The compiler emits the migration code, agents don't invent it.

Current Glyph has nothing on type evolution. The step-6 dogfooding plan (fridge shopping list, **JSON on disk**) will hit this in week one: "I added `category: Category?` to `ShoppingItem`; how do old `shopping-list.json` files load?" Today the answer is "schema parser does it implicitly via defaults" which is fragile and silent on rollback.

- **Option A:** Adopt `@migrates_from` style migrations as a first-class language feature. The compiler generates and verifies migration functions. Real verifiability win for any persisted data.
- **Option B:** Stdlib pattern — `migrate.from<OldT, NewT>(fn) { ... }` returns a migration object. Less syntactic weight.
- **Option C:** Treat as out of scope for v1 — defaults-in-the-parser is enough for the shopping list. Revisit when persistence becomes serious.

Strongly worth deciding before step 6 starts, because the shopping list will need *some* answer.

### Lower-priority items from the sketch (worth a note, not a question)

- **`@total`** annotation on functions: current Glyph's match exhaustiveness is *always* checked (per the step-4 plan using Maranget). The annotation would be gratuitous unless there's a non-total mode planned.
- **`sealed` modifier** on tagged unions: current Glyph's unions (D8) are already implicitly sealed. The keyword is meaningful only if open variants are ever added — currently they're not.
- **`@timeout` / `@cancellation`** annotations: meaningful only inside Q18's structured-concurrency answer. Bundle with Q18.
- **`@verify` clauses inside function bodies**: a subset of Q14 design-by-contract. Track under Q14.
- **`@idempotent on idempotency_key`**: likely a stdlib pattern, not language syntax. Worth a small spec note that this is the stdlib's responsibility, not the language's.

## Cross-cutting gaps surfaced by archive/glyph-annotation-sketch-pt3.md

The third sketch (examples 11–15) adds two genuinely new dimensions current Glyph has not touched, plus sharpens Q11, Q15, Q17 with stronger framings (already incorporated above).

### Q22. Content-addressed imports / dependency trust

D15 covers import syntax (three forms, full-path, no relative). It is **silent on dependency trust**. Pt3 proposes:

```glyph
@import http     from glyph:std/http       @hash:blake3:7f3a...e2c1 @audit:stdlib
@import stripe   from vendor:stripe/sdk    @hash:blake3:5e21...d3a8 @audit:third-party @last_reviewed:2026-04-02
```

Dependencies are pinned by cryptographic hash of the source AST, not by version strings. An agent can verify the entire dependency graph in one pass. Supply-chain attacks via typosquatting (`react` vs `reakt` vs `react-dom-utils`) are structurally prevented. Audit markers (`@audit:stdlib`, `@audit:internal`, `@audit:third-party`) plus `@last_reviewed:DATE` make stale-review state grep-able.

This is a security and verifiability feature, not just sugar. It dovetails with Glyph's stance of "we ship to npm" — npm is the supply-chain attack surface the manifesto inherits but does not address.

- **Option A:** Adopt as a first-class language feature. The `glyph` CLI (step 8 package story) computes and pins hashes; deviations fail the build.
- **Option B:** Stay in npm's lane — `package.json` already supports `integrity:` hashes via `package-lock.json`. Add audit markers as `glyph`-key metadata inside `package.json` (per `08-09-packaging.md`), not as import-level annotations.
- **Option C:** Reject for v1. Glyph imports compile to TS imports; npm and pnpm already do hash pinning. Don't re-invent.

**Option B is probably the right answer** if the `"glyph"` key in `package.json` decision (item 8) stands — it composes cleanly with npm's existing integrity hashing. But the *audit marker* dimension and the *AST-hash vs file-hash* distinction are real and deserve a brainstorm position.

### Q23. First-class observability declarations + PII redaction

Pt3 declares tracing, metrics, logging, and PII redaction in the function signature:

```glyph
@trace   span: "checkout.complete_order"
@metrics counter: "orders.completed" by status
@metrics histogram: "orders.completion_ms" buckets: [10, 50, 100, 500, 1000, 5000]
@log     level: info  on success
@log     level: error on failure with stack: false
@redact  fields: [card.number, card.cvv, customer.email]
```

The runtime emits the corresponding telemetry automatically. Agents cannot "forget to add a span." Stack traces are opt-in (sketch's note: "stack traces are useless to agents"). PII redaction is enforced at the type level — `card.number` is redacted before any log/metric serialization.

This is a new dimension. Current Glyph has nothing on observability. Production code in any language above toy-size has it. Whether it's *language*-level or *stdlib*-level is the brainstorm question.

- **Option A:** First-class language annotations as in the sketch (`@trace`/`@metrics`/`@log`/`@redact`). Heavy syntactic weight; biggest agent-safety payoff. Requires the transpiler to emit telemetry code at call sites.
- **Option B:** Stdlib decorators or function wrappers — `trace.span("name", || { ... })`, `redact.field<"card.number">(...)`. Less syntactic weight; agents can still forget to wrap.
- **Option C:** Type-level redaction only — `type Pii<T> = T where ...` enforces redaction at serialization boundaries; tracing/metrics live in the stdlib. Splits the question cleanly.
- **Option D:** Defer entirely to v2. Production observability is real but solvable later.

**`@redact` is the easiest to defend as language-level** because it's the security-critical piece. The tracing/metrics annotations have weaker pillar arguments — they're convenience over correctness.

## Cross-cutting gaps surfaced by archive/glyph-annotation-sketch-pt4.md

The fourth sketch (examples 16–20) adds four dimensions, one of which directly contradicts the manifesto's rejection of linear types.

### Q24. `owned` / linear resources — narrow exception to the no-linear-types rule?

The manifesto explicitly says: *"No effects, no dependent types, **no linear types**, no macros."* The rejection was about complexity (Haskell-style affine + linear lattice). But the sketch shows a **narrow** linear-types story scoped to resource handles only:

```glyph
let owned handle: FileHandle = fs.open(path) ?? return propagate
let owned upload: S3Upload   = s3.start_multipart(bucket, path.basename) ?? return propagate
// `owned` values MUST be consumed exactly once on every path.
// Forgetting `.close()` = compile error. Double-close = compile error.
```

Do TS developers wish they had this? **Yes** — see TC39 `using` (stage 3) for explicit resource management. The pain point is real (forgotten `.close()`, `try/finally` boilerplate, leaked DB connections), and current Glyph offers nothing structural.

- **Option A:** Adopt `owned` as a narrow modifier for resource handles. Compiler tracks single-consumption on every path. Verifiability pillar wins; the manifesto's anti-linear-types stance is honored in spirit because we don't ship a general affine system.
- **Option B:** Use a stdlib `with(handle, fn (h) => { ... })` pattern, like TS's `using` desugar. Lighter weight; agents can still forget.
- **Option C:** Reject. Leaks are a known pain but the manifesto is explicit. v2 conversation.

This is a manifesto-touching question. **Worth the brainstorm to decide whether the "no linear types" rule has a carve-out.**

### Q25. Compiler-enforced semantic versioning

Pt4 proposes the toolchain compute the public API surface from the AST and diff it against the registry on `glyph publish`:

- Body-only edit → patch.
- Optional defaulted parameter added → minor.
- Field removed, return type widened, required parameter added → major.

```glyph
@semver_change patch since 2.4.0    // body-only edit
@semver_change minor since 2.4.0    // additive only
@semver_change major since 2.4.0    // breaking; @breaking_since 3.0.0
```

Implementation cost: moderate (AST diff is mechanical). Verifiability win: real (agents cannot ship "patch" upgrades that silently break downstream).

Composes with Q22 (content-addressed imports) — together they form a story where the supply chain is hash-pinned *and* the version bumps are honest.

- **Option A:** First-class language feature with `@public_surface auto` and `glyph publish` enforcement.
- **Option B:** Lint, not enforcement. The toolchain warns; the developer can override.
- **Option C:** Defer to v2. Step 8's packaging story (npm with `"glyph"` key) doesn't ship a registry, so there's nothing to publish *to* yet.

Probably v2 unless Glyph commits to a registry. But worth holding the position now.

### Q26. Complexity annotations — v2+ research territory

Pt4 proposes:
```glyph
@complexity time: O(n log k)
@complexity space: O(k)
@allocations bounded: k + 1
@worst_case latency: 50ms per 1M elements on ref-hardware
```

The compiler *verifies* these statically. **This is open research.** General Big-O inference is unsolved; nontrivial allocation accounting and worst-case latency analysis require WCET-style hardware modeling.

- **Option A:** Reject for the foreseeable future. v2+ at earliest.
- **Option B:** Annotations are documentation-only — the compiler doesn't verify but the LSP can flag obvious violations (e.g., `.sort()` inside a loop in an `O(n)`-declared function).
- **Option C:** Narrow verification — only check that "no nested loop" / "no recursion" patterns match the declared complexity. Heuristic, not proof.

The right answer is almost certainly Option A or B. Document the position so this doesn't keep reappearing.

### Q27. Bidirectional functions (`bifn`)

Pt4 proposes a single declaration that generates both encode and decode, with a compiler-checked round-trip:

```glyph
bifn iso8601 :: String<iso8601> <-> Timestamp {
    forward (s) -> Timestamp { ... }
    inverse (t) -> String<iso8601> { ... }
    @property forall t. iso8601.forward(iso8601.inverse(t)) == t
    @property forall s. iso8601.inverse(iso8601.forward(s)) == s.canonical()
}
```

Drift between parse/print, encode/decode, serialize/deserialize pairs is a real problem. Agents write the pair and they drift apart over edits. `bifn` ties them at the declaration site and compiler-verifies the round-trip property.

Implementation cost: the compiler doesn't have to *derive* both directions — it just verifies the round-trip property holds. That's a property-based test executed at compile time, conceptually the same machinery as Q11's `@property`.

- **Option A:** Adopt `bifn` as a language construct. Implementation cost is mostly the round-trip verification, which is property-test infrastructure (Q11 already requires it).
- **Option B:** Stdlib pattern — `Bifn.new(forward, inverse, property)` returns an object that bundles both. No compiler verification; tests run in the test pass.
- **Option C:** Reject. Pairs of fn declarations are fine; tests catch drift.

This deserves real brainstorm consideration — it's a narrow feature with a strong agent-correctness story.

### Strengthening D9 — no `_ => ...` catch-all on sealed unions

Not a full open question; an extension to an existing decision.

Current Glyph (D9): `else` is the catch-all *arm* in `match`; `_` is a position-level wildcard. Tagged unions (D8) are implicitly sealed.

Pt4 adds the rule: **`_ => ...` and `else =>` are illegal when matching on a sealed union.** Adding a variant to `TrafficLight` forces every match on `TrafficLight` to be updated; the compiler points to each exact line. With a catch-all, the new variant silently falls through and the bug surfaces at runtime.

This sharpens the greppability + verifiability pillars at the cost of a small ergonomic loss (you sometimes *want* a catch-all). Worth folding into the spec as a clarification of D9, or rejecting explicitly so re-litigation doesn't loop.

## Cross-cutting gaps surfaced by archive/glyph-annotation-sketch-pt5.md

The fifth sketch (examples 21–25) adds five more dimensions. Q32 (dual human/agent file representation) is **the most architecturally radical idea across all five sketches** — it proposes an entirely different theory of diff stability than current Glyph's fixed-width formatter.

### Q28. Typestate (state machines as types)

The state of a value is part of its type. `HttpRequest<Draft>`, `HttpRequest<Built>`, `HttpRequest<Sent>`, `HttpRequest<Received>` are distinct types; methods are scoped to specific states. Calling `.send()` on a `Draft` is a compile error.

```glyph
typestate HttpRequest {
    state Draft     -> { Built }     on .build()
    state Built     -> { Sent }      on .send()
    state Sent      -> { Received }  on .await_response()
    state Received  -> terminal
}
```

Related to Q24 (linear/owned) but more general — tracks protocol position, not consumption. Real protocol-correctness win for HTTP, gRPC, OAuth flows, payment state machines. Rust achieves this via type-parameter conventions; Glyph could ship native syntax.

- **Option A:** First-class `typestate` declaration. The typechecker tracks state transitions.
- **Option B:** Achieve the same outcome via generics — `HttpRequest<S extends RequestState>` with each state as a marker type. No new keyword; relies on the typechecker's narrowing.
- **Option C:** Reject. Protocol bugs are rare relative to other issues. v2 conversation.

### Q29. Structured edit API — agents emit edits, not text

Pt5's most workflow-relevant idea. Agents don't patch source by string manipulation. They emit `@edit` blocks that the compiler applies atomically:

```glyph
edit {
    add_field @fid:009 timezone: String<iana_tz> = "UTC"
        after @fid:008
        with_migration auto
}
@verify {
    compiles
    all_tests_pass
    @semver_change minor
    no_callers_broken
}
```

If any clause in `@verify` fails, the edit is rejected as a single unit with structured rejection data (`failed: "all_tests_pass", counterexamples: [...]`). The agent receives actionable feedback, not a diff to debug. **"The agent broke the file" becomes structurally impossible.**

This is more a tooling feature than a language feature — but it composes deeply with Q13 (stable IDs, since edits target `@gid`/`@fid` not text positions) and with the LSP (step 7). If the LSP exposes an `applyEdit` operation that returns `{ ok, rejected, reason }`, agents work through that interface rather than the filesystem.

- **Option A:** First-class language construct (`edit { ... } @verify { ... }`) with toolchain enforcement.
- **Option B:** LSP-only feature — `glyph` ships a structured-edit RPC that the LSP exposes; not a language construct. Composes with step 7.
- **Option C:** Defer to step 6 dogfooding — let the shopping list app's "agent rewrites a function" experience drive the API shape.

**Option B is probably the right v1 shape** — it doesn't bloat the language and it gives every agent (Claude Code, Copilot, others) a uniform interface. Worth deciding before step 7 scoping.

### Q30. Replayable traces as regression tests

`@replayable` marks a function as deterministic given (inputs, capabilities). Production traces become regression tests by reconstructing the exact capability state:

```glyph
@gid:test.pricing.regression_2026_05_14.v1
@source  trace://prod/2026-05-14T09:12:44Z/t_8a3f02b9e91
@expect  Quote { amount: Money.eur(149.99), valid_until: "2026-05-21" }
test replay(quote) from trace
```

The test re-runs the exact db reads, exact clock, exact inputs from production. Bit-exact reproduction. "Cannot reproduce" stops being an excuse.

This depends on Q17 (capabilities for non-determinism, especially time/randomness) and Q23 (first-class observability). It's the integration product — once those land, this is mostly stdlib + tooling, not new language syntax.

- **Option A:** First-class language construct (`test replay(fn) from trace`) that requires `@replayable` and capability-pinned dependencies.
- **Option B:** Stdlib pattern — `Trace.replay(trace_id, fn)` returns the expected output for comparison. No new syntax.
- **Option C:** Defer to v2 — solid story but requires Q17 and Q23 to ship first.

### Q31. Executable documentation

`@doc """ ... ```glyph @run ... ``` """` blocks are compiled and executed on every build. Failed asserts fail the build. Same machinery as Q11 (testing) but co-located with documentation.

Rust has doctests; this is a tighter framing because the assertions are integrated with the compiler's exhaustive build pass, not a separate test runner. Sharpens Q11 considerably — docs *are* tests, tests *are* docs.

- **Option A:** First-class `@doc` with `@run` blocks. Probably the right answer if any inline-tests story ships (cf. Q11 Option A/B).
- **Option B:** Doc-test framework as stdlib (`doc.run("example", || { ... })`). Less integrated.
- **Option C:** Reject. README examples can be a separate test target. v1 isn't shipping a doc generator anyway.

If Q11 lands Option A or B (inline `@example`), Option A here is nearly free — same compile-time-execution machinery.

### Q32. Dual human/agent file representation — the most radical idea

**The single most architecturally novel idea in any of the five sketches.** Source files have two synchronized views:

- **Human view:** idiomatic, readable, formatted for skim.
- **Agent view:** canonical, line-stable, fully-qualified, SSA-like (`L001 return`, `L002 $0 = s`, `L003 $1 = pipe(...)`).

Agents edit the agent view. Humans review the human view. The compiler enforces semantic equivalence on every save. Stable line numbers and stable token names in the agent view mean diffs in agent-edited code are minimal and unambiguous — no formatter churn, no "AI reformatted my file" arguments.

This is an **alternative theory of diff stability**. Current Glyph's answer is "fixed-width formatter, single-element-per-line, no line-length reflow." Pt5's answer is "bifurcate the representation: humans get readable text, agents get a canonical IR-like form, the compiler reconciles." The two approaches solve the same problem differently.

- **Option A:** Adopt dual-view as a first-class feature. Massive implementation investment (parser, formatter, and the synchronization invariant). Diff-stability pillar shifts from formatter-enforced to representation-enforced.
- **Option B:** Reject and keep the current fixed-width-formatter approach. The current pillar implementation is enough; bifurcating adds complexity for marginal gain.
- **Option C:** Hybrid — the LSP shows an "agent view" virtual document on top of a normal source file. The on-disk file is still the human view; agents query the LSP for the canonical form when needed. **Probably the realistic shape if this lands at all.**
- **Option D:** Defer to v2 once the fixed-width-formatter approach has been used at scale. If churn is acceptable, this isn't needed; if not, this becomes obviously valuable.

This deserves explicit brainstorm consideration — it's not a feature to bolt on, it's a fork in the road. The current Glyph manifesto and `glyph-strategy.md` are silent on this alternative because it was conceived after they were written. If dual-view is the right answer, several decisions in `docs/language/spec.md` (D2 trailing commas, D8 union punctuation, D17 trailing commas everywhere) become less load-bearing because the agent view erases formatting choices entirely.

## Cross-cutting gaps surfaced by archive/glyph-annotation-sketch-pt6.md

The sixth sketch (examples 26–35) is the biggest batch — ten new dimensions. Five are real production-correctness concerns; one is the most aggressively agent-native idea across all six sketches. Grouped below by impact, not file order.

### Q33. Provenance / taint tracking — SQL injection as compile error

Information-flow types. `String<tainted:user>` cannot reach `String<trusted:sql>` sinks without passing through a declared sanitizer. The taint flows transitively through assignments, pipelines, and function calls — agents cannot launder it by passing through a helper. SQL injection, XSS, command injection become compile errors.

Real verifiability win. Composes with Q15 (refinement types via `where` clauses) — taint is a refinement category. Existing research: Jif, FlowCaml, JFlow.

- **Option A:** Adopt as a narrow refinement-type extension targeting common sinks (SQL, shell, eval, HTML output). Stdlib defines the taint categories and sanitizers; the typechecker enforces flow.
- **Option B:** Stdlib pattern only — `Tainted<T>` newtype with a `sanitize(T, Sanitizer) -> Trusted<T>` discipline. No flow analysis; relies on developers reading types.
- **Option C:** Reject for v1. Most agents won't write SQL-injectable code if Glyph's stdlib provides only parameterized query APIs.

**Option A is the strongest verifiability-pillar argument among Q24/Q33/Q39** — it solves a real, named, common class of production bugs.

### Q34. Budgeted execution — runaway LLM bills as compile-time concern

```glyph
@budget     wall_time:    30s
@budget     llm_tokens:   8_000
@budget     llm_calls:    4
@budget     usd_cost:     0.25
@on_exceed  return Summary.partial(_)
```

Runtime enforces; compiler refuses bodies that can't be proven to fit. **This is uniquely Glyph's pitch territory** — current TS doesn't have anything like this and TS developers building AI orchestration systems wish they did.

Composes with Q26 (complexity) — same shape, but practical (runtime ceilings) rather than theoretical (Big-O proofs). Far more achievable than Q26.

- **Option A:** First-class language annotations enforced at the runtime boundary. The transpiler emits a wrapper that tracks resource use.
- **Option B:** Stdlib decorators — `withBudget({wallTime: 30s, llmTokens: 8000}, || { ... })`. Less syntactically integrated.
- **Option C:** Defer to v2.

**Strongest agent-native argument** for any pt6 idea. AI tooling builders feel this pain daily.

### Q35. Feature flags as language primitives

Typed, scoped, sunset-dated flags. `@sunset 2026-08-01` (build fails after this date), `@rollout percentage(20%) | allowlist(beta_users)`. Dead flags fail the build. Conflicting flags fail the build.

Real production pain — but is it the *language's* problem?

- **Option A:** First-class language primitive (`@flag:X`, `when @flag:X.enabled(...) { ... }`).
- **Option B:** Stdlib pattern, integrated with the `"glyph"` key in `package.json` for flag metadata. Sunset enforcement is a CI lint, not a compile error.
- **Option C:** Reject. Feature flags are an infrastructure concern; LaunchDarkly et al exist.

**Option B is probably right.** The compile-error-on-sunset idea is good but doesn't justify language-level syntax.

### Q36. Domain units as types — `Money<USD>`, `Quantity<m/s>`, `Duration<ms>`

```glyph
fn kinetic_energy(mass: Quantity<kg>, velocity: Quantity<m/s>) -> Quantity<J>
fn convert_charge(amount: Money<USD>, rate: ExchangeRate<USD, EUR>) -> Money<EUR>
fn deadline(received_at: Timestamp, sla: Duration<ms>) -> Timestamp
```

`Meters + Seconds` = compile error. `Money<USD> + Money<EUR>` = compile error. The Mars Climate Orbiter, every cents-vs-dollars bug, every ms-vs-s bug — all gone at the type level.

F# has units of measure. Frink does this. Real correctness win for finance, physics, scheduling code.

- **Option A:** First-class units of measure with dimensional inference. Major typechecker work.
- **Option B:** Achieve via Q15 refinement types — `type Meters = Float where unit == :m`. Less ergonomic (no automatic unit inference on `mass * velocity^2`) but cheaper to implement.
- **Option C:** Stdlib only — `Money.usd(x)`, `Duration.ms(x)` constructors with method-level conversion. Most TS libraries already work this way.
- **Option D:** Reject — defer to v2.

The brainstorm question: is unit safety a v1 differentiator or a v2 polish?

### Q37. Content-preserving refactors — extension of Q29

`@refactor:rename_param.v1` blocks the compiler verifies preserve behavior. `update_all_callers automatic`, `@verify { behavior_equivalent_to_pre_edit }`. Declarative refactoring with proof obligations.

Composes with Q29 (structured edit API). If Q29 lands as the LSP/tooling layer, Q37 is a category of edit within Q29 — not a separate question. **Track as an extension of Q29 scope**, not a standalone language feature.

### Q38. Differential typing across versions — extension of Q25

```glyph
@delta_from v3 {
    behavior:  "ties now broken by recency"
    surface:   unchanged
}
@verify_delta { @property forall q, items . rank.v4(q, items).top(1) == rank.v3(q, items).top(1) unless ties_in_top_1(q, items) }
```

Reviewers read the `@delta_from` clause; the compiler verifies it. Composes with Q25 (compiler-enforced semver) — `@delta_from` is the per-function refinement of which Q25's module-level semver is the aggregate.

**Track as an extension of Q25 scope.** If Q25 lands, the per-function delta annotation comes nearly free.

### Q39. Policy-as-types — HIPAA / GDPR / SOC2 at compile time

```glyph
@classification PHI
@residency      [region:eu, region:uk]
@retention      max: 7.years
@access         requires(role:clinician) and purpose(:treatment | :billing)
type MedicalRecord { ... }
```

Routing PII to the wrong region = compile error. Returning data to an unauthorized caller = compile error. Auditors get a machine-readable policy graph.

This is **enterprise-feature territory** — real verifiability win, but the audience is narrower than Glyph's core TS-developers pitch.

- **Option A:** First-class language primitive. Big typechecker investment; opens the enterprise door.
- **Option B:** Stdlib + typechecker plugin model — policies are user-defined refinement categories layered on Q15.
- **Option C:** Reject for v1. Defer until enterprise customers ask for it.

**Probably Option C for v1**, but worth holding the position so it isn't reinvented inconsistently across stdlib code.

### Q40. Incremental type-driven generation — the most agent-native idea

```glyph
@generate    by: agent:claude
             prompt: "Detect language using n-gram frequency; no external IO"
@example     detect_language("the quick brown fox") == "en"
@property    forall s . detect_language(s).matches(/^[a-z]{2}$/)
@budget      latency: 5ms, memory: 1.MiB
fn detect_language(text: String) -> String<iso639_1> {
    @body generated
    ...
}
```

The body is **owned by the generator**, not by humans. To improve the implementation, you edit the `@intent` / `@example` / `@property` block — not the body. Regenerating preserves the contract by construction.

This **inverts the human/agent collaboration model**: humans (or LLMs) own contracts; LLMs own implementations. The most aggressive expression of Glyph's "agents as first-class collaborators" thesis in any sketch.

- **Option A:** First-class language feature with toolchain integration. The transpiler invokes a configurable generator; CI regenerates and verifies.
- **Option B:** Stdlib + external tool — `@generate` is a documentation annotation; a separate `glyph regen` command regenerates bodies. Much lighter.
- **Option C:** Reject. The body should always be human-owned (or at least human-reviewed). Treat as v2+ research.

**Worth a real brainstorm conversation** — it's either central to Glyph's pitch or out-of-scope, not in between. If central, this changes the manifesto (which currently positions Glyph as "agents read and write Glyph," not "agents generate from spec").

### Q41. FFI with contract inheritance — mandatory for non-TS interop

```glyph
@ffi  target: c
      library: "libvips.so.42" @hash:blake3:f0a1...92be
      symbol:  "vips_resize"
@pure true
@effects []
fn resize(img: Image, width: Int where width in 1..16384, ...) -> Image | ResizeError
```

Since Glyph compiles to TS, the *implicit* FFI is "import an npm package." That covers JS/TS. The pt6 sketch's `@ffi` is for **C, Rust, Python** — non-TS targets.

Realistically: in v1, Glyph rides npm and doesn't need a non-TS FFI. In v2+, this matters (database drivers, image processing, native crypto).

- **Option A:** First-class `@ffi` annotations for v1. Big investment for a feature most users don't need yet.
- **Option B:** Defer to v2 — when v1 needs non-TS, write a TS wrapper around the foreign call and import it as normal Glyph.
- **Option C:** Hybrid — v1 ships only `@ffi target: js` (typed npm import), v2 expands to C/Rust/Python.

**Probably Option B for v1.** Worth a one-line acknowledgment that the FFI dimension exists.

### Q42. Change propagation graph — restating Q5 in user terms

```glyph
@impact {
    callers:        47
    services_calling_via_api: [auth, billing, notifications]
    breaking_for_callers_that_rely_on_plus_tags: [analytics.dedup.v3]
}
```

The compiler maintains a live dataflow graph; any edit reports its blast radius. This is **"find references on steroids"** — exactly what Q5 (salsa-style incremental queries) buys.

If Q5 resolves toward salsa-style queries, Q42 is the UX layer on top — the LSP and the structured edit API (Q29) consume the graph to produce blast-radius reports. **Not a separate decision; fold into Q5 + Q29 + step 7 LSP scoping.**

## Soft questions for later

- **App #2 choice.** Recipe-to-shopping-list converter is suggested, but not pre-committed. Decide after #1 ships.
- **Self-hosting in v1.1 or v2?** Currently a v1.0 non-goal. When does it come back on the table?
- **Schemas-as-immutable spec language.** Already true in practice; needs writing down when formal spec is drafted.
- **RFC process — should one exist?** The earlier session proposed a formal RFC track in an `rfcs/` directory (numbered, with options + tradeoffs + recommendation per RFC). Current process is informal — session logs and proposals like `archive/glyph-transpiler-plan.md`. Lightweight is probably fine for solo work; consider revisiting if contributors arrive.

---

## Decisions that have been made but are worth re-litigating in brainstorm

The point of brainstorming is to revisit assumptions, not just to fill in blanks. Three decisions worth re-pressure-testing:

1. **`match` as the only conditional (D3).** It's elegant. Is it actually pleasant at 2000 lines? Step-6 dogfooding will surface this; the brainstorm can pre-empt it.
2. **No object literal shorthand (D10).** Greppability pillar. Costs keystrokes on every single literal. Is the trade actually correct, or is it a posture?
3. **No template literals (D12).** Forward-compatible to add; the day-0 parser file argued for it; session 1 deferred. Is now the time?

These are *not* open questions in the same sense as Q1–Q9 — they're settled. But the brainstorm is the cheapest moment to reverse a settled decision before it ossifies in the transpiler.
