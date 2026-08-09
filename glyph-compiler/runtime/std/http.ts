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
//
// A `Response` carries its headers, so the server is not limited to a JSON API:
// `html` serves a page, `redirect` sets a `location`, and `with_header` adds
// anything else. The content type is inferred from the body only when the
// response does not already name one.

import { Result, Ok, Err } from "./result";
import { Option, Some, None } from "./option";
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
  /// The URL the response actually came from. After a followed redirect this is
  /// where you landed, not where you asked, which is the only way a client can
  /// tell it was redirected at all. Empty for a response a handler builds:
  /// nothing has been fetched.
  url: string;
};

export type HttpError = { status: number; message: string };

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

/// Build an `application/json` response.
export function json(status: number, body: unknown): Response {
  return { status, headers: { "content-type": "application/json" }, body, url: "" };
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
  return { status, headers: { "content-type": "text/plain; charset=utf-8" }, body, url: "" };
}

/// Build a `text/html` response, so a browser renders the markup instead of
/// showing it as source.
export function html(status: number, body: string): Response {
  return { status, headers: { "content-type": "text/html; charset=utf-8" }, body, url: "" };
}

/// Build a redirect: the given status (302, 301, 303, 307, 308) and a
/// `location` header pointing at `location`. The body is empty.
export function redirect(status: number, location: string): Response {
  return { status, headers: { location }, body: "", url: "" };
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
  return { status: resp.status, headers, body: resp.body, url: resp.url };
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
        raw,
      });
    });
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
    const headers: Record<string, string> = {};
    res.headers.forEach((value, key) => {
      headers[key.toLowerCase()] = value;
    });
    if (!res.ok) {
      // A `manual` redirect lands here (3xx is not ok), and the caller asked to
      // see it, so hand back the response rather than an error.
      if (bounds.redirect === "manual" && res.status >= 300 && res.status < 400) {
        return Ok({ status: res.status, headers, body: parsed, url: res.url });
      }
      return Err({ status: res.status, message: text });
    }
    return Ok({ status: res.status, headers, body: parsed, url: res.url });
  } catch (e: unknown) {
    if (controller.signal.aborted) {
      return Err({
        status: 0,
        message: `request to ${url} exceeded ${bounds.timeout_ms}ms and was aborted`,
      });
    }
    const message = (e as { message?: string } | null)?.message ?? String(e);
    return Err({ status: 0, message });
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
