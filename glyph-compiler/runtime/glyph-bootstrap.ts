// Runtime prelude bootstrap. The emitter references a few prelude values
// without an import — `number`, `par`, `print` — so they must exist as globals
// at run time. This module installs them onto `globalThis` as a side effect;
// the `glyph run` entrypoint imports it before invoking the program. The
// matching ambient *types* live in `glyph-prelude.d.ts`.

import { Result, Ok, Err } from "./std/result";

const number = {
  to_string(n: number): string {
    return String(n);
  },
  parse(s: string): Result<number, string> {
    if (s.trim() === "") {
      return Err("not a number");
    }
    const n = Number(s);
    return Number.isNaN(n) ? Err("not a number") : Ok(n);
  },
};

const par = {
  async all<T>(xs: ReadonlyArray<T | Promise<T>>): Promise<Array<Awaited<T>>> {
    return Promise.all(xs) as Promise<Array<Awaited<T>>>;
  },
  all_ok<T, E>(xs: ReadonlyArray<Result<T, E>>): Result<Array<T>, E> {
    const out: Array<T> = [];
    for (const r of xs) {
      if (r.tag === "Err") {
        return Err(r.value) as Result<Array<T>, E>;
      }
      out.push(r.value);
    }
    return Ok(out);
  },
};

function print(message: string): void {
  console.log(message);
}

function assert(condition: boolean): void {
  if (!condition) {
    throw new Error("assertion failed");
  }
}

const g = globalThis as unknown as Record<string, unknown>;
g.number = number;
g.par = par;
g.print = print;
g.assert = assert;
