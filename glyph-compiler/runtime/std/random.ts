// std/random — a seeded, reproducible pseudo-random generator.
//
// Native kernel: this module stays TypeScript on purpose. mulberry32 is the
// equivalent of a libc numeric primitive, and the runtime prelude is hand-written
// TS by design. Glyph itself can now express the core (the shift operators `>>>`
// and `<<` it needs are supported, D36), so this is a native-core choice, not a
// dogfooding gap.
//
// `seeded(n)` returns an `Rng` whose sequence is fully determined by the seed,
// so a test or simulation replays identically. It is a small, fast PRNG
// (mulberry32), suitable for generation and sampling, *not* for cryptography,
// use `std/crypto` for security-sensitive randomness.

export type Rng = {
  // The next float in [0, 1).
  next: () => number;
  // A whole number in the half-open range [lo, hi).
  int: (lo: number, hi: number) => number;
  // A boolean, true with the given probability (default 0.5).
  bool: (probability?: number) => boolean;
  // A uniformly chosen element of a non-empty array.
  pick: <T>(items: ReadonlyArray<T>) => T;
};

export function seeded(seed: number): Rng {
  let state = seed >>> 0;
  const nextFloat = (): number => {
    state = (state + 0x6d2b79f5) | 0;
    let t = Math.imul(state ^ (state >>> 15), 1 | state);
    t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t;
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
  return {
    next: nextFloat,
    int: (lo: number, hi: number) => lo + Math.floor(nextFloat() * (hi - lo)),
    bool: (probability: number = 0.5) => nextFloat() < probability,
    pick: <T>(items: ReadonlyArray<T>) => items[Math.floor(nextFloat() * items.length)],
  };
}
