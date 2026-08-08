// Node's timer scheduling.
//
// `setInterval` and friends are globals, and Glyph resolves imported module
// names rather than ambient globals, so they are reached through the `timers`
// builtin module instead. Node exports exactly the same functions there.
//
// The handle is opaque: nothing here inspects it, it is only handed back to
// `clearInterval`, so an opaque object type is more honest than Node's
// `Timeout` class and keeps this declaration free of the rest of `@types/node`.
declare module "timers" {
  export function setInterval(handler: () => void, ms: number): object;
  export function clearInterval(handle: object): void;
  export function setTimeout(handler: () => void, ms: number): object;
  export function clearTimeout(handle: object): void;
}
