// std/websocket — a WebSocket client.
//
// The host's `WebSocket` is a global class, and Glyph resolves imported module
// names rather than ambient globals, so before this module a Glyph program
// could not name it at all: not the constructor (`new WebSocket(url)` is
// E0103), not the type, and not the fields of the events it delivers. A Discord
// gateway client written against it needed six separate escapes to raw
// TypeScript, which is six places where the type checker stopped helping.
//
// The shape here is deliberately not a mirror of the browser API. `addEventListener`
// with an event-name string and an event object whose useful field depends on
// which string you passed is a JavaScript idiom, and reproducing it would put
// every callback's parameter back into `unknown`. Instead each event is its own
// method taking exactly what that event carries: `on_message` gets the text,
// `on_close` gets the code and the reason. Nothing needs narrowing, and a typo
// in an event name is not possible because there are no event-name strings.
//
// Only text frames are delivered. Binary frames are decoded as UTF-8 text,
// which is what a JSON protocol over a socket wants; a program that needs the
// bytes is not served by this module yet.

/** A connection. Created by `connect`. */
export type Socket = {
  readonly __socket: unique symbol;
};

/** The minimum of the host global this module uses. */
type HostSocket = {
  send(data: string): void;
  close(code?: number, reason?: string): void;
  addEventListener(type: string, listener: (event: unknown) => void): void;
  readyState: number;
};

type HostSocketCtor = new (url: string) => HostSocket;

function host(): HostSocketCtor {
  const ctor = (globalThis as { WebSocket?: HostSocketCtor }).WebSocket;
  if (typeof ctor !== "function") {
    throw new Error(
      "std/websocket: this runtime has no WebSocket. Node 22+ provides one; " +
        "on an older Node, run with --experimental-websocket.",
    );
  }
  return ctor;
}

/**
 * Open a connection to `url` (`ws://` or `wss://`).
 *
 * Returns immediately; the connection is not open yet. Register the handlers
 * you want first, and write in `on_open`. A send before the socket is open is
 * dropped rather than throwing, so a race during startup does not crash the
 * program.
 */
export function connect(url: string): Socket {
  const Ctor = host();
  return new Ctor(url) as unknown as Socket;
}

function raw(socket: Socket): HostSocket {
  return socket as unknown as HostSocket;
}

/** Called once, when the connection is established. */
export function on_open(socket: Socket, handler: () => void): void {
  raw(socket).addEventListener("open", () => handler());
}

/**
 * Called for each message, with the frame's text.
 *
 * A binary frame is decoded as UTF-8 rather than dropped, so a server that
 * compresses or sends `Buffer`s still delivers readable text.
 */
export function on_message(socket: Socket, handler: (text: string) => void): void {
  raw(socket).addEventListener("message", (event: unknown) => {
    handler(text_of((event as { data?: unknown }).data));
  });
}

/**
 * Called once, when the connection closes, with the close code and reason.
 *
 * The code is what distinguishes an outage worth retrying from a rejection that
 * will be rejected identically forever, so it is handed over rather than
 * summarized. A close with no code reports 1006 ("abnormal closure"), which is
 * what a connection that failed before it opened produces.
 */
export function on_close(
  socket: Socket,
  handler: (code: number, reason: string) => void,
): void {
  raw(socket).addEventListener("close", (event: unknown) => {
    const e = event as { code?: number; reason?: string };
    handler(typeof e.code === "number" ? e.code : 1006, e.reason || "");
  });
}

/**
 * Called when the connection errors.
 *
 * A close always follows, so this is for reporting; the decision about what to
 * do next belongs in `on_close`, which knows the code.
 */
export function on_error(socket: Socket, handler: () => void): void {
  raw(socket).addEventListener("error", () => handler());
}

/**
 * Send a text frame.
 *
 * Returns whether it was sent. A socket that is not open yet, or is already
 * closing, reports `false` instead of throwing: a program that writes into a
 * connection the network has just dropped should get a value it can act on, not
 * an exception on a callback stack it does not control.
 */
export function send(socket: Socket, text: string): boolean {
  const s = raw(socket);
  // 1 is OPEN in every WebSocket implementation.
  if (s.readyState !== 1) {
    return false;
  }
  s.send(text);
  return true;
}

/**
 * Close the connection.
 *
 * `on_close` still fires. Closing an already-closed socket does nothing, so
 * teardown paths are safe to run more than once.
 */
export function close(socket: Socket): void {
  const s = raw(socket);
  // 2 is CLOSING, 3 is CLOSED.
  if (s.readyState === 2 || s.readyState === 3) {
    return;
  }
  s.close();
}

/** Whether the connection is open and can carry a frame right now. */
export function is_open(socket: Socket): boolean {
  return raw(socket).readyState === 1;
}

function text_of(data: unknown): string {
  if (typeof data === "string") {
    return data;
  }
  // A binary frame: ArrayBuffer, a view over one, or a Node Buffer.
  if (data instanceof ArrayBuffer) {
    return new TextDecoder().decode(data);
  }
  if (ArrayBuffer.isView(data)) {
    return new TextDecoder().decode(data as unknown as ArrayBufferView);
  }
  return String(data);
}
