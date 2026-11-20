/// <reference path="../glyph-prelude.d.ts" />
// The depth limit's enforcement (D50, G369): the constant, the private
// sentinel, and the helpers generated checks call. Internal: the resolver
// does not seed this module, so a Glyph program cannot import it, and only
// emitted code and `std/schema` reach it. That is what keeps the sentinel
// private. If Glyph code could call `too_deep`, any expression reachable from
// a check could throw it, out of a part the emitter proved can never go too
// deep and so did not wrap, or into a part it did wrap, forging a depth
// refusal. `std/schema` re-exports `MAX_DEPTH` as the public contract.
//
// A generated check refuses a value nested more than `MAX_DEPTH` objects and
// arrays deep, counted as JSON nests them: the root object or array is depth
// 1, an `Option` or union `{ tag, value }` object counts like any other, and
// only the parts the type walks are counted (an `unknown` field is not
// entered). Every check takes the depth of the value it is handed as a
// parameter (`T.__is(value, depth)`, `T.__parse(value, depth)`, a generic
// descriptor's checker `(v, depth) => boolean`) and compares it once per
// object or array it enters. Past the limit it throws the sentinel through
// `too_deep()`. The sentinel is caught where the answer's path is known: a
// public `T.is` / `is T` answers `false` (`depth_refused`, `within`), and each
// field of a `parse` whose part can throw it answers one issue built by
// `depth_issue`, at that field's path, with `code: "depth"` and nothing else
// beside it. Every catch goes through `depth_refused`, which rethrows anything
// that is not the sentinel, so a `RangeError` or a host checker's own
// exception propagates unchanged. `JSON.parse` itself has no limit.

/// The deepest a value may nest, in objects and arrays, for a generated check
/// to accept it (D50). Emitted code refers to it by this name.
export const MAX_DEPTH = 1000;

class TooDeep {}
const TOO_DEEP = new TooDeep();

/// Refuse the value being checked: it nests deeper than `MAX_DEPTH`. Throws
/// the private sentinel that only `depth_refused` recognises.
export function too_deep(): never {
  throw TOO_DEEP;
}

/// The answer of a check that threw `e`: `false` for the depth sentinel, and
/// `e` rethrown for anything else.
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

/// Whether `check` holds for every one of `items`, each handed `depth`: the
/// walk a generated check makes over an array's elements or a `Record`'s
/// values when their depth is not a constant. The check is passed once per
/// container, usually as the descriptor's own `__is`, so nothing is
/// allocated per element or per call of the check holding the walk. A hole
/// in a sparse array is skipped, as `Array.prototype.every` skips it, so the
/// answer does not depend on which of the two walks a check was emitted with.
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
