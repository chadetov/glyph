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
// The data is drawn from a seeded PRNG, not `crypto.randomBytes`, for the same
// reason `std/stream` draws a fixed series for property testing: a check that
// finds something has to be re-runnable. The seed is printed on failure and can
// be set with GLYPH_CODEC_SEED to replay a CI run or to sweep a wider space.
//
// Run: npx tsx scripts/check_bytes_codecs.mjs
import * as bytes from "../glyph-compiler/runtime/std/bytes.ts";

const SEED = Number(process.env.GLYPH_CODEC_SEED ?? 20260815) | 0;

// mulberry32, the generator `std/random` uses. Not cryptographic, which is the
// point: this makes test data, and test data has to repeat.
function rng_from(seed) {
  let a = seed | 0;
  return () => {
    a = (a + 0x6d2b79f5) | 0;
    let t = Math.imul(a ^ (a >>> 15), 1 | a);
    t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t;
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}
const rand = rng_from(SEED);

function randomBytes(n) {
  const b = new Uint8Array(n);
  for (let i = 0; i < n; i++) b[i] = (rand() * 256) | 0;
  return b;
}
let checked = 0, bad = 0;
const fail = (m) => { bad++; if (bad < 10) console.log("FAIL:", m); };

// 1. Encoders must agree with Buffer byte-for-byte, at every length 0..300 and
//    a spread of larger ones (so every partial-group case is covered).
const lens = [...Array(301).keys(), 511, 512, 513, 1023, 1024, 4095, 65535];
for (const n of lens) {
  const b = randomBytes(n);
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
  // Characters drawn directly rather than by taking a byte modulo the alphabet
  // size, which would skew the distribution toward its first entries.
  const ALPHABET = "ABCZaz09+/-_= !";
  let s = "";
  for (let k = 0, len = 2 + (i % 12); k < len; k++) {
    s += ALPHABET.charAt((rand() * ALPHABET.length) | 0);
  }
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
  const b = randomBytes(i % 97);
  const buf = Buffer.from(b);
  if (bytes.from_base64(buf.toString("base64")).tag !== "Ok") fail(`rejected Buffer base64 len ${b.length}`);
  if (bytes.from_hex(buf.toString("hex")).tag !== "Ok") fail(`rejected Buffer hex len ${b.length}`);
  checked += 2;
}
if (bad === 0) {
  console.log(`bytes codecs OK: ${checked} differential checks against Buffer passed.`);
} else {
  console.error(`${bad} FAILURES out of ${checked} differential checks.`);
  console.error(`replay with GLYPH_CODEC_SEED=${SEED}`);
  process.exit(1);
}
