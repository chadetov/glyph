// std/io — line-oriented console I/O. `println` writes to stdout, `eprintln`
// to stderr; `read_line`/`read_to_string` read from stdin.

import { Option, Some, None } from "./option";
import { readFileSync } from "node:fs";

export function println(message: string): void {
  console.log(message);
}

export function eprintln(message: string): void {
  console.error(message);
}

// stdin is read once, synchronously, and buffered into lines; `read_line`
// hands them out one at a time and returns `None` at end of input.
let lines: Array<string> | null = null;
let cursor = 0;

function ensure_loaded(): Array<string> {
  if (lines === null) {
    const text = read_to_string();
    // Drop a single trailing newline so a file ending in "\n" does not yield a
    // trailing empty line.
    const trimmed = text.endsWith("\n") ? text.slice(0, -1) : text;
    lines = trimmed === "" ? [] : trimmed.split("\n");
  }
  return lines;
}

export function read_line(): Option<string> {
  const all = ensure_loaded();
  if (cursor >= all.length) {
    return None;
  }
  const line = all[cursor];
  cursor += 1;
  return Some(line);
}

export function read_to_string(): string {
  try {
    return readFileSync(0, "utf8");
  } catch {
    return "";
  }
}
