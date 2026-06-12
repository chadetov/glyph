# Autonomous build loop

This brief lets the Glyph build cycle run unattended — on a schedule, in the
cloud, independent of any one machine. It is the durable form of the
"implement the next slice, review, fix, commit, push" routine.

## How to run it laptop-independently

A local Claude Code CLI session is suspended when its host sleeps, so it cannot
progress with the laptop closed. To run independently of any one machine, drive
this brief from **Claude Code on the web + Routines** (`claude.ai/code/routines`):
a cloud sandbox that persists, runs on a schedule, and can push to `main`.

1. Connect the `chadetov/glyph` repo under Routines.
2. Enable **"Allow unrestricted branch pushes"** so it can push to `main`. To
   keep a human gate instead, leave it off: the loop pushes to a `claude/*`
   branch and opens a PR you merge.
3. Create a routine whose prompt is the [routine prompt](#routine-prompt) below,
   pointed at this repo, scheduled hourly. Each run performs one cycle; the
   schedule is what makes it loop.

Alternative: a cron `claude-code-action` GitHub Actions workflow (6h/job cap).
Routines is simpler for a continuous loop.

## Current goal (the stop condition)

The **Phase 1 Week 4 emission gate**: all four files in `examples/`
(`01_validator`, `02_async_errors`, `03_react_component`, `04_cli_tool`) emit
TypeScript via `glyph build`, and each emitted `.ts` passes
`tsc --strict --noEmit`. When the gate is met the loop stops and hands back.

Update this section whenever the milestone advances (see
`docs/roadmap/overview.md` for the live status table).

### Status: the "emit" half is met

**All four `examples/*.glyph` now emit TypeScript via `glyph build` with no
diagnostics.** The former head-blockers are cleared: binding match arms, the
two-binding `for K, V in`, the mid-chain `?` (via expression hoisting), nested
constructor patterns, `component` + D6 JSX (elements + `<if>`/`<for>`/`<match>`
directives), plus implicit tail returns (functions, blocks, and lambdas) and
colorless-async `await` placement found along the way.

### Status: the runtime prelude is in; the `tsc` half has known language blockers

The runtime prelude and stdlib type surface now exist (`glyph-compiler/runtime/`
+ `examples/.types/` for the examples' external imports). With them the four
examples' imports **resolve and type precisely** (no more `any`), and the
self-contained `examples/corpus/` programs pass `tsc --strict` standalone.
Probing the four hard-case examples against the real types surfaced that **full
`tsc` passing is gated on language features beyond the emitter, not on more
stubs**:

- **Flow narrowing (Phase 2)** — `01_validator` narrows `input` with
  `is string` / `is Array<..>` arms but the narrowed type is not yet tracked,
  so `Ok(input)` types as `Ok<unknown>` against `Schema<string>`. The dominant
  blocker; cascades into `02`/`04`.
- **`Result` combinators vs `?`** — RESOLVED. `02`/`04` call
  `result.map_err(f)`; `Result` now carries `map`/`map_err`, and the `?`
  lowering re-wraps the propagated error (`return __glyph_err(__r.value)`, a
  `Result<never, E>`) so it stays assignable across success types. **`04_cli_tool`
  now passes `tsc --strict`.**
- **`T.schema` descriptor member** — DONE. Record descriptors now emit a
  `Schema<T>` `schema` member (built by the `std/schema` factory), so `02`'s
  `User.schema` / `Post.schema.array()` type-check.
- **`02` map_err/`?` order** — DONE. The example mapped the error after `?`
  (`await http.get(url)? .map_err(...)`), propagating the raw `HttpError`;
  reordered to `.map_err(...)?` so it propagates the `FeedError` the signature
  promises. **`02_async_errors` now passes.**
- **React JSX prop typing** — DONE. The React stub now types `createElement`'s
  props so an inline `on_*` handler's `event` parameter infers; **`03` passes.**

- **`is`-match narrowing** — DONE (emitter side). An `is`-match now checks a
  plain-identifier scrutinee directly (so `typeof input === "string"` narrows
  `input` in the arm bodies) and emits an `is Record<K, V>` check as a
  type-predicate IIFE (so the scrutinee narrows to the indexable record type,
  not `{}`). This cleared four of `01`'s five errors. (Glyph's *own* typechecker
  narrowing — substep 5c, for Glyph diagnostics/LSP — is a separate, still-open
  feature; it does not affect the emitted TS.)

- **`object_schema<Out>` / infer_shape (Q1)** — DONE (v1 stand-in). The parse
  builds a `Record<string, unknown>` and returns it as the caller's generic
  `Out`; with no `as` in Glyph, the emitter now casts a function's return value
  to its declared return type when that type references one of the function's
  generic parameters (`return { ... } as Schema<Out>`). Non-generic returns stay
  precisely checked. This is the v1 stand-in for `infer_shape` (whose v1.1 plan
  is to derive `Out` from the shape so no assertion is needed).

## Gate met

**The Phase 1 Week 4 emission gate is fully met, end to end through the
toolchain.** `glyph build src/ --out dist/` now writes the bundled runtime
(`dist/.glyph-runtime/`), copies `<src>/.types/` ambient declarations
(`dist/.types/`), and generates `dist/tsconfig.json`, so the output is
self-contained and `tsc -p dist/tsconfig.json` (or `glyph build --check`) types
it against real types. All four `examples/*.glyph` emit with no diagnostics and
**pass `tsc --strict --noEmit`**; the nine `examples/corpus/` programs pass too.
Re-probe any future emitter change with `glyph build examples --out <dir>
--check` (tsc on PATH) to keep the gate met.

## Routine prompt

```
You are continuing autonomous implementation of the Glyph compiler (Rust
workspace under glyph-compiler/). Read CLAUDE.md and docs/implementation-plan.md
FIRST; they govern everything. Also read docs/autonomous-loop.md (this brief)
and the roadmap under docs/roadmap/.

STOP CONDITION (the goal): the Phase 1 Week 4 emission gate — all four files in
examples/ (01_validator, 02_async_errors, 03_react_component, 04_cli_tool) must
emit TypeScript via `glyph build`, AND each emitted .ts must pass
`tsc --strict --noEmit`. At the start of every run, check this first. If it is
already met, STOP and report in one paragraph — do not invent new work.

Otherwise do exactly ONE smallest-coherent cycle this run:
1. Find the next slice: run `glyph build` on examples/ (and tiny probe files) to
   find the next construct the emitter rejects (EmitError::Unsupported) or that
   fails tsc. Likely remaining: nested / `await` `?`, JSX + `component` lowering
   (D6), async par.all. Pick the smallest unit that stands on its own.
2. Implement it (glyph-emit, plus parser/typechecker support if needed),
   matching existing conventions in the crate.
3. Verify: `cargo test --workspace` and `cargo clippy` must be clean. Add tests.
   Where you emit TS, validate it with `tsc --strict --noEmit` (use npx
   typescript if tsc isn't on PATH).
4. Update docs: docs/roadmap/overview.md, docs/roadmap/05-typechecker.md, and the
   relevant module doc-comment.
5. Commit code and docs as SEPARATE commits per CLAUDE.md: imperative mood,
   capitalized, no trailing period, <70 chars, no prefixes, no em dashes, no AI
   attribution. NEVER write "day-N" anywhere. Cargo.lock is committed.
6. Self-review the diff adversarially (line-by-line correctness; removed
   behavior; edge cases like reserved-word or colliding identifiers, e.g. a type
   named `value`; invalid-TS shapes). Fix real findings; commit the fix.
7. Push to main.

CONSTRAINTS: Do not relax Glyph syntax "to be helpful." Do not reintroduce the
abandoned annotation-heavy designs listed in CLAUDE.md. Keep each commit a
coherent unit. If you hit a genuine blocker, STOP and explain rather than hack.
```
