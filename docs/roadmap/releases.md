# Release roadmap (0.1.x → 1.0)

The 12-step plan in [`overview.md`](overview.md) built the toolchain — that work
is shipped. This file tracks the **feature releases** layered on top and
published to npm as `@glyphlang/glyph`. One release carries the "Next" marker and
is committed; everything after it is directional and re-sorts as we learn.

Each item keeps a rough T-shirt effort (S/M/L) and traces to a real source: the
persona-testing issue inventory, the generation follow-ups, the site's "on the
way" promises, or the standing deferrals in CLAUDE.md.

## Shipped

- **0.1.0–0.1.2** — first public preview: the language + Rust compiler, the
  standard library, the site and playground, `std/http` server, `glyph init`,
  and a wave of correctness/JSX fixes from persona testing.
- **0.1.3 — Generated types, not hand-written DTOs.** `type` is the zod
  replacement (declare a type, get a validated boundary); `glyph gen openapi`
  and `glyph gen dts` generate committed, descriptor-bearing types; the
  typed-APIs guide and the runnable REST example.
- **0.1.4 — TypeScript 7 handling for `glyph gen dts`.** A clean "install
  `typescript@6`" diagnostic instead of a cryptic crash on the native compiler.

## 0.1.5 — Next · Finish the generation / typed-API story

**Status: committed, the active target.** Carries the 0.1.3/0.1.4 momentum to
completion and makes the site's "on the way" promises real.

- **`gen openapi` client + handler codegen** (M). Typed client functions per
  operation and/or `std/http` handler signatures, not just the DTOs (B3 from the
  0.1.3 plan; the site already promises it).
- **Discriminated unions in generation** (M). A `oneOf` / TS union *with* a
  discriminator maps wire-faithfully to a Glyph tagged union; today every `oneOf`
  narrows to `unknown`.
- **`gen dts` on TypeScript 7** (M/L). Support the native "tsgo" API (or bundle a
  compatible parser) so the default `npm install typescript` works without the
  `typescript@6` pin.
- **`gen zod`** (M). Materialize a `zod` schema into a Glyph type + descriptor —
  the "value → type" loop from the other side.
- **Untrusted input as `Option`** (M, correctness). `req.headers[k]` and
  `Request.body` return `Option` instead of `undefined`/`unknown`, so missing
  input can't slip past validation. Directly strengthens the typed-API story.

## 0.1.6 — Planned · Correctness & diagnostics

The sharp edges first-time-user agents actually hit. The warning tier is
foundational infrastructure, so this lane leads with it.

- **Warning-severity diagnostics** (M, unblocker). The diagnostic system is
  errors-only today; a warning tier unblocks the item below and future work.
- **`Result` must-use warning** (S, needs the tier above). Warn when a
  `Result`-returning expression is dropped as a statement.
- **Source-mapped `tsc` errors** (L, high value). Errors only `tsc` catches point
  at generated `.ts`, not `.glyph` source — the biggest gap against the
  Elm-quality-errors claim. Needs a source map through emit.
- **Nested record-payload whole-ident bind** (S). `Err(BadQty(b))` binding the
  whole record still `tsc`-errors in the nested case (`BadQty({sku})` works).
- **`\${...}` template-literal escaping** (M). A literal `\${` still emits a live
  interpolation — the documented D22 footgun; needs a real lexer template mode.

## 0.1.7 — Planned · Approved language features

Decided in earlier brainstorms; just need building.

- **JSX fragments `<>...</>`** (S/M) — parser + emitter.
- **Member-expression JSX `<Ns.Comp>`** (S/M) — parser + emitter.
- **Bounded generics `<T: Bound>`** (M) — parser + checker + emit.
- **Extend the targeted type hint** (S) to `int`/`any`/`Promise<T>`.

## Rolling · Ergonomics & polish

Small wins that can land in any release rather than wait for their own.

- **`glyph build --out X` cleans stale files first** (S).
- **A shared-state / store pattern** (M, needs a design call). Shared mutable
  state has exactly one legal home (a `let` in `main`); no clean store module.
- **`@redact` full enforcement** (M). Real masking in the emitted serializer +
  field-name validation (D24); a prior attempt broke the descriptor tests, so
  retry carefully.
- **`glyph regen` implementation** (M). The `@generate` → regenerate-body command
  (Q40) is still a stub; it now fits the `gen openapi`/`gen dts` family.

## Parked (v2 / later)

- `@ffi target:` syntax (v2).
- Mapped types / `infer_shape` (substep 5b).
- `owned` closure-capture soundness (needs real capture analysis).
- Self-hosting (a v1.0 non-goal).

---

*Sequencing note:* 0.1.5 is committed; the 0.1.6/0.1.7 split and the rolling lane
are a proposal, ordered by dependency (warning tier before must-use, etc.). We
re-sort at each release boundary.
