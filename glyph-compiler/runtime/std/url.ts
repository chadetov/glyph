// std/url — parse, build, and resolve URLs, and percent-encoding both ways.
//
// A URL arrives as a string and gets taken apart with `string.split` and
// `string.index_of` until something is wrong, which is how a program ends up
// treating `https://evil.com@example.com/` as a request to `example.com`. The
// parse here is the host's WHATWG one, which is the same parser a browser and
// `fetch` use, so a string that looks like a URL to them looks like one here.
//
// `Url` is a record rather than an opaque handle, so its parts are ordinary
// fields you match and grep for, and `format` puts one back together. That is
// the opposite of the choice `std/net` makes for `Socket`, and for the opposite
// reason: a socket is a live host resource with no meaningful parts, a parsed
// URL is data.
//
// `URL` and `encodeURIComponent` are language globals rather than node's, so
// this module runs in a bare realm the way `std/bytes` does.

import { type Result, Ok, Err } from "./result";
import { type Option, Some, None } from "./option";

/**
 * The parts of a URL.
 *
 * `query` and `fragment` carry no leading `?` or `#`. `port` is `None` when the
 * URL leaves it to the scheme's default, so a round trip through `format` does
 * not turn `https://x/` into `https://x:443/`.
 */
export type Url = {
  readonly scheme: string;
  readonly host: string;
  readonly port: Option<number>;
  readonly path: string;
  readonly query: string;
  readonly fragment: Option<string>;
};

/** One `key=value` from a query string. */
export type Param = {
  readonly key: string;
  readonly value: string;
};

type HostUrlCtor = new (url: string, base?: string) => {
  protocol: string;
  hostname: string;
  port: string;
  pathname: string;
  search: string;
  hash: string;
  href: string;
};

function ctor(): HostUrlCtor {
  const c = (globalThis as { URL?: HostUrlCtor }).URL;
  if (typeof c !== "function") {
    throw new Error("std/url: this runtime has no URL global");
  }
  return c;
}

/**
 * Parse an absolute URL.
 *
 * `Err` for anything the host parser rejects, which includes a relative
 * reference: use `join` when you have a base to resolve against.
 */
export function parse(text: string): Result<Url, string> {
  const U = ctor();
  let u: InstanceType<HostUrlCtor>;
  try {
    u = new U(text);
  } catch {
    return Err(`not a URL: ${JSON.stringify(text)}`);
  }
  return Ok(from_host(u));
}

function from_host(u: InstanceType<HostUrlCtor>): Url {
  return {
    // The host reports these with their punctuation attached.
    scheme: u.protocol.endsWith(":") ? u.protocol.slice(0, -1) : u.protocol,
    host: u.hostname,
    port: u.port === "" ? None : Some(Number(u.port)),
    path: u.pathname,
    query: u.search.startsWith("?") ? u.search.slice(1) : u.search,
    fragment: u.hash === "" ? None : Some(u.hash.startsWith("#") ? u.hash.slice(1) : u.hash),
  };
}

/**
 * Resolve `relative` against `base`, the way a browser resolves a link.
 *
 * `join("https://x/a/b", "../c")` is `https://x/c`, and an absolute `relative`
 * replaces the base entirely. This is the operation to reach for when following
 * a `Location` header or a link in a document; string concatenation gets the
 * `..` and the scheme-relative `//host/path` cases wrong.
 */
export function join(base: string, relative: string): Result<Url, string> {
  const U = ctor();
  try {
    return Ok(from_host(new U(relative, base)));
  } catch {
    return Err(`cannot resolve ${JSON.stringify(relative)} against ${JSON.stringify(base)}`);
  }
}

/** Render a `Url` back to a string. */
export function format(u: Url): string {
  const port = u.port.tag === "Some" ? `:${u.port.value}` : "";
  const query = u.query === "" ? "" : `?${u.query}`;
  const fragment = u.fragment.tag === "Some" ? `#${u.fragment.value}` : "";
  return `${u.scheme}://${u.host}${port}${u.path}${query}${fragment}`;
}

/**
 * Every `key=value` in a query string, in order and including repeats.
 *
 * An array rather than a `Record`, because `?tag=a&tag=b` is legal and common,
 * and a map would silently drop one of them. Values are percent-decoded, and
 * `+` counts as a space, which is what a form post sends.
 */
export function query_params(query: string): Array<Param> {
  const out: Array<Param> = [];
  for (const pair of query.split("&")) {
    if (pair === "") continue;
    const eq = pair.indexOf("=");
    const key = eq < 0 ? pair : pair.slice(0, eq);
    const value = eq < 0 ? "" : pair.slice(eq + 1);
    out.push({ key: form_decode(key), value: form_decode(value) });
  }
  return out;
}

/** The first value for `name`, or `None`. Repeats need `query_params`. */
export function query_param(query: string, name: string): Option<string> {
  for (const p of query_params(query)) {
    if (p.key === name) return Some(p.value);
  }
  return None;
}

/** Build a query string from pairs, percent-encoding both sides. */
export function to_query(params: ReadonlyArray<Param>): string {
  return params
    .map((p) => `${encode_component(p.key)}=${encode_component(p.value)}`)
    .join("&");
}

// A malformed escape decodes to itself rather than throwing: a query string is
// attacker-controlled, and one bad byte in one parameter should not take down
// the read of the others. `decode_component` is the strict form for when the
// caller wants to know.
function form_decode(text: string): string {
  try {
    return decodeURIComponent(text.replace(/\+/g, " "));
  } catch {
    return text;
  }
}

/**
 * Percent-encode a string for use as one path segment or one query value.
 *
 * Encodes `/`, `?`, `&`, `=` and `#`, so the result cannot escape the position
 * it is placed in. This is the function to use when interpolating anything into
 * a URL.
 */
export function encode_component(text: string): string {
  return encodeURIComponent(text);
}

/**
 * Decode one percent-encoded component, reporting a malformed escape.
 *
 * `decodeURIComponent` throws on `%zz` or a truncated `%4`; this returns the
 * reason instead. `+` is left alone: it means a space in a form body, not in a
 * path, and only the caller knows which one it has.
 */
export function decode_component(text: string): Result<string, string> {
  try {
    return Ok(decodeURIComponent(text));
  } catch {
    return Err(`malformed percent-encoding in ${JSON.stringify(text)}`);
  }
}
