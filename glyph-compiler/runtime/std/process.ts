// std/process — process arguments and exit. `args()` returns the program
// arguments (node's argv with the runtime + script entries dropped).

export function args(): Array<string> {
  return process.argv.slice(2);
}

export function exit(code: number): never {
  return process.exit(code);
}
