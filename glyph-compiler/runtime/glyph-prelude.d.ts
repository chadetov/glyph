// Ambient prelude declarations: the names a Glyph program may use without an
// import. The emitter references these directly (`par.all`, `print`, `number`,
// the bare `Schema<T>` / `Issue` types), so they are global rather than module
// exports. Behavioral runtime (the real `par`/`print`/`number`) ships
// separately; these are the types `tsc` needs.

/// Structured concurrency helpers (Q18). `all` awaits a list of async values;
/// `all_ok` collapses a list of `Result`s into a `Result` of the list.
declare const par: {
  all<T>(xs: ReadonlyArray<T | Promise<T>>): Promise<Array<Awaited<T>>>;
  all_ok<T, E>(
    xs: ReadonlyArray<import("./std/result").Result<T, E>>,
  ): import("./std/result").Result<Array<T>, E>;
};

/// Print a line to standard output (the prelude logging primitive).
declare function print(message: string): void;

/// Value equality for `==` / `!=` on anything that is not a known primitive.
/// Compares records, tagged unions and arrays by structure, which is what the
/// operator means; `===` alone made it reference equality for those.
declare function __glyph_eq(a: unknown, b: unknown): boolean;

/// A bounds-checked read (G30). Typed to preserve exactly what `c[k]` would
/// have, so wrapping an index changes when it fails and not what it is.
declare function __glyph_index<C, K extends keyof C>(container: C, index: K): C[K];

/// The key/value pairs of a `for k, v in it` whose iterand type the emitter
/// could not settle statically. An array yields a numeric index, a record a
/// string key, decided at run time.
declare function __glyph_pairs(it: unknown): Iterable<[any, any]>;

/// Assert a condition (D26 `@doc @run` blocks). A false condition throws,
/// failing the build that runs the doc example.
declare function assert(condition: boolean): void;

/// The `number` prelude namespace (used without an import). `parse` validates a
/// string into a `Result` (the examples match its `Ok`/`Err`).
declare const number: {
  to_string(n: number): string;
  parse(s: string): import("./std/result").Result<number, string>;
};

/// One problem reported by a record/schema parser. `code` classifies the
/// failure so a handler can branch on it without matching the human-readable
/// `message`: `"missing"` for a required field that was absent, `"type"` for a
/// value of the wrong shape, `"refinement"` for a value that passed its base
/// type but failed a `where` predicate, and `"unexpected"` for a key the type
/// does not declare. It is optional, so an `Issue` built by hand still checks.
type Issue = {
  path: ReadonlyArray<string | number>;
  message: string;
  code?: "missing" | "type" | "refinement" | "unexpected";
};

/// A runtime validator for `T`, produced by `T.schema` and consumed by
/// decoders. `parse` validates an `unknown`; `array` lifts a schema to one for
/// arrays of `T`.
type Schema<T> = {
  name: string;
  parse(input: unknown): import("./std/result").Result<T, Array<Issue>>;
  array(): Schema<Array<T>>;
};
