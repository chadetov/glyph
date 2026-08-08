// std/timers — run something later, or repeatedly, and stop it again.
//
// Scheduling is a global in JavaScript (`setTimeout`, `setInterval`) and Glyph
// resolves imported module names rather than ambient globals, so before this
// module the only way to schedule anything was a hand-written `.d.ts` for
// Node's `timers` or a raw `extern_ts` string. Every long-running program needs
// this: a heartbeat, a retry, a poll, a debounce.
//
// A `Timer` is opaque on purpose. Nothing about the underlying handle is useful
// to a Glyph program except giving it back to `cancel`, and keeping it opaque
// means this module is the same shape on any host that can schedule work.
//
// Timers keep the process alive while they are pending, which is what makes a
// scheduled program a program rather than a script that exits. `unref` opts a
// timer out of that when it is a background tick that should not by itself stop
// the process from finishing.

/** An opaque handle to scheduled work. Give it to `cancel` to stop it. */
export type Timer = { readonly __timer: unique symbol };

/**
 * Run `handler` once, after `delay_ms`.
 *
 * The process stays alive until it fires (or the timer is cancelled).
 */
export function after(delay_ms: number, handler: () => void): Timer {
  return setTimeout(handler, delay_ms) as unknown as Timer;
}

/**
 * Run `handler` every `interval_ms`, until cancelled.
 *
 * The first run is one interval away, not immediate. A repeating timer keeps
 * the process alive for as long as it is pending, so a program with one of
 * these does not end on its own.
 */
export function every(interval_ms: number, handler: () => void): Timer {
  return setInterval(handler, interval_ms) as unknown as Timer;
}

/**
 * Stop a timer, whether it was scheduled with `after` or `every`.
 *
 * Cancelling a timer that has already fired, or one that was already cancelled,
 * does nothing. That makes cleanup paths safe to run more than once, which
 * matters when a connection can be torn down by several routes.
 */
export function cancel(timer: Timer): void {
  // One call covers both: Node's clearTimeout and clearInterval accept either
  // handle, and neither complains about one that is already gone.
  clearTimeout(timer as unknown as ReturnType<typeof setTimeout>);
  clearInterval(timer as unknown as ReturnType<typeof setInterval>);
}

/**
 * Let the process exit even though this timer is still pending.
 *
 * A background tick (refreshing a cache, sampling a metric) should not be the
 * reason a program refuses to finish. Returns the same timer so it can be
 * written inline. On a host with no such notion this is a no-op.
 */
export function unref(timer: Timer): Timer {
  const t = timer as unknown as { unref?: () => void };
  if (typeof t.unref === "function") {
    t.unref();
  }
  return timer;
}

/**
 * Resolve after `delay_ms`, for `await`ing a pause inside an async function.
 *
 * `time.sleep` is the same thing spelled with a `Duration`; this is here so a
 * program that already imports `timers` does not need both.
 */
export function sleep(delay_ms: number): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, delay_ms));
}
