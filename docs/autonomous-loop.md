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
- **`Result` combinators vs `?`** — `02`/`04` call `result.map_err(f)`, but a
  `map`/`map_err` method makes `Result` `T`-dependent and breaks the `?`
  operator's cross-success-type propagation (`return __r` of `Result<X, E>` from
  a `Result<Y, E>` function). Resolving this needs a combinator design — most
  likely `?` re-wrapping the error (`return Err(__r.value)`) so the propagated
  value is `Result<never, E>`. See `docs/roadmap/05-typechecker.md`.
- **Example structure** — `02`'s `await http.get(url)?.map_err(...)` runs the
  `?` before `.map_err`, so it propagates the pre-conversion `HttpError` where
  the function returns `FeedError`. An example-level quirk.
- **React JSX prop typing** — `03` is two errors away (an untyped JSX event
  handler param and a `User[]` element type), both needing the real React type
  surface, not our stubs.
- **`T.schema` descriptor member** — `02` uses `User.schema` / `Post.schema.array()`;
  the record descriptor does not yet emit a `.schema` member (an emitter slice,
  coupled to the `Schema`/`Result` design above).

So the gate's `tsc` half is no longer "write stubs" — it is the Phase-2
flow-narrowing work plus the `Result` combinator design. The emitter itself is
done for these examples (the corpus proves it emits fully `tsc`-clean TS).
Re-probe with `glyph build` plus a `tsc` run against `glyph-compiler/runtime/`
(see that directory's README) every run.

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
