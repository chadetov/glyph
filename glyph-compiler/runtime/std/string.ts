// std/string — string helpers.

export function from(value: unknown): string {
  return String(value);
}

export function join(parts: ReadonlyArray<string>, separator: string): string {
  return parts.join(separator);
}

export function split(s: string, separator: string): Array<string> {
  return s.split(separator);
}

export function len(s: string): number {
  return s.length;
}

export function trim(s: string): string {
  return s.trim();
}

export function lower(s: string): string {
  return s.toLowerCase();
}

export function upper(s: string): string {
  return s.toUpperCase();
}

export function contains(s: string, substring: string): boolean {
  return s.includes(substring);
}

export function starts_with(s: string, prefix: string): boolean {
  return s.startsWith(prefix);
}

export function ends_with(s: string, suffix: string): boolean {
  return s.endsWith(suffix);
}
