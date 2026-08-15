#!/usr/bin/env node
// Differential check: std/bytes's codecs against node's Buffer, over random
// data rather than fixed vectors.
//
// The codecs are hand-written so they can refuse malformed input and run where
// `Buffer` does not exist. That means the platform is not checking them, and a
// change made for speed can quietly make a decoder lax. The published RFC 4648
// vectors in the compiler's own suite cover the documented cases; this covers
// the ones nobody thought to write down.
//
// Four properties, ~168k checks:
//   1. Encoders agree with `Buffer` byte for byte, at every length 0..300 plus
//      a spread of larger ones, so every partial-group case is exercised.
//   2. Every encoder round-trips through its decoder, base32 included.
//   3. Canonicality: anything a decoder accepts must re-encode to what it was
//      given. This is what catches a decoder gone lax.
//   4. Anything `Buffer` can produce is accepted.
//
// Run: npx tsx scripts/check_bytes_codecs.mjs
import * as bytes from "../glyph-compiler/runtime/std/bytes.ts";
import { randomBytes } from "node:crypto";
let checked = 0, bad = 0;
const fail = (m) => { bad++; if (bad < 10) console.log("FAIL:", m); };

// 1. Encoders must agree with Buffer byte-for-byte, at every length 0..300 and
//    a spread of larger ones (so every partial-group case is covered).
const lens = [...Array(301).keys(), 511, 512, 513, 1023, 1024, 4095, 65535];
for (const n of lens) {
  const b = new Uint8Array(randomBytes(n));
  const buf = Buffer.from(b);
  if (bytes.to_hex(b) !== buf.toString("hex")) fail(`to_hex len ${n}`);
  if (bytes.to_base64(b) !== buf.toString("base64")) fail(`to_base64 len ${n}`);
  if (bytes.to_base64url(b) !== buf.toString("base64url")) fail(`to_base64url len ${n}`);
  checked += 3;

  // 2. Every encoder round-trips through its decoder, base32 included.
  for (const [enc, dec] of [
    [bytes.to_hex, bytes.from_hex],
    [bytes.to_base64, bytes.from_base64],
    [bytes.to_base64url, bytes.from_base64url],
    [bytes.to_base32, bytes.from_base32],
  ]) {
    const r = dec(enc(b));
    if (r.tag !== "Ok") fail(`round-trip rejected len ${n}: ${r.value.message}`);
    else if (!bytes.equals(r.value, b)) fail(`round-trip differs len ${n}`);
    checked++;
  }
}

// 3. Canonicality: anything a decoder ACCEPTS must re-encode to what was given.
//    This is the invariant that catches a decoder gone lax, on inputs no fixed
//    vector covers.
for (let i = 0; i < 40000; i++) {
  const s = Array.from(randomBytes(2 + (i % 12)))
    .map((c) => "ABCZaz09+/-_= !".charAt(c % 15))
    .join("");
  for (const [dec, enc, strip] of [
    [bytes.from_base64, bytes.to_base64, false],
    [bytes.from_base64url, bytes.to_base64url, false],
    [bytes.from_base32, bytes.to_base32, true],
    // hex accepts either case and emits lowercase, so canonicality is
    // case-insensitive here, same as base32.
    [bytes.from_hex, bytes.to_hex, true],
  ]) {
    const r = dec(s);
    if (r.tag === "Ok") {
      let back = enc(r.value);
      let want = s;
      if (strip) { back = back.toUpperCase(); want = s.toUpperCase(); }
      // Padding is optional on input, so compare with it removed from both.
      if (back.replace(/=+$/, "") !== want.replace(/=+$/, "")) {
        fail(`accepted non-canonical ${JSON.stringify(s)} -> re-encodes as ${JSON.stringify(back)}`);
      }
    }
    checked++;
  }
}

// 4. Every string Buffer can produce must be accepted.
for (let i = 0; i < 3000; i++) {
  const b = new Uint8Array(randomBytes(i % 97));
  const buf = Buffer.from(b);
  if (bytes.from_base64(buf.toString("base64")).tag !== "Ok") fail(`rejected Buffer base64 len ${b.length}`);
  if (bytes.from_hex(buf.toString("hex")).tag !== "Ok") fail(`rejected Buffer hex len ${b.length}`);
  checked += 2;
}
if (bad === 0) {
  console.log(`bytes codecs OK: ${checked} differential checks against Buffer passed.`);
} else {
  console.error(`${bad} FAILURES out of ${checked} differential checks.`);
  process.exit(1);
}
