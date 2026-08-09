// tsc --strict accepts this. An index signature says every string key has a
// string value, so a misspelled key types as `string` and prints "undefined".
export function nameOf(row: Record<string, string>): string {
  return row.naem;
}
