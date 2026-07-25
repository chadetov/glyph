// std/math — numeric helpers over the JavaScript `Math` object.

export const PI: number = Math.PI;
export const E: number = Math.E;

export function abs(x: number): number {
  return Math.abs(x);
}

export function min(a: number, b: number): number {
  return Math.min(a, b);
}

export function max(a: number, b: number): number {
  return Math.max(a, b);
}

export function floor(x: number): number {
  return Math.floor(x);
}

export function ceil(x: number): number {
  return Math.ceil(x);
}

export function round(x: number): number {
  return Math.round(x);
}

export function trunc(x: number): number {
  return Math.trunc(x);
}

export function sqrt(x: number): number {
  return Math.sqrt(x);
}

export function pow(base: number, exponent: number): number {
  return base ** exponent;
}

// Clamp `x` into the inclusive range `[lo, hi]`.
export function clamp(x: number, lo: number, hi: number): number {
  return Math.min(Math.max(x, lo), hi);
}

// -1, 0, or 1 by the sign of `x`.
export function sign(x: number): number {
  return Math.sign(x);
}
