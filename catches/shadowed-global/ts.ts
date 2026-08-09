// tsc --strict accepts this. A local `Error` shadows the global one for the
// rest of the module, so a later `new Error(...)` means something else and the
// name no longer says which Error you are reading.
export type Error = { code: number; detail: string };

export function describe(e: Error): string {
  return `${e.code}: ${e.detail}`;
}
