// Runtime prelude bootstrap. The emitter references a few prelude values
// without an import — `number`, `par`, `print` — so they must exist as globals
// at run time. This module installs them onto `globalThis` as a side effect;
// the `glyph run` entrypoint imports it before invoking the program. The
// matching ambient *types* live in `glyph-prelude.d.ts`.

import { type Result, Ok, Err } from "./std/result";

const number = {
  to_string(n: number): string {
    return String(n);
  },
  parse(s: string): Result<number, string> {
    if (s.trim() === "") {
      return Err("not a number");
    }
    const n = Number(s);
    return Number.isNaN(n) ? Err("not a number") : Ok(n);
  },
};


function print(message: string): void {
  console.log(message);
}

/**
 * Value equality for `==` and `!=`.
 *
 * Glyph's `==` is a value comparison, which is what the operator has always
 * been documented to mean. Emitting `===` made it a *reference* comparison the
 * moment either side was a record, a tagged union, or an array, so
 * `Some("a") == Some("a")` was false, silently, with no diagnostic. The same
 * expression written as an `@example` compared structurally and passed, so a
 * test could report that code worked while the code did not.
 *
 * `===` first, so primitives and identical references cost nothing. Function
 * properties are skipped: a value's methods (a `Result`'s `map`, say) are
 * behaviour rather than data, and they differ per instance.
 */
// A bounds-checked array read (G30).
//
// `xs[i]` is typed `T`, and out of range JavaScript hands back `undefined`,
// which then travels until something dereferences it and fails somewhere else
// entirely. Rust's `xs[i]` tells the same lie in the type and panics at the bad
// index; this makes Glyph do the same, so the failure names the mistake instead
// of describing its consequence three frames later.
//
// Only arrays are checked. Anything else (a record used as a map, a string)
// passes straight through, because reading a key that may be absent is already
// E0224 at compile time.
function __glyph_index(container: unknown, index: unknown): unknown {
  if (Array.isArray(container) && typeof index === "number") {
    if (!Number.isInteger(index) || index < 0 || index >= container.length) {
      throw new RangeError(
        `index ${index} is out of range for an array of length ${container.length}`,
      );
    }
  }
  return (container as Record<string, unknown>)[index as unknown as string];
}

function __glyph_eq(a: unknown, b: unknown): boolean {
  if (a === b) return true;
  if (typeof a !== "object" || typeof b !== "object" || a === null || b === null) {
    return false;
  }
  if (Array.isArray(a) || Array.isArray(b)) {
    if (!Array.isArray(a) || !Array.isArray(b) || a.length !== b.length) return false;
    return a.every((x, i) => __glyph_eq(x, b[i]));
  }
  const ao = a as Record<string, unknown>;
  const bo = b as Record<string, unknown>;
  const ak = Object.keys(ao).filter((k) => typeof ao[k] !== "function");
  const bk = Object.keys(bo).filter((k) => typeof bo[k] !== "function");
  if (ak.length !== bk.length) return false;
  return ak.every((k) => Object.prototype.hasOwnProperty.call(bo, k) && __glyph_eq(ao[k], bo[k]));
}

// The key/value pairs of `for k, v in it`, chosen by what `it` actually is.
//
// An array's pairs are `it.entries()`, whose index is a NUMBER; a record's are
// `Object.entries(it)`, whose key is a STRING. The emitter picks between them
// from the iterand's static type and used to fall back to the record form when
// that type was unknown, which is a silent miscompile: the loop index arrived
// as a string, so `index + 1` computed `"01"` in a build reporting no
// diagnostics and a clean `tsc --strict`. A value whose type Glyph cannot see
// (the `Ok` payload of a generic record's `parse`, for one) took that path.
//
// The compiler cannot always know the shape. The runtime always can, so when
// the static type does not settle it, this decides. Typed iterands keep their
// direct `.entries()` / `Object.entries(...)` emit and never reach here.
function __glyph_pairs(it: unknown): Iterable<[unknown, unknown]> {
  if (Array.isArray(it)) {
    return it.entries() as Iterable<[unknown, unknown]>;
  }
  return Object.entries(it as Record<string, unknown>) as Iterable<[unknown, unknown]>;
}

function assert(condition: boolean): void {
  if (!condition) {
    throw new Error("assertion failed");
  }
}

const g = globalThis as unknown as Record<string, unknown>;
g.number = number;
g.print = print;
g.assert = assert;
g.__glyph_eq = __glyph_eq;
g.__glyph_index = __glyph_index;
g.__glyph_pairs = __glyph_pairs;
