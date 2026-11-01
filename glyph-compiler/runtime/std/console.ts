// std/console — a line to the host's console, on any host.
//
// `std/io` is the module for a process's own streams, and it imports
// `node:fs` at load time, so it cannot go into a page. A browser program, a
// worker and a server all have a console, and this is the module that writes to
// it. `log` is what the prelude `print` does; `warn` and `error` are the two
// levels a browser's devtools filter on.
//
// Importing this module binds `console` inside the importing file, which hides
// the host global there. Glyph code cannot name that global anyway, and the
// emitted code never does, so nothing is lost.
//
// The module imports nothing from node, so a bundler can take it into a page.

/** Write a line at the ordinary level. */
export function log(text: string): void {
  globalThis.console.log(text);
}

/** Write a line at the warning level. */
export function warn(text: string): void {
  globalThis.console.warn(text);
}

/** Write a line at the error level. Under node it goes to stderr. */
export function error(text: string): void {
  globalThis.console.error(text);
}
