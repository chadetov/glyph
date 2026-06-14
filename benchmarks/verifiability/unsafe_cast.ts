// An agent treats untyped input as a `User` with an `as` cast. tsc --strict
// compiles this clean — `as` asserts a type with no runtime check. If `input` is
// not actually a User, `user.name` is undefined at runtime and `.toUpperCase()`
// throws. The cast makes the type system lie. Compare unsafe_cast.glyph, which
// has no cast expression and does not compile.
type User = {
  name: string;
};

export function coerce(input: unknown): User {
  return input as User;
}

export function shout(input: unknown): string {
  const user = coerce(input);
  return user.name.toUpperCase(); // throws at runtime if input is not a User
}
