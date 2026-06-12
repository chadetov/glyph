// The Glyph `Option` type and its constructors.
//
// Same flat tagged wire format as `Result` (see `result.ts`): a match on an
// `Option` switches on `.tag`, and a `Some` payload is read from `.value`.

export type Option<T> =
  | { tag: "Some"; value: T }
  | { tag: "None" };

/// Construct a present value.
export function Some<T>(value: T): Option<T> {
  return { tag: "Some", value };
}

/// The absent value. A single shared object — `None` carries no payload.
export const None: Option<never> = { tag: "None" };
