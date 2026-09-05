// The boundary this project declares by hand. No Glyph module is named
// `wirestat` and no package of that name is installed: this file is the whole
// reason the import type-checks, and the compiler never reads past it.
declare module "wirestat" {
  export type Status = { kind: "Live" } | { kind: "Dead" };
  export function read(): Status;
}
