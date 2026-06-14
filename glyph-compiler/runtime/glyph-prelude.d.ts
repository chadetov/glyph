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

/// Assert a condition (D26 `@doc @run` blocks). A false condition throws,
/// failing the build that runs the doc example.
declare function assert(condition: boolean): void;

/// The `number` prelude namespace (used without an import). `parse` validates a
/// string into a `Result` (the examples match its `Ok`/`Err`).
declare const number: {
  to_string(n: number): string;
  parse(s: string): import("./std/result").Result<number, string>;
};

/// One problem reported by a record/schema parser.
type Issue = {
  path: ReadonlyArray<string | number>;
  message: string;
};

/// A runtime validator for `T`, produced by `T.schema` and consumed by
/// decoders. `parse` validates an `unknown`; `array` lifts a schema to one for
/// arrays of `T`.
type Schema<T> = {
  name: string;
  parse(input: unknown): import("./std/result").Result<T, Array<Issue>>;
  array(): Schema<Array<T>>;
};
