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

export function any<T>(xs: ReadonlyArray<T>, predicate: (x: T) => boolean): boolean {
  return xs.some(predicate);
}

// `contains` uses primitive value equality (`===`); for records, prefer
// `any(xs, x => ...)` with an explicit comparison.
export function contains<T>(xs: ReadonlyArray<T>, value: T): boolean {
  return xs.includes(value);
}

// `sort` is value-oriented: it returns a new sorted array and never mutates the
// input. `compare(a, b)` returns a negative number if `a` sorts before `b`.
export function sort<T>(xs: ReadonlyArray<T>, compare: (a: T, b: T) => number): Array<T> {
  return [...xs].sort(compare);
}

// `fold` is TS `reduce` with the argument order the rest of this module uses:
// the collection first, the seed second, the callback last, so it reads like
// `map(xs, f)` and `sort(xs, compare)`. The callback takes `(acc, x)` only —
// there is no index and no source-array argument.
export function fold<T, A>(xs: ReadonlyArray<T>, init: A, f: (acc: A, x: T) => A): A {
  return xs.reduce(f, init);
}

// `index_of` uses primitive value equality (`===`), like `contains`; for
// records, prefer `find`/`any` with an explicit comparison. It returns the
// prelude `Option` rather than TS's `-1` sentinel.
export function index_of<T>(xs: ReadonlyArray<T>, value: T): Option<number> {
  const i = xs.indexOf(value);
  return i < 0 ? None : Some(i);
}

// `flat_map` flattens exactly one level, like TS `flatMap`.
export function flat_map<T, U>(xs: ReadonlyArray<T>, f: (x: T) => Array<U>): Array<U> {
  return xs.flatMap(f);
}

// `range`/`range_from` are the counted loops `for` has no other source for:
// `for i in range(n)` walks `0..n-1`. `range` clamps like `string.repeat` does,
// so a negative or fractional `count` becomes `Math.max(0, Math.trunc(count))`
// and `range(-1)` is `[]` rather than a throw.
export function range(count: number): Array<number> {
  const n = Math.max(0, Math.trunc(count));
  const out: Array<number> = [];
  for (let i = 0; i < n; i++) {
    out.push(i);
  }
  return out;
}

// `range_from(start, end)` walks `start..end-1`. The second argument is an
// exclusive end bound, the same reading `array.slice` and `string.slice` give a
// second numeric argument, so `range_from(2, 5)` is `[2, 3, 4]`. An end at or
// below `start` gives `[]`.
export function range_from(start: number, end: number): Array<number> {
  const lo = Math.trunc(start);
  const hi = Math.trunc(end);
  const out: Array<number> = [];
  for (let i = lo; i < hi; i++) {
    out.push(i);
  }
  return out;
}
