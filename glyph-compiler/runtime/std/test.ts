// std/test — property testing (Q11 -> Option A). `property` is a plain stdlib
// function, not a language primitive; it is invoked inside an `@example` or a
// `@doc @run` block and runs at build time.
//
// It checks `predicate` over a deterministic sample of `gen` and returns a
// `Result`: `Ok(void)` when every sample passes, or `Err` describing the first
// counterexample. Compose it with `@example` as
// `@example test.property(pred, stream.ints()) == Ok(void)`.

import { Result, Ok, Err } from "./result";
import { Stream } from "./stream";

export function property<T>(
  predicate: (x: T) => boolean,
  gen: Stream<T>,
  count?: number,
): Result<void, string> {
  const n = count ?? 100;
  for (let i = 0; i < n; i++) {
    const x = gen.sample(i);
    if (!predicate(x)) {
      return Err(`property failed for ${JSON.stringify(x)}`);
    }
  }
  return Ok(undefined);
}
