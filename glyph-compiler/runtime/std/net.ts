// std/net — TCP, as a server that accepts many clients and a client that talks
// to one.
//
// This is the last raw host call in the examples tree. `chat/daemon.glyph`
// imports node's `net` directly and holds an opaque `Socket`, which E0304
// refuses to validate, so the one program in the repo that speaks to a real
// network is also the one place the type checker stops helping.
//
// The shape follows `std/websocket` rather than node: each event is its own
// function taking exactly what that event carries, so no callback parameter
// needs narrowing and there are no event-name strings to misspell. It follows
// `std/http` for the server's lifetime: `serve` resolves when the server closes
// or fails to bind, so a port already in use is a value you match on rather
// than an exception thrown at a listener you may not have registered.
//
// **Text and bytes are separate handlers, and the difference matters.** TCP is a
// stream of octets with no message boundaries, so a multi-byte character can be
// split across two packets. Decoding each chunk on its own turns that character
// into two replacement characters, and the bug only appears under load or with
// non-ASCII input, which is the worst combination to debug. `on_text` holds a
// decoder per socket and emits only whole characters. `on_data` gives the
// octets untouched, for a binary protocol.

import { type Result, Ok, Err } from "./result";
import { type Option, Some, None } from "./option";
import { type Bytes } from "./bytes";
import { createServer, connect as node_connect, type Socket as HostSocket } from "node:net";
import { StringDecoder } from "node:string_decoder";

/** One connection. Delivered to `serve`'s handler, or created by `connect`. */
export type Socket = {
  readonly __net_socket: unique symbol;
};

function raw(s: Socket): HostSocket {
  return s as unknown as HostSocket;
}

/**
 * Accept connections on `port`, calling `on_connection` with each one.
 *
 * Resolves when the server stops: `Ok` on a clean close, `Err` with the reason
 * if it could not bind, which is how a port already in use arrives. Await it to
 * keep the process alive for the server's lifetime.
 */
export function serve(
  port: number,
  on_connection: (socket: Socket) => void,
): Promise<Result<void, string>> {
  return new Promise((resolve) => {
    const server = createServer((socket) => {
      on_connection(socket as unknown as Socket);
    });
    server.on("error", (err) => {
      resolve(Err(err.message ?? "server error"));
    });
    server.on("close", () => {
      resolve(Ok(undefined));
    });
    server.listen(port);
  });
}

/**
 * Open a connection to `host:port`.
 *
 * Returns immediately; the socket is not connected yet. Register handlers
 * first and write in `on_connect`, the same order `std/websocket` asks for.
 */
export function connect(host: string, port: number): Socket {
  return node_connect(port, host) as unknown as Socket;
}

/** The connection is established and writes will go out. */
export function on_connect(socket: Socket, handler: () => void): void {
  raw(socket).on("connect", handler);
}

/**
 * Text arrived.
 *
 * Whole characters only. A UTF-8 sequence split across two packets is held
 * until the rest of it arrives rather than being decoded to U+FFFD, so a
 * protocol carrying anything but ASCII does not corrupt under load. Registering
 * this sets the socket's encoding; use `on_data` instead for a binary protocol.
 */
export function on_text(socket: Socket, handler: (text: string) => void): void {
  const decoder = new StringDecoder("utf8");
  raw(socket).on("data", (chunk) => {
    const text = decoder.write(chunk);
    if (text.length > 0) handler(text);
  });
}

/** Octets arrived, exactly as they came off the wire. */
export function on_data(socket: Socket, handler: (data: Bytes) => void): void {
  raw(socket).on("data", (chunk) => {
    handler(new Uint8Array(chunk.buffer, chunk.byteOffset, chunk.byteLength));
  });
}

/**
 * The connection closed, for any reason.
 *
 * Node always follows `error` with `close`, so this is the one place to
 * deregister a connection: doing it in `on_error` leaves `close` unable to find
 * what it is meant to clean up.
 */
export function on_close(socket: Socket, handler: () => void): void {
  raw(socket).on("close", handler);
}

/**
 * The connection failed.
 *
 * A failed write usually means the peer vanished, and `on_close` will follow,
 * so this handler is for reporting rather than cleanup. Registering one is not
 * optional: an unhandled socket error is thrown, which ends the process.
 */
export function on_error(socket: Socket, handler: (message: string) => void): void {
  raw(socket).on("error", (err) => {
    handler(err.message ?? "socket error");
  });
}

/** Write text, UTF-8 encoded. */
export function send(socket: Socket, text: string): void {
  raw(socket).write(text);
}

/** Write octets. */
export function send_bytes(socket: Socket, data: Bytes): void {
  raw(socket).write(data);
}

/** Finish writing and close this end. The peer sees a clean shutdown. */
export function close(socket: Socket): void {
  raw(socket).end();
}

/** Drop the connection now, without flushing what is queued. */
export function destroy(socket: Socket): void {
  raw(socket).destroy();
}

/**
 * Send small writes immediately rather than coalescing them (Nagle off).
 *
 * Worth setting for an interactive protocol, where a few bytes of input should
 * not wait for more to accumulate. Not worth it for bulk transfer.
 */
export function no_delay(socket: Socket, enabled: boolean): void {
  raw(socket).setNoDelay(enabled);
}

/** The peer's address, or `None` once the socket has closed. */
export function peer_address(socket: Socket): Option<string> {
  const a = raw(socket).remoteAddress;
  return a === undefined ? None : Some(a);
}

/** The peer's port, or `None` once the socket has closed. */
export function peer_port(socket: Socket): Option<number> {
  const p = raw(socket).remotePort;
  return p === undefined ? None : Some(p);
}
