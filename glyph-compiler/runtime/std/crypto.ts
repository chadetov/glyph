// std/crypto — hashing, HMAC, and randomness over node's `crypto`. Security
// primitives belong in the standard library, not an unvetted dependency: these
// are thin wrappers over the platform implementation.
//
// Each algorithm comes in two forms. The plain name takes a UTF-8 string and
// returns lowercase hex, which is what a program hashing a password or an ETag
// wants. The `_bytes` form takes and returns `Bytes`, which is what a wire
// protocol wants: an HMAC key is arbitrary octets, and routing one through a
// string replaces every byte that is not valid UTF-8 with U+FFFD, so the string
// form of a real key computes a different MAC than the specification says.

import {
  createHash,
  createHmac,
  randomBytes as node_random_bytes,
  randomUUID,
  timingSafeEqual,
} from "node:crypto";
import { type Bytes } from "./bytes";

// node's digests return `Buffer`, whose `slice` aliases rather than copies.
// `Bytes` promises a copy, so a digest is re-wrapped as a plain `Uint8Array`
// view over the same memory before it crosses into Glyph.
// Typed as `Uint8Array` rather than `Buffer`, which is the one name that means
// the same thing under the bundled node shim and under a project's own
// `@types/node`.
function to_bytes(buf: Uint8Array): Bytes {
  return new Uint8Array(buf.buffer, buf.byteOffset, buf.byteLength);
}

// SHA-1 is **not collision-resistant** and must not be used for signatures,
// certificates, or deduplicating untrusted content. It is here because some
// protocols specify it and interoperating means computing what they compute:
// RFC 4226/6238 one-time passwords default to HMAC-SHA-1, as does every
// authenticator app, and the WebSocket handshake hashes its key with it. Reach
// for `sha256` unless a specification names this one.
export function sha1(input: string): string {
  return createHash("sha1").update(input, "utf8").digest("hex");
}

export function sha256(input: string): string {
  return createHash("sha256").update(input).digest("hex");
}

export function sha512(input: string): string {
  return createHash("sha512").update(input).digest("hex");
}

export function sha1_bytes(input: Bytes): Bytes {
  return to_bytes(createHash("sha1").update(input).digest());
}

export function sha256_bytes(input: Bytes): Bytes {
  return to_bytes(createHash("sha256").update(input).digest());
}

export function sha512_bytes(input: Bytes): Bytes {
  return to_bytes(createHash("sha512").update(input).digest());
}

// See `sha1` for why SHA-1 is available at all.
export function hmac_sha1(key: string, input: string): string {
  return createHmac("sha1", key).update(input, "utf8").digest("hex");
}

export function hmac_sha256(key: string, input: string): string {
  return createHmac("sha256", key).update(input).digest("hex");
}

export function hmac_sha512(key: string, input: string): string {
  return createHmac("sha512", key).update(input).digest("hex");
}

export function hmac_sha1_bytes(key: Bytes, input: Bytes): Bytes {
  return to_bytes(createHmac("sha1", key).update(input).digest());
}

export function hmac_sha256_bytes(key: Bytes, input: Bytes): Bytes {
  return to_bytes(createHmac("sha256", key).update(input).digest());
}

export function hmac_sha512_bytes(key: Bytes, input: Bytes): Bytes {
  return to_bytes(createHmac("sha512", key).update(input).digest());
}

// A cryptographically random UUID (v4), e.g. for identifiers.
export function random_uuid(): string {
  return randomUUID();
}

// `count` random bytes as a hex string (its length is `count * 2`).
export function random_hex(count: number): string {
  return node_random_bytes(count).toString("hex");
}

// `count` cryptographically random octets, for a key, a nonce, or a salt.
export function random_bytes(count: number): Bytes {
  return to_bytes(node_random_bytes(count));
}

// Compare two secrets without leaking where they first differ.
//
// `bytes.equals` returns as soon as it finds a mismatch, so how long it takes
// says how many leading bytes were right, and an attacker who can submit
// guesses and time the answer recovers a MAC or a session token a byte at a
// time. Verifying anything an attacker supplies against anything secret uses
// this instead. Lengths are compared first and unequal lengths return `false`
// immediately: a length is not a secret, and the platform primitive throws
// rather than answering when they differ.
export function timing_safe_equal(a: Bytes, b: Bytes): boolean {
  if (a.length !== b.length) return false;
  return timingSafeEqual(a, b);
}
