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

/**
 * Open a verified TLS connection to `host:port`.
 *
 * Resolves once the handshake has completed and the certificate has been
 * accepted, so an `Ok` means the peer proved it is who it said. `Err` carries
 * the reason: an expired certificate, a name that does not match, an issuer the
 * system does not trust, or the connection failing before any of that.
 *
 * Awaiting the handshake rather than returning immediately is the difference
 * from `net.connect`. A plain socket has nothing to fail at yet; this one does,
 * and a caller that writes a password to an unverified peer has already lost,
 * so the failure has to arrive before there is anything to write to.
 */
export function connect(host: string, port: number): Promise<Result<Socket, string>> {
  return new Promise((resolve) => {
    let settled = false;
    // SNI names the host being asked for, and an IP address is not a name:
    // node rejects `servername` set to one, and it does so by throwing from
    // `connect` rather than by failing the handshake. Omitted for an IP, which
    // also means such a connection is verified against the certificate's IP
    // entries rather than a name.
    const options =
      isIP(host) === 0 ? { host, port, servername: host } : { host, port };
    let socket: TLSSocket;
    try {
      socket = node_connect(options, () => {
        if (settled) return;
        settled = true;
        // `authorized` is node's verdict on the certificate chain and the name.
        // It is false rather than throwing, which is exactly the shape that
        // gets ignored, so it is checked here and turned into an `Err`.
        if (!socket.authorized) {
          const why = socket.authorizationError ?? "certificate not accepted";
          socket.destroy();
          resolve(Err(`${host}: ${String(why)}`));
          return;
        }
        resolve(Ok(socket as unknown as Socket));
      });
    } catch (e: unknown) {
      // A throw before the connection is even attempted (a malformed option, a
      // host node will not accept) has to arrive the same way every other
      // failure here does. Without this the one thing this module promises,
      // that a failed connection is a value, is false for a whole class of it.
      settled = true;
      resolve(Err((e as { message?: string } | null)?.message ?? String(e)));
      return;
    }
    socket.on("error", (err: { message?: string }) => {
      if (settled) return;
      settled = true;
      resolve(Err(err.message ?? "tls error"));
    });
  });
}
