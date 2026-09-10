// tsc --strict accepts this. `Nullable<int>` emits as `number | null`, and a
// `const` whose initializer is a number narrows back to `number` for the rest
// of the function, so the call is checked against `number` and the null half
// of the declared type is never tested. Widen the declaration back out (pass
// `v` on to another function, read it after a branch) and the null is there
// again, unchecked.
function takesInt(n: number): number {
  return n;
}

export function run(): number {
  const v: number | null = 3;
  return takesInt(v);
}
