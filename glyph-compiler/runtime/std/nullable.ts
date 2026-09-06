// The Glyph `Nullable<T>` type and its bridge to `Option` (D45).
//
// A `Nullable<T>` is exactly TypeScript's `T | null`: the shape a real API
// sends for a field that may be missing a value, and the shape `json.stringify`
// writes back out, so the wire form round-trips in both directions. The emitter
// spells the type inline as `T | null` and never imports it; this alias exists
// so the runtime's own signatures can name it.
//
// It is deliberately not an `Option`. There is no `match` on a `Nullable` and no
// `Some`/`None` for it; the crossing is one of the calls below, so the places a
// null from outside becomes a value the program reasons about are greppable.

import { type Option, Some, None } from "./option";

export type Nullable<T> = T | null;

/// `null` becomes `None`; anything else becomes `Some(value)`.
export function to_option<T>(n: Nullable<T>): Option<T> {
  return n === null ? None : Some(n);
}

/// `None` becomes `null`; `Some(value)` becomes `value`.
export function from_option<T>(o: Option<T>): Nullable<T> {
  return o.tag === "Some" ? o.value : null;
}

/// True for `null`, false for a value.
export function is_null<T>(n: Nullable<T>): boolean {
  return n === null;
}
