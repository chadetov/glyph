# Step 6 — Dogfooding

Status: **in progress.** The fridge app (`examples/apps/fridge.glyph`) is built,
runs end to end via `glyph run`, passes `tsc --strict`, and its `@example` tests
pass. The concrete gap list it produced is in [`docs/dogfooding-gaps.md`](../dogfooding-gaps.md)
(20 gaps, several silent-miscompile bugs). Remaining: fix the top gaps, then use
the app for real. Full session log in `archive/glyph_step6_session.md`.

## Updates from brainstorm session 1 (2026-05-26)

- **Q2 → step 6 also produces the transpiler test corpus.** The shopping list app and its successors are the source of the 30–50 example programs step 4 needs for CI. No separate "write synthetic examples" phase. Real-app code IS the corpus.
- **Q21 → stdlib migration pattern.** No new language syntax. Stdlib ships `Migration<From, To>` plus `migrate.from<Old, New>((old) => new)` plus `Schema.parse_versioned(input)` that walks migrations. The shopping list's persistence boundary is the first stress test — when an item gains an optional `category` field, write a one-line migration. Forward-compatible to language-level migrations later.

## Updates from brainstorm session 3 (2026-05-26)

The shopping list app is now the first real stress test for several v1 decisions resolved in session 3:

- **Q3 stdlib bootstrap set is stress-tested here first.** `result`, `option`, `array`, `string`, `io`, `json`, `fs`, `time` — if any feels wrong at week 2 of dogfooding, escalate before locking the stdlib API.
- **D23 `@example` tests are written for every new function.** This is the workflow that proves Q11+Q40 together: write the spec block, write the function, the tests are colocated. Step 6 produces the *first* large body of `@example`-tested Glyph code.
- **D25 `owned` modifier is stress-tested via the persistence boundary.** Saving `shopping-list.json` opens a file handle; the `owned` discipline says it must be consumed before the function returns. If this feels gratuitous on a 10-line save function, escalate.
- **Q33 `Tainted<T>` stdlib discipline is stress-tested if the shopping list ever gains a search box.** User input → query → file read is the smallest pipeline that exercises the taint discipline.
- **Q34 `withBudget` stdlib helper is stress-tested if the shopping list ever calls an LLM** (e.g., "summarize my weekly meals"). Run the LLM-touching code under `withBudget({wallTime: 5s, llmTokens: 1000, usdCost: 0.05}, ...)` and see if the API feels right.

## Target

A **fridge shopping list app**, built in Glyph, JSON on disk, used personally for two weeks before starting any next app.

Rejected alternatives:
- **JarvisX components** (originally proposed) — not part of a daily workflow; dogfooding fails when the dogfood is fake.
- **Docker Compose with swappable databases** (proposed mid-session) — none of it is Glyph code. The three-day bug becomes "Postgres in Docker can't see the host," not "Glyph's type system buckled here." Zero dogfooding signal. Defer to step 11 (killer demo) if it ever lands at all.

## Revised scope vs original

| Before | After |
|---|---|
| 2 weeks | **4–6 weeks** (dogfooding finds compiler/stdlib bugs; fixing them is part of the work) |
| JarvisX components | **Fridge shopping list app, JSON on disk** |
| Vague exit ("write something real") | **Concrete exit: shipped, in personal use, with a written list of N specific compiler and stdlib gaps** |
| Self-hosting deferred to year 3 | **Self-hosting a non-goal for v1.0** (not a delay-credibility flex) |
| Step 6 → step 7 direct | **Re-lock the syntax corpus between 6 and 7** if dogfooding produced breaking changes |
| 1+ apps, unspecified count | **One at a time; #2 only starts when #1 is in actual use** |

## Why a shopping list

It stress-tests all four pillars in a realistic way:

- **Verifiability** — file I/O with parse failures; a real `ShoppingList` must parse back from disk, not be cast.
- **Greppability** — code surface across CLI/UI, stdlib calls, domain types.
- **Diff stability** — code you'll edit 20 times a month as features grow.
- **Abstraction** — non-trivial domain model: items, quantities, units, categories, expiry.

And critically: **you'll actually use it.** Dogfooding fails when the dogfood is fake.

## What this step is hunting for

The roadmap line "find design mistakes that examples didn't surface" is too vague. Concrete targets:

1. **Stdlib gaps.** Session 1 examples imported `std/result`, `std/http`, `std/json`, `std/array`, `std/fs`, `std/process`, `std/io`, `std/string`, `std/time`, `react`. Half don't exist. Dogfooding tells you which to build first and what their APIs should look like.
2. **Ergonomics failures.** Patterns tolerable at 200 lines become intolerable at 2,000. Likely candidate: `match` arms three deep.
3. **Type inference cliffs.** Inferred-shape `object_schema` works in examples. It will fail somewhere real with a terrible error message. Find that case now.
4. **Auto-generated `T.schema` cost.** Schema emission for every type was committed in Session 1. Dogfooding tells you compile time at 5,000 lines. If it's bad, find out now.

Exit with these written down as **concrete issues, not vibes**.

## Predictable design pressures for app #1

Worth writing down in advance so signal can be distinguished from noise:

- **Optional fields everywhere.** Quantity, category, expiry are all optional. `T?` sugar over `Option<T>` was deferred in Session 1. Three weeks in, the deferral will be felt. Promote the sugar based on real frequency, not first-week feeling.
- **List mutations.** Add, remove, check off, reorder, merge duplicates. Every one exercises `mut` semantics on arrays.
- **Persistence boundary.** Saving to disk crosses the verifiability boundary. The on-disk shopping list must parse back into a real `ShoppingList`, not a cast hope. This is exactly Example 1 from the manifesto.
- **Stdlib gaps likely needed.** Dates, currency/quantity formatting, fuzzy string match for "did I mean cilantro vs coriander."
- **The "shared list" temptation.** You'll want to share with a partner/family. **Resist in v1** — multi-user sync turns a two-week project into a two-month CRDT debugging session.

## Sequential, not parallel

Three or four apps in parallel from week one means three or four half-finished apps and you can't tell which is telling you the truth about the language. Suggested order if continuing past #1:

1. **Shopping list** (CLI or simple web UI, local file storage) — best first choice
2. **Recipe-to-shopping-list converter** — stresses parsing, schemas, error handling
3. **Pantry tracker** — stresses dates, sorting, queries
4. **Meal planner** — stresses UI state

Each only starts when the previous is in actual use. **App #2 is not pre-committed** — decide after #1 ships, driven by what gaps it surfaced.

## Re-lock gate before step 7

The LSP bakes in syntactic assumptions. If dogfooding produced any breaking changes to the spec (a new D-decision, an overruled old one), re-run the syntax-lock review against the new examples before starting step 7. The grammar should be **final** before the LSP commits to it.
