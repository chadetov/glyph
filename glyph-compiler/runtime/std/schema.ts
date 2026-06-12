// The `Schema<T>` factory behind a record type's auto-generated `T.schema`
// member (Q8/Q40). `Schema<T>` itself is an ambient prelude type
// (`glyph-prelude.d.ts`); this factory builds one from a type guard so the
// recursive `array()` method (`Schema<T>` -> `Schema<Array<T>>`) can be
// expressed without inlining it at every record descriptor.
//
// The emitter emits `T.schema = schema<T>("T", (v): v is T => T.is(v))`, reusing
// the descriptor's `is` guard (lazily, so the descriptor const is fully
// initialized by the time `parse` runs). Validation is shallow, matching the
// descriptor's `is`.

import { Result, Ok, Err } from "std/result";

export function schema<T>(
  name: string,
  is: (value: unknown) => value is T,
): Schema<T> {
  return {
    name,
    parse(input: unknown): Result<T, Array<Issue>> {
      return is(input)
        ? Ok(input)
        : Err([{ path: [], message: `expected ${name}` }]);
    },
    array(): Schema<Array<T>> {
      return schema<Array<T>>(
        `${name}[]`,
        (value): value is Array<T> =>
          Array.isArray(value) && value.every((item) => is(item)),
      );
    },
  };
}
