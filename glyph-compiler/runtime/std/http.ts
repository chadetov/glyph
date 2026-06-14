// std/http — an HTTP client over the global `fetch`. Calls are async and return
// a `Result`; a Glyph caller `await`s them. Network and non-2xx responses are
// values: a thrown fetch or a non-ok status becomes `Err(HttpError)`.

import { Result, Ok, Err } from "./result";

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

export function json(status: number, body: unknown): Response {
  return { status, body };
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
