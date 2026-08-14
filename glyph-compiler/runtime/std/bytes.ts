// std/bytes — an immutable sequence of octets, and the bridges between octets
// and text.
//
// Every other external boundary in this standard library is string-in,
// string-out, which leaves no spelling for "these bytes". A PNG's first byte is
// 0x89, which is not valid UTF-8 on its own, so a program that reads one as text
// has already lost; an HMAC key is arbitrary octets, and routing it through a
// string silently replaces every invalid sequence with U+FFFD. `Bytes` is the
// type those programs were missing.
//
// A `Bytes` is a `Uint8Array` at run time, so it hands straight to any host API
// that takes one with no unwrapping. Nothing here mutates its argument: `slice`,
// `concat` and `join` all return a new value, matching `std/array`. One caveat
// travels with that: node hands back `Buffer`, a `Uint8Array` subclass whose
// `slice` returns a *view* sharing memory instead of copying. So a `Buffer`
// crossing into Glyph is re-wrapped as a plain `Uint8Array` at the point it
// enters (see `fs.read_bytes`), which is a view over the same memory rather than
// a copy, and every `Bytes` in circulation slices by value.
//
// The naming follows `std/array` and `std/string`, which are this type's peers:
// `len`, `get`, `slice`, `concat`, `index_of`, `starts_with`. The codecs are
// `to_*`/`from_*` pairs on this module rather than additions to `std/encoding`,
// whose six functions are text-to-text and stay that way.
//
// This module reaches for no host API: `Uint8Array`, `TextEncoder` and
// `TextDecoder` are the whole of it, and the base64 and hex codecs are written
// out rather than delegated to node's `Buffer`. So a bundle that only touches
// `std/bytes` still runs in a bare realm such as a Web Worker, and the codecs
// reject malformed input, which `Buffer` does not: it skips characters outside
// the alphabet and reports success.
//
// What is deliberately not here: reading a multi-byte integer out of a buffer.
// Round 28 wrote big-endian decoding in ordinary Glyph over D36's operators and
// checked it against published vectors, so the arithmetic is not the gap; only
// the octets were.

import { type Result, Ok, Err } from "./result";
import { type Option, Some, None } from "./option";

// An immutable sequence of octets.
export type Bytes = Uint8Array;

// Why a byte sequence could not be produced. `message` says what is wrong;
// `index` says where, and is always a position in the input that was rejected:
// an element index for `from_array`, a character index for a decode, a byte
// index for `to_text`. When input is missing rather than wrong (an odd-length
// hex string), `index` is the position the missing input would have occupied.
export type BytesError = { message: string; index: number };

function fail(message: string, index: number): Result<never, BytesError> {
  return Err({ message, index });
}

// The empty sequence. Zero-length, so there is nothing in it to share.
export const empty: Bytes = new Uint8Array(0);

// Build bytes from integers, rejecting anything that is not an octet. A silent
// `& 0xff` would turn 256 into 0 and a typo into data, so the check is a
// boundary and its failure is a value.
export function from_array(xs: ReadonlyArray<number>): Result<Bytes, BytesError> {
  const out = new Uint8Array(xs.length);
  for (let i = 0; i < xs.length; i++) {
    const v = xs[i] as number;
    if (!Number.isInteger(v) || v < 0 || v > 255) {
      return fail(`${v} is not a byte (expected an integer 0..255)`, i);
    }
    out[i] = v;
  }
  return Ok(out);
}

// The octets as numbers, for arithmetic. Each is 0..255.
export function to_array(b: Bytes): Array<number> {
  return Array.from(b);
}

// UTF-8 encode. Total: every string has a UTF-8 encoding.
export function from_text(text: string): Bytes {
  return new TextEncoder().encode(text);
}

// UTF-8 decode, rejecting anything that is not valid UTF-8. `Buffer.toString`
// substitutes U+FFFD for a malformed sequence and reports success, which turns
// a truncated read into plausible-looking text; this returns the position of the
// first byte that cannot start or continue a valid sequence instead.
export function to_text(b: Bytes): Result<string, BytesError> {
  try {
    return Ok(new TextDecoder("utf-8", { fatal: true }).decode(b));
  } catch {
    return fail("not valid UTF-8", first_invalid_utf8(b));
  }
}

// The index of the first byte that breaks UTF-8. Only ever called after the
// decoder has already refused the input, so the scan is on the error path and
// `b.length` is unreachable in practice; it is returned rather than thrown so
// this cannot itself become a crash.
function first_invalid_utf8(b: Bytes): number {
  let i = 0;
  while (i < b.length) {
    const c = b[i] as number;
    // Continuation length, and the smallest code point the sequence may encode
    // (overlong encodings are invalid, and so are surrogates and > U+10FFFF).
    let extra: number;
    let min: number;
    if (c < 0x80) {
      i += 1;
      continue;
    } else if (c >= 0xc2 && c <= 0xdf) {
      extra = 1;
      min = 0x80;
    } else if (c >= 0xe0 && c <= 0xef) {
      extra = 2;
      min = 0x800;
    } else if (c >= 0xf0 && c <= 0xf4) {
      extra = 3;
      min = 0x10000;
    } else {
      return i;
    }
    if (i + extra >= b.length) return i;
    let cp = c & (0x7f >> extra);
    for (let k = 1; k <= extra; k++) {
      const cc = b[i + k] as number;
      if ((cc & 0xc0) !== 0x80) return i;
      cp = (cp << 6) | (cc & 0x3f);
    }
    if (cp < min || (cp >= 0xd800 && cp <= 0xdfff) || cp > 0x10ffff) return i;
    i += extra + 1;
  }
  return b.length;
}

export function len(b: Bytes): number {
  return b.length;
}

// The octet at `i`, or `None` when `i` is outside the sequence. Same shape as
// `array.get`, so an out-of-range read is a value rather than `undefined`.
export function get(b: Bytes, i: number): Option<number> {
  if (!Number.isInteger(i) || i < 0 || i >= b.length) {
    return None;
  }
  return Some(b[i] as number);
}

// A copy of the octets from `start` up to (not including) `end`, which defaults
// to the end of the sequence. Out-of-range bounds clamp, as they do in
// `array.slice`.
export function slice(b: Bytes, start: number, end?: number): Bytes {
  return b.slice(start, end);
}

export function concat(a: Bytes, b: Bytes): Bytes {
  const out = new Uint8Array(a.length + b.length);
  out.set(a, 0);
  out.set(b, a.length);
  return out;
}

// Concatenate many, in one allocation. `join` rather than `concat` because
// `string.join` is the peer that already takes an array.
export function join(parts: ReadonlyArray<Bytes>): Bytes {
  let total = 0;
  for (const p of parts) total += p.length;
  const out = new Uint8Array(total);
  let at = 0;
  for (const p of parts) {
    out.set(p, at);
    at += p.length;
  }
  return out;
}

// Value equality. `==` on two `Bytes` compares references, so two separately
// decoded copies of the same key are unequal; this compares the octets. It is
// not constant-time: comparing a secret needs `crypto.timing_safe_equal`.
export function equals(a: Bytes, b: Bytes): boolean {
  if (a.length !== b.length) return false;
  for (let i = 0; i < a.length; i++) {
    if (a[i] !== b[i]) return false;
  }
  return true;
}

// Where `needle` first occurs in `haystack`. An empty needle is at 0, matching
// `string.index_of`.
export function index_of(haystack: Bytes, needle: Bytes): Option<number> {
  if (needle.length === 0) return Some(0);
  const last = haystack.length - needle.length;
  outer: for (let i = 0; i <= last; i++) {
    for (let k = 0; k < needle.length; k++) {
      if (haystack[i + k] !== needle[k]) continue outer;
    }
    return Some(i);
  }
  return None;
}

// Does this begin with `prefix`? The check a format reader opens with: a PNG is
// the eight bytes 137 80 78 71 13 10 26 10 and nothing else.
export function starts_with(b: Bytes, prefix: Bytes): boolean {
  if (prefix.length > b.length) return false;
  for (let i = 0; i < prefix.length; i++) {
    if (b[i] !== prefix[i]) return false;
  }
  return true;
}

// Lowercase hex, two characters per octet.
export function to_hex(b: Bytes): string {
  let out = "";
  for (let i = 0; i < b.length; i++) {
    out += (b[i] as number).toString(16).padStart(2, "0");
  }
  return out;
}

// Parse hex, accepting either case. `Buffer.from(s, "hex")` stops silently at
// the first character that is not a hex digit and returns the prefix it managed,
// so `"zz"` decodes to nothing at all and reports no error; this names the
// offending character instead.
export function from_hex(encoded: string): Result<Bytes, BytesError> {
  if (encoded.length % 2 !== 0) {
    return fail("hex needs two characters per byte, and this has an odd number", encoded.length);
  }
  const out = new Uint8Array(encoded.length / 2);
  for (let i = 0; i < encoded.length; i += 2) {
    const hi = hex_digit(encoded.charCodeAt(i));
    if (hi < 0) return fail(`${JSON.stringify(encoded[i])} is not a hex digit`, i);
    const lo = hex_digit(encoded.charCodeAt(i + 1));
    if (lo < 0) return fail(`${JSON.stringify(encoded[i + 1])} is not a hex digit`, i + 1);
    out[i / 2] = hi * 16 + lo;
  }
  return Ok(out);
}

function hex_digit(code: number): number {
  if (code >= 48 && code <= 57) return code - 48; // 0-9
  if (code >= 97 && code <= 102) return code - 87; // a-f
  if (code >= 65 && code <= 70) return code - 55; // A-F
  return -1;
}

// The three RFC 4648 alphabets. base32's has no lowercase members, so case
// carries no information there and a decode folds it; in base64 the case of a
// character is part of the value, and folding it would change the bytes.
const BASE64_STANDARD = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
const BASE64_URL = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
const BASE32_STANDARD = "ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";

// Standard base64 (RFC 4648 §4), padded.
export function to_base64(b: Bytes): string {
  return encode(b, BASE64_STANDARD, 6, true);
}

// URL-safe base64 (RFC 4648 §5): `+/` become `-_`, and there is no padding.
export function to_base64url(b: Bytes): string {
  return encode(b, BASE64_URL, 6, false);
}

// base32 (RFC 4648 §6), padded and uppercase. This is how a TOTP secret is
// written: `otpauth://` URIs carry the shared key in base32, so an authenticator
// starts by decoding one.
export function to_base32(b: Bytes): string {
  return encode(b, BASE32_STANDARD, 5, true);
}

// Parse standard base64. Padding is optional on input, but the alphabet is not:
// node's decoder skips any character it does not recognize, so a base64url
// string decodes under the standard alphabet to quietly wrong bytes.
export function from_base64(encoded: string): Result<Bytes, BytesError> {
  return decode(encoded, BASE64_STANDARD, 6, false, "base64");
}

// Parse URL-safe base64.
export function from_base64url(encoded: string): Result<Bytes, BytesError> {
  return decode(encoded, BASE64_URL, 6, false, "base64url");
}

// Parse base32, in either case. Padding is optional, which matters because most
// `otpauth://` secrets are written without it.
export function from_base32(encoded: string): Result<Bytes, BytesError> {
  return decode(encoded, BASE32_STANDARD, 5, true, "base32");
}

// One encoder for all three: take `bits` at a time off the front of the octets
// and look each group up in `alphabet`. The trailing partial group is padded
// with zero bits, then the output is padded with `=` to a whole number of
// groups when the encoding calls for it.
function encode(b: Bytes, alphabet: string, bits: number, pad: boolean): string {
  const mask = (1 << bits) - 1;
  let out = "";
  let acc = 0;
  let held = 0;
  for (let i = 0; i < b.length; i++) {
    acc = (acc << 8) | (b[i] as number);
    held += 8;
    while (held >= bits) {
      held -= bits;
      out += alphabet[(acc >> held) & mask];
    }
  }
  if (held > 0) {
    out += alphabet[(acc << (bits - held)) & mask];
  }
  if (pad) {
    // A whole group is `lcm(8, bits) / bits` characters: 4 for base64, 8 for
    // base32.
    const group = bits === 6 ? 4 : 8;
    while (out.length % group !== 0) out += "=";
  }
  return out;
}

// The matching decoder, and the only place any of the three can refuse. Three
// things are rejected, all of which node's `Buffer` accepts: a character outside
// the alphabet, a trailing group too short to encode a whole byte, and a final
// character carrying bits past the end of the data.
function decode(
  encoded: string,
  alphabet: string,
  bits: number,
  fold_case: boolean,
  name: string,
): Result<Bytes, BytesError> {
  // Trailing padding is optional on input, and must be the last thing present.
  let end = encoded.length;
  while (end > 0 && encoded[end - 1] === "=") end--;
  const group = bits === 6 ? 4 : 8;
  if (encoded.length - end >= group) {
    return fail(`more padding than one ${name} group can hold`, end);
  }
  // A leftover group of r characters carries r*bits bits. When that leaves a
  // whole unused character's worth, the last character encodes nothing and the
  // string is not a valid encoding of any byte sequence.
  const leftover = (end % group) * bits;
  if (leftover % 8 >= bits) {
    return fail(`a trailing ${name} group cannot be ${end % group} character(s) long`, end - 1);
  }
  const out = new Uint8Array(Math.floor((end * bits) / 8));
  let acc = 0;
  let held = 0;
  let at = 0;
  for (let i = 0; i < end; i++) {
    const ch = encoded[i] as string;
    const v = alphabet.indexOf(fold_case ? ch.toUpperCase() : ch);
    if (v < 0) {
      return fail(`${JSON.stringify(ch)} is not in the ${name} alphabet`, i);
    }
    acc = (acc << bits) | v;
    held += bits;
    if (held >= 8) {
      held -= 8;
      out[at++] = (acc >> held) & 0xff;
    }
  }
  // The leftover bits of a partial group are padding and must be zero, so one
  // encoded string means one byte sequence and encoding the result gives back
  // what was parsed.
  if (held > 0 && (acc & ((1 << held) - 1)) !== 0) {
    return fail(`the final ${name} character has bits set past the end of the data`, end - 1);
  }
  return Ok(out);
}
