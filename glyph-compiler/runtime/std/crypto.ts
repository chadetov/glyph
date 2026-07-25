// std/crypto — hashing, HMAC, and randomness over node's `crypto`. Security
// primitives belong in the standard library, not an unvetted dependency: these
// are thin wrappers over the platform implementation, returning hex strings.

import {
  createHash,
  createHmac,
  randomBytes,
  randomUUID,
} from "node:crypto";

export function sha256(input: string): string {
  return createHash("sha256").update(input).digest("hex");
}

export function sha512(input: string): string {
  return createHash("sha512").update(input).digest("hex");
}

export function hmac_sha256(key: string, input: string): string {
  return createHmac("sha256", key).update(input).digest("hex");
}

// A cryptographically random UUID (v4), e.g. for identifiers.
export function random_uuid(): string {
  return randomUUID();
}

// `count` random bytes as a hex string (its length is `count * 2`).
export function random_hex(count: number): string {
  return randomBytes(count).toString("hex");
}
