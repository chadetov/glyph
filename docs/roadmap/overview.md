# Roadmap overview

Original 12-step plan: `archive/glyph-strategy.md`. Revisions to steps 4–9 are logged in the per-step docs in this folder. Where this file and `archive/glyph-strategy.md` disagree, this file wins.

## Phase 1 — Make it programmable

| # | Step | Status | Source of truth |
|---|------|--------|-----------------|
| 1 | Manifesto (≤2000 words, four pillars, three before/after examples) | ✅ Done | `docs/manifesto.md` |
| 2 | Lock syntax with examples (30–50 small programs) | ⚠️ Partial — 4 hard-case examples written; 26–46 remaining. Grammar decisions outran the corpus. | `archive/SESSION_1.md` |
| 3 | Formal grammar + tree-sitter parser | ⚠️ Written, not verified — `tree-sitter generate` never run; scaffolding incomplete. | `docs/language/grammar-status.md` |
| 4 | Transpiler to TypeScript (originally 4–6 weeks) | 🟦 Planned, not started | `docs/roadmap/04-transpiler.md` |
| 5 | Typechecker (originally 4–6 weeks → revised to ~13 weeks) | 🟦 Planned, **scope contested** | `docs/roadmap/05-typechecker.md` |
| 6 | Dogfood (originally 2 weeks → revised to 4–6 weeks, sequential apps) | 🟦 Planned, target chosen (fridge shopping list, JSON on disk) | `docs/roadmap/06-dogfooding.md` |

## Phase 2 — Make it usable

| # | Step | Status | Source of truth |
|---|------|--------|-----------------|
| 7 | Ship the LSP (4 weeks → revised scope) | 🟦 Planned, rename + find-references cut to v1.1; formatter-on-save added | `docs/roadmap/07-lsp.md` |
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

See `open-questions.md`. The two biggest open decisions:

1. **`infer_shape<Shape>` v1 or v2** — gates step 5 scope and may force a rewrite of `01_validator.glyph` *now* before any transpiler work starts.
2. **Compiler architecture: salsa-style incremental queries or not** — the step-4 plan ("dumb AST→TS visitor, no IR") makes the LSP timeline in step 7 unworkable. One of these positions has to give.
