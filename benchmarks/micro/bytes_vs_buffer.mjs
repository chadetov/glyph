// std/bytes codecs against node's Buffer, on the same octets.
//
// std/bytes hand-writes hex, base64, base64url and base32 rather than
// delegating to Buffer. That bought two things: the decoders refuse malformed
// input, where Buffer silently skips it, and the module touches no host API so
// it runs in a bare realm. This measures what it cost.
//
// The decode comparison is not like-for-like, and that is the point: Buffer
// does no validation at all. So each decode is measured three ways, which
// separates "what validating costs" from "what not using native code costs":
//
//   Buffer         native decode, no validation (what we did not ship)
//   std/bytes      validation + JS decode       (what we shipped)
//   checked Buffer validation + native decode   (what we could ship)
//
// "checked Buffer" validates *after* a native decode instead of scanning first.
// Node's hex decoder stops at the first bad pair and its base64 decoder skips
// anything outside the alphabet, so in both cases the decoded length comes out
// short of what the input claims, and comparing the two catches it in O(1). Two
// extra things are needed for base64: Node accepts base64url characters under
// "base64", which two native `indexOf` scans rule out, and a final character
// carrying bits past the end of the data survives the length check, so the last
// group is re-encoded and compared. `/tmp` scratch checks confirmed this accepts
// and rejects exactly what `bytes.from_base64` does across 13 cases.
//
// Run: node --experimental-strip-types benchmarks/micro/bytes_vs_buffer.mjs
//   or: npx tsx benchmarks/micro/bytes_vs_buffer.mjs
import * as bytes from "../../glyph-compiler/runtime/std/bytes.ts";

const SIZES = [
  { name: "32 B (a key)", n: 32, iters: 200_000 },
  { name: "1 KB (a frame)", n: 1024, iters: 20_000 },
  { name: "1 MB (a file)", n: 1024 * 1024, iters: 40 },
];

function sample(n) {
  const b = new Uint8Array(n);
  // Deterministic, and full-range so no codec sees a friendly special case.
  for (let i = 0; i < n; i++) b[i] = (i * 37 + (i >> 3) * 11) & 0xff;
  return b;
}

// Guard against the optimizer removing work whose result nothing reads.
let sink = 0;
function keep(v) {
  sink += typeof v === "string" ? v.length : v.length ?? 0;
}

function time(iters, fn) {
  for (let i = 0; i < Math.min(iters, 50); i++) keep(fn()); // warm
  const t0 = process.hrtime.bigint();
  for (let i = 0; i < iters; i++) keep(fn());
  const t1 = process.hrtime.bigint();
  return Number(t1 - t0) / 1e6 / iters; // ms per op
}

function hexChecked(s) {
  if (s.length % 2 !== 0) return null;
  const b = Buffer.from(s, "hex");
  return b.length === s.length / 2 ? b : null;
}
function b64Checked(s) {
  let end = s.length;
  while (end > 0 && s[end - 1] === "=") end--;
  if (s.length - end > 2 || end % 4 === 1) return null;
  if (s.indexOf("-") >= 0 || s.indexOf("_") >= 0) return null;
  const b = Buffer.from(s, "base64");
  const expect = Math.floor((end * 6) / 8);
  if (b.length !== expect) return null;
  const tail = expect % 3;
  if (tail) {
    const canon = b.subarray(expect - tail).toString("base64").replace(/=+$/, "");
    if (canon !== s.slice(end - (tail === 1 ? 2 : 3), end)) return null;
  }
  return b;
}

const rows = [];
for (const { name, n, iters } of SIZES) {
  const raw = sample(n);
  const buf = Buffer.from(raw);
  const hex = buf.toString("hex");
  const b64 = buf.toString("base64");
  const text = "x".repeat(n); // valid UTF-8 of the same length

  const r = (label, ours, theirs, hybrid) => rows.push({ size: name, label, ours, theirs, hybrid });

  r("hex encode",
    time(iters, () => bytes.to_hex(raw)),
    time(iters, () => buf.toString("hex")), null);
  r("hex decode",
    time(iters, () => bytes.from_hex(hex).value),
    time(iters, () => Buffer.from(hex, "hex")),
    time(iters, () => hexChecked(hex)));
  r("base64 encode",
    time(iters, () => bytes.to_base64(raw)),
    time(iters, () => buf.toString("base64")), null);
  r("base64 decode",
    time(iters, () => bytes.from_base64(b64).value),
    time(iters, () => Buffer.from(b64, "base64")),
    time(iters, () => b64Checked(b64)));
  r("utf8 encode",
    time(iters, () => bytes.from_text(text)),
    time(iters, () => Buffer.from(text, "utf8")), null);
  const utf8 = Buffer.from(text, "utf8");
  const utf8u8 = new Uint8Array(utf8.buffer, utf8.byteOffset, utf8.byteLength);
  r("utf8 decode",
    time(iters, () => bytes.to_text(utf8u8).value),
    time(iters, () => utf8.toString("utf8")), null);
  // base32 has no Buffer equivalent at all.
  r("base32 encode", time(iters, () => bytes.to_base32(raw)), null, null);
}

const f = (v) => (v === null ? "n/a" : v < 0.001 ? v.toFixed(5) : v < 1 ? v.toFixed(4) : v.toFixed(2));
console.log("| size | operation | std/bytes | Buffer | checked Buffer | ours vs Buffer |");
console.log("|---|---|---|---|---|---|");
for (const row of rows) {
  const ratio = row.theirs ? `${(row.ours / row.theirs).toFixed(1)}x` : "no equivalent";
  console.log(
    `| ${row.size} | ${row.label} | ${f(row.ours)} | ${f(row.theirs)} | ${f(row.hybrid)} | ${ratio} |`,
  );
}
console.error(`(ms per operation; sink=${sink})`);
