// std/collections — ordered collections beyond the built-in `Array` and
// `Record`. A `Deque<T>` is a double-ended queue: push and pop at either end in
// constant amortized time, useful as a queue (`push_back`/`pop_front`) or a
// stack (`push_back`/`pop_back`). Ends that may be empty return an `Option<T>`.

import { None, type Option, Some } from "./option";

export type Deque<T> = {
  push_back: (value: T) => void;
  push_front: (value: T) => void;
  pop_back: () => Option<T>;
  pop_front: () => Option<T>;
  peek_front: () => Option<T>;
  peek_back: () => Option<T>;
  len: () => number;
  values: () => Array<T>;
};

export function deque<T>(initial?: ReadonlyArray<T>): Deque<T> {
  const items: Array<T> = initial ? Array.from(initial) : [];
  const opt = (value: T | undefined): Option<T> =>
    value === undefined ? None : Some(value);
  return {
    push_back: (value: T) => {
      items.push(value);
    },
    push_front: (value: T) => {
      items.unshift(value);
    },
    pop_back: () => opt(items.pop()),
    pop_front: () => opt(items.shift()),
    peek_front: () => opt(items[0]),
    peek_back: () => opt(items[items.length - 1]),
    len: () => items.length,
    values: () => Array.from(items),
  };
}
