# i18n

## What it is

A localized message formatter. It loads one JSON catalogue per locale,
validates each against the reference locale before anything renders (every key
present, the same placeholder set, exactly the plural categories that locale's
CLDR rules can select), then formats with named placeholders, CLDR plural
selection, locale-aware numbers and currency, and a `pl-PL` to `pl` to `en`
fallback chain that reports which locale actually answered.

## Running it

```sh
glyph run examples/apps/i18n/main.glyph --locale pl-PL
glyph run examples/apps/i18n/main.glyph --check
glyph run examples/apps/i18n/main.glyph --dir examples/apps/i18n/broken --check
```

## What it changed in Glyph

Shipped **0.1.74**.

**G113: `Intl` was unreachable, so CLDR plural data had no route.** It is a host
global and Glyph resolves names from modules, so `new Intl.PluralRules(loc, {})`
was `[E0103] unresolved name`. Method forms passed through and were type-checked,
so locale-aware number, percent, currency and collation were all available and
undocumented, but a reusable formatter had no route at all. The fix shipped
`std/intl`, whose `plural_category` returns the string-literal union of the six
CLDR categories, so a match over it is exhaustive. An app branching on `n == 1`
is wrong in most of the world; Polish alone needs one, few and many.

Worth stating plainly: **this app has not been ported onto the fix it caused.**
`format.glyph` still uses the method-form path and its header still states the
gap verbatim, and `plural.glyph` hand-writes the English and Polish rules rather
than calling `intl.plural_category`. It documents a gap that is closed.

## What it exercises

A six-variant nullary union with exhaustive match, a defect union with record
payloads, `Record<string, T>` maps with explicit type arguments, and boundary
validation per catalogue. Twenty-six `@example` rows.
