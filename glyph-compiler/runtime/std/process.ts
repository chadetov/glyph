// std/process — process arguments, environment, working directory, and exit.
// `args()` returns the program arguments (node's argv with the runtime + script
// entries dropped).

import { Option, Some, None } from "./option";

export function args(): Array<string> {
  return process.argv.slice(2);
}

/**
 * Stop the process now with `code`.
 *
 * Immediate: pending work is abandoned, and output still queued on a pipe can
 * be truncated. Use it when there is nothing left worth finishing (a server
 * that could not bind its port), and `set_exit_code` when there is.
 */
export function exit(code: number): never {
  return process.exit(code);
}

/**
 * Record the code the process will exit with, without stopping it.
 *
 * `main` returning sets the exit code, but a long-running program has usually
 * outlived its `main` by the time it learns it failed, and the return value is
 * spent. Before this the only way to fail late was `exit`, which tears the
 * process down mid-flight: a shutdown that still has a connection to close, or
 * a line to flush, had to choose between doing it and reporting failure.
 *
 * This records the verdict and lets the program end on its own terms. The
 * process leaves with this code once nothing is left to wait for. Calling it
 * again overwrites the code, so the last verdict wins.
 */
export function set_exit_code(code: number): void {
  process.exitCode = code;
}

/** The exit code the process will currently leave with. */
export function exit_code(): number {
  return process.exitCode ?? 0;
}

export function env(name: string): Option<string> {
  const value = process.env[name];
  return value === undefined ? None : Some(value);
}

export function cwd(): string {
  return process.cwd();
}
