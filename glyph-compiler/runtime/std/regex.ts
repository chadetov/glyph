// std/regex — regular expressions over the JavaScript engine.
//
// Patterns are ordinary strings compiled to a `RegExp`; every function is
// stateless (a fresh `RegExp` per call), so there is no shared `lastIndex`
// surprise. `find_all` and `replace_all` are global; `captures` returns the
// capture groups of the first match and `captures_all` returns them for every
// match.

export function matches(pattern: string, text: string): boolean {
  return new RegExp(pattern).test(text);
}

export function find_all(pattern: string, text: string): Array<string> {
  return Array.from(text.matchAll(new RegExp(pattern, "g")), (m) => m[0]);
}

export function find_first(pattern: string, text: string): string {
  const m = new RegExp(pattern).exec(text);
  return m ? m[0] : "";
}

// The capture groups of the first match (group 1 onward), or an empty array.
export function captures(pattern: string, text: string): Array<string> {
  const m = new RegExp(pattern).exec(text);
  return m ? m.slice(1).map((g) => g ?? "") : [];
}

// The capture groups of every match: one inner array per match, holding groups 1
// onward. `captures` is the first match; this is all of them, and the two follow
// the same two conventions, which both differ from `String.prototype.matchAll`.
// The whole match is not included (matchAll puts it at index 0), so group 1 is at
// index 0. And a group that did not participate is `""` rather than `undefined`,
// so an empty capture and an absent one read the same; a scanner that decides
// which alternation branch fired by asking which group is non-empty cannot use
// this. The outer array is empty when nothing matched.
export function captures_all(pattern: string, text: string): Array<Array<string>> {
  return Array.from(text.matchAll(new RegExp(pattern, "g")), (m) =>
    m.slice(1).map((g) => g ?? ""),
  );
}

export function replace_all(
  pattern: string,
  text: string,
  replacement: string,
): string {
  return text.replace(new RegExp(pattern, "g"), replacement);
}

export function split(pattern: string, text: string): Array<string> {
  return text.split(new RegExp(pattern));
}
