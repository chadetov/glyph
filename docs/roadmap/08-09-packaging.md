# Steps 8–9 — Formatter, package story, installer, playground

Status: **step 8 in progress** (formatter shipped in Phase 1; npm CLI distribution
shipped — see below). Step 9 planned. Full notes in `archive/glyph-day-0-parser.md §Part 1`.

## Step 8 progress

- **Formatter — done** (Phase 1 week 5): the `glyph-formatter` crate + `glyph fmt`
  reprint the AST in one canonical, round-tripping, comment-preserving layout.
- **npm CLI distribution — shipped** (`npm/`): `glyph` is packaged the
  esbuild/swc way — a tiny launcher package (`npm/glyph`, published as `glyph`)
  with one prebuilt-binary optional dependency per platform
  (`npm/platform/<key>`, published as `@glyph/<key>` for darwin-x64/arm64,
  linux-x64/arm64, win32-x64). The launcher (`bin/glyph.js` + a unit-tested
  `bin/resolve.js`) resolves the matching binary and forwards argv, propagating
  the exit code; a `GLYPH_BINARY` override makes it testable against a local
  build. `npm/scripts/stage.mjs` stages a built binary into its platform package
  and `.github/workflows/release.yml` builds all five on native runners and
  publishes on a `v*` tag (gated on an `NPM_TOKEN` secret). End user story:
  `npm install -g glyph` / `npx glyph`. Verified locally (launcher passes argv
  and exit codes; 7 resolver unit tests pass). **The one remaining step is the
  user running the publish** (needs the npm account + `NPM_TOKEN`); everything
  up to `npm publish` is automated.
- **`"glyph"` key + `glyph publish` audit gate — shipped.** The `"glyph"` key in
  `package.json` is read by a typed config layer (`glyph-cli/src/config.rs`): an
  `imports` map carrying per-import `audit` (`first-party`/`third-party`/
  `internal`) + `last_reviewed`, and an `audit` policy (`max_age_months` default
  6, `enforce` default true). `glyph publish [dir]` runs the Q22 audit-currency
  gate (third-party imports must be reviewed within the window — a stale or
  never-reviewed one fails an enforcing policy, warns otherwise), then builds and
  `tsc`-checks the project into `dist/` and reports it ready for `npm publish`.
  The date math is dependency-free (Hinnant civil-from-days); the gate is a pure,
  unit-tested function. Verified end to end (a 29-months-stale import fails; a
  current one builds clean and reports ready).
- **Still to do for step 8:** producing a fully *standalone* npm tarball from a
  Glyph library — the emitted JS still imports `std/*` by bare specifier, which
  resolves via the build's `tsconfig` paths but not for an arbitrary consumer, so
  shipping a self-contained package needs specifier rewriting / bundling (shared
  work with `glyph run`'s runtime bundling). Deferred until there are Glyph
  libraries to publish. Q41 FFI-wrapper audit declarations reuse the same
  `imports` schema; the `glyph regen`/Q40 path is separate.

## Step 8 — Formatter and package story

### What's kept from the original strategy

- **Fixed-width, one-element-per-line-above-two-elements, no config.** Follows directly from the diff-stability pillar; Example 3 of the manifesto is its strongest argument. Ship as described. The formatter implementation is straightforward — recursive AST walk printing to a string, ~600 lines, no Prettier-style document model needed (because no line-length reflow).
- **npm piggyback.** Zero ecosystem to bootstrap, instant access to the largest package registry. Don't build a Glyph-specific registry — five-year distraction masquerading as a two-week task.

### What's revised

- **No separate `glyph.json` wrapper.** Two config files invite drift, and now agents have to reason about which file is authoritative for what. Instead: **a `"glyph"` key inside `package.json`.** One file, one source of truth, composes cleanly with existing npm tooling, more consistent with the "not configurable" stance.

### Sequencing constraint

**The parser must be genuinely frozen before week one of formatter work**, or the formatter gets rewritten twice. The re-lock gate between steps 6 and 7 (`06-dogfooding.md`) is what makes this safe.

## Step 9 — Installer and playground

### What's kept

- **Playground as highest-leverage marketing artifact.** For a language whose pitch is "agents can read and edit this," a visitor needs to *see* Glyph and the TS it compiles to, side by side, in under 30 seconds. Default example should produce a *meaningful* TS diff — the `load_feed` function from `02_async_errors.glyph` compiling to TS with try/catch and manual `Promise.all` error handling sells the language better than the manifesto does.

### What's added

- **A third playground pane: the same code edited by an agent.** A one-line semantic change producing a one-line diff. Diff stability is the pillar that's hardest to *feel* from a static sample — verifiability shows up in type signatures, greppability in naming. Diff stability is invisible until you watch an edit happen. The third pane is the demo that makes the pillar legible.

### What's revised

- **No curl-pipe-bash installer.** Target audience already has Node and trusts npm. **Ship `glyph` as an npm package** (`npm install -g glyph`, `npx glyph`). Lower friction, cross-platform by default, no "do I trust this shell script" hesitation. Curl-pipe-bash is a Rust/Go convention; for a transpile-to-TS language it's an unforced reach for credibility through aesthetics.

## CLI shape

From the step-4 plan (`04-transpiler.md`) and session-2 resolutions:

- `glyph build src/ --out dist/` — walks the module graph, typechecks the whole program, emits TS files into `dist/`, then shells out to `tsc` with a generated `tsconfig.json`.
- `glyph run examples/todo.glyph add "buy milk"` — runs end-to-end through tsc → node.
- `glyph fmt` — runs the formatter (and is what the LSP's format-on-save calls).
- `glyph regen <fn>` — **added by Q40 session-2 resolution.** Regenerates the body of a function whose `@generate by:` metadata declares a generator. The user runs this explicitly; `glyph build` does NOT invoke generators automatically. The function's `@example` / `@property` blocks (per Q11) are run after regeneration; mismatch fails the regen.
- `glyph --explain E0042` — opens the longer-form error documentation for an error code. Top 20 errors each get a paragraph + a code-fix example. ~12 hours of writing; pays back in agent task-success-rate forever.

## Updates from brainstorm session 2 (2026-05-26)

- **Q40 → stdlib `@generate` metadata + external `glyph regen` CLI.** The stdlib defines a `Generate` annotation type that carries the generator name, prompt, and any constraints. Function bodies are normal Glyph code. The `glyph regen` CLI command is what an agent (or human) invokes when they want to regenerate from the spec block. This keeps the manifesto's "agents read and write Glyph" framing intact — agents are users of `glyph regen`, not invokers of a runtime generation system.

## Updates from brainstorm session 3 (2026-05-26)

- **Q22 → content-addressed imports via `"glyph"` key in `package.json`.** Audit metadata lives in the `"glyph"` key (composed with item-8's "no separate glyph.json" stance from `archive/glyph-day-0-parser.md`). Schema:
  ```json
  "glyph": {
    "imports": {
      "vendor/stripe": { "audit": "third-party", "last_reviewed": "2026-04-02" },
      "org/obs/metrics": { "audit": "internal" }
    }
  }
  ```
  `glyph publish` checks audit-currency (e.g., third-party imports without a review in the last N months get a warning or hard fail depending on config). npm's existing `package-lock.json` integrity hashes carry the AST-pinning load.
- **Q41 → FFI minimal stance: TS wrappers only for v1.** Non-TS interop is via npm packages that wrap C/Rust/Python (`node-ffi`, `neon`, etc.). No `@ffi target: c/rust/python` syntax in v1. Step 8 ships the `"glyph"` key schema for declaring such wrappers' audit status but doesn't add Glyph-level FFI primitives.
- **`glyph regen` CLI shape locked** (from Q40): reads the `@generate by:` annotation on the target function, invokes the configured generator with the `@example`/`@property`/`@budget` context, replaces the body, runs the `@example` tests, and either commits or rolls back. No automatic invocation during `glyph build`.

## Open question

The original strategy described items 8 and 9 as "make it real to outsiders" tasks placed late. They're sequenced correctly, but the formatter dependence on a frozen parser means the **re-lock gate after step 6 is load-bearing**. If dogfooding produces a breaking change after the formatter ships, the formatter is rewritten. Worth flagging in the brainstorm whether the gate is strict enough.
