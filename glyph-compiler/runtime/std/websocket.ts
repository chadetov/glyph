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
// Text and binary frames are delivered separately. `on_message` gets text,
// `on_binary` gets `Bytes`, and a frame goes to exactly one of them. Until
// 0.1.80 there was no byte type, so a binary frame was decoded as UTF-8 and
// handed to `on_message`, which is fine for a JSON protocol and silently
// corrupts anything else.
//
// Unlike TCP, a WebSocket frame carries its own boundaries, so a message
// arrives whole and there is none of the partial-character handling `std/net`
// needs. The split here is by payload kind, not by chunking.

import { type Result, Ok, Err } from "./result";
import { type Bytes } from "./bytes";

/** A connection. Created by `connect`. */
export type Socket = {
  readonly __socket: unique symbol;
};

/** The minimum of the host global this module uses. */
type HostSocket = {
  send(data: string | Uint8Array): void;
  binaryType: string;
  protocol: string;
  close(code?: number, reason?: string): void;
  addEventListener(type: string, listener: (event: unknown) => void): void;
  readyState: number;
};

type HostSocketCtor = new (url: string, protocols?: string | string[]) => HostSocket;

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
  const s = new Ctor(url);
  // Ask for octets rather than a Blob, so `on_binary` has something it can
  // hand over without an async read. The default differs between runtimes,
  // which is exactly the kind of thing a program should not have to know.
  s.binaryType = "arraybuffer";
  return s as unknown as Socket;
}

/**
 * Open a connection, offering one or more subprotocols.
 *
 * The server picks one and `protocol` reports which, or the empty string if it
 * declined them all. A server that recognizes none of them closes the
 * connection, so the choice is worth reading before sending anything that
 * depends on it.
 */
export function connect_with(url: string, protocols: ReadonlyArray<string>): Socket {
  const Ctor = host();
  const s = new Ctor(url, protocols as string[]);
  s.binaryType = "arraybuffer";
  return s as unknown as Socket;
}

/** The subprotocol the server accepted, or `""` if it accepted none. */
export function protocol(socket: Socket): string {
  return raw(socket).protocol;
}

function raw(socket: Socket): HostSocket {
  return socket as unknown as HostSocket;
}

/** Called once, when the connection is established. */
export function on_open(socket: Socket, handler: () => void): void {
  raw(socket).addEventListener("open", () => handler());
}

/**
 * Called for each **text** frame, with its text.
 *
 * A binary frame does not reach this handler; register `on_binary` for those.
 * Before 0.1.80 a binary frame was decoded as UTF-8 and delivered here, which
 * was right for a JSON protocol and wrong for everything else.
 */
export function on_message(socket: Socket, handler: (text: string) => void): void {
  raw(socket).addEventListener("message", (event: unknown) => {
    const data = (event as { data?: unknown }).data;
    if (typeof data === "string") handler(data);
  });
}

/**
 * Called for each **binary** frame, with its octets.
 *
 * The frame is delivered whole: WebSocket carries message boundaries, so unlike
 * a TCP read this is never half of anything.
 */
export function on_binary(socket: Socket, handler: (data: Bytes) => void): void {
  raw(socket).addEventListener("message", (event: unknown) => {
    const data = (event as { data?: unknown }).data;
    const bytes = octets_of(data);
    if (bytes !== null) handler(bytes);
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
 * Send a binary frame.
 *
 * Same contract as `send`: `false` rather than a throw when the socket is not
 * open. The peer receives it as a binary frame, so it arrives at the other
 * end's `on_binary` and not its `on_message`.
 */
export function send_bytes(socket: Socket, data: Bytes): boolean {
  const s = raw(socket);
  if (s.readyState !== 1) {
    return false;
  }
  s.send(data);
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

// The octets of a binary frame, or `null` when the frame was text. Runtimes
// differ on what they hand over (an `ArrayBuffer` after `binaryType` is set, a
// `Buffer` on some Node builds), so both are accepted; the view is over the
// same memory rather than a copy.
function octets_of(data: unknown): Bytes | null {
  if (typeof data === "string") {
    return null;
  }
  if (data instanceof ArrayBuffer) {
    return new Uint8Array(data);
  }
  if (ArrayBuffer.isView(data)) {
    const v = data as ArrayBufferView;
    return new Uint8Array(v.buffer, v.byteOffset, v.byteLength);
  }
  return null;
}

// ---------------------------------------------------------------------------
// Server
// ---------------------------------------------------------------------------
//
// A server-side connection is the same `Socket` as a client one. It has to be:
// otherwise `on_message`, `send`, `send_bytes`, `close` and `is_open` would each
// need a server-side twin, and every program handling both ends would carry two
// vocabularies for one idea. So the frame codec below presents exactly the shape
// the client functions already reach for (`send`, `close`, `addEventListener`,
// `readyState`, `protocol`), and everything above this line works unchanged on
// either end.
//
// RFC 6455 in the parts that matter: the handshake proves the peer is a
// WebSocket client rather than something that wandered onto the port, frames
// from a client are always masked, a message can be split across a first frame
// and continuations, and control frames (ping/close) can arrive in the middle of
// a fragmented message and must be answered without disturbing it.

import { createHash } from "node:crypto";
import * as net from "./net";

/** A listening WebSocket server, from `listen`. Stop it with `stop`. */
export type Server = {
  readonly __ws_server: unique symbol;
};

// The constant every WebSocket handshake concatenates, from RFC 6455 §1.3. It
// exists so a server cannot accidentally accept a plain HTTP request.
const GUID = "258EAFA5-E914-47DA-95CA-C5AB0DC85B11";

const OPCODE_CONTINUATION = 0x0;
const OPCODE_TEXT = 0x1;
const OPCODE_BINARY = 0x2;
const OPCODE_CLOSE = 0x8;
const OPCODE_PING = 0x9;
const OPCODE_PONG = 0xa;

type Listener = (event: unknown) => void;

/**
 * Accept WebSocket connections on `host:port`.
 *
 * The same contract as `net.listen`, including the explicit host: `Ok` means the
 * port is bound, `Err` says why it is not. Each accepted connection is an
 * ordinary `Socket`, so the handler uses `on_message`, `on_binary`, `send` and
 * `close` exactly as a client does.
 */
export function listen(
  host: string,
  port: number,
  on_connection: (socket: Socket) => void,
): Promise<Result<Server, net.ServerError>> {
  return net
    .listen(host, port, (conn: net.Socket) => {
      accept(conn, on_connection);
    })
    .then((r) => (r.tag === "Ok" ? Ok(r.value as unknown as Server) : Err(r.value)));
}

/**
 * Stop accepting. Open connections are left to finish, as in `net.stop`, and
 * stopping twice does nothing.
 */
export function stop(server: Server): void {
  net.stop(server as unknown as net.Server);
}

/** The port this server is listening on, which `listen(host, 0, ...)` picks. */
export function port(server: Server): number {
  return net.port(server as unknown as net.Server);
}

/** The server has stopped and its last connection has gone. */
export function on_stop(server: Server, handler: () => void): void {
  net.on_stop(server as unknown as net.Server, handler);
}

// One accepted TCP connection, from raw bytes to a `Socket`.
function accept(conn: net.Socket, on_connection: (socket: Socket) => void): void {
  let handshaken = false;
  let pending: Uint8Array = new Uint8Array(0);
  const listeners: Record<string, Listener[]> = {};
  // The frame codec is written against octets, so the handshake is read from
  // the same stream rather than from a text handler: a client is entitled to
  // put the first frame in the same packet as the request.
  net.on_data(conn, (chunk) => {
    pending = concat(pending, chunk);
    if (!handshaken) {
      const end = header_end(pending);
      if (end < 0) return;
      const request = latin1(pending.subarray(0, end));
      pending = pending.subarray(end);
      const key = header_value(request, "sec-websocket-key");
      if (key === null) {
        net.send(conn, "HTTP/1.1 400 Bad Request\r\n\r\n");
        net.close(conn);
        return;
      }
      const accept_key = createHash("sha1").update(key + GUID).digest("base64");
      net.send(
        conn,
        "HTTP/1.1 101 Switching Protocols\r\n" +
          "Upgrade: websocket\r\n" +
          "Connection: Upgrade\r\n" +
          `Sec-WebSocket-Accept: ${accept_key}\r\n\r\n`,
      );
      handshaken = true;
      on_connection(socket);
      emit("open", {});
    }
    drain();
  });
  net.on_close(conn, () => emit("close", { code: 1006, reason: "" }));
  net.on_error(conn, () => emit("error", {}));

  let closing = false;
  // Whole messages only. A fragmented message accumulates here until its FIN,
  // so a handler never sees half of one, which is the guarantee that lets
  // `on_message` take a `string` rather than a chunk.
  let fragment_opcode = 0;
  let fragments: Uint8Array = new Uint8Array(0);

  function emit(type: string, event: unknown): void {
    for (const l of listeners[type] ?? []) l(event);
  }

  function drain(): void {
    for (;;) {
      const frame = parse_frame(pending);
      if (frame === null) return;
      pending = pending.subarray(frame.size);
      const op = frame.opcode;
      if (op === OPCODE_CLOSE) {
        // Echo the close and let the TCP close deliver the event, so a peer
        // that closes cleanly and one that vanishes look the same to a handler.
        if (!closing) {
          closing = true;
          // Echo the peer's own status back, which is what the RFC asks for,
          // and 1000 when it sent none.
          write_frame(
            conn,
            OPCODE_CLOSE,
            frame.payload.length >= 2 ? frame.payload : close_payload(1000),
          );
        }
        net.close(conn);
        return;
      }
      if (op === OPCODE_PING) {
        write_frame(conn, OPCODE_PONG, frame.payload);
        continue;
      }
      if (op === OPCODE_PONG) continue;
      if (op === OPCODE_CONTINUATION) {
        fragments = concat(fragments, frame.payload);
      } else {
        fragment_opcode = op;
        fragments = frame.payload;
      }
      if (!frame.fin) continue;
      const body = fragments;
      fragments = new Uint8Array(0);
      emit("message", {
        data: fragment_opcode === OPCODE_TEXT ? utf8(body) : body,
      });
    }
  }

  // The object the client-side functions above already know how to drive.
  const socket = {
    get readyState(): number {
      return closing ? 3 : handshaken ? 1 : 0;
    },
    protocol: "",
    binaryType: "arraybuffer",
    send(data: string | Uint8Array): void {
      if (typeof data === "string") {
        write_frame(conn, OPCODE_TEXT, encode_utf8(data));
      } else {
        write_frame(conn, OPCODE_BINARY, data);
      }
    },
    close(): void {
      if (closing) return;
      closing = true;
      // A close frame with no payload makes the peer report 1005, "no status
      // received", which is the code for a connection that ended without
      // saying why. This one has a reason: it was closed deliberately, so it
      // carries 1000 and the peer's `on_close` can tell the two apart.
      write_frame(conn, OPCODE_CLOSE, close_payload(1000));
      net.close(conn);
    },
    addEventListener(type: string, listener: Listener): void {
      (listeners[type] ??= []).push(listener);
    },
  } as unknown as Socket;
}

// --- framing ---------------------------------------------------------------

type Frame = { fin: boolean; opcode: number; payload: Uint8Array; size: number };

// `null` when the buffer does not hold a whole frame yet, which is the normal
// case on a stream: TCP splits wherever it likes.
function parse_frame(b: Uint8Array): Frame | null {
  if (b.length < 2) return null;
  const first = b[0] as number;
  const second = b[1] as number;
  const fin = (first & 0x80) !== 0;
  const opcode = first & 0x0f;
  const masked = (second & 0x80) !== 0;
  let len = second & 0x7f;
  let at = 2;
  if (len === 126) {
    if (b.length < at + 2) return null;
    len = ((b[at] as number) << 8) | (b[at + 1] as number);
    at += 2;
  } else if (len === 127) {
    if (b.length < at + 8) return null;
    // The high four octets would mean a payload past 4 GiB, which nothing here
    // can hold anyway; reading them as a float keeps the arithmetic honest
    // rather than silently wrapping.
    let big = 0;
    for (let i = 0; i < 8; i++) big = big * 256 + (b[at + i] as number);
    if (big > Number.MAX_SAFE_INTEGER) return null;
    len = big;
    at += 8;
  }
  let mask: Uint8Array | null = null;
  if (masked) {
    if (b.length < at + 4) return null;
    mask = b.subarray(at, at + 4);
    at += 4;
  }
  if (b.length < at + len) return null;
  const raw = b.subarray(at, at + len);
  // A client MUST mask (RFC 6455 §5.1); unmasking in place would corrupt the
  // caller's buffer, so this copies.
  const payload = new Uint8Array(len);
  for (let i = 0; i < len; i++) {
    payload[i] = mask === null ? (raw[i] as number) : (raw[i] as number) ^ (mask[i & 3] as number);
  }
  return { fin, opcode, payload, size: at + len };
}

// Server frames are never masked (RFC 6455 §5.1).
function write_frame(conn: net.Socket, opcode: number, payload: Uint8Array): void {
  const len = payload.length;
  const head = len < 126 ? 2 : len < 65536 ? 4 : 10;
  const out = new Uint8Array(head + len);
  out[0] = 0x80 | opcode;
  if (len < 126) {
    out[1] = len;
  } else if (len < 65536) {
    out[1] = 126;
    out[2] = (len >> 8) & 0xff;
    out[3] = len & 0xff;
  } else {
    out[1] = 127;
    let rest = len;
    for (let i = 9; i >= 2; i--) {
      out[i] = rest & 0xff;
      rest = Math.floor(rest / 256);
    }
  }
  out.set(payload, head);
  net.send_bytes(conn, out as unknown as Bytes);
}

// --- small helpers ---------------------------------------------------------

// A close frame's status is two octets, big-endian, ahead of any reason text.
function close_payload(code: number): Uint8Array {
  return new Uint8Array([(code >> 8) & 0xff, code & 0xff]);
}

function concat(a: Uint8Array, b: Uint8Array): Uint8Array {
  const out = new Uint8Array(a.length + b.length);
  out.set(a, 0);
  out.set(b, a.length);
  return out;
}

function header_end(b: Uint8Array): number {
  for (let i = 3; i < b.length; i++) {
    if (b[i - 3] === 13 && b[i - 2] === 10 && b[i - 1] === 13 && b[i] === 10) return i + 1;
  }
  return -1;
}

function header_value(request: string, name: string): string | null {
  for (const line of request.split("\r\n")) {
    const colon = line.indexOf(":");
    if (colon < 0) continue;
    if (line.slice(0, colon).trim().toLowerCase() === name) {
      return line.slice(colon + 1).trim();
    }
  }
  return null;
}

function latin1(b: Uint8Array): string {
  return new TextDecoder("latin1").decode(b);
}

function utf8(b: Uint8Array): string {
  return new TextDecoder("utf-8").decode(b);
}

function encode_utf8(s: string): Uint8Array {
  return new TextEncoder().encode(s);
}
