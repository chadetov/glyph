# Website roadmap

The site at glyphlang.io has its own lifecycle, separate from the compiler's.
The compiler ships numbered releases to npm (see [releases.md](releases.md));
the site ships whenever a pull request merges to `main`, because the Pages
workflow deploys `web/` on every push. A site change needs no compiler release,
and a compiler release changes the site only where a generated number or a
version string moves.

## How the site is built and checked

- **Source of truth:** `site/` (pages, partials, styles, translations,
  release notes). `scripts/build_site.py` renders it into `web/`, which is
  committed. Nothing under `web/` is edited by hand.
- **Generated content:** the `/catches/` page and every count of checked cases
  come from `catches/`; release notes come from `site/releases/0.1.N.html`;
  sitemap dates come from content hashes in `site/lastmod.json`.
- **Gates on every pull request:** `check_site_built.py` (web/ matches the
  source), `check_site.py` (links, HTML, sub-nav, sitemap, JSON-LD, balanced
  CSS), `check_i18n.py` (translation structure, code identity, wording),
  `check_releases.py`, `check_docs_compile.py` (every code sample compiles),
  `check_scaffold_docs.py` and `check_catches.py`.
- **Languages:** English is the source. Brazilian Portuguese, Spanish,
  Japanese, Turkish, Simplified Chinese, Korean, German, Russian and Thai are
  translated page by page through segment files (`site/i18n/README.md`). When
  the English changes, the translation is reported stale and the page says so
  until the changed segments are redone. Release notes stay English.

## Shipped

**2026-11-19 · The redesign** (PR #177). The site moved to a generator with one
design system in light and dark; journey pages for TypeScript engineers, teams,
agent builders, skeptics and language designers; `/catches/` generated from
checked cases; a journeys menu; monthly release notes with a feed; the home
page in nine languages. Every existing URL was kept.

**2026-11-19 · Fixes and corrections** (PRs #178, #179, #180, #190). Layout
audit fixes (aligned edges, no sideways scroll, one heading scale); the phone
menu after scrolling; two corrected claims about what `tsc` lets through;
distinct icons on the agents page.

**2026-11-19 · Phone, SEO, highlighting, content** (PRs #181, #191, #192,
#193). Tap targets and wrapping output on phones, one content width; link
previews, generated sitemap dates, structured data, self-hosted fonts, a 404
page, dated release notes, Markdown copies for agents; build-time syntax
highlighting for every code block; one generated count of checked cases, plainer
copy, an `npx`-first install, and a comparison with ReScript, Gleam and Elm.

**2026-11-20 · Every page in ten languages** (PRs #194, #196). A CSS hotfix
for desktop styles; all 48 non-release pages translated into nine languages;
the site check now fails on unbalanced CSS braces.

**2026-11-20 · "Where to go from here" as an index.** The six tinted tiles on
the home page became a numbered list with hairline rules, the whole row as the
link, and trying it moved to a dark strip with two buttons. Same text, so every
translation carried over unchanged.

**2026-11-20 · A payment example on the start page; "cases", not
"mistakes".** The first-program walkthrough breaks a payment `match` by adding
`Refunded`, the way an agent would, instead of deleting an arm, and the fix no
longer suggests an `else`. The checked TypeScript examples are called "cases"
everywhere they are counted.

## Next

1. **Launch readiness.** An "Open in playground" link on each case on
   `/catches/`; an FAQ that answers why the compiler is written in Rust and how
   Glyph was built; the README opening on a TypeScript-passes / Glyph-refuses
   pair; a 60 to 90 second demo.
2. **"Your agent can ask the compiler first."** A home section after the catches
   with a real MCP exchange: `glyph_impact` on a union change, the sites it
   names, the fix, and a passing check.
3. **One page per catch.** Each of the 37 cases as its own page, titled the way
   engineers search for the problem, generated from the case files.
4. **The scannable answer format for the remaining answers and the pillars**,
   as piloted on `/answers/verifiability/` and `/answers/typed-apis/`. Awaiting
   a decision.
5. **Search data.** The sitemap submitted to Search Console, indexing of all
   pages and languages checked, and the search queries that bring people in,
   read through seo-monster once its Google sign-in is set up.
6. **Translation polish.** Shorten translated descriptions that run past search
   result length; one number format policy across languages; remove the unused
   `Reference` menu key.
7. **Small corrections.** The manifesto's pillar order matches the site;
   `/teams/` generates its count of November releases instead of writing it by
   hand.
8. **Launch.** Show HN, then r/typescript and the other channels in the launch
   plan, posted by the author. Target: the week of 8 December 2026, or mid
   January.
