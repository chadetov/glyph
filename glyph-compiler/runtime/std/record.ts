// std/record — helpers over `Record<K, V>`, Glyph's associative collection (a
// plain object with string keys; `r[key]` reads/writes and `for k, v in r`
// iterates). Reads are absence-aware: `get` returns an `Option` rather than the
// raw `undefined` a bare `r[key]` yields for a missing key. Updates are
// value-oriented — `set`/`remove` return a new record and never mutate the input.

import { Option, Some, None } from "./option";

export function get<V>(record: Record<string, V>, key: string): Option<V> {
  return Object.prototype.hasOwnProperty.call(record, key) ? Some(record[key]) : None;
}

export function has<V>(record: Record<string, V>, key: string): boolean {
  return Object.prototype.hasOwnProperty.call(record, key);
}

export function keys<V>(record: Record<string, V>): Array<string> {
  return Object.keys(record);
}

export function values<V>(record: Record<string, V>): Array<V> {
  return Object.values(record);
}

export function set<V>(record: Record<string, V>, key: string, value: V): Record<string, V> {
  return { ...record, [key]: value };
}

export function remove<V>(record: Record<string, V>, key: string): Record<string, V> {
  const out: Record<string, V> = { ...record };
  delete out[key];
  return out;
}
