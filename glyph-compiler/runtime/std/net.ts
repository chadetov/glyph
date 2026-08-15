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

/** One connection. Delivered to a server's handler, or created by `connect`. */
export type Socket = {
  readonly __net_socket: unique symbol;
};

/**
 * A listening server, from `listen`.
 *
 * It is a resource like a socket: you hold it, and you stop it. There is no
 * separate cancellation object, and no second value to await, because the
 * process stays alive while a listener is pending, so "run until stopped" is
 * just not stopping it.
 */
export type Server = {
  readonly __net_server: unique symbol;
};

function raw(s: Socket): HostSocket {
  return s as unknown as HostSocket;
}

type HostServer = {
  close(cb?: () => void): unknown;
  on(event: string, listener: (arg?: unknown) => void): unknown;
  listen(port: number, host: string, cb?: () => void): unknown;
  address(): unknown;
};

function raw_server(s: Server): HostServer {
  return s as unknown as HostServer;
}

/**
 * Accept connections on `port`, calling `on_connection` with each one.
 *
 * Resolves when the server stops: `Ok` on a clean close, `Err` with the reason
 * if it could not bind, which is how a port already in use arrives. Await it to
 * keep the process alive for the server's lifetime.
 */
/** Why a server could not start. `kind` is what a caller branches on. */
export type ServerError = {
  readonly kind: ServerErrorKind;
  readonly message: string;
  /// The raw errno (`EADDRINUSE`, `EACCES`, ...), or `""` when there was none.
  /// Everything the `kind` does not name is still reachable here.
  readonly code: string;
};

/**
 * The reasons a bind fails that lead to different decisions.
 *
 * `in_use` means try another port; `denied` means this one will never work for
 * this process (ports below 1024 need privilege) and retrying is pointless;
 * `unavailable` means the host address is not one this machine has. Scraping
 * those out of a message string is what this exists to avoid.
 */
export type ServerErrorKind = "in_use" | "denied" | "unavailable" | "other";

function server_error(e: unknown): ServerError {
  const code = (e as { code?: string } | null)?.code ?? "";
  const message = (e as { message?: string } | null)?.message ?? String(e);
  const kind: ServerErrorKind =
    code === "EADDRINUSE"
      ? "in_use"
      : code === "EACCES" || code === "EPERM"
        ? "denied"
        : code === "EADDRNOTAVAIL"
          ? "unavailable"
          : "other";
  return { kind, message, code };
}

/**
 * Start listening on `host:port`, and hand back the server.
 *
 * Resolves when the socket is **bound**, not when the server stops, so an `Ok`
 * means the port is yours and an `Err` says why it is not.
 *
 * `host` is explicit and has no default. `"127.0.0.1"` accepts only local
 * connections; `"0.0.0.0"` accepts from anywhere on the network. Node's own
 * default is the second one, and a standard library that will not ship a switch
 * for turning off certificate checking should not quietly expose a port to the
 * network either. Say which you meant.
 *
 * The result is a resource: `stop` ends it, `on_stop` says when it ended. The
 * process stays alive while a listener is pending, so a server that is never
 * stopped runs for the life of the program with nothing awaiting it.
 */
export function listen(
  host: string,
  port: number,
  on_connection: (socket: Socket) => void,
): Promise<Result<Server, ServerError>> {
  const server = createServer((socket) => {
    // Node throws an unhandled 'error' event, and this socket is one the server
    // handed the caller rather than one they opened, so a peer that resets the
    // connection would end the process before any handler could exist. This
    // makes that impossible; `on_error` adds reporting on top of it rather than
    // being the only thing between a remote client and the process.
    socket.on("error", () => {});
    on_connection(socket as unknown as Socket);
  });
  return adopt(server as unknown as HostListenable, host, port);
}

/**
 * @internal Shared with `std/http`, not user surface.
 *
 * The subset of a node server `adopt` drives. `std/http` passes its own.
 */
export type HostListenable = {
  on(event: string, listener: (arg?: unknown) => void): unknown;
  listen(port: number, host: string, cb?: () => void): unknown;
};

/**
 * @internal Shared with `std/http`, not user surface.
 *
 * Bind a node server and turn it into a `Server`.
 *
 * Shared with `std/http`, because node's HTTP server is a TCP server and the
 * bind, the error classification and the reporting list are the same three
 * things either way. Two copies would be two chances to get the `bound` flag
 * wrong, which is the bug this whole shape exists to fix.
 */
export function adopt(
  server: HostListenable,
  host: string,
  port: number,
): Promise<Result<Server, ServerError>> {
  return new Promise((resolve) => {
    let bound = false;
    const reported: Array<(e: ServerError) => void> = [];
    error_handlers.set(server as unknown as Server, reported);
    // A permanent sink, so a server error never ends the process. It stays after
    // the bind resolves: removing it would make forgetting `on_server_error`
    // fatal, and killing the process to punish a missing handler is not a trade
    // worth making. What it must not do is swallow the error, so anything nobody
    // asked about goes to stderr.
    server.on("error", (err) => {
      if (!bound) {
        bound = true;
        resolve(Err(server_error(err)));
        return;
      }
      if (reported.length === 0) {
        console.error(
          `std/net: server error after listening: ` +
            `${(err as { message?: string } | null)?.message ?? String(err)}`,
        );
        return;
      }
      for (const h of reported) h(server_error(err));
    });
    server.listen(port, host, () => {
      if (bound) return;
      bound = true;
      resolve(Ok(server as unknown as Server));
    });
  });
}

// Per-server reporting handlers, so `listen` can tell "nobody asked" from "the
// caller wants these" without a second listener that changes throw behaviour.
const error_handlers = new WeakMap<Server, Array<(e: ServerError) => void>>();
const stopped = new WeakSet<Server>();

/**
 * Stop a server: it stops accepting, and closes once its open connections end.
 *
 * Connections already established are **not** dropped, which is what makes this
 * a graceful shutdown rather than a kill; close the sockets yourself if it has
 * to happen sooner. Stopping an already-stopped server does nothing, so a
 * teardown path is safe to run more than once.
 */
export function stop(server: Server): void {
  if (stopped.has(server)) return;
  stopped.add(server);
  raw_server(server).close();
}

/** The port this server is listening on, which `listen(host, 0, ...)` picks. */
export function port(server: Server): number {
  const a = raw_server(server).address();
  return typeof a === "object" && a !== null ? (a as { port: number }).port : 0;
}

/** The server has stopped and its last connection has gone. */
export function on_stop(server: Server, handler: () => void): void {
  raw_server(server).on("close", () => handler());
}

/**
 * A failure after the server was already listening, such as running out of
 * file descriptors.
 *
 * Registering one is optional and changes nothing about whether the process
 * survives: `listen` already keeps the server from throwing. What it changes is
 * where the report goes. With no handler, such an error is printed to stderr,
 * because a server problem nobody hears about is the worse failure.
 */
export function on_server_error(server: Server, handler: (error: ServerError) => void): void {
  const list = error_handlers.get(server);
  if (list) list.push(handler);
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
 * protocol carrying anything but ASCII does not corrupt under load.
 *
 * This does not change how the socket reads, so registering `on_data` as well
 * is fine and both see every byte. A sequence still incomplete when the
 * connection ends is reported rather than dropped: half a character at EOF
 * means the peer was cut off mid-write, which the program wants to know.
 */
export function on_text(socket: Socket, handler: (text: string) => void): void {
  const decoder = new StringDecoder("utf8");
  raw(socket).on("data", (chunk) => {
    const text = decoder.write(chunk);
    if (text.length > 0) handler(text);
  });
  raw(socket).on("close", () => {
    // `end()` returns whatever the decoder was still holding, as replacement
    // characters. Silently discarding it would make a truncated stream look
    // like a clean one, in the module whose whole point is not corrupting a
    // split character.
    const rest = decoder.end();
    if (rest.length > 0) handler(rest);
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
