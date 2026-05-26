type User = {
  name: string;
  age: number;
};

type ParseResult =
  | { ok: true; value: User }
  | { ok: false; error: string };

export function parseUser(input: unknown): ParseResult {
  if (typeof input !== "object" || input === null) {
    return { ok: false, error: "expected object" };
  }
  const obj = input as Record<string, unknown>;
  if (typeof obj.name !== "string") {
    return { ok: false, error: "name: expected string" };
  }
  if (typeof obj.age !== "number") {
    return { ok: false, error: "age: expected number" };
  }
  return { ok: true, value: { name: obj.name, age: obj.age } };
}
