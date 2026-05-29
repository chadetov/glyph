# Glyph docs

Glyph is a TypeScript-family language designed so AI agents can read, write, and modify code safely. This `docs/` folder is the working set — a synthesized view of where the project actually stands. Verbatim source documents (sessions, proposals, the original strategy) are in `archive/`. If a doc here conflicts with a doc there, this folder wins.

## Start here

- `manifesto.md` — the four pillars and the bet (post-brainstorm: includes one narrow carve-out for `owned` resource discipline)
- `implementation-plan.md` — **the canonical 40-week sequence from current state to v1.0 launch, with day-by-day status sections appended as work ships.** Read this for the live record of what's done, what's next, and which tests pass.
- `language/spec.md` — the 27 numbered grammar decisions (D1–D27)
- `language/grammar-status.md` — production Rust parser status + the role of the archived tree-sitter grammar as a reference spec
- `roadmap/overview.md` — the 12-step plan with the current state of each step (higher-level than `implementation-plan.md`'s daily granularity)
- `open-questions.md` — historical record of brainstorm resolutions (sessions 1, 2, 3) plus the original question framings

## Per-step roadmap notes

These exist for steps whose scope has changed beyond the original strategy doc:

- `roadmap/04-transpiler.md`
- `roadmap/05-typechecker.md`
- `roadmap/06-dogfooding.md`
- `roadmap/07-lsp.md`
- `roadmap/08-09-packaging.md`

Steps 1–3 are partially done (see `roadmap/overview.md`). Steps 10–12 (docs, killer demo, launch) have not been re-scoped — refer to `archive/glyph-strategy.md` for the original framing.

## Where the original wording lives

Everything in `archive/` is the historical record. Notable files:

- `archive/MANIFESTO.md` — the original manifesto, full text
- `archive/SPEC_DECISIONS.md` — the 20 decisions with full rationale (this folder's `language/spec.md` is the condensed version)
- `archive/glyph-strategy.md` — the original 12-step plan
- `archive/SESSION_1.md`, `archive/glyph_step6_session.md` — session logs
- `archive/glyph-transpiler-plan.md`, `archive/glyph_step5_notes.md`, `archive/glyph-lsp-discussion.md`, `archive/glyph-day-0-parser.md` — proposals that re-scoped later steps
- `archive/glyph-session.md`, `archive/glyph-annotation-sketch.md`, `archive/glyph-annotation-sketch-pt2.md`, `archive/glyph-annotation-sketch-pt3.md`, `archive/glyph-annotation-sketch-pt4.md`, `archive/glyph-annotation-sketch-pt5.md`, `archive/glyph-annotation-sketch-pt6.md` — **seven pre-current-direction design explorations**, same family. The first used `@fn`/`intent:`/`effects:`/`@do`-pipeline syntax. The other six (examples 1–35 in a continuous series) used `@gid`/`@fid`/`@example`/`requires`/`ensures`/`@capabilities`/`parallel { }`/`@migrates_from`/`type X = Y where ...`/`@import @hash`/`@trace`/`@metrics`/`@redact`/`owned`/`@semver`/`bifn`/`@complexity`/`typestate`/`edit { }`/`@replayable`/`@doc @run`/`@view human`/`String<tainted:user>`/`@budget`/`@flag`/`Money<USD>`/`@refactor`/`@delta_from`/`@classification`/`@generate`/`@ffi`/`@impact` annotation-rich syntax. All abandoned in favor of the current "looks almost like TypeScript" stance. The four pillars survived; the syntax did not. The underlying *ideas* are tracked in `docs/open-questions.md` as Q10 through Q42 — several of them (Q20 loop construct, Q21 migrations, Q23 PII redaction, Q24 owned resources, Q29 structured edit API, Q32 dual human/agent view, Q33 taint tracking, Q34 budgets, Q40 type-driven generation) revealed either real gaps in the current spec or architectural alternatives worth considering, not just rejected-syntax suggestions.
- `archive/grammar.js`, `archive/scanner.c` — the step-3 tree-sitter grammar and external scanner
