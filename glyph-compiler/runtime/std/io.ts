// std/io — console I/O. `println`/`eprintln` write a line, `print`/`eprint`
// write without one, and `read_line`/`read_to_string` read from stdin.
// `is_terminal`/`stdin_is_terminal` say whether a person or a pipe is on the
// other end.

import { type Option, Some, None } from "./option";
import { readSync } from "node:fs";
import { StringDecoder } from "node:string_decoder";

export function println(message: string): void {
  console.log(message);
}

export function eprintln(message: string): void {
  console.error(message);
}

// Writing without a newline is what a prompt is: `> ` with the answer typed on
// the same line. `println` cannot do it, so an interactive program had to put
// its prompt on a line of its own and every session read as alternating lines
// rather than as a conversation.
//
// `console.log` appends the newline and otherwise goes through the same stream,
// so mixing `print` and `println` keeps its order. On a terminal the write is
// synchronous and the prompt appears before the read that follows it; on a pipe
// it is buffered, which changes when the bytes leave but not what order they
// leave in.
export function print(message: string): void {
  process.stdout.write(message);
}

export function eprint(message: string): void {
  process.stderr.write(message);
}

// Whether stdout is a terminal rather than a pipe or a file.
//
// This is the predicate behind every CLI that colours its output, draws a
// progress line, or prompts. Without it a program has to be told which it is by
// a flag, and the default is wrong half the time.
export function is_terminal(): boolean {
  return process.stdout.isTTY === true;
}

// Whether stdin is a terminal: whether a person is typing, rather than a file
// being piped in. A program that prompts should ask this first, because
// prompting into a pipe writes noise nobody reads.
export function stdin_is_terminal(): boolean {
  return process.stdin.isTTY === true;
}

// stdin is read incrementally, one synchronous chunk at a time, into a shared
// decoded buffer. `read_line` returns as soon as a full line has arrived, so a
// prompt/read/respond loop answers while the writer is still connected; the
// earlier implementation slurped fd 0 to EOF, which made every interactive
// program impossible. `read_to_string` drains the same buffer, so mixing the
// two reads "the rest" instead of losing whatever the other one buffered.
let pending = "";
let eof = false;
const chunk = Buffer.alloc(65536);
const decoder = new StringDecoder("utf8");

// Sleep synchronously. `Atomics.wait` on a throwaway shared array is the only
// blocking sleep Node offers; a bare retry loop on `EAGAIN` would pin a core.
function sleep(ms: number): void {
  Atomics.wait(new Int32Array(new SharedArrayBuffer(4)), 0, 0, ms);
}

// Read one chunk from fd 0 into `pending`. Returns whether bytes arrived.
// A read that hits end of input sets `eof` and flushes the decoder.
function fill(): boolean {
  if (eof) {
    return false;
  }
  for (;;) {
    let n = 0;
    try {
      n = readSync(0, chunk, 0, chunk.length, null);
    } catch (e) {
      const code = (e as { code?: string }).code;
      if (code === "EAGAIN") {
        // A non-blocking tty has nothing right now: back off and retry.
        sleep(10);
        continue;
      }
      // "EOF" is how some platforms report end of a tty stream. Any other
      // failure (no stdin at all, a closed descriptor) degrades to empty input
      // rather than throwing, so a program run without stdin still terminates.
      eof = true;
      pending += decoder.end();
      return false;
    }
    if (n === 0) {
      eof = true;
      pending += decoder.end();
      return false;
    }
    // A multi-byte character split across the chunk boundary is held by the
    // decoder and completed by the next chunk.
    pending += decoder.write(chunk.subarray(0, n));
    return true;
  }
}

export function read_line(): Option<string> {
  for (;;) {
    const nl = pending.indexOf("\n");
    if (nl >= 0) {
      const line = pending.slice(0, nl);
      pending = pending.slice(nl + 1);
      // Strip a single trailing "\r" so CRLF input yields the same lines as LF.
      return Some(line.endsWith("\r") ? line.slice(0, -1) : line);
    }
    if (eof) {
      if (pending === "") {
        return None;
      }
      // Input that ended without a newline still has one last line in it.
      const line = pending.endsWith("\r") ? pending.slice(0, -1) : pending;
      pending = "";
      return Some(line);
    }
    fill();
  }
}

export function read_to_string(): string {
  while (!eof) {
    fill();
  }
  const rest = pending;
  pending = "";
  return rest;
}

// Pretty-print any value for debugging: a readable, indented, multi-line
// rendering to stderr (so it never mixes into a program's stdout output).
// `render` returns the same string instead of printing, for tests and logs.
export function inspect(value: unknown): void {
  console.error(render(value));
}

export function render(value: unknown): string {
  return JSON.stringify(value, (_k, v) => (typeof v === "function" ? "[function]" : v), 2) ??
    String(value);
}
