// std/stream — deterministic generators for property testing (Q11).
//
// A `Stream<T>` samples a value by index. Property testing draws a fixed,
// reproducible series of indices rather than random values, so a build that
// runs `@example test.property(...)` is deterministic (no RNG, no flaky tests).

export type Stream<T> = { sample: (i: number) => T };

/// Integers covering zero and both signs: 0, -1, 1, -2, 2, ...
export function ints(): Stream<number> {
  return { sample: (i) => (i % 2 === 0 ? i / 2 : -((i + 1) / 2)) };
}

/// Alternating booleans.
export function bools(): Stream<boolean> {
  return { sample: (i) => i % 2 === 0 };
}

/// Draw from a fixed list, cycling by index.
export function from<T>(values: ReadonlyArray<T>): Stream<T> {
  return { sample: (i) => values[i % values.length] };
}
