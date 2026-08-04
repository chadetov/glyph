// std/task — structured concurrency helpers over the JS Promise model.
//
// `all` runs a list of task thunks concurrently and joins them, returning the
// results in order. It is fail-fast (Promise.all): if one task's promise
// rejects, `all` rejects with that reason. `race` returns the first task to
// settle. Glyph tasks return values (or a `Result`), so the ordinary path
// collects every result; `all_settled` collects one outcome per task even when
// some reject, so a partial failure never loses the successes.
//
// Cancellation: JavaScript cannot force-cancel a task that is already running,
// so a failure in `all` abandons (ignores) its siblings' results rather than
// halting their work. For cooperative cancellation, thread an AbortSignal into
// your task bodies and check it. The scope here bounds the *lifetime you await*,
// which is what makes concurrent work joinable rather than detached.

export async function all<T>(
  tasks: ReadonlyArray<() => Promise<T> | T>,
): Promise<T[]> {
  return Promise.all(tasks.map((t) => t()));
}

export async function race<T>(
  tasks: ReadonlyArray<() => Promise<T> | T>,
): Promise<T> {
  return Promise.race(tasks.map((t) => t()));
}

// `pool` runs the tasks with at most `limit` in flight at once, joining their
// results in order. Unlike `all` (which starts every task immediately), a pool
// bounds concurrency, so it is the primitive for "dispatch to N destinations,
// but no more than K at a time". Fail-fast like `all`: the first task that
// rejects rejects the pool. A `limit` below 1 is treated as 1 (no deadlock).
export async function pool<T>(
  limit: number,
  tasks: ReadonlyArray<() => Promise<T> | T>,
): Promise<T[]> {
  const results = new Array<T>(tasks.length);
  const workers = Math.max(1, Math.min(Math.floor(limit), tasks.length || 1));
  let next = 0;
  async function run(): Promise<void> {
    while (next < tasks.length) {
      const i = next++;
      results[i] = await tasks[i]();
    }
  }
  await Promise.all(Array.from({ length: workers }, () => run()));
  return results;
}

// One task's outcome: `ok` with its value, or a failure with the thrown reason.
export type Settled<T> =
  | { readonly ok: true; readonly value: T }
  | { readonly ok: false; readonly error: unknown };

export async function all_settled<T>(
  tasks: ReadonlyArray<() => Promise<T> | T>,
): Promise<Array<Settled<T>>> {
  const results = await Promise.allSettled(tasks.map((t) => t()));
  return results.map((r) =>
    r.status === "fulfilled"
      ? { ok: true as const, value: r.value }
      : { ok: false as const, error: r.reason },
  );
}
