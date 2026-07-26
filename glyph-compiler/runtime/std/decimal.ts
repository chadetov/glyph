// std/decimal — exact base-10 fixed-point arithmetic over BigInt.
//
// JS `number` is an IEEE-754 binary float: `0.1 + 0.2 !== 0.3`, and it loses
// precision past 2^53. Neither is acceptable for money. A `Decimal` holds an
// arbitrary-precision integer `units` scaled by `scale` fractional digits, so
// `10.50` is `{ units: 1050n, scale: 2 }`. Addition, subtraction, and
// multiplication are exact; division cannot always be, so it takes an explicit
// result scale and rounds half-up (round half away from zero).
//
// There is no operator overloading in Glyph, so operations are methods:
// `price.add(tax).to_string()`. Construction validates its input and returns a
// `Result`, so a malformed amount is a handled value, never a silent `NaN`.

import { Err, Ok, Result } from "./result";

export type Decimal = {
  add: (other: Decimal) => Decimal;
  sub: (other: Decimal) => Decimal;
  mul: (other: Decimal) => Decimal;
  // Divide, rounding the result to `scale` fractional digits (half away from
  // zero). Division has no exact answer in general, so the scale is explicit.
  div: (other: Decimal, scale: number) => Decimal;
  // Round this value to `scale` fractional digits (half away from zero).
  round: (scale: number) => Decimal;
  neg: () => Decimal;
  abs: () => Decimal;
  // -1 if this < other, 0 if equal in value, 1 if this > other.
  cmp: (other: Decimal) => number;
  eq: (other: Decimal) => boolean;
  is_zero: () => boolean;
  is_negative: () => boolean;
  // The number of fractional digits carried.
  scale: () => number;
  // Canonical string, e.g. "10.50" (trailing zeros of the scale are kept).
  to_string: () => string;
  // Lossy: a JS float for display/interop only. Never round-trip money through it.
  to_number: () => number;
};

function pow10(n: number): bigint {
  return 10n ** BigInt(n);
}

// Round |num| / |den| to the nearest integer, ties away from zero. `den > 0`.
function divRoundHalfUp(num: bigint, den: bigint): bigint {
  const neg = num < 0n;
  const a = neg ? -num : num;
  const q = a / den;
  const r = a % den;
  // r/den >= 1/2  <=>  2r >= den
  const rounded = r * 2n >= den ? q + 1n : q;
  return neg ? -rounded : rounded;
}

// Rescale `units` (at `from` digits) to `to` digits, rounding half away from
// zero when narrowing.
function rescale(units: bigint, from: number, to: number): bigint {
  if (to === from) return units;
  if (to > from) return units * pow10(to - from);
  return divRoundHalfUp(units, pow10(from - to));
}

function bringToCommon(a: Decimal, b: Decimal): { an: bigint; bn: bigint; scale: number } {
  const raw = a as unknown as { _u: bigint; _s: number };
  const rb = b as unknown as { _u: bigint; _s: number };
  const scale = raw._s > rb._s ? raw._s : rb._s;
  return {
    an: rescale(raw._u, raw._s, scale),
    bn: rescale(rb._u, rb._s, scale),
    scale,
  };
}

function toStringOf(units: bigint, scale: number): string {
  const neg = units < 0n;
  let digits = (neg ? -units : units).toString();
  if (scale === 0) return (neg ? "-" : "") + digits;
  while (digits.length <= scale) digits = "0" + digits;
  const cut = digits.length - scale;
  return (neg ? "-" : "") + digits.slice(0, cut) + "." + digits.slice(cut);
}

function make(units: bigint, scale: number): Decimal {
  // The raw fields ride along under `_u`/`_s` for the arithmetic helpers; the
  // public surface is the methods below.
  const self = {
    _u: units,
    _s: scale,
  } as unknown as Decimal & { _u: bigint; _s: number };

  self.add = (other) => {
    const { an, bn, scale: s } = bringToCommon(self, other);
    return make(an + bn, s);
  };
  self.sub = (other) => {
    const { an, bn, scale: s } = bringToCommon(self, other);
    return make(an - bn, s);
  };
  self.mul = (other) => {
    const ro = other as unknown as { _u: bigint; _s: number };
    return make(units * ro._u, scale + ro._s);
  };
  self.div = (other, resultScale) => {
    const ro = other as unknown as { _u: bigint; _s: number };
    // (units / 10^scale) / (ro._u / 10^ro._s) rounded to `resultScale` digits.
    // q = round( units * 10^(ro._s + resultScale) / (ro._u * 10^scale) )
    const num = units * pow10(ro._s + resultScale);
    const den = ro._u * pow10(scale);
    const denNeg = den < 0n;
    const q = divRoundHalfUp(num, denNeg ? -den : den);
    return make(denNeg ? -q : q, resultScale);
  };
  self.round = (resultScale) => make(rescale(units, scale, resultScale), resultScale);
  self.neg = () => make(-units, scale);
  self.abs = () => make(units < 0n ? -units : units, scale);
  self.cmp = (other) => {
    const { an, bn } = bringToCommon(self, other);
    return an < bn ? -1 : an > bn ? 1 : 0;
  };
  self.eq = (other) => self.cmp(other) === 0;
  self.is_zero = () => units === 0n;
  self.is_negative = () => units < 0n;
  self.scale = () => scale;
  self.to_string = () => toStringOf(units, scale);
  self.to_number = () => Number(toStringOf(units, scale));
  return self;
}

// Parse a decimal from a string: an optional sign, digits, an optional
// fractional part. Whitespace is not tolerated; an empty or malformed input is
// an `Err`, never a silent `NaN`.
export function decimal(text: string): Result<Decimal, string> {
  const m = /^(-?)(\d+)(?:\.(\d+))?$/.exec(text);
  if (m === null) {
    return Err(`not a decimal: ${JSON.stringify(text)}`);
  }
  const sign = m[1] === "-" ? -1n : 1n;
  const intPart = m[2];
  const fracPart = m[3] ?? "";
  const units = sign * BigInt(intPart + fracPart);
  return Ok(make(units, fracPart.length));
}

// Build a Decimal from an integer count of the smallest unit (e.g. cents):
// `from_units(1050n-equivalent)` ... here `units` is a plain integer and
// `scale` says how many of its low digits are fractional. `from_int(1050, 2)`
// is `10.50`. Total, no parsing, for values you already hold as scaled integers.
export function from_int(units: number, scale: number): Decimal {
  return make(BigInt(units), scale);
}

export const zero: Decimal = make(0n, 0);
