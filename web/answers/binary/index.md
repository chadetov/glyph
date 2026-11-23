<!-- Markdown copy of https://glyphlang.io/answers/binary/, generated from the same source as the page. Last changed 2026-11-21. -->

20 · Binary data

# Can I handle binary data, or is everything a string?

`std/bytes` gives you an immutable sequence of octets, and `std/fs` and `std/crypto` both speak it. Until 0.1.78 they did not: every boundary in the standard library was string-in, string-out, so a PNG could not be read and an HMAC could not be given a real key.

One-time password, start to finish

```
module totp

import std/bytes
import std/crypto
import std/math
import std/array
import std/option { Some, None }
import std/result { Result, Ok }

fn at(b: bytes.Bytes, i: int) -> int {
  return match bytes.get(b, i) {
    Some(v) => v,
    None => 0,
  }
}

pub fn code(secret: string, counter: number) -> Result<number, bytes.BytesError> {
  let key = bytes.from_base32(secret)?
  let message = bytes.from_array(array.map(array.range(8), fn(i: int) -> int {
    return math.floor(counter / math.pow(256, 7 - i)) % 256
  }))?

  let mac = crypto.hmac_sha1_bytes(key, message)
  let offset = at(mac, bytes.len(mac) - 1) % 16
  return Ok((((at(mac, offset) % 128) * 16777216)
    + (at(mac, offset + 1) * 65536)
    + (at(mac, offset + 2) * 256)
    + at(mac, offset + 3)) % 100000000)
}
```

See it: an RFC 6238 authenticator, which needs bytes on both sides of the HMAC. The secret arrives base32-encoded, the message is an eight-byte counter that is mostly zero, and the digest is indexed by its own last byte.

## Why it matters

Two applications written independently, one a PNG reader and one an authenticator, stopped on this same sentence in the same week. Neither could be worked around inside the language.

- **A file read as text** A PNG's first byte is `0x89`, which is not valid UTF-8 on its own. Read that file as text and every byte the decoder cannot make sense of becomes U+FFFD, so the file is corrupt before your program sees it and nothing reports an error.
- **A key routed through a string** The same thing happens to a key: an HMAC secret is arbitrary octets, and routing one through a string changes it, so you compute a different MAC than the specification says and every test you wrote against your own code passes.

## What a `Bytes` value is

`Bytes` is a `Uint8Array` at run time, so it hands to any host API that takes one without unwrapping.

The sequence operations are named after their peers in `std/array` and `std/string`: `len`, `get`, `slice`, `concat`, `index_of`, `starts_with`. Nothing mutates its argument.

## Every decode can refuse, and says where

This is the part that cost the most to build and is the reason to use it. Node's `Buffer` accepts malformed input and reports success:

What the platform does quietly

```
Buffer.from("zz", "hex")        // an empty buffer, no error
Buffer.from("a-b_", "base64")   // skips - and _, decodes the rest, no error
buf.toString("utf8")            // U+FFFD for anything malformed, no error
```

What Glyph does

```
match bytes.from_hex(input) {
  Ok(b) => use(b),
  Err(e) => io.eprintln("${e.message} at ${number.to_string(e.index)}"),
}

// "z" is not a hex digit at 0
// a trailing base64 group cannot be 1 character(s) long at 3
// not valid UTF-8 at 2
```

**Details what each codec refuses **

Every codec is written out rather than delegated to `Buffer`, so hex, base64, base64url and base32 all refuse a character outside their alphabet, a trailing group too short to encode a whole byte, and a final character carrying bits past the end of the data. `to_text` scans to find the first byte that cannot be part of a valid UTF-8 sequence, so you get a position rather than a verdict. `from_array` rejects anything outside 0..255 for the same reason: a silent `& 0xff` turns 256 into 0 and a typo into data.

Writing the codecs out bought something that was not the motive. The module reaches for no host API at all, only `Uint8Array`, `TextEncoder` and `TextDecoder`, so a bundle that touches only `std/bytes` still runs in a Web Worker.

## Which HMAC you pick changes the answer

`std/crypto` now has both forms of every algorithm.

- **The plain name** The plain name takes a string and returns hex, which is what hashing a password wants.
- **The `_bytes` form** The `_bytes` form takes and returns `Bytes`, which is what a wire protocol wants.

If the key came from `bytes.from_base64` or `crypto.random_bytes`, it has bytes a string cannot hold, and the text form will quietly compute something else.

Verifying an attacker-supplied value against a secret uses `crypto.timing_safe_equal`, not `bytes.equals`. `equals` returns as soon as it finds a mismatch, so how long it takes says how many leading bytes were right.

## Where it stands

### Shipping today

- `std/bytes` with 21 functions: the sequence operations, the UTF-8 bridge, and hex, base64, base64url and base32 codecs that return a `Result` naming the offending position.
- `fs.read_bytes`, `write_bytes` and `append_bytes` read and write a file undecoded.
- `std/crypto` has a `_bytes` form of every digest and HMAC, plus SHA-1 for the protocols that specify it, `random_bytes`, and `timing_safe_equal`.
- The codecs are pinned against the RFC 4648 vectors and the HMAC against RFC 6238, in the compiler's own test suite.

### Getting sharper

- There is no integer codec yet, so reading a big-endian `u32` out of a header is arithmetic you write: past 32 bits a shift is the wrong tool anyway, so it divides.
- Hex literals still do not parse, which makes a 256-entry CRC32 table written in decimal hard to read.
- `std/websocket` still decodes a binary frame as UTF-8 text; the type it needed now exists, and the frames are next.
- And `std/encoding`'s six older string-to-string functions are still silent on malformed input, so use the `std/bytes` codecs for anything that has to be right.
- The codecs are written in TypeScript rather than delegating to node's `Buffer`, which is what lets them run in a Web Worker, and they are between 1x and 35x slower than the platform's on a megabyte.
- Encoding a key or a token costs nothing you can measure either way; the performance guide has the table if you are moving tens of megabytes.

Code examples on this page are checked against Glyph 0.1.167 on every build. Last changed 21 Nov 2026.
