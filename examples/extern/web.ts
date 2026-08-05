// WORKAROUND, not a design choice. See the friction notes at the top of
// shortlink.glyph.
//
// `std/http`'s `Response` is `{ status, body }` with no headers, and its server
// hard-codes `content-type: text/plain` for a string body. A URL shortener
// cannot be written against that: there is no way to send `Location:` (so no
// 302 redirect, which is the app's whole point) and no way to send
// `text/html` (so a browser shows the page source instead of the page).
//
// This file is the smallest transport shim that fixes both. Every piece of
// application logic stays in shortlink.glyph; the request shape below is
// deliberately identical to `std/http`'s `Request`, so `http.path`,
// `http.segments`, and `http.raw` still work on it.
//
// `decode_component` is here for the same reason: `std/string` has no
// `slice`/`char_code`, so percent-decoding an
// `application/x-www-form-urlencoded` body cannot be written in Glyph at all.

import { createServer, type IncomingMessage, type ServerResponse } from "node:http";

export type WebRequest = {
  url: string;
  method: string;
  headers: Record<string, string>;
  body: unknown;
  raw: string;
};

export type WebResponse = {
  status: number;
  content_type: string;
  // "" when the response is not a redirect.
  location: string;
  body: string;
};

export type WebHandler = (req: WebRequest) => WebResponse;

export function decode_component(s: string): string {
  try {
    return decodeURIComponent(s);
  } catch {
    return s;
  }
}

export function serve_web(port: number, handler: WebHandler): Promise<string> {
  return new Promise((resolve) => {
    const server = createServer((nreq: IncomingMessage, nres: ServerResponse) => {
      nreq.setEncoding("utf8");
      let raw = "";
      nreq.on("data", (chunk: string) => {
        raw += chunk;
      });
      nreq.on("end", () => {
        const headers: Record<string, string> = {};
        for (const [key, value] of Object.entries(nreq.headers)) {
          if (typeof value === "string") {
            headers[key] = value;
          }
        }
        const req: WebRequest = {
          url: nreq.url ?? "",
          method: nreq.method ?? "GET",
          headers,
          body: raw,
          raw,
        };
        let res: WebResponse;
        try {
          res = handler(req);
        } catch (e: unknown) {
          const message = (e as { message?: string } | null)?.message ?? String(e);
          res = { status: 500, content_type: "text/plain; charset=utf-8", location: "", body: message };
        }
        const out: Record<string, string> = { "content-type": res.content_type };
        if (res.location !== "") {
          out["location"] = res.location;
        }
        nres.writeHead(res.status, out);
        nres.end(res.body);
      });
    });
    server.on("error", (err: { message?: string }) => {
      resolve(err.message ?? "server error");
    });
    server.on("close", () => {
      resolve("");
    });
    server.listen(port);
  });
}
