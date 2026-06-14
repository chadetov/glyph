// std/array — list helpers. Thin wrappers over native Array methods so the
// runtime behavior matches the Glyph stdlib signatures; `find` returns the
// prelude `Option` rather than `undefined`.

import { Option, Some, None } from "./option";

export function find<T>(xs: ReadonlyArray<T>, predicate: (x: T) => boolean): Option<T> {
  for (const x of xs) {
    if (predicate(x)) {
      return Some(x);
    }
  }
  return None;
}

export function filter<T>(xs: ReadonlyArray<T>, predicate: (x: T) => boolean): Array<T> {
  return xs.filter(predicate);
}

export function map<T, U>(xs: ReadonlyArray<T>, f: (x: T) => U): Array<U> {
  return xs.map(f);
}

export function zip<A, B, C>(
  a: ReadonlyArray<A>,
  b: ReadonlyArray<B>,
  combine: (a: A, b: B) => C,
): Array<C> {
  const n = Math.min(a.length, b.length);
  const out: Array<C> = [];
  for (let i = 0; i < n; i++) {
    out.push(combine(a[i], b[i]));
  }
  return out;
}
