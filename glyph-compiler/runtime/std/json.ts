// std/json — JSON encode/decode. `parse` returns a `Result` whose `Err` is a
// list of `Issue`s (the same shape a record/schema parser reports), so a
// failure can be matched and surfaced like any other validation error. v1 does
// no schema validation against `T` — it decodes the JSON and asserts the shape;
// `T.parse` (the runtime descriptor) is the validating path.

import { Result, Ok, Err } from "./result";

export function parse<T>(text: string): Result<T, Array<Issue>> {
  try {
    return Ok(JSON.parse(text) as T);
  } catch (e: unknown) {
    const message = e instanceof Error ? e.message : String(e);
    return Err([{ path: [], message }]);
  }
}

export function stringify(value: unknown, options?: { indent?: number }): string {
  return JSON.stringify(value, null, options?.indent);
}
