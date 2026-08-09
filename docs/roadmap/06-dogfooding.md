# Step 6 — Dogfooding

Status: **core complete; soft exit pending.** The fridge app
(`examples/apps/fridge.glyph`) is built, runs end to end via `glyph run`, passes
`tsc --strict`, and its `@example` tests pass. It was dogfooded over three
rounds: (1) build + use across the whole command surface, producing a 20-item
gap list in [`docs/dogfooding-gaps.md`](../dogfooding-gaps.md); (2) extend
(merge-on-add, summary footer) and surface round-2 findings; (3) an adversarial
multi-agent review of the fixes, with round-3 fixes. That first list closed with
zero open bugs: the critical/high tier fixed (correctness, silent-green,
multi-file, typechecker field/arg checks, validating recursive `json.parse`), and
the medium/low tier fixed or resolved as documented, forward-compatible v1.1
deferrals. The syntax corpus did not change, so no re-lock is needed before
step 7. Full session log in `archive/glyph_step6_session.md`.

Dogfooding did not stop there. It is now a standing loop: one app per release,
each one written to find something. `examples/apps/` holds the apps and
[`docs/dogfooding-gaps.md`](../dogfooding-gaps.md) is the live list. **Read it,
not this paragraph, for what is open.** Each round's fix and its unfixed
findings are written up per release in [`releases.md`](releases.md).

## The loop

A round is not "ship an app." It is "find the next thing the language cannot do,
and close it." The app is the instrument, not the product.

**0. Build the compiler, then check it is the one you built.**
`python3 scripts/check_binary_fresh.py`. A binary older than the crate sources
or the runtime does not fail loudly, it fails plausibly. One built before the
node shims landed reported `Cannot find name 'net'` against a correct file,
which reads exactly like a gap and would have been written up as one.

**1. Pick an app that has to do something no existing app does.** A round that
re-treads a solved shape finds nothing. Look at the table in
[`examples/README.md`](../../examples/README.md) and go somewhere else.

**2. Write it in Glyph, and stop at the first thing Glyph cannot do.**

This is the rule the loop lives or dies on, so it is stated as a prohibition:
**do not work around a gap.** Not with a hand-written `.d.ts`, not with
`extern_ts`, not by restructuring the program into a shape that dodges the
problem, not by narrowing the app until the problem is out of scope. Stop where
you are and report.

A finished app that routed around three gaps has taught us nothing and hidden
three gaps, and it looks like a success, which is worse than looking like a
failure. An app that stops on line 40 with one clear reproduction has done the
entire job of the round. Two rounds were lost to this before it was written
down: one shipped a session replayer instead of the multi-client server that was
asked for, and one shipped a chat app whose sockets were typed by a
hand-written `net.d.ts` sitting inside the app. Both built clean. Both were
worth less than stopping would have been.

`scripts/check_apps_are_glyph.py` catches the TypeScript-shaped workarounds. It
cannot catch a program quietly reshaped to avoid a gap, so that one is on
whoever is writing.

**3. Write the gap down before fixing anything.** What you were trying to write,
what you expected, what happened, and the smallest program that reproduces it.
The reproduction is the part that survives; the prose around it goes stale.

**4. The orchestrator decides what happens next, not the agent that found it.**
Fix now, defer to a named release, or decide it is not a defect. An agent that
hits a fork reports the options and the tradeoff. See the sub-agent rules in the
repo's working notes.

**5. Re-check the premise before implementing anything older than a few
releases.** Gaps rot: the compiler moves under them. A reconciliation pass found
five of ten entries either already fixed, closable with no code change, or
resting on a premise that had stopped being true. Reproduce the gap against the
compiler you just built. If it does not reproduce, the round's work is marking
the entry, not writing a fix.

**6. Fix it with a test that fails without the fix, and prove that it does.**
Revert the fix, watch the test fail, put it back. A test written after a fix,
never seen red, is a test that might assert nothing. The same applies to
reviewing an agent's work: "I added `owned` to the socket" is a claim. Breaking
the discipline on purpose and getting `E0206` back is evidence.

**7. Resume the app from where it stopped**, and keep going until the next gap
or until it is done.

**8. Reconcile before the release.** Markers and counts in the gap list
(`scripts/check_gaps.py`), the release entry in
[`releases.md`](releases.md), the docs that state current status, and the
engineer Q&A on the site for anything user-visible. Three releases once shipped
with no site update at all.

## The next round is chosen, and why

An application built on **real npm packages**. Nothing in `examples/apps/` uses
one, which means the 1.0 interop gate has only ever been tested by guides. It is
also what decides whether Glyph needs a construct for containing a throwing host
call: that was scheduled, then unscheduled, because twenty-six rounds produced
two host-throw incidents and the library absorbed both. An app that actually
depends on npm is the only thing that can settle it, and settling it by writing
the app is the method that has beaten reasoning from first principles twice.

## The gates a round runs

| Gate | What it stops |
|---|---|
| `check_binary_fresh.py` | Verifying against a compiler older than the code, which invents gaps |
| `check_apps_are_glyph.py` | An app that reaches for TypeScript instead of reporting the gap |
| `check_docs_compile.py` | A doc snippet that no longer compiles, and one that looks broken but is fine |
| `check_scaffold_docs.py` | A first-run walkthrough drifting from what `glyph init` writes |
| `check_gaps.py` | Status markers and counts falling behind the compiler |
| `check_versions.py`, `check_site.py` | Version skew across the packages; broken links and sub-nav |

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
