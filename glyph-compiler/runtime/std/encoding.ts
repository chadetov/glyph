// std/encoding — base64 and hex text encodings, over the platform primitives.

// Base64-encode a UTF-8 string.
export function base64_encode(text: string): string {
  return Buffer.from(text, "utf8").toString("base64");
}

// Decode a base64 string back to UTF-8 text.
export function base64_decode(encoded: string): string {
  return Buffer.from(encoded, "base64").toString("utf8");
}

// URL-safe base64 (RFC 4648 §5): `+/` become `-_`, no padding.
export function base64url_encode(text: string): string {
  return Buffer.from(text, "utf8").toString("base64url");
}

export function base64url_decode(encoded: string): string {
  return Buffer.from(encoded, "base64url").toString("utf8");
}

// Lowercase hex of a UTF-8 string's bytes.
export function hex_encode(text: string): string {
  return Buffer.from(text, "utf8").toString("hex");
}

export function hex_decode(encoded: string): string {
  return Buffer.from(encoded, "hex").toString("utf8");
}
