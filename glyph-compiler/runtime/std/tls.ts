// std/tls — TCP with the certificate checked.
//
// This module is `std/net` with one difference, and the difference is the whole
// point: the peer's certificate is verified against the system trust store and
// against the name you asked for. That check is on by default and there is no
// argument to turn it off, because "disable certificate validation" is a line
// that gets added to make a development environment work and then ships.
//
// A `tls.Socket` is a `net.Socket`, so everything in `std/net` applies to it:
// `on_text`, `on_data`, `send`, `close`, and the same warning that TCP has no
// message boundaries. Only `connect` differs, which is why this module is small.
//
// What is deliberately absent is a server. A TLS server needs a certificate and
// a private key, which means a file format, a reload story for renewal, and a
// cipher policy, and getting any of those wrong is worse than not offering it.
// Terminate TLS in front of a `std/net` or `std/http` server for now.

import { type Result, Ok, Err } from "./result";
import { type Socket } from "./net";
import { connect as node_connect, type TLSSocket } from "node:tls";
import { isIP } from "node:net";

/// The largest delay `setTimeout` holds without clamping: 2^31-1 milliseconds,
/// a little under 25 days.
const MAX_TIMEOUT_MS = 2_147_483_647;

/**
 * Open a verified TLS connection to `host:port`, within `timeout_ms`.
 *
 * Resolves once the handshake has completed and the certificate has been
 * accepted, so an `Ok` means the peer proved it is who it said. `Err` carries
 * the reason: an expired certificate, a name that does not match, an issuer the
 * system does not trust, the connection failing before any of that, or the
 * deadline passing with the handshake still unfinished.
 *
 * Awaiting the handshake rather than returning immediately is the difference
 * from `net.connect`. A plain socket has nothing to fail at yet; this one does,
 * and a caller that writes a password to an unverified peer has already lost,
 * so the failure has to arrive before there is anything to write to.
 *
 * **The deadline is required and there is no value meaning "wait forever".** A
 * peer that completes the TCP handshake and then sends nothing (a wedged
 * endpoint, a middlebox swallowing the records, a firewall dropping the SYN)
 * leaves node's promise pending with no handle to abort: no `Ok`, no `Err`, and
 * a socket that keeps the event loop alive for the life of the process. Racing
 * a timer against it does not help, because the losing dial is what pins the
 * loop. So the bound belongs to the dial, which is the only place that can
 * reach the socket and destroy it.
 *
 * The clock starts at the call, not at the handshake, so a slow TCP connect
 * spends the same budget and the `Err` arrives on time in every phase. What is
 * tested is the socket phase: the deadline destroys the socket, and the process
 * exits without waiting on it. A name still being resolved when the deadline
 * passes is not tested, and `socket.destroy()` has nothing to reach in that
 * phase, so a dial spent in name resolution may deliver its `Err` on time and
 * still keep the process alive until the resolver answers.
 */
export function connect(
  host: string,
  port: number,
  timeout_ms: number,
): Promise<Result<Socket, string>> {
  return new Promise((resolve) => {
    let settled = false;
    // Both usage errors are unprefixed, where every other `Err` here is
    // `${host}: <network reason>`. A caller that logs the string would
    // otherwise file a programming error as an endpoint being down, which in an
    // uptime monitor is a page for the wrong thing.
    if (!(timeout_ms > 0)) {
      resolve(
        Err(`a TLS dial needs a deadline greater than 0ms, got ${String(timeout_ms)}`),
      );
      return;
    }
    // Node keeps a timer's delay in a signed 32-bit integer and *clamps* a
    // larger one to 1ms rather than refusing it. Unchecked, a dial asked to
    // wait 35 days fails after a millisecond and reports the failure as if the
    // 35 days had elapsed: a confident wrong answer, which is worse than the
    // hang this deadline replaces. `int` arithmetic reaches the limit without
    // anyone writing a suspicious literal, `days * 86400 * 1000` being enough.
    if (timeout_ms > MAX_TIMEOUT_MS) {
      resolve(
        Err(
          `a TLS dial deadline must be at most ${String(MAX_TIMEOUT_MS)}ms, got ${String(timeout_ms)}`,
        ),
      );
      return;
    }
    // SNI names the host being asked for, and an IP address is not a name:
    // node rejects `servername` set to one, and it does so by throwing from
    // `connect` rather than by failing the handshake. Omitted for an IP, which
    // also means such a connection is verified against the certificate's IP
    // entries rather than a name.
    const options =
      isIP(host) === 0 ? { host, port, servername: host } : { host, port };
    let socket: TLSSocket;
    // Armed once the socket exists and cleared on every settling path, so a
    // connection that lands promptly does not hold the process open until the
    // deadline it never needed.
    let timer: ReturnType<typeof setTimeout> | undefined;
    const settle = (value: Result<Socket, string>) => {
      if (settled) return false;
      settled = true;
      if (timer !== undefined) clearTimeout(timer);
      resolve(value);
      return true;
    };
    try {
      socket = node_connect(options, () => {
        if (settled) return;
        // `authorized` is node's verdict on the certificate chain and the name.
        // It is false rather than throwing, which is exactly the shape that
        // gets ignored, so it is checked here and turned into an `Err`.
        if (!socket.authorized) {
          const why = socket.authorizationError ?? "certificate not accepted";
          socket.destroy();
          settle(Err(`${host}: ${String(why)}`));
          return;
        }
        settle(Ok(socket as unknown as Socket));
      });
    } catch (e: unknown) {
      // A throw before the connection is even attempted (a malformed option, a
      // host node will not accept) has to arrive the same way every other
      // failure here does. Without this the one thing this module promises,
      // that a failed connection is a value, is false for a whole class of it.
      settle(Err((e as { message?: string } | null)?.message ?? String(e)));
      return;
    }
    socket.on("error", (err: { message?: string }) => {
      settle(Err(err.message ?? "tls error"));
    });
    timer = setTimeout(() => {
      // Destroying is the point, not the reporting. An abandoned dial holds a
      // file descriptor and a libuv handle, and node exits when its handles are
      // gone, so a program whose last act was a dial that never answered stays
      // alive until this runs.
      if (settle(Err(`${host}: no TLS handshake within ${String(timeout_ms)}ms`))) {
        socket.destroy();
      }
    }, timeout_ms);
  });
}
