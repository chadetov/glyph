// tsc --strict accepts this. The union crosses a module boundary intact, and a
// switch is still not required to cover it, so `width` returns undefined for
// "bool" while claiming a number.
export type ColType = "text" | "int" | "real" | "bool";

export function width(c: ColType): number {
  switch (c) {
    case "text":
      return 32;
    case "int":
      return 8;
    case "real":
      return 8;
  }
  return undefined as unknown as number;
}
