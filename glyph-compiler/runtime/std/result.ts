// The Glyph `Result` type and its constructors.
//
// This is the runtime prelude: hand-written TypeScript shipped with the
// compiler. The wire format — a flat object discriminated by a string `tag`,
// with the payload under `value` — is the contract the emitter targets. The
// `?` operator's lowering reads `.tag === "Err"` and `.value`, and a match on a
// `Result` switches on `.tag`.
//
// A `Result` also carries the combinator methods `map`/`map_err`, so Glyph's
// method-call syntax (`result.map_err(f)`) works directly. These methods make
// `Result` vary in `T`, which is sound only because the `?` operator propagates
// an `Err` by *re-wrapping* it (`return Err(__r.value)`, a `Result<never, E>`):
// `never` in the success position is assignable to any `Result<Y, E>`, so the
// propagation type-checks regardless of the methods.

type ResultMethods<T, E> = {
  /// Transform the success value, leaving an `Err` untouched.
  map<U>(f: (value: T) => U): Result<U, E>;
  /// Transform the error value, leaving an `Ok` untouched.
  map_err<F>(f: (error: E) => F): Result<T, F>;
};

export type Result<T, E> =
  | (ResultMethods<T, E> & { tag: "Ok"; value: T })
  | (ResultMethods<T, E> & { tag: "Err"; value: E });

/// Construct a success. `never` for the error parameter so an `Ok` is
/// assignable to any `Result<T, E>`.
export function Ok<T>(value: T): Result<T, never> {
  return {
    tag: "Ok",
    value,
    map<U>(f: (value: T) => U): Result<U, never> {
      return Ok(f(value));
    },
    map_err<F>(_f: (error: never) => F): Result<T, F> {
      return Ok(value);
    },
  };
}

/// Construct a failure. `never` for the success parameter so an `Err` is
/// assignable to any `Result<T, E>`.
export function Err<E>(error: E): Result<never, E> {
  return {
    tag: "Err",
    value: error,
    map<U>(_f: (value: never) => U): Result<U, E> {
      return Err(error);
    },
    map_err<F>(f: (error: E) => F): Result<never, F> {
      return Err(f(error));
    },
  };
}
