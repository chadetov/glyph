// tsc --strict accepts this. An index signature says every string key has a
// string value, so a read of a key that is not there types as `string` and
// prints "undefined". The typo makes it obvious; the correctly spelled
// `row.name` has the same hole when the row has no name.
// `noUncheckedIndexedAccess` would catch it, but `--strict` does not turn it on.
export function nameOf(row: Record<string, string>): string {
  return row.naem;
}
