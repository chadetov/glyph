// std/process — process arguments, environment, working directory, and exit.
// `args()` returns the program arguments (node's argv with the runtime + script
// entries dropped).

import { Option, Some, None } from "./option";

export function args(): Array<string> {
  return process.argv.slice(2);
}

export function exit(code: number): never {
  return process.exit(code);
}

export function env(name: string): Option<string> {
  const value = process.env[name];
  return value === undefined ? None : Some(value);
}

export function cwd(): string {
  return process.cwd();
}
