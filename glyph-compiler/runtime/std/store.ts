// std/store — a small shared-state primitive.
//
// A `Store<T>` holds a value in a closure; `get` reads it, `set` replaces it,
// and `update` maps it. Create one at module scope with `const s = create(...)`
// so many functions share the same state without threading a `let` through
// `main` and capturing closures. The binding stays `const` (D20) and no `mut`
// reassignment is involved — only the store's internal value changes, through a
// method call — so every mutation is a greppable `s.set(...)`/`s.update(...)`.

export type Store<T> = {
  get: () => T;
  set: (next: T) => void;
  update: (change: (current: T) => T) => void;
};

export function create<T>(initial: T): Store<T> {
  let value = initial;
  return {
    get: () => value,
    set: (next: T) => {
      value = next;
    },
    update: (change: (current: T) => T) => {
      value = change(value);
    },
  };
}
