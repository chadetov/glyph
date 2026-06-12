// The Glyph `Result` type and its constructors.
//
// This is the runtime prelude: hand-written TypeScript shipped with the
// compiler. The wire format — a flat object discriminated by a string `tag`,
// with the payload under `value` — is the contract the emitter targets. The
// `?` operator's lowering reads `.tag === "Err"` and `.value` and propagates an
// `Err` by returning it; a match on a `Result` switches on `.tag`. Both depend
// on exactly this shape, and on an `Err` being assignable across success types
// (a `?` returns an `Err` of `Result<X, E>` from a function returning
// `Result<Y, E>`), so the payload stays a plain value with no `T`-dependent
// methods. Combinator methods (`map`, `map_err`) are a separate design item:
// see `docs/roadmap/05-typechecker.md`.

export type Result<T, E> =
  | { tag: "Ok"; value: T }
  | { tag: "Err"; value: E };

/// Construct a success. `never` for the error parameter so an `Ok` is
/// assignable to any `Result<T, E>`.
export function Ok<T>(value: T): Result<T, never> {
  return { tag: "Ok", value };
}

/// Construct a failure. `never` for the success parameter so an `Err` is
/// assignable to any `Result<T, E>`.
export function Err<E>(value: E): Result<never, E> {
  return { tag: "Err", value };
}
