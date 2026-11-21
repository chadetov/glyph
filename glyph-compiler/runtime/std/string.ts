// std/string — string helpers. Thin wrappers over native String methods, with
// two deliberate divergences from TypeScript: a search that can fail returns the
// prelude `Option` rather than a `-1` sentinel, and `repeat` clamps a negative
// count instead of throwing (Glyph has no try/catch, so a `RangeError` crossing
// the boundary would be unrecoverable).
//
// Indices are UTF-16 code units, the same space `len` and `split` already use.
// No accessor here returns half a surrogate pair, because the half reads as a
// character until it is written out. `char_at` and `code_point_at` take a
// code-unit index and return the whole code point that starts there, or `None`
// when the index is out of range or falls on the second half of a pair or on a
// lone surrogate (amended in 0.1.161, decision record 154). There is still no
// `chars`.
//
// Argument order: this module is subject-first, so the string being operated on
// is always the first argument. `std/regex` is the one module that is
// pattern-first, so `regex.replace_all(pattern, text, replacement)` and
// `regex.split(pattern, text)` take their subject second. Every parameter of
// both pairs is a `string`, so swapping them compiles and runs.

import { type Option, Some, None } from "./option";

export function from(value: unknown): string {
  return String(value);
}

export function join(parts: ReadonlyArray<string>, separator: string): string {
  return parts.join(separator);
}

export function split(s: string, separator: string): Array<string> {
  return s.split(separator);
}

export function len(s: string): number {
  return s.length;
}

export function trim(s: string): string {
  return s.trim();
}

export function lower(s: string): string {
  return s.toLowerCase();
}

export function upper(s: string): string {
  return s.toUpperCase();
}

export function contains(s: string, substring: string): boolean {
  return s.includes(substring);
}

export function starts_with(s: string, prefix: string): boolean {
  return s.startsWith(prefix);
}

export function ends_with(s: string, suffix: string): boolean {
  return s.endsWith(suffix);
}

// `repeat` clamps a negative count to `""`, where TS `String.prototype.repeat`
// throws a `RangeError`. That clamp is what makes `repeat(pad, width - len(s))`
// safe when the string is already at or past the width. A fractional count
// truncates, which is what TS does too (`String.prototype.repeat` runs
// `ToIntegerOrInfinity` first), so that case is not a divergence. An infinite
// count still throws.
export function repeat(s: string, count: number): string {
  return s.repeat(Math.max(0, Math.trunc(count)));
}

// `pad_start`/`pad_end` are a no-op when the string is already at least `width`
// units long. A multi-character `pad` is truncated at the boundary. `pad`
// defaults to a single space.
export function pad_start(s: string, width: number, pad?: string): string {
  return s.padStart(width, pad ?? " ");
}

export function pad_end(s: string, width: number, pad?: string): string {
  return s.padEnd(width, pad ?? " ");
}

// `slice` matches `array.slice`: `end` is exclusive and defaults to the end of
// the string, and a negative index counts back from the end.
export function slice(s: string, start: number, end?: number): string {
  return s.slice(start, end);
}

// `index_of` returns the prelude `Option` rather than TS's `-1` sentinel. The
// optional `from` starts the search at that index, so "the next occurrence at or
// after here" needs no arithmetic on a sentinel.
export function index_of(s: string, needle: string, from?: number): Option<number> {
  const i = s.indexOf(needle, from ?? 0);
  return i < 0 ? None : Some(i);
}

// The code point that starts at code-unit index `i`, or `undefined` when no
// whole one does: `i` is not an integer or is out of range, or the unit there
// is the second half of a surrogate pair, or a surrogate with no partner.
function whole_code_point(s: string, i: number): number | undefined {
  if (!Number.isInteger(i) || i < 0 || i >= s.length) {
    return undefined;
  }
  const unit = s.charCodeAt(i);
  if (unit >= 0xdc00 && unit <= 0xdfff) {
    return undefined;
  }
  if (unit >= 0xd800 && unit <= 0xdbff) {
    const next = i + 1 < s.length ? s.charCodeAt(i + 1) : 0;
    return next >= 0xdc00 && next <= 0xdfff ? s.codePointAt(i) : undefined;
  }
  return unit;
}

// The character that starts at code-unit index `i`, as a string of one or two
// units, or `None`. `s.at(i)` answers `undefined` past the end, counts from the
// end for a negative `i`, and hands back half a pair; this does none of those
// (G392, decision record 154). On "a😀", index 1 is `Some("😀")` and index 2,
// the emoji's second half, is `None`.
export function char_at(s: string, i: number): Option<string> {
  const cp = whole_code_point(s, i);
  return cp === undefined ? None : Some(String.fromCodePoint(cp));
}

// The code point that starts at code-unit index `i`, or `None`, by the same
// rule as `char_at`. `s.codePointAt(i)` answers the value of a lone second half
// as if it were a character.
export function code_point_at(s: string, i: number): Option<number> {
  const cp = whole_code_point(s, i);
  return cp === undefined ? None : Some(cp);
}

// `replace_all` replaces every occurrence, unlike TS
// `String.prototype.replace`, which replaces only the first. There is no
// first-only form.
export function replace_all(s: string, from: string, to: string): string {
  return s.replaceAll(from, to);
}

export function trim_start(s: string): string {
  return s.trimStart();
}

export function trim_end(s: string): string {
  return s.trimEnd();
}
