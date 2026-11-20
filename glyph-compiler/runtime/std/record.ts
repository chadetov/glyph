// std/record — helpers over `Record<K, V>`, Glyph's associative collection (a
// plain object with string keys; `r[key]` reads/writes and `for k, v in r`
// iterates). Reads are absence-aware: `get` returns an `Option` rather than the
// raw `undefined` a bare `r[key]` yields for a missing key. Updates are
// value-oriented — `set`/`remove` return a new record and never mutate the input.
//
// Every function is generic in the key, so a `Record<Tier, V>` keeps its key
// through `set` and `remove` (G373). Glyph's checker binds `K` from the record
// argument and refuses `remove` on a key with a fixed member set; `has` asks
// about any string. `keys` is `Array<K>` for a string-like key and
// `Array<string>` for a numeric one, since `Object.keys` yields strings, and
// Glyph's checker types it the same way. The key argument is `KeyOf<K>`, so
// `K` is the record's key, as Glyph binds it: a written `"a"` would otherwise
// narrow `K` to `"a"` and `set` would return a `Record<"a", V>`.
//
// `V` comes first and `K` defaults to `string`, so a call written with one type
// argument (`record.set<Message>(m, key, v)`), which is what these functions
// took before they were generic in the key, still names the value type and
// still means a `string` key.

import { type Option, Some, None } from "./option";

// Blocks inference from the key argument, so K is the record's key
// (`NoInfer<K>` needs TypeScript 5.4; the runtime's floor is 4.9).
type KeyOf<K> = [K][K extends unknown ? 0 : never];

export function get<V, K extends PropertyKey = string>(record: Record<K, V>, key: KeyOf<K>): Option<V> {
  return Object.prototype.hasOwnProperty.call(record, key) ? Some(record[key]) : None;
}

export function has<V, K extends PropertyKey = string>(record: Record<K, V>, key: string): boolean {
  return Object.prototype.hasOwnProperty.call(record, key);
}

export function keys<V, K extends PropertyKey = string>(
  record: Record<K, V>,
): Array<K extends string ? K : string> {
  return Object.keys(record) as Array<K extends string ? K : string>;
}

export function values<V, K extends PropertyKey = string>(record: Record<K, V>): Array<V> {
  return Object.values(record);
}

export function set<V, K extends PropertyKey = string>(record: Record<K, V>, key: KeyOf<K>, value: V): Record<K, V> {
  return { ...record, [key]: value } as Record<K, V>;
}

export function remove<V, K extends PropertyKey = string>(record: Record<K, V>, key: KeyOf<K>): Record<K, V> {
  const out = { ...record } as Record<K, V>;
  // A `Record<K, V>` declares every key required, so `delete` needs the index
  // signature view. Glyph never calls this with a key that has a fixed member
  // set.
  delete (out as Record<PropertyKey, V>)[key];
  return out;
}
