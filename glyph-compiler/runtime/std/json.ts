// std/json — JSON encode/decode. `parse` returns a `Result` whose `Err` is a
// list of `Issue`s (the same shape a record/schema parser reports), so a
// failure can be matched and surfaced like any other validation error.
//
// `parse<T>` decodes and casts (no shape check) — the escape hatch. The
// validating path is `parse_with`, which runs the decoded value through a
// `Schema<T>` (a type's auto-generated `T.schema`). The emitter rewrites the
// namespace form `json.parse<T>(text)` to `json.parse_with(text, <schema>)`
// when a schema can be derived for `T` — a record/union descriptor (`T.schema`)
// or an `Array<T>` thereof (`T.schema.array()`). A type with no descriptor (a
// primitive, an imported type) keeps the casting `parse`. (The named-import
// form `import std/json { parse }; parse<T>(...)` is not rewritten; use the
// `json.parse<T>` namespace form to get validation.)

import { Result, Ok, Err } from "./result";
import { Option, Some, None } from "./option";

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

/// Read a string-valued property from an already-decoded value, or `None` if
/// the value is not an object or the property is missing/non-string. Used to
/// dispatch a discriminated union on its discriminator property (a generated
/// `gen openapi`/`gen zod` union whose variants are distinguished by a named
/// property carrying a string tag, e.g. `{ "petType": "cat", ... }`).
export function discriminant(value: unknown, field: string): Option<string> {
  if (value !== null && typeof value === "object") {
    const v = (value as Record<string, unknown>)[field];
    if (typeof v === "string") {
      return Some(v);
    }
  }
  return None;
}
