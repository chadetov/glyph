// std/json — JSON encode/decode. `parse` returns a `Result` whose `Err` is a
// list of `Issue`s (the same shape a record/schema parser reports), so a
// failure can be matched and surfaced like any other validation error.
//
// `parse<T>` decodes and casts (no shape check) — the escape hatch. The
// validating path is `parse_with`, which runs the decoded value through a
// `Schema<T>` (a type's auto-generated `T.schema`). The emitter rewrites
// `json.parse<T>(text)` to `json.parse_with(text, T.schema)` whenever `T` has a
// descriptor, so a typed parse validates against the type rather than trusting
// the input.

import { Result, Ok, Err } from "./result";

export function parse<T>(text: string): Result<T, Array<Issue>> {
  try {
    return Ok(JSON.parse(text) as T);
  } catch (e: unknown) {
    const message = e instanceof Error ? e.message : String(e);
    return Err([{ path: [], message }]);
  }
}

export function parse_with<T>(text: string, schema: Schema<T>): Result<T, Array<Issue>> {
  let decoded: unknown;
  try {
    decoded = JSON.parse(text);
  } catch (e: unknown) {
    const message = e instanceof Error ? e.message : String(e);
    return Err([{ path: [], message }]);
  }
  return schema.parse(decoded);
}

export function stringify(value: unknown, options?: { indent?: number }): string {
  return JSON.stringify(value, null, options?.indent);
}
