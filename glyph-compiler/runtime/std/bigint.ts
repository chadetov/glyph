// std/bigint: the two conversions between `number` and `bigint` (G571, D38).
//
// Glyph keeps the two types apart, as `tsc` does, and has no bare `BigInt()` or
// `Number()`: each one goes wrong on part of its input. `BigInt(1.5)` throws a
// RangeError, and so do `NaN` and the infinities. `Number(b)` never throws:
// past `Number.MAX_SAFE_INTEGER` it rounds to the nearest float, so
// `Number(9007199254740993n)` is `9007199254740992` and nothing says so.
//
// Both functions here return a `Result` instead, so the input each direction
// cannot carry is a value the caller handles. Nothing in this module throws.

import { Err, Ok, type Result } from "./result";

/// `n` as a `bigint`. `Err` when `n` is not a whole number: a fraction, `NaN`,
/// `Infinity` or `-Infinity`. `-0` is `0n`. A whole number past
/// `Number.MAX_SAFE_INTEGER` converts to exactly the value the float holds.
export function from_number(n: number): Result<bigint, string> {
  if (!Number.isInteger(n)) {
    return Err(`std/bigint.from_number: ${String(n)} is not a whole number`);
  }
  return Ok(BigInt(n));
}

const MAX_SAFE = BigInt(Number.MAX_SAFE_INTEGER);

/// `b` as a `number`. `Err` when `b` is above `Number.MAX_SAFE_INTEGER` or
/// below `-Number.MAX_SAFE_INTEGER`, where a `number` can no longer hold every
/// whole number and the conversion would round.
export function to_number(b: bigint): Result<number, string> {
  if (b > MAX_SAFE || b < -MAX_SAFE) {
    return Err(
      `std/bigint.to_number: ${b.toString()} is outside the range a number holds exactly (-9007199254740991 to 9007199254740991)`,
    );
  }
  return Ok(Number(b));
}
