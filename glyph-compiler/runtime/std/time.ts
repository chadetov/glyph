// std/time — clock, delays, and debouncing. `now` is epoch milliseconds;
// `sleep` resolves after a `Duration` (a Glyph caller `await`s it). `Duration`
// is both a type and its constructor factory.

export type Duration = { readonly ms: number };

export const Duration: { ms(milliseconds: number): Duration } = {
  ms(milliseconds: number): Duration {
    return { ms: milliseconds };
  },
};

export function now(): number {
  return Date.now();
}

export function sleep(duration: Duration): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, duration.ms));
}

export function debounce<A extends ReadonlyArray<unknown>>(
  delay: Duration,
  f: (...args: A) => void,
): (...args: A) => void {
  let handle: ReturnType<typeof setTimeout> | null = null;
  return (...args: A): void => {
    if (handle !== null) {
      clearTimeout(handle);
    }
    handle = setTimeout(() => f(...args), delay.ms);
  };
}
