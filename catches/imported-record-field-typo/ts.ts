// tsc --strict accepts this, because the value arrives as `any` from an
// untyped boundary. That is the shape a real codebase has at the seam with a
// package that ships no types, and every field read past it is unchecked.
export function rowName(sheet: any): string {
  return sheet.rowz;
}
