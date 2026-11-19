// The corrected guard: it checks that `sock` really is a socket. The `as
// Record<string, unknown>` reads are the same as in `ts.ts`, so anything the
// full configuration reports on them is not about the mistake.
import { Socket } from "net";

export type Conn = { id: number; sock: Socket };

export function isConn(value: unknown): value is Conn {
  return (
    typeof value === "object" &&
    value !== null &&
    typeof (value as Record<string, unknown>).id === "number" &&
    (value as Record<string, unknown>).sock instanceof Socket
  );
}

export function parse(value: unknown): Conn | null {
  return isConn(value) ? value : null;
}

console.log(parse({ id: 1, sock: "not a socket" }));
