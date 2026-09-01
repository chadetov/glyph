// std/http — an HTTP client over the global `fetch`, plus a small server.
//
// Client calls are async and return a `Result`; a Glyph caller `await`s them. A
// thrown fetch or a non-2xx status becomes `Err(HttpError)`.
//
// The server (`listen`) is errors-as-values too: a `Handler` returns
// `Result<Response, string>` — `Ok(response)` is written with the handler's
// status (a 404 is a normal `Ok`), and `Err(message)` (or a thrown exception)
// becomes a 500. `listen` resolves when the socket is **bound**, handing back a
// `net.Server`, because node's HTTP server is a TCP server: stop it with
// `net.stop`, and `net.on_stop` says when it stopped. The process stays alive
// while a listener is pending, so a server nobody stops runs for the life of
// the program with nothing awaiting it.
//
// A `Response` carries its headers, so the server is not limited to a JSON API:
// `html` serves a page, `redirect` sets a `location`, and `with_header` adds
// anything else. The content type is inferred from the body only when the
// response does not already name one.

import { type Result, Ok, Err } from "./result";
import { type Server, type ServerError, adopt, type HostListenable } from "./net";
import { type Option, Some, None } from "./option";
import { createServer, type IncomingMessage, type ServerResponse } from "node:http";

export type Request = {
  url: string;
  method: string;
  headers: Record<string, string>;
  body: unknown;
  // The unparsed request body exactly as received. `body` is `parse_body(raw)`;
  // `raw` is the bytes themselves, needed when a signature (HMAC) must be
  // verified over the exact payload the client sent. Empty string when there is
  // no body.
  raw: string;
};

/// An HTTP response, on both halves of the module: what a handler returns and
/// what a client call resolves to. `headers` is required, not optional. A
/// response always has a header set, and reading `resp.headers` should never
/// need an absence check. Every constructor below fills it in, so the field is
/// only ever spelled out when a program builds a `Response` by hand.
export type Response = {
  status: number;
  headers: Record<string, string>;
  body: unknown;
  /// The unparsed response body exactly as received, mirroring `Request.raw`.
  /// `body` is `parse_body(raw)`, which is lossy: a `text/plain` body of `42`
  /// parses to the number 42 and a body of `"hi"` to the string `hi`, so the
  /// parsed value cannot tell you what the server actually sent. `to_text`
  /// reads this. Empty string when there is no body.
  raw: string;
  /// The URL the response actually came from. After a followed redirect this is
  /// where you landed, not where you asked, which is the only way a client can
  /// tell it was redirected at all. Empty for a response a handler builds:
  /// nothing has been fetched.
  url: string;
};

/// Why a request failed, following `FsError.kind`: the reason is a value you
/// match on, not a string you parse. `timeout` is the request this client
/// aborted because it outlived its budget; `network` is one that never got an
/// answer (DNS, refused connection); `status` is a response that arrived and
/// was not ok. A caller that must tell "the site is slow" from "the site is
/// gone" had no way to before, because both were `status: 0`.
export type HttpErrorKind = "timeout" | "network" | "status";

export type HttpError = { status: number; message: string; kind: HttpErrorKind };

/// What to do with a 3xx. `follow` is fetch's default and what `get`/`post` do.
/// `manual` returns the redirect response itself, so `status` and the
/// `location` header are readable. `error` fails the call instead.
export type RedirectPolicy = "follow" | "manual" | "error";

/// One request, spelled out. `send` takes this rather than a pile of optional
/// arguments: an optional trailing parameter is exactly the shape Glyph's
/// checker cannot model, and a request that cannot be bounded is not a request
/// you can ship. `timeout_ms` of 0 means no timeout.
export type Fetch = {
  url: string;
  method: string;
  body: Option<unknown>;
  timeout_ms: number;
  redirect: RedirectPolicy;
};

/// A `Fetch` with the defaults `get` uses: follow redirects, no timeout, no
/// body. Build on it rather than writing all five fields every time.
export function fetch_of(url: string, method: string): Fetch {
  return { url, method, body: None, timeout_ms: 0, redirect: "follow" };
}

/// Issue a request under the bounds it carries. A timeout aborts the request
/// rather than abandoning it: `task.race` leaves the loser in flight, which is
/// the thing `std/task`'s own scope rule exists to prevent.
export async function send(f: Fetch): Promise<Result<Response, HttpError>> {
  return request(f.url, f.method, f.body.tag === "Some" ? f.body.value : undefined, {
    timeout_ms: f.timeout_ms,
    redirect: f.redirect,
  });
}

/// A HEAD request: the status and headers with no body fetched.
export async function head(url: string): Promise<Result<Response, HttpError>> {
  return request(url, "HEAD", undefined, { timeout_ms: 0, redirect: "follow" });
}

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

/// The response body as the exact text the server sent.
///
/// `Response.body` is the best-effort JSON parse of that text, and the parse is
/// lossy in a way that matters: a `text/plain` body of `42` becomes the number
/// 42, one of `"hi"` becomes the string `hi`, and an empty body becomes `null`.
/// The parsed value cannot tell you what arrived. Before this, printing a body
/// went through `string.from(response.body)`, and `String(...)` on an object is
/// a legal, silent operation, so a JSON endpoint printed `[object Object]` with
/// no diagnostic anywhere in the pipeline.
///
/// This returns `Result` rather than `string` because the failing case is
/// coming, not because it exists yet: today every response carries its bytes, so
/// the answer is always `Ok`. Whether a non-text `content-type` should be `Err`
/// is an open call recorded against G118 in the gap ledger.
export function to_text(response: Response): Result<string, string> {
  // The exact bytes, not a guess from the parsed value. Testing
  // `typeof body === "string"` looked right and was wrong three ways: an empty
  // body is stored as `null` so it reported "not text" for a body that was
  // simply absent, a `text/plain` body of `42` parses to a number and failed,
  // and a JSON body that happens to be a string succeeded. `raw` is what the
  // server sent.
  return Ok(response.raw);
}

/// Build an `application/json` response.
export function json(status: number, body: unknown): Response {
  return {
    status,
    headers: { "content-type": "application/json" },
    body,
    raw: JSON.stringify(body),
    url: "",
  };
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
  return {
    status,
    headers: { "content-type": "text/plain; charset=utf-8" },
    body,
    raw: body,
    url: "",
  };
}

/// Build a `text/html` response, so a browser renders the markup instead of
/// showing it as source.
export function html(status: number, body: string): Response {
  return {
    status,
    headers: { "content-type": "text/html; charset=utf-8" },
    body,
    raw: body,
    url: "",
  };
}

/// Build a redirect: the given status (302, 301, 303, 307, 308) and a
/// `location` header pointing at `location`. The body is empty.
export function redirect(status: number, location: string): Response {
  return { status, headers: { location }, body: "", raw: "", url: "" };
}

/// A copy of `resp` with one more header set (replacing any header of the same
/// name, compared case-insensitively). Returns a new `Response` rather than
/// mutating: Glyph has no record-field mutation, so the copy is the honest
/// shape. `http.with_header(http.html(200, page), "cache-control", "no-store")`.
export function with_header(resp: Response, name: string, value: string): Response {
  const headers: Record<string, string> = {};
  const lower = name.toLowerCase();
  for (const [key, existing] of Object.entries(resp.headers)) {
    if (key.toLowerCase() !== lower) {
      headers[key] = existing;
    }
  }
  headers[name] = value;
  return { status: resp.status, headers, body: resp.body, raw: resp.raw, url: resp.url };
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

/// The unparsed request body exactly as received (empty string when there is no
/// body). `req.body` is the parsed value; `raw` is the bytes themselves, which
/// is what a signature check (HMAC over the payload) must run over, since
/// re-serializing a parsed body changes whitespace and key order.
export function raw(req: Request): string {
  return req.raw;
}

/// An `application/x-www-form-urlencoded` request body parsed into a record
/// (`a=1&b=hello+world` -> `{ a: "1", b: "hello world" }`). Decodes `+` as a
/// space and percent-escapes as their bytes. A key repeated in the body keeps
/// the last value. Reads `req.raw`, so it is independent of `req.body`: a
/// program that wants the raw bytes or a JSON body still gets them unchanged.
export function form(req: Request): Record<string, string> {
  const out: Record<string, string> = {};
  for (const [key, value] of new URLSearchParams(req.raw)) {
    out[key] = value;
  }
  return out;
}

/// The request path split into its non-empty segments: `/tasks/5` becomes
/// `["tasks", "5"]`. Designed for routing with array patterns, e.g.
/// `match segments(req) { ["tasks", id] => ... }`.
export function segments(req: Request): Array<string> {
  return path(req)
    .split("/")
    .filter((s) => s.length > 0);
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
/**
 * Start serving on `host:port`, and hand back the server.
 *
 * The same contract as `net.listen`, and the same `Server`, because node's HTTP
 * server *is* a TCP server: `Ok` means the port is bound, `Err` says why it is
 * not, `net.stop` ends it and `net.on_stop` says when it ended.
 *
 * This replaced a `serve` that resolved only when the server closed, with
 * nothing able to close one. Its `Ok` branch could never run, and because a
 * `match` on a `Result` must be exhaustive, every caller was forced to write an
 * arm that could not execute. Worse, a failure *after* a successful bind
 * resolved that promise with `Err` while the server was still listening and
 * still answering requests, so the value said dead and the process said alive.
 */
export function listen(
  host: string,
  port: number,
  handler: Handler,
): Promise<Result<Server, ServerError>> {
  const server = createServer((nreq, nres) => {
    void respond(nreq, nres, handler);
  });
  return adopt(server as unknown as HostListenable, host, port);
}

async function respond(
  nreq: IncomingMessage,
  nres: ServerResponse,
  handler: Handler,
): Promise<void> {
  const outcome = await read_request(nreq);
  if (outcome.tag === "gone") {
    // The client left before finishing its body. There is nothing to answer,
    // and writing to a dead socket is how a server takes itself down.
    return;
  }
  if (outcome.tag === "too_large") {
    nres.writeHead(413, { "content-type": "text/plain; charset=utf-8" });
    nres.end("request body too large");
    return;
  }
  const req = outcome.request;
  let result: Result<Response, string>;
  try {
    result = await handler(req);
  } catch (e: unknown) {
    const message = (e as { message?: string } | null)?.message ?? String(e);
    result = Err(message);
  }
  const resp: Response = result.tag === "Ok" ? result.value : json(500, { error: result.value });
  const is_text = typeof resp.body === "string";
  const headers: Record<string, string> = {};
  for (const [key, value] of Object.entries(resp.headers)) {
    headers[key] = sanitize_header_value(value);
  }
  // The content type is inferred (string body -> text/plain, anything else ->
  // JSON) only when the response does not already carry one, so a response
  // built by `json`/`text`/`html` keeps exactly the type it declared.
  const has_content_type = Object.keys(headers).some(
    (key) => key.toLowerCase() === "content-type",
  );
  if (!has_content_type) {
    headers["content-type"] = is_text ? "text/plain; charset=utf-8" : "application/json";
  }
  nres.writeHead(resp.status, headers);
  nres.end(is_text ? (resp.body as string) : JSON.stringify(resp.body));
}

/// Drop every character Node refuses to write in a header value, keeping tab
/// and the printable Latin-1 range. CR and LF are the dangerous case: a newline
/// in a header is response splitting, and an attacker-controlled value (a
/// `location` built from a query parameter, say) could otherwise inject headers
/// or a second response. Anything above U+00FF is rejected too, so a redirect to
/// a URL containing an emoji would throw. Node's check is
/// `/[^\t\x20-\x7e\x80-\xff]/`, and it throws `ERR_INVALID_CHAR` from
/// `writeHead`, where `respond` has no `Result` to put the error in and the
/// throw would take the process down. The characters are removed instead, so no
/// header value the server writes can ever be one Node rejects.
function sanitize_header_value(value: string): string {
  return value.replace(/[^\t\x20-\x7e\x80-\xff]/g, "");
}

/**
 * The largest request body this server will hold in memory.
 *
 * There was no limit, which made an unauthenticated client able to exhaust the
 * process's memory by POSTing forever. Eight megabytes is far above what an API
 * request carries, including a base64 payload, and far below what a stream
 * would need. It is not configurable because a program that genuinely needs
 * more wants a streaming read rather than a bigger buffer, and Glyph has no
 * streaming read yet (G105); when that lands, this becomes part of its design
 * rather than a constant.
 */
const MAX_BODY_BYTES = 8 * 1024 * 1024;

/**
 * What reading a request produced.
 *
 * `gone` is the case that used to hang: a client that disconnects mid-body
 * never fires `end`, so a read waiting only for `end` never settles, `respond`
 * never returns, and the request's whole closure is retained for the life of
 * the process. One interrupted upload in a loop was a memory leak with nothing
 * in the log.
 */
type ReadOutcome =
  | { tag: "request"; request: Request }
  | { tag: "too_large" }
  | { tag: "gone" };

function read_request(nreq: IncomingMessage): Promise<ReadOutcome> {
  return new Promise((resolve) => {
    nreq.setEncoding("utf8");
    let raw = "";
    let size = 0;
    let settled = false;
    const settle = (outcome: ReadOutcome): void => {
      if (settled) return;
      settled = true;
      resolve(outcome);
    };
    nreq.on("data", (chunk) => {
      if (settled) return;
      // Bytes, not characters: `chunk.length` counts UTF-16 code units, so a
      // body of three-byte characters would be allowed to reach three times the
      // limit before this noticed.
      size += Buffer.byteLength(chunk);
      if (size > MAX_BODY_BYTES) {
        settle({ tag: "too_large" });
        return;
      }
      raw += chunk;
    });
    nreq.on("end", () => {
      const headers: Record<string, string> = {};
      for (const [key, value] of Object.entries(nreq.headers)) {
        if (typeof value === "string") {
          headers[key] = value;
        }
      }
      settle({
        tag: "request",
        request: {
          url: nreq.url ?? "",
          method: nreq.method ?? "GET",
          headers,
          body: raw === "" ? null : parse_body(raw),
          raw,
        },
      });
    });
    // Both of these mean there is no request and no one to answer.
    nreq.on("aborted", () => settle({ tag: "gone" }));
    nreq.on("error", () => settle({ tag: "gone" }));
  });
}

async function request(
  url: string,
  method: string,
  body: unknown,
  bounds: { timeout_ms: number; redirect: RedirectPolicy } = {
    timeout_ms: 0,
    redirect: "follow",
  },
): Promise<Result<Response, HttpError>> {
  // An `AbortController` cancels the request itself. Racing a timer against the
  // promise would resolve the caller while the request stayed in flight, which
  // is the workaround this replaces.
  const controller = new AbortController();
  let timer: ReturnType<typeof setTimeout> | undefined;
  if (bounds.timeout_ms > 0) {
    timer = setTimeout(() => controller.abort(), bounds.timeout_ms);
  }
  try {
    const init: RequestInit = {
      method,
      redirect: bounds.redirect,
      signal: controller.signal,
    };
    if (body !== undefined) {
      init.body = JSON.stringify(body);
      init.headers = { "content-type": "application/json" };
    }
    const res = await fetch(url, init);
    const text = method === "HEAD" ? "" : await res.text();
    const parsed: unknown = text === "" ? null : parse_body(text);
    const raw = text;
    const headers: Record<string, string> = {};
    res.headers.forEach((value, key) => {
      headers[key.toLowerCase()] = value;
    });
    if (!res.ok) {
      // A `manual` redirect lands here (3xx is not ok), and the caller asked to
      // see it, so hand back the response rather than an error.
      if (bounds.redirect === "manual" && res.status >= 300 && res.status < 400) {
        return Ok({ status: res.status, headers, body: parsed, raw, url: res.url });
      }
      return Err({ status: res.status, message: text, kind: "status" });
    }
    return Ok({ status: res.status, headers, body: parsed, raw, url: res.url });
  } catch (e: unknown) {
    if (controller.signal.aborted) {
      return Err({
        status: 0,
        message: `request to ${url} exceeded ${bounds.timeout_ms}ms and was aborted`,
        kind: "timeout",
      });
    }
    const message = (e as { message?: string } | null)?.message ?? String(e);
    return Err({ status: 0, message, kind: "network" });
  } finally {
    if (timer !== undefined) {
      clearTimeout(timer);
    }
  }
}

function parse_body(text: string): unknown {
  try {
    return JSON.parse(text);
  } catch {
    return text;
  }
}
