// std/http — an HTTP client over the global `fetch`, plus a small server.
//
// Client calls are async and return a `Result`; a Glyph caller `await`s them. A
// thrown fetch or a non-2xx status becomes `Err(HttpError)`.
//
// The server (`serve`) is errors-as-values too: a `Handler` returns
// `Result<Response, string>` — `Ok(response)` is written with the handler's
// status (a 404 is a normal `Ok`), and `Err(message)` (or a thrown exception)
// becomes a 500. `serve` itself resolves `Ok(void)` when the server closes and
// `Err(message)` on a bind failure. Because `serve` stays pending while the
// server listens, a Glyph `main` that does `await http.serve(...)` never returns,
// so the process stays alive without any keep-alive hack.

import { Result, Ok, Err } from "./result";
import { Option, Some, None } from "./option";
import { createServer, type IncomingMessage, type ServerResponse } from "node:http";

export type Request = {
  url: string;
  method: string;
  headers: Record<string, string>;
  body: unknown;
};

export type Response = { status: number; body: unknown };

export type HttpError = { status: number; message: string };

export async function get(url: string): Promise<Result<Response, HttpError>> {
  return request(url, "GET", undefined);
}

export async function post(url: string, body: unknown): Promise<Result<Response, HttpError>> {
  return request(url, "POST", body);
}

export async function put(url: string, body: unknown): Promise<Result<Response, HttpError>> {
  return request(url, "PUT", body);
}

export async function patch(url: string, body: unknown): Promise<Result<Response, HttpError>> {
  return request(url, "PATCH", body);
}

// `del`, not `delete`: `delete` is a reserved word and cannot be an import name.
export async function del(url: string): Promise<Result<Response, HttpError>> {
  return request(url, "DELETE", undefined);
}

export function json(status: number, body: unknown): Response {
  return { status, body };
}

// ---------------------------------------------------------------------------
// Server
// ---------------------------------------------------------------------------

/// A request handler. May be sync or async; returns `Ok(response)` for any
/// status (a 404 is a normal `Ok`) or `Err(message)` to send a 500.
export type Handler = (
  req: Request,
) => Result<Response, string> | Promise<Result<Response, string>>;

/// Build a `text/plain` response.
export function text(status: number, body: string): Response {
  return { status, body };
}

/// The URL query string parsed into a record (`/x?a=1&b=2` -> `{ a: "1", b: "2" }`).
export function query(req: Request): Record<string, string> {
  const out: Record<string, string> = {};
  const q = req.url.indexOf("?");
  if (q < 0) {
    return out;
  }
  for (const [key, value] of new URLSearchParams(req.url.slice(q + 1))) {
    out[key] = value;
  }
  return out;
}

/// The URL path, without the query string.
export function path(req: Request): string {
  const q = req.url.indexOf("?");
  return q < 0 ? req.url : req.url.slice(0, q);
}

/// A request header by name (case-insensitive), or `None` if it is absent.
/// Untrusted input is an `Option`, so a missing header cannot be mistaken for a
/// present one — you must handle the `None` case.
export function header(req: Request, name: string): Option<string> {
  const value = req.headers[name.toLowerCase()];
  return value === undefined ? None : Some(value);
}

/// A single URL query parameter by name, or `None` if it is absent.
export function query_param(req: Request, name: string): Option<string> {
  const q = req.url.indexOf("?");
  if (q < 0) {
    return None;
  }
  const value = new URLSearchParams(req.url.slice(q + 1)).get(name);
  return value === null ? None : Some(value);
}

/// Start an HTTP server on `port`, dispatching each request to `handler`.
/// Resolves `Ok(void)` when the server closes, `Err(message)` on a bind failure.
/// Stays pending while listening, so `await http.serve(...)` keeps `main` (and
/// the process) alive.
export function serve(port: number, handler: Handler): Promise<Result<void, string>> {
  return new Promise((resolve) => {
    const server = createServer((nreq, nres) => {
      void respond(nreq, nres, handler);
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

async function respond(
  nreq: IncomingMessage,
  nres: ServerResponse,
  handler: Handler,
): Promise<void> {
  const req = await read_request(nreq);
  let result: Result<Response, string>;
  try {
    result = await handler(req);
  } catch (e: unknown) {
    const message = (e as { message?: string } | null)?.message ?? String(e);
    result = Err(message);
  }
  const resp: Response =
    result.tag === "Ok" ? result.value : { status: 500, body: { error: result.value } };
  const is_text = typeof resp.body === "string";
  nres.writeHead(resp.status, {
    "content-type": is_text ? "text/plain; charset=utf-8" : "application/json",
  });
  nres.end(is_text ? (resp.body as string) : JSON.stringify(resp.body));
}

function read_request(nreq: IncomingMessage): Promise<Request> {
  return new Promise((resolve) => {
    nreq.setEncoding("utf8");
    let raw = "";
    nreq.on("data", (chunk) => {
      raw += chunk;
    });
    nreq.on("end", () => {
      const headers: Record<string, string> = {};
      for (const [key, value] of Object.entries(nreq.headers)) {
        if (typeof value === "string") {
          headers[key] = value;
        }
      }
      resolve({
        url: nreq.url ?? "",
        method: nreq.method ?? "GET",
        headers,
        body: raw === "" ? null : parse_body(raw),
      });
    });
  });
}

async function request(
  url: string,
  method: string,
  body: unknown,
): Promise<Result<Response, HttpError>> {
  try {
    const init: RequestInit = { method };
    if (body !== undefined) {
      init.body = JSON.stringify(body);
      init.headers = { "content-type": "application/json" };
    }
    const res = await fetch(url, init);
    const text = await res.text();
    const parsed: unknown = text === "" ? null : parse_body(text);
    if (!res.ok) {
      return Err({ status: res.status, message: text });
    }
    return Ok({ status: res.status, body: parsed });
  } catch (e: unknown) {
    const message = (e as { message?: string } | null)?.message ?? String(e);
    return Err({ status: 0, message });
  }
}

function parse_body(text: string): unknown {
  try {
    return JSON.parse(text);
  } catch {
    return text;
  }
}
