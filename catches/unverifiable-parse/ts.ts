// tsc --strict accepts this. The guard claims `value is Conn` while only
// checking that `sock` is present, so `parse` returns a typed value whose
// socket may be a string. A cast is all that stands behind the claim.
export type Socket = { handle: number };
export type Conn = { id: number; sock: Socket };

export function isConn(value: unknown): value is Conn {
  return (
    typeof value === "object" &&
    value !== null &&
    typeof (value as Record<string, unknown>).id === "number" &&
    (value as Record<string, unknown>).sock !== undefined
  );
}

export function parse(value: unknown): Conn | null {
  return isConn(value) ? value : null;
}
