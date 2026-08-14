// std/encoding — base64 and hex text encodings, over the platform primitives.
//
// Text in, text out: these are the convenience form for when both sides are
// strings. When either side is octets (a key, a binary file, a wire frame), the
// codecs are on `std/bytes` — `bytes.to_base64`, `bytes.from_hex` — which also
// covers base32 and refuses malformed input rather than skipping it.
//
// These six do not. `Buffer.from` ignores any character outside the alphabet, so
// `base64_decode("!!!")` is `""` with no error, and `toString("utf8")`
// substitutes U+FFFD for a byte sequence that is not valid UTF-8. Their
// signatures have no room to say so, which is why the `std/bytes` decoders
// return a `Result`; changing these would be a breaking change to six shipped
// signatures and is tracked as its own item.

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
