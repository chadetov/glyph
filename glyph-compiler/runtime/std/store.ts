// std/store — a small shared-state primitive.
//
// A `Store<T>` holds a value in a closure; `get` reads it, `set` replaces it,
// and `update` maps it. Create one at module scope with `const s = create(...)`
// so many functions share the same state without threading a `let` through
// `main` and capturing closures. The binding stays `const` (D20); only the
// store's internal value changes, through `set` or `update`, and a Glyph program
// writes each as a `mut` statement (`mut s.set(...)`, `mut s.update(...)`, D5),
// so `grep "^\s*mut "` finds every mutation.

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
