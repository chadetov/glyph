// The boundary this project declares by hand. `tinylog` has no Glyph module and
// no installed package: this file is the whole reason the name type-checks, and
// an edge into it is a claim rather than a proof.
declare module "tinylog" {
  export function log(message: string): void;
}
