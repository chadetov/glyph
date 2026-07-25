// std/set — a hash set with value semantics for primitives.
//
// `create` returns a `Set<T>` whose methods add, test, and remove members and
// read the size or the values as an array. Like `std/store`, the state lives in
// a closure and every mutation is a greppable method call (`s.add(...)`); the
// binding stays `const`. Maps are covered by the built-in `Record<K, V>`.

export type Set<T> = {
  add: (value: T) => void;
  has: (value: T) => boolean;
  remove: (value: T) => boolean;
  size: () => number;
  values: () => Array<T>;
};

export function create<T>(initial?: ReadonlyArray<T>): Set<T> {
  const inner = new globalThis.Set<T>(initial ?? []);
  return {
    add: (value: T) => {
      inner.add(value);
    },
    has: (value: T) => inner.has(value),
    remove: (value: T) => inner.delete(value),
    size: () => inner.size,
    values: () => Array.from(inner),
  };
}

// The de-duplicated members of an array, order-preserving.
export function unique<T>(values: ReadonlyArray<T>): Array<T> {
  return Array.from(new globalThis.Set<T>(values));
}
