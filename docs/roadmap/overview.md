# Roadmap overview

Original 12-step plan: `archive/glyph-strategy.md`. Revisions to steps 4–9 are logged in the per-step docs in this folder. Where this file and `archive/glyph-strategy.md` disagree, this file wins. For the live day-by-day implementation record, see `docs/implementation-plan.md`.

## Phase 1 — Make it programmable

| # | Step | Status | Source of truth |
|---|------|--------|-----------------|
| 1 | Manifesto (≤2000 words, four pillars, three before/after examples) | ✅ Done | `docs/manifesto.md` |
| 2 | Lock syntax with examples (30–50 small programs) | ⚠️ Partial — 4 hard-case examples written and parsing end-to-end; 26–46 remaining (Q2 folded into step 6 dogfooding). | `archive/SESSION_1.md`, `examples/` |
| 3 | Formal grammar + tree-sitter parser | ⚠️ Written as reference spec, not verified — the production parser is the hand-written Rust Pratt parser in `glyph-compiler/crates/glyph-parser/` (per Q5 hybrid). The tree-sitter grammar in `archive/` survives as an editor-tooling source. | `docs/language/grammar-status.md` |
| 4 | Transpiler to TypeScript (originally 4–6 weeks; revised 6–8) | 🟨 **In progress, 24 days shipped.** Phase 1 weeks 1–2 done: lexer + Pratt parser + AST (all 27 D-decisions, all 4 examples parse), name resolution, module graph, type representation, full salsa-tracked incremental pipeline, `glyph build` CLI, ariadne diagnostics. Week 3 (typechecker substep 5a) well underway — see step 5. TS emission (week 4) still ahead. **237 workspace tests pass.** | `docs/roadmap/04-transpiler.md`, `docs/implementation-plan.md` |
| 5 | Typechecker (originally 4–6 weeks → revised ~9 weeks) | 🟨 **Substep 5a well underway (through day 24).** Shipped: match exhaustiveness for user-defined and prelude `Result`/`Option` unions, `?`-outside-`Result` rejection, call/`await` type synthesis, match-arm payload typing, generic instantiation at call sites, primitive return-type mismatch, and **D25 `owned` single-consumption** (consume = move into an `owned` parameter; I1 resolved → `resource` keyword). Still ahead: runtime descriptors (Q8, next major item), nested-pattern exhaustiveness, a fuller unifier. Q1 resolved (mapped types → v1.1, substep 5b deferred). | `docs/roadmap/05-typechecker.md`, `docs/implementation-plan.md` |
| 6 | Dogfood (originally 2 weeks → revised to 4–6 weeks, sequential apps) | 🟦 Planned, target chosen (fridge shopping list, JSON on disk) | `docs/roadmap/06-dogfooding.md` |

## Phase 2 — Make it usable

| # | Step | Status | Source of truth |
|---|------|--------|-----------------|
| 7 | Ship the LSP (4 weeks → revised 5–6 weeks) | 🟦 Planned. Foundation already in place: salsa-tracked incremental queries (days 5–12) give the LSP its memoization substrate for free. Rename + find-references cut to v1.1; formatter-on-save and `agent://` virtual document RPC (Q32) added. | `docs/roadmap/07-lsp.md` |
| 8 | Formatter and package story (2 weeks) | 🟦 Planned, `glyph.json` rejected in favor of `"glyph"` key in `package.json` | `docs/roadmap/08-09-packaging.md` |
| 9 | Installer and playground (2 weeks) | 🟦 Planned, npm distribution over curl-pipe-bash; third playground pane (agent edit → one-line diff) added | `docs/roadmap/08-09-packaging.md` |
| 10 | Docs and book outline (4 weeks) | ⬜ Not yet re-scoped | `archive/glyph-strategy.md §4` |
| 11 | Killer demo (6–8 weeks) | ⬜ Not yet re-scoped | `archive/glyph-strategy.md §4` |
| 12 | Launch + first 100 users | ⬜ Not yet re-scoped | `archive/glyph-strategy.md §4` |

## Timeline honesty

The original strategy says **6–9 months focused work, 12–18 months calendar** for v0.1. Two revisions stretch the critical path before that estimate has accounted for them:

- Step 5: 4–6 weeks → **~13 weeks** (+7–9 weeks)
- Step 6: 2 weeks → **4–6 weeks** (+2–4 weeks)

The honest revised range is **9–12 months focused work, 15–24 months calendar**, *assuming* steps 7–12 don't undergo similar expansion when their turns come. They probably will. Plan accordingly.

## The trap at every step

Scope creep. At step 5, effect types. At step 7, a custom protocol instead of LSP. At step 11, rewriting the compiler. The discipline that ships a language is saying "v0.2" to every good idea not on this list.

## Self-hosting

**Non-goal for v1.0** (promoted from a year-three deferral in session 2). The compiler is Rust until v1.0 ships and there are users.

## What's blocking forward motion right now

The two original blockers are **resolved** as of the 2026-05-26 brainstorm sessions:

1. **`infer_shape<Shape>` v1 or v2** → **v1.1** (Q1). `01_validator.glyph` was rewritten with an explicit output type parameter; substep 5b is no longer mandatory for v1.
2. **Compiler architecture: salsa-style incremental queries or not** → **Q5 hybrid**. The typechecker + name resolver are salsa-backed; AST→TS emission stays a dumb visitor. Both shipped as of day 12 (full per-decl invalidation, automatic cross-file diagnostics).

Current near-term work (per `docs/implementation-plan.md`): remaining week-3 typechecker pieces — runtime descriptors (Q8, the next major item), nested-pattern exhaustiveness, and a fuller unifier. `owned` single-consumption (D25) and the `?`/exhaustiveness/inference pieces shipped through day 24. Then phase 1 week 4 (TS emission). Active open questions are tracked in `docs/open-questions.md`.
