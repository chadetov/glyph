// std/time — clock, delays, debouncing, and calendar helpers. `now` is epoch
// milliseconds; `sleep` resolves after a `Duration` (a Glyph caller `await`s
// it). ISO-8601 is the string form (`format_iso`/`parse_iso`), and the calendar
// accessors and `add_*` helpers work in UTC, so results are stable regardless of
// the host timezone. `Duration` is both a type and its constructor factory.

import { None, type Option, Some } from "./option";

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

// The two accepted shapes: a bare calendar date, or a date and time carrying an
// explicit UTC designator or numeric offset. An offset-less datetime such as
// "2026-01-03T10:00" is deliberately absent, because ECMAScript reads that form
// in the host's local time.
const ISO_SHAPE =
  /^(\d{4})-(\d{2})-(\d{2})(?:T\d{2}:\d{2}(?::\d{2}(?:\.\d+)?)?(?:Z|[+-]\d{2}:\d{2}))?$/;

function days_in_month(year: number, month: number): number {
  if (month === 2) {
    const leap = year % 4 === 0 && (year % 100 !== 0 || year % 400 === 0);
    return leap ? 29 : 28;
  }
  return month === 4 || month === 6 || month === 9 || month === 11 ? 30 : 31;
}

// Parse an ISO-8601 string to epoch milliseconds, or `None` if it is not one.
// Accepted: a bare date `YYYY-MM-DD` (UTC midnight), or
// `YYYY-MM-DDTHH:MM(:SS)?(.sss)?` followed by `Z`, `+HH:MM`, or `-HH:MM`.
// Everything else is `None`, including forms `Date.parse` would take: an
// offset-less datetime ("2026-01-03T10:00"), a non-padded date ("2026-1-3"),
// and free-form text ("January 5 2026"). Those three are read in the host's
// local time, which would break the UTC guarantee this file's header makes and
// that `year`/`month`/`day` depend on: the same string would name a different
// calendar day depending on where the process runs. An impossible day is `None`
// too ("2026-02-31"), which `Date.parse` reports as success after rolling it
// over to March 3.
export function parse_iso(iso: string): Option<number> {
  const shape = ISO_SHAPE.exec(iso);
  if (shape === null) {
    return None;
  }
  const t = Date.parse(iso);
  if (Number.isNaN(t)) {
    return None;
  }
  const year = Number(shape[1]);
  const month = Number(shape[2]);
  const day = Number(shape[3]);
  if (month < 1 || month > 12 || day < 1 || day > days_in_month(year, month)) {
    return None;
  }
  return Some(t);
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
