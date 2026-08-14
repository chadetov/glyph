// The `Schema<T>` factory behind a record type's auto-generated `T.schema`
// member (Q8/Q40). `Schema<T>` itself is an ambient prelude type
// (`glyph-prelude.d.ts`); this factory builds one from a type guard so the
// recursive `array()` method (`Schema<T>` -> `Schema<Array<T>>`) can be
// expressed without inlining it at every record descriptor.
//
// The emitter emits
// `T.schema = schema<T>("T", (v): v is T => T.is(v), (v) => T.parse(v))`,
// reusing both halves of the descriptor lazily, so the descriptor const is
// fully initialized by the time either runs.
//
// The third argument is what makes `json.parse<T>(text)` report the same
// field paths as `T.parse` (G68). Built from the guard alone, a schema can
// only answer yes or no, so every field-level failure collapsed to one issue
// reading `expected T` — and the one-step form is the one the guide teaches.
// It stays optional: a schema over a type with no descriptor still has a
// guard and nothing deeper to report.

import { type Result, Ok, Err } from "std/result";

export function schema<T>(
  name: string,
  is: (value: unknown) => value is T,
  parse_deep?: (value: unknown) => Result<T, Array<Issue>>,
): Schema<T> {
  return {
    name,
    parse(input: unknown): Result<T, Array<Issue>> {
      if (parse_deep !== undefined) {
        return parse_deep(input);
      }
      return is(input)
        ? Ok(input)
        : Err([{ path: [], message: `expected ${name}` }]);
    },
    array(): Schema<Array<T>> {
      // Element failures keep their own paths, prefixed by the index, so a bad
      // third row reports `2.port` rather than `expected T[]`.
      const parse_each =
        parse_deep === undefined
          ? undefined
          : (value: unknown): Result<Array<T>, Array<Issue>> => {
              if (!Array.isArray(value)) {
                return Err([{ path: [], message: `expected ${name}[]`, code: "type" }]);
              }
              const out: Array<T> = [];
              const issues: Array<Issue> = [];
              for (let i = 0; i < value.length; i++) {
                const r = parse_deep(value[i]);
                if (r.tag === "Err") {
                  for (const issue of r.value) {
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
      return schema<Array<T>>(
        `${name}[]`,
        (value): value is Array<T> =>
          Array.isArray(value) && value.every((item) => is(item)),
        parse_each,
      );
    },
  };
}
