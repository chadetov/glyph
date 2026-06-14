// The same omission in TypeScript. An agent added the "triangle" variant but
// handled only two cases. tsc --strict compiles this clean; at runtime
// area(triangle) silently returns 0. TypeScript has no built-in exhaustiveness
// check — catching this requires the manual `assertNever(default)` idiom, which
// agents routinely forget. Compare exhaustiveness.glyph, which fails to compile.
type Shape =
  | { tag: "circle"; radius: number }
  | { tag: "square"; side: number }
  | { tag: "triangle"; base: number; height: number };

export function area(s: Shape): number {
  let result = 0;
  switch (s.tag) {
    case "circle":
      result = 3.14159 * s.radius * s.radius;
      break;
    case "square":
      result = s.side * s.side;
      break;
  }
  return result;
}
