// tsc --strict accepts this and types `host` as a `string`. `split` returns a
// `string[]` of any length, so for "nobody" `host` is `undefined`, and the
// template prints "nobody at undefined".
export function describe(s: string): string {
  // show
  const [user, host] = s.split("@");
  // end show
  return `${user} at ${host}`;
}

console.log(describe("nobody"));
