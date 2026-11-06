/// <reference path="../glyph-prelude.d.ts" />
// std/json — JSON encode/decode. `parse` returns a `Result` whose `Err` is a
// list of `Issue`s (the same shape a record/schema parser reports; the ambient
// `Issue`/`Schema` types come from the reference above, so this file compiles
// in a host project that never reads the generated tsconfig), so a
// failure can be matched and surfaced like any other validation error.
//
// `parse_with` is the validating path: it runs the decoded value through a
// `Schema<T>`. The emitter rewrites `parse<T>(text)` with a written `T` to
// `parse_with(text, <schema for T>)`, in both import spellings (the
// namespace form `json.parse<T>` and the named `import std/json { parse }`,
// which calls it through a hidden aliased import). The schema is `T.schema`
// for a record or union, `T.schema.array()` for an `Array` of one, and for any
// other `T` (a primitive, `Option`, `Nullable`, `Record`, a literal union) a
// schema built from the check a record field of type `T` gets.
//
// A record holding a field with no runtime check is refused (E0304).
//
// `parse` itself decodes without checking. The emitter leaves it in place for
// `parse(text)` with no type argument and for `parse<unknown>`, which claim
// nothing about the value. A `T` the emitter has no check for at all (a type
// parameter, an `extern_ts` type, a package type) also still reaches it, and
// is cast.

import { type Result, Ok, Err } from "./result";
import { type Option, Some, None } from "./option";

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
