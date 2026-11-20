/// <reference path="../glyph-prelude.d.ts" />
// The `Schema<T>` factory behind a record type's auto-generated `T.schema`
// member (Q8/Q40), and the depth limit every generated runtime check obeys
// (D50). `Schema<T>` itself is an ambient prelude type
// (`glyph-prelude.d.ts`), pulled in by the reference above so this file
// compiles in a host project that never reads the generated tsconfig (G122);
// this factory builds one from a type guard so the
// recursive `array()` method (`Schema<T>` -> `Schema<Array<T>>`) can be
// expressed without inlining it at every record descriptor.
//
// The emitter emits
// `T.schema = schema<T>("T", (v, d): v is T => T.__is(v, d), (v, d) => T.__parse(v, d))`,
// reusing both halves of the descriptor lazily, so the descriptor const is
// fully initialized by the time either runs.
//
// The third argument is what makes `json.parse<T>(text)` report the same
// field paths as `T.parse` (G68). Built from the guard alone, a schema can
// only answer yes or no, so every field-level failure collapsed to one issue
// reading `expected T` — and the one-step form is the one the guide teaches.
// It stays optional: a schema over a type with no descriptor still has a
// guard and nothing deeper to report.
//
// Depth (D50, G369). Every generated check refuses a value nested more than
// `MAX_DEPTH` objects and arrays deep. The limit is exported here, as the
// public contract a program may read; the helpers the generated checks call
// to enforce it, and the private sentinel they throw, live in
// `../internal/depth`, which no Glyph program can import, so no Glyph code
// can throw the sentinel or forge a depth refusal. The factory below hands
// `is_at` and `parse_at` the depth of the value they check, 1 at the root,
// and an `array()` schema checks its elements one level under the array.

import { type Result, Ok, Err } from "./result";

import { MAX_DEPTH as LIMIT, depth_issue, depth_refused, every_at, too_deep } from "../internal/depth";

/// The deepest a value may nest, in objects and arrays, for a generated check
/// to accept it (D50). The value is defined once, in `../internal/depth`, and
/// emitted code names it `MAX_DEPTH` too.
export const MAX_DEPTH: number = LIMIT;

/// Run a check for a schema built from a guard alone: its answer, or
/// `undefined` when it threw the depth sentinel. Anything else it throws
/// propagates.
function depth_checked(check: () => boolean): boolean | undefined {
  try {
    return check();
  } catch (e: unknown) {
    depth_refused(e);
    return undefined;
  }
}

export function schema<T>(
  name: string,
  is_at: (value: unknown, depth: number) => value is T,
  parse_at?: (value: unknown, depth: number) => Result<T, Array<Issue>>,
): Schema<T> {
  const parse_deep =
    parse_at ??
    ((value: unknown, depth: number): Result<T, Array<Issue>> => {
      const ok = depth_checked(() => is_at(value, depth));
      if (ok === undefined) {
        return Err([depth_issue([])]);
      }
      return ok ? Ok(value as T) : Err([{ path: [], message: `expected ${name}` }]);
    });
  return {
    name,
    parse(input: unknown): Result<T, Array<Issue>> {
      return parse_deep(input, 1);
    },
    array(): Schema<Array<T>> {
      // The array is one level; each element sits one level under it.
      const is_each = (value: unknown, depth: number): value is Array<T> =>
        Array.isArray(value) &&
        (depth <= MAX_DEPTH || too_deep()) &&
        every_at(value, depth + 1, is_at);
      // Element failures keep their own paths, prefixed by the index, so a bad
      // third row reports `2.port` rather than `expected T[]`.
      const parse_each =
        parse_at === undefined
          ? undefined
          : (value: unknown, depth: number): Result<Array<T>, Array<Issue>> => {
              if (!Array.isArray(value)) {
                return Err([{ path: [], message: `expected ${name}[]`, code: "type" }]);
              }
              if (depth > MAX_DEPTH) {
                return Err([depth_issue([])]);
              }
              const out: Array<T> = [];
              const issues: Array<Issue> = [];
              for (let i = 0; i < value.length; i++) {
                const r = parse_at(value[i], depth + 1);
                if (r.tag === "Err") {
                  for (const issue of r.value) {
                    if (issue.code === "depth") {
                      return Err([depth_issue([i, ...issue.path])]);
                    }
                    issues.push({
                      path: [i, ...issue.path],
                      message: issue.message,
                      code: issue.code,
                    });
                  }
                } else {
                  out.push(r.value);
                }
              }
              return issues.length > 0 ? Err(issues) : Ok(out);
            };
      return schema<Array<T>>(`${name}[]`, is_each, parse_each);
    },
  };
}
