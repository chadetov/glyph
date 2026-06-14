// std/io — line-oriented console output. `println` writes to stdout,
// `eprintln` to stderr.

export function println(message: string): void {
  console.log(message);
}

export function eprintln(message: string): void {
  console.error(message);
}
