// std/time — clock, delays, debouncing, and calendar helpers. `now` is epoch
// milliseconds; `sleep` resolves after a `Duration` (a Glyph caller `await`s
// it). ISO-8601 is the string form (`format_iso`/`parse_iso`), and the calendar
// accessors and `add_*` helpers work in UTC, so results are stable regardless of
// the host timezone. `Duration` is both a type and its constructor factory.

import { None, Option, Some } from "./option";

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

// An epoch-milliseconds instant as an ISO-8601 UTC string, e.g.
// "2026-07-25T18:33:08.000Z".
export function format_iso(epoch_ms: number): string {
  return new Date(epoch_ms).toISOString();
}

// Parse an ISO-8601 string to epoch milliseconds, or `None` if it is not a
// valid date.
export function parse_iso(iso: string): Option<number> {
  const t = Date.parse(iso);
  return Number.isNaN(t) ? None : Some(t);
}

export function add_days(epoch_ms: number, days: number): number {
  return epoch_ms + days * 86_400_000;
}

export function add_hours(epoch_ms: number, hours: number): number {
  return epoch_ms + hours * 3_600_000;
}

// UTC calendar accessors. `month` is 1-12 (not JavaScript's 0-11).
export function year(epoch_ms: number): number {
  return new Date(epoch_ms).getUTCFullYear();
}

export function month(epoch_ms: number): number {
  return new Date(epoch_ms).getUTCMonth() + 1;
}

export function day(epoch_ms: number): number {
  return new Date(epoch_ms).getUTCDate();
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
