// Minimal ambient declarations for the Node surface the runtime wrappers use.
// The generated tsconfig sets `types: []` (no `@types/node`), so rather than
// pull in the full Node typings, the few APIs `std/fs` and `std/process` touch
// are declared here. `console` comes from the `dom` lib already.

declare module "node:fs" {
  export function readFileSync(path: string, encoding: "utf8"): string;
  export function writeFileSync(path: string, data: string, encoding: "utf8"): void;
}

declare const process: {
  argv: string[];
  exit(code: number): never;
};
