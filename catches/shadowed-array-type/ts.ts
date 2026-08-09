// tsc --strict accepts this. `Array` now means this record for the rest of the
// module, so every later `Array<string>` in the same file means something else
// than it does in every other file. Nothing warns; the name is simply taken.
export type Array = { items: number };

export function count(a: Array): number {
  return a.items;
}
