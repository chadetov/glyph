# Glyph — Session 2: Step 6 Scoping (Dogfooding)

**Context:** Continuing from Session 1 (syntax lock via examples). Roadmap step 6 is "self-host your tooling" — write a real dogfooding project in Glyph to surface design mistakes that examples didn't.

**Status at end of session:** Step 6 scoped. Target chosen: a fridge shopping list app, JSON-on-disk, built sequentially.

---

## The roadmap revision

Original step 6 said: *"Build Glyph in Glyph? No, not yet — that's a year-three decision. But: write your own dogfooding project in Glyph. Something real. JarvisX components are a good candidate. 2 weeks."*

Revised:

- **Duration:** 4–6 weeks, not 2. Dogfooding finds compiler/stdlib bugs; fixing them is part of the work.
- **Target:** A fridge shopping list app, not JarvisX components. Real personal use beats fake production code.
- **Count:** One app shipped and used for two weeks before starting the next. Sequential, not parallel.
- **Self-hosting:** Promoted from "deferred to year three" to **non-goal for v1.0**. Not a delay-credibility flex.
- **Exit criteria:** Not "two weeks elapsed." Exit when the app is shipped, in actual personal use, with a written list of N specific compiler and stdlib gaps.
- **Re-lock gate:** Between step 6 and step 7 (LSP), re-run the syntax lock against the new examples if dogfooding produced any breaking changes. The LSP bakes in syntactic assumptions; they should be final ones.

---

## Why a shopping list app

A fridge shopping list satisfies the four-pillar stress test:

- **Verifiability** — file I/O with parse failures (a real ShoppingList must parse back, not be cast)
- **Greppability** — code surface across CLI/UI, stdlib calls, domain types
- **Diff stability** — code you'll edit 20 times a month as features grow
- **Abstraction** — non-trivial domain model: items, quantities, units, categories, expiry

And critically: **you'll actually use it**. Dogfooding fails when the dogfood is fake. JarvisX components fail this test because they're not yet part of a daily workflow. A shopping list is.

---

## Why one, not three or four

Sequential, not parallel. Three or four apps in parallel from week one means three or four half-finished apps and you can't tell which is telling you the truth about the language.

Pattern that works: ship one, use it for two weeks, *then* start the next with everything you learned. Three or four sequentially over six weeks is fine. In parallel from day one is the trap.

Suggested order if you continue past the first:

1. **Shopping list** (CLI or simple web UI, local file storage) — best first choice
2. **Recipe-to-shopping-list converter** — stresses parsing, schemas, error handling
3. **Pantry tracker** — stresses dates, sorting, queries
4. **Meal planner** — stresses UI state

Increasing complexity on purpose. Each one only starts when the previous is in actual use.

---

## What to watch for in app #1

Predictable design pressures from this specific app — written down in advance so signal can be distinguished from noise:

- **Optional fields everywhere.** Quantity, category, expiry are all optional. `T?` sugar over `Option<T>` was deferred in Session 1. Three weeks in, the deferral will be felt. That's data — promote the sugar based on real frequency, not first-week feeling.
- **List mutations.** Add, remove, check off, reorder, merge duplicates. Every one exercises `mut` semantics on arrays. If anything feels wrong, it surfaces here.
- **Persistence boundary.** Saving to disk crosses the verifiability boundary. The on-disk shopping list must parse back into a real `ShoppingList`, not a cast hope. This is exactly Example 1 from the manifesto. Watch how it actually feels in practice.
- **Stdlib gaps.** Likely: dates, currency/quantity formatting, fuzzy string match for "did I mean cilantro vs coriander."
- **The "shared list" temptation.** You'll want to share with a partner/family. Resist in v1 — adding multi-user sync turns a two-week project into a two-month CRDT debugging session and drowns the dogfooding signal.

---

## What step 6 is actually hunting for

The roadmap line "find design mistakes that examples didn't surface" is vague. Specific targets:

1. **Stdlib gaps.** Session 1 examples imported `std/result`, `std/http`, `std/json`, `std/array`, `std/fs`, `std/process`, `std/io`, `std/string`, `std/time`, `react`. Half don't exist yet. Dogfooding tells you which to build first and what their APIs should look like.
2. **Ergonomics failures.** Patterns tolerable in 200-line examples become intolerable at 2,000 lines. Likely candidate: `match` arms three deep.
3. **Type inference cliffs.** Inferred-shape `object_schema` works in examples. It will fail somewhere real with a terrible error message. Find that case now.
4. **Auto-generated `T.schema` cost.** Schema emission for every type was committed in Session 1. Dogfooding tells you the compile time at 5,000 lines. If it's bad, find out now.

Exit step 6 with these written down as a list of concrete issues, not vibes.

---

## The Docker + multi-database trap (rejected mid-session)

Proposed: run the shopping list on Docker Compose with different databases swappable.

Rejected. The reasoning:

- **None of it is Glyph code.** YAML, SQL dialects, connection strings, container networking. The three-day bug will be "Postgres in Docker can't see the host," not "Glyph's type system buckled here." Zero dogfooding signal.
- **It breaks both properties that made the app worth doing.** Not small (a week of infrastructure work, zero Glyph). Not real (you don't actually need three databases for a personal shopping list).
- **It's "make the toy project serious" disguised as engineering rigor.** A common dogfooding failure mode.

What the multi-DB plan would actually test, honestly:

- *"It proves Glyph works with real infra."* No — it proves the driver libraries (pg, mysql2) work. Glyph is calling out to npm packages that already exist.
- *"It stresses types across schemas."* Marginally. One DB exercises the row-to-record mapping; three exercises it three times.
- *"It looks impressive."* True, but step 6 is a dogfooding milestone, not a launch demo. No one is watching.

**Decision:** JSON file on disk, like `04_cli_tool.txt` already demonstrates. If after two weeks of real use JSON-on-disk genuinely breaks down (concurrent phone/laptop edits, history), *then* add SQLite. One database. Local file. No Docker.

Docker Compose with multiple databases is a step 11 (killer demo) consideration if it makes the demo land harder. It is not a step 6 activity.

---

## Net changes to roadmap step 6

| Before                              | After                                                              |
| ----------------------------------- | ------------------------------------------------------------------ |
| 2 weeks                             | 4–6 weeks                                                          |
| JarvisX components                  | Fridge shopping list app, JSON on disk                             |
| Vague exit ("write something real") | Concrete exit: shipped, in personal use, written gap list          |
| Self-hosting deferred to year 3     | Self-hosting a non-goal for v1.0                                   |
| Step 6 → step 7 direct              | Re-lock the syntax corpus between 6 and 7 if dogfooding broke anything |
| 1+ apps, unspecified count          | One at a time; #2 only starts when #1 is in actual use             |

---

## Open question for next session

Whether app #2 (recipe-to-shopping-list converter) is the right second target, or whether the gaps surfaced by app #1 should drive that choice. Decide after app #1 ships — don't pre-commit.
