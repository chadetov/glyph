# i18n

## What it is

A localized message formatter. It loads one JSON catalogue per locale,
validates each against the reference locale before anything renders (every key
present, the same placeholder set, exactly the plural categories that locale's
CLDR rules can select), then formats with named placeholders, CLDR plural
selection, locale-aware numbers and currency, and a `pl-PL` to `pl` to `en`
fallback chain that reports which locale actually answered.

`catalog.glyph` owns loading and validation, `pattern.glyph` fills named
placeholders into a message body, `plural.glyph` picks the CLDR category a
count selects, `format.glyph` formats numbers and currency, and `render.glyph`
ties them together into the two calls an application actually makes:
`render.text` for a plain message and `render.counted` for one that varies by
count. `main.glyph` is a small CLI over all of it.

## Running it

```sh
glyph run examples/apps/i18n/main.glyph --locale pl-PL
glyph run examples/apps/i18n/main.glyph --check
glyph run examples/apps/i18n/main.glyph --dir examples/apps/i18n/broken --check
```

## What it exercises

A six-variant nullary union (`PluralCategory`) with exhaustive match, a defect
union with record payloads, `Record<string, T>` maps with explicit type
arguments, and boundary validation per catalogue against untrusted JSON. 25
`@example` rows across `pattern.glyph`, `plural.glyph` and `catalog.glyph`.

## What it found, and what happened

**G113: `Intl` was unreachable, so CLDR plural data had no route.** `Intl` is a
host global and Glyph resolves names from modules, so `new
Intl.PluralRules(locale, {})` was `[E0103] unresolved name`. Method forms hung
off a value did pass through to TypeScript and were type-checked
(`value.toLocaleString(locale, options)`, `a.localeCompare(b, locale)`), so
locale-aware number, percent, currency and collation were reachable the whole
time, just undocumented. What had no method form had no route at all:
`PluralRules`, and a reusable `NumberFormat`/`Collator`/`DateTimeFormat`. This
app's own `plural.glyph` was the evidence: it hand-wrote the English and Polish
CLDR rules (`select_en`, `select_pl`, about forty lines of `i % 10` and `i %
100` arithmetic) because there was no way to ask the host for the real ~200
rule sets.

Fixed in 0.1.74 by `std/intl`. Its `plural_category(locale, count)` calls
`Intl.PluralRules` and returns the string-literal union of the six CLDR
categories, so a `match` over it is exhaustive with no catch-all.

`plural.glyph` is now ported onto the fix: `select` is

```glyph
pub fn select(locale: string, count: number) -> PluralCategory {
  return category_from_name(intl.plural_category(locale, count))
}
```

`select_en`, `select_pl`, and their `integer_part`/`has_fraction` helpers are
gone. The module still owns its own `PluralCategory` union and
`category_from_name`, because a catalogue and `render.glyph` match over that
union, not over the bare string `std/intl` returns; the six-variant type is
what keeps that match exhaustive, and it is not something `std/intl` is
responsible for. Every existing `@example` in `plural.glyph` still passes
unchanged, and the `pl-PL` demo still prints the right Polish plural for 0, 1,
2, 5, 12, 22 and 112 (12 and 112 both land on "many" because 12 and 112 mod 100
fall in the 12-14 exception the naive `i % 10` rule alone would miss), now
answered by the host's own CLDR data instead of forty lines that reimplemented
it.

`format.glyph` was not changed. Its method-form calls
(`value.toLocaleString(...)`, `a.localeCompare(b, locale)`) were never blocked
by G113: the gap ledger is explicit that method forms passed through
before the fix too. `std/intl` now gives those same operations named,
documented functions (`format_number`, `format_currency`, `compare`, and so
on), but the method-form path `format.glyph` uses is still valid Glyph, still
type-checked against `Intl.NumberFormatOptions`, and still a fine thing for a
reader to learn: the split between this file and `plural.glyph` now shows the
two real routes to `Intl` side by side, the one that was always reachable and
the one that needed a stdlib wrapper.

## What is deliberately still awkward

Nothing currently. G113 was the only gap this app is recorded as having
surfaced, and it is closed and ported. If a future gap shows up here, record it
in `docs/dogfooding-gaps.md` and describe the awkward shape it forces in this
section, rather than working around it quietly.
