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
// Depth (D50, G369). A generated check refuses a value nested more than
// `MAX_DEPTH` objects and arrays deep, counted as JSON nests them: the root
// object or array is depth 1, an `Option` or union `{ tag, value }` object
// counts like any other, and only the parts the type walks are counted (an
// `unknown` field is not entered). Every generated check takes the depth of
// the value it is handed as a parameter (`T.__is(value, depth)`,
// `T.__parse(value, depth)`, a generic descriptor's checker
// `(v, depth) => boolean`), and compares it once per object or array it
// enters. Past the limit the check throws a private sentinel through
// `too_deep()`. The sentinel is caught only where a check's answer leaves
// the generated code: a public `T.is` / `is T` answers `false`
// (`depth_refused`, `within`), and a `parse` answers one issue built by
// `depth_issue`, at the path the part's `type` issue would carry, with
// `code: "depth"`, and nothing else beside it. Nothing else is caught: a
// `RangeError` or a host checker's own exception propagates unchanged. `JSON.parse` itself has no
// limit; the untyped `json.parse(text)` decodes any depth and checks nothing.

import { type Result, Ok, Err } from "./result";

/// The deepest a value may nest, in objects and arrays, for a generated check
/// to accept it (D50). Emitted code refers to it by this name.
export const MAX_DEPTH = 1000;

class TooDeep {}
const TOO_DEEP = new TooDeep();

/// Refuse the value being checked: it nests deeper than `MAX_DEPTH`. Throws
/// the private sentinel that only `depth_refused` and the `parse` entries
/// recognise.
export function too_deep(): never {
  throw TOO_DEEP;
}

/// The answer of a public `is` whose check threw `e`: `false` for the depth
/// sentinel, and `e` rethrown for anything else.
export function depth_refused(e: unknown): false {
  if (e === TOO_DEEP) {
    return false;
  }
  throw e;
}

/// Run `check` for a public `is` that walks deeper than its own entry (an
/// `is Array<T>` or `is Record<K, V>` pattern): its answer, or `false` when
/// it threw the depth sentinel.
export function within(check: () => boolean): boolean {
  try {
    return check();
  } catch (e: unknown) {
    return depth_refused(e);
  }
}

/// Run a check for a schema built from a guard alone: its answer, or
/// `undefined` when it threw the depth sentinel. Anything else it throws
/// propagates.
function depth_checked(check: () => boolean): boolean | undefined {
  try {
    return check();
  } catch (e: unknown) {
    if (e === TOO_DEEP) {
      return undefined;
    }
    throw e;
  }
}

/// Whether `check` holds for every one of `items`, each handed `depth`: the
/// walk a generated check makes over an array's elements or a `Record`'s
/// values when their depth is not a constant. The depth is an argument
/// rather than captured by the per-part closure, which would allocate on
/// every call of the check that holds it. A hole in a sparse array is
/// skipped, as `Array.prototype.every` skips it, so the answer does not
/// depend on which of the two walks a check was emitted with.
export function every_at(
  items: ReadonlyArray<unknown>,
  depth: number,
  check: (value: unknown, depth: number) => boolean,
): boolean {
  for (let i = 0; i < items.length; i++) {
    if (!check(items[i], depth) && i in items) {
      return false;
    }
  }
  return true;
}

/// The one issue a `parse` reports for a value nested deeper than
/// `MAX_DEPTH`, at `path`.
export function depth_issue(path: ReadonlyArray<string | number>): Issue {
  return {
    path,
    message: `nested more than ${MAX_DEPTH} objects and arrays deep`,
    code: "depth",
  };
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
        value.every((item) => is_at(item, depth + 1));
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
