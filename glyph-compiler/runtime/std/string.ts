// std/string — string helpers.

export function from(value: unknown): string {
  return String(value);
}

export function join(parts: ReadonlyArray<string>, separator: string): string {
  return parts.join(separator);
}
