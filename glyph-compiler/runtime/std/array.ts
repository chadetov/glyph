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

export function len<T>(xs: ReadonlyArray<T>): number {
  return xs.length;
}

// `push`/`concat`/`reverse` are value-oriented: they return a new array and
// never mutate the input. In-place mutation is the `mut xs.push(x)` statement.
export function push<T>(xs: ReadonlyArray<T>, x: T): Array<T> {
  return [...xs, x];
}

export function concat<T>(a: ReadonlyArray<T>, b: ReadonlyArray<T>): Array<T> {
  return [...a, ...b];
}

export function reverse<T>(xs: ReadonlyArray<T>): Array<T> {
  return [...xs].reverse();
}

export function slice<T>(xs: ReadonlyArray<T>, start: number, end?: number): Array<T> {
  return xs.slice(start, end);
}
