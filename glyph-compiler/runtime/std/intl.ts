// std/intl — locale-aware plurals, numbers, dates, lists and collation.
//
// JavaScript ships the CLDR data every localized program needs, behind the
// `Intl` global. Glyph resolves imported module names rather than ambient
// globals, so `new Intl.PluralRules(locale, {})` is `[E0103] unresolved name`
// and there was no route to it at all. Method forms that hang off a value
// (`n.toLocaleString(locale, opts)`, `a.localeCompare(b, locale)`) did pass
// through to TypeScript, so number and currency formatting were reachable but
// undocumented; everything namespace-only was not reachable by any spelling.
//
// The gap that mattered is plurals. An app that guesses `n === 1` is wrong in
// most of the world: Polish selects between one, few, many and other, and
// Arabic uses all six categories. The correct rules are ~200 locale-specific
// tables, and the host already has them.
//
// `plural_category` returns a **string-literal union**, not a bare string, so a
// `match` over it is exhaustive without a catch-all (D30). That is the point of
// wrapping rather than exposing `Intl` directly: the caller gets a closed set of
// answers the compiler can check they covered.

/**
 * The CLDR plural category `count` selects in `locale`.
 *
 * The six categories are the complete CLDR set. Which of them a locale ever
 * uses is the locale's business: `en` answers only `"one"`/`"other"`, `pl` adds
 * `"few"`/`"many"`, `ar` uses all six. A `match` covering the six is exhaustive
 * everywhere, which is why the return type names them rather than `string`.
 */
export function plural_category(
  locale: string,
  count: number,
): "zero" | "one" | "two" | "few" | "many" | "other" {
  return new Intl.PluralRules(locale).select(count) as
    | "zero"
    | "one"
    | "two"
    | "few"
    | "many"
    | "other";
}

/**
 * The plural category for an **ordinal** ("1st", "2nd", "3rd"), which follows
 * different rules from a count: English cardinals are one/other, but English
 * ordinals are one/two/few/other.
 */
export function ordinal_category(
  locale: string,
  count: number,
): "zero" | "one" | "two" | "few" | "many" | "other" {
  return new Intl.PluralRules(locale, { type: "ordinal" }).select(count) as
    | "zero"
    | "one"
    | "two"
    | "few"
    | "many"
    | "other";
}

/** `1234567.89` as the locale writes it: `1,234,567.89` in en, `1 234 567,89` in pl. */
export function format_number(locale: string, value: number): string {
  return new Intl.NumberFormat(locale).format(value);
}

/**
 * `value` with exactly `digits` fraction digits, padded and rounded.
 *
 * Separate from `format_number` because the common need ("always two decimals")
 * otherwise means constructing an options object, and Glyph has no spelling for
 * a partial one.
 */
export function format_fixed(locale: string, value: number, digits: number): string {
  return new Intl.NumberFormat(locale, {
    minimumFractionDigits: digits,
    maximumFractionDigits: digits,
  }).format(value);
}

/**
 * `value` as money in `currency` (an ISO 4217 code: `"USD"`, `"EUR"`, `"PLN"`).
 *
 * The symbol, its position, and the separators all come from the locale:
 * `$1,234.50`, `1.234,50 €`, `1234,50 zł`. This formats for display only —
 * for money *arithmetic* use `std/decimal`, which is exact.
 */
export function format_currency(locale: string, value: number, currency: string): string {
  return new Intl.NumberFormat(locale, { style: "currency", currency }).format(value);
}

/** `0.42` as `42%`, written the way the locale writes a percentage. */
export function format_percent(locale: string, value: number): string {
  return new Intl.NumberFormat(locale, { style: "percent" }).format(value);
}

/**
 * A list joined the way the locale joins one: `"a, b, and c"` in en,
 * `"a, b i c"` in pl. Not `join(", ")`, which is wrong in most languages.
 */
export function format_list(locale: string, items: Array<string>): string {
  return new Intl.ListFormat(locale).format(items);
}

/**
 * A relative time: `relative_time("en", -3, "day")` is `"3 days ago"`.
 *
 * `unit` is one of `"year"`, `"quarter"`, `"month"`, `"week"`, `"day"`,
 * `"hour"`, `"minute"`, `"second"`. A negative value is in the past.
 */
export function relative_time(
  locale: string,
  value: number,
  unit: "year" | "quarter" | "month" | "week" | "day" | "hour" | "minute" | "second",
): string {
  return new Intl.RelativeTimeFormat(locale, { numeric: "auto" }).format(value, unit);
}

/** A Unix-millisecond timestamp as the locale's date, with no time part. */
export function format_date(locale: string, epoch_ms: number): string {
  return new Intl.DateTimeFormat(locale).format(new Date(epoch_ms));
}

/** A Unix-millisecond timestamp as the locale's date and time. */
export function format_datetime(locale: string, epoch_ms: number): string {
  return new Intl.DateTimeFormat(locale, {
    dateStyle: "medium",
    timeStyle: "short",
  }).format(new Date(epoch_ms));
}

/**
 * Compare two strings the way `locale` sorts them: negative, zero or positive,
 * the shape `array.sort` wants.
 *
 * Not `a < b`, which compares UTF-16 code units and puts `Z` before `a` and
 * every accented letter after `z`.
 */
export function compare(locale: string, a: string, b: string): number {
  return new Intl.Collator(locale).compare(a, b);
}

/**
 * The locale the host would actually use for `requested`, or `""` when it
 * supports none of them.
 *
 * A fallback chain is the caller's to define, but knowing whether `"pl-PL"`
 * resolves to `pl-PL`, to `pl`, or to nothing is what decides it.
 */
export function best_locale(requested: Array<string>): string {
  const supported = Intl.NumberFormat.supportedLocalesOf(requested);
  return supported.length > 0 ? supported[0] : "";
}
